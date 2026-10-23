//! wlr-output-management (doc_own_stack.md OS4 and OS5; open question 10 of the OS9 plan): the
//! outputs as heads, with their modes, position, transform and scale, and configurations that
//! set an output's scale. It is privileged: the allow-list grants it to the control center. The
//! pinned Smithay has no server for it, so the compositor serves it here.
//!
//! A configuration must name every head, as the protocol asks. It succeeds when it changes
//! nothing but scales, each from 1 to 4 as `--scale` takes them; one that disables a head or asks
//! for another mode, position, transform or adaptive sync than the head has fails.
// ponytail: scale only; modes, positions, transforms and disabled outputs when a client needs
// them, through the backends that can change them (the DRM backend's `use_mode`).

use crate::identity::Privilege;
use crate::state::{granted, State};
use smithay::desktop::{layer_map_for_output, Space, Window};
use smithay::output::{Mode, Output, Scale};
use smithay::reexports::wayland_protocols_wlr::output_management::v1::server::{
    zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, AdaptiveSyncState, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};
use smithay::reexports::wayland_server::backend::{ClientId, InvalidId};
use smithay::reexports::wayland_server::protocol::wl_output;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, WEnum};
use smithay::utils::{Logical, Point};
use smithay::wayland::{Dispatch2, GlobalDispatch2};

/// The version offered: 4, with adaptive sync, which every head reports off.
const VERSION: u32 = 4;

/// The scales a configuration may set, as `--scale` takes them.
const SCALES: std::ops::RangeInclusive<f64> = 1.0..=4.0;

/// The user data of the global and of every object of the protocol but the modes.
pub struct Heads;

/// The user data of a mode: the head it was sent for, which tells a mode the compositor has
/// finished since from one that never belonged to the head.
pub struct ModeOf(ZwlrOutputHeadV1);

/// What the clients of the protocol were sent, and their configurations in progress.
#[derive(Default)]
pub struct OutputManagement {
    /// The serial of the last `done`: a configuration made on another one is cancelled.
    serial: u32,
    managers: Vec<Manager>,
    configurations: Vec<Configuration>,
}

/// A client's manager and the heads sent to it.
struct Manager {
    manager: ZwlrOutputManagerV1,
    heads: Vec<Head>,
}

/// An output as a manager's client has it: its head, its modes and what it was last sent.
struct Head {
    output: Output,
    head: ZwlrOutputHeadV1,
    modes: Vec<(Mode, ZwlrOutputModeV1)>,
    sent: Sent,
}

/// The properties of a head that change while it lives.
#[derive(Clone, Copy, PartialEq)]
struct Sent {
    mode: Option<Mode>,
    position: Point<i32, Logical>,
    transform: wl_output::Transform,
    scale: f64,
}

impl Sent {
    /// What `output` has now, placed in `space`.
    fn of(output: &Output, space: &Space<Window>) -> Self {
        Self {
            mode: output.current_mode(),
            position: space
                .output_geometry(output)
                .map_or_else(Point::default, |area| area.loc),
            transform: output.current_transform().into(),
            scale: output.current_scale().fractional_scale(),
        }
    }
}

/// A configuration in progress: the heads named so far, each with what its configuration head
/// asks for, or none if it is disabled.
struct Configuration {
    configuration: ZwlrOutputConfigurationV1,
    manager: ZwlrOutputManagerV1,
    serial: u32,
    heads: Vec<(ZwlrOutputHeadV1, Option<Change>)>,
    /// Applied or tested: any further request is a protocol error.
    used: bool,
}

/// What a configuration head asks for. A custom mode with a refresh of 0 takes any refresh.
struct Change {
    head: ZwlrOutputConfigurationHeadV1,
    mode: Option<Mode>,
    position: Option<Point<i32, Logical>>,
    transform: Option<wl_output::Transform>,
    scale: Option<f64>,
    adaptive_sync: Option<AdaptiveSyncState>,
    /// The mode set was one of the head's that the compositor finished before the request came,
    /// a race with a hot-plug: the configuration fails.
    mode_gone: bool,
}

/// Offers the global to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ZwlrOutputManagerV1, _>(VERSION, Heads);
}

impl GlobalDispatch2<ZwlrOutputManagerV1, State> for Heads {
    fn bind(
        &self,
        state: &mut State,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ZwlrOutputManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, Heads);
        let mut heads = Vec::new();
        for output in state.space.outputs() {
            match announce(dh, client, &manager, output, &state.space) {
                Ok(head) => heads.push(head),
                Err(error) => tracing::warn!(%error, "a head could not be sent"),
            }
        }
        manager.done(state.output_management.serial);
        state
            .output_management
            .managers
            .push(Manager { manager, heads });
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::OutputManagement)
    }
}

/// Sends `output` as a new head to `manager`'s client, with its modes and properties.
fn announce(
    dh: &DisplayHandle,
    client: &Client,
    manager: &ZwlrOutputManagerV1,
    output: &Output,
    space: &Space<Window>,
) -> Result<Head, InvalidId> {
    let version = manager.version();
    let head = client.create_resource::<ZwlrOutputHeadV1, _, State>(dh, version, Heads)?;
    manager.head(&head);
    head.name(output.name());
    head.description(output.description());
    let physical = output.physical_properties();
    head.physical_size(physical.size.w, physical.size.h);
    if version >= 2 {
        head.make(physical.make);
        head.model(physical.model);
        head.serial_number(physical.serial_number);
    }
    let mut sent = Head {
        output: output.clone(),
        head,
        modes: Vec::new(),
        sent: Sent::of(output, space),
    };
    sent.send_modes(dh, client)?;
    let preferred = output.preferred_mode();
    for (mode, resource) in &sent.modes {
        if Some(*mode) == preferred {
            resource.preferred();
        }
    }
    sent.head.enabled(1);
    sent.send_current_mode();
    let now = sent.sent;
    sent.head.position(now.position.x, now.position.y);
    sent.head.transform(now.transform);
    sent.head.scale(now.scale);
    if version >= 4 {
        sent.head.adaptive_sync(AdaptiveSyncState::Disabled);
    }
    Ok(sent)
}

impl Head {
    /// Sends the output's modes the client has not been sent, and finishes those it has lost;
    /// whether any was sent or finished.
    fn send_modes(&mut self, dh: &DisplayHandle, client: &Client) -> Result<bool, InvalidId> {
        let modes = self.output.modes();
        let mut changed = false;
        self.modes.retain(|(mode, resource)| {
            let kept = modes.contains(mode);
            if !kept {
                resource.finished();
                changed = true;
            }
            kept
        });
        for mode in modes {
            if self.modes.iter().any(|(sent, _)| *sent == mode) {
                continue;
            }
            // A child takes its parent's version, as wlroots sends it.
            let version = self.head.version();
            let resource = client.create_resource::<ZwlrOutputModeV1, _, State>(
                dh,
                version,
                ModeOf(self.head.clone()),
            )?;
            self.head.mode(&resource);
            resource.size(mode.size.w, mode.size.h);
            if mode.refresh > 0 {
                resource.refresh(mode.refresh);
            }
            self.modes.push((mode, resource));
            changed = true;
        }
        Ok(changed)
    }

    /// Sends the current mode, if the output has one among those sent.
    fn send_current_mode(&self) {
        if let Some((_, resource)) = self
            .modes
            .iter()
            .find(|(mode, _)| Some(*mode) == self.sent.mode)
        {
            self.head.current_mode(resource);
        }
    }

    /// Sends what changed since the head was last sent; whether anything did.
    fn update(&mut self, dh: &DisplayHandle, client: &Client, now: Sent) -> bool {
        // A mode finished and another sent leave as many: the modes are told apart by identity.
        let modes_changed = self.send_modes(dh, client).unwrap_or_else(|error| {
            tracing::warn!(%error, "a mode could not be sent");
            true
        });
        let was = std::mem::replace(&mut self.sent, now);
        if now.mode != was.mode || modes_changed {
            self.send_current_mode();
        }
        if now.position != was.position {
            self.head.position(now.position.x, now.position.y);
        }
        if now.transform != was.transform {
            self.head.transform(now.transform);
        }
        if now.scale != was.scale {
            self.head.scale(now.scale);
        }
        now != was || modes_changed
    }

    /// The output is gone: its modes and its head are finished.
    fn finish(&self) {
        for (_, resource) in &self.modes {
            resource.finished();
        }
        self.head.finished();
    }
}

impl State {
    /// The outputs changed: every manager is sent its new heads, the properties that changed and
    /// the heads gone, then a new serial, which cancels the configurations made on the old one.
    pub fn output_heads_changed(&mut self) {
        let dh = self.display_handle.clone();
        let outputs: Vec<(Output, Sent)> = self
            .space
            .outputs()
            .map(|output| (output.clone(), Sent::of(output, &self.space)))
            .collect();
        let management = &mut self.output_management;
        management.managers.retain(|m| m.manager.is_alive());
        // Without a manager nothing tells a change from none: a configuration left by a stopped
        // manager is cancelled all the same.
        let mut changed = management.managers.is_empty();
        for manager in &mut management.managers {
            let Some(client) = manager.manager.client() else {
                continue;
            };
            manager.heads.retain(|head| {
                let kept = outputs.iter().any(|(output, _)| *output == head.output);
                if !kept {
                    head.finish();
                    changed = true;
                }
                kept
            });
            for (output, now) in &outputs {
                if let Some(head) = manager.heads.iter_mut().find(|h| h.output == *output) {
                    changed |= head.update(&dh, &client, *now);
                    continue;
                }
                match announce(&dh, &client, &manager.manager, output, &self.space) {
                    Ok(head) => manager.heads.push(head),
                    Err(error) => tracing::warn!(%error, "a head could not be sent"),
                }
                changed = true;
            }
        }
        if changed {
            management.serial = management.serial.wrapping_add(1);
            for manager in &management.managers {
                manager.manager.done(management.serial);
            }
        }
    }

    /// Applies or tests `configuration`, which has every head configured, and answers it.
    fn finish_configuration(&mut self, configuration: &ZwlrOutputConfigurationV1, apply: bool) {
        let management = &mut self.output_management;
        let Some(pending) = management
            .configurations
            .iter_mut()
            .find(|pending| &pending.configuration == configuration)
        else {
            return;
        };
        if pending.used {
            configuration.post_error(
                zwlr_output_configuration_v1::Error::AlreadyUsed,
                "the configuration was applied or tested already",
            );
            return;
        }
        pending.used = true;
        if pending.serial != management.serial {
            configuration.cancelled();
            return;
        }
        let Some(manager) = management
            .managers
            .iter()
            .find(|manager| manager.manager == pending.manager)
        else {
            configuration.cancelled();
            return;
        };
        let unconfigured = manager.heads.iter().any(|head| {
            head.head.is_alive() && !pending.heads.iter().any(|(named, _)| *named == head.head)
        });
        if unconfigured {
            configuration.post_error(
                zwlr_output_configuration_v1::Error::UnconfiguredHead,
                "every head must be enabled or disabled",
            );
            return;
        }
        let mut scales = Vec::new();
        for (named, change) in &pending.heads {
            let output = manager
                .heads
                .iter()
                .find(|head| head.head == *named)
                .map(|head| head.output.clone());
            match (output, change) {
                (Some(output), Some(change)) if change.only_scale(&output, &self.space) => {
                    scales.push((output, change.scale));
                }
                _ => {
                    configuration.failed();
                    return;
                }
            }
        }
        if apply {
            for (output, scale) in scales {
                if let Some(scale) = scale {
                    output.change_current_state(None, None, Some(Scale::Fractional(scale)), None);
                    layer_map_for_output(&output).arrange();
                }
            }
            // Side by side again, at their new logical sizes; the managers hear of it there.
            self.arrange_outputs();
            self.request_redraw();
        }
        configuration.succeeded();
    }
}

impl Change {
    /// Whether this asks for nothing `output` does not have but a scale the compositor takes.
    fn only_scale(&self, output: &Output, space: &Space<Window>) -> bool {
        let now = Sent::of(output, space);
        let mode = self.mode.is_none_or(|wanted| {
            now.mode.is_some_and(|mode| {
                mode.size == wanted.size && (wanted.refresh == 0 || mode.refresh == wanted.refresh)
            })
        });
        !self.mode_gone
            && mode
            && self.position.is_none_or(|at| at == now.position)
            && self.transform.is_none_or(|t| t == now.transform)
            && self.scale.is_none_or(|scale| SCALES.contains(&scale))
            && self
                .adaptive_sync
                .is_none_or(|sync| sync == AdaptiveSyncState::Disabled)
    }
}

impl Dispatch2<ZwlrOutputManagerV1, State> for Heads {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let management = &mut state.output_management;
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                let configuration = data_init.init(id, Heads);
                management.configurations.push(Configuration {
                    configuration,
                    manager: manager.clone(),
                    serial,
                    heads: Vec::new(),
                    used: false,
                });
            }
            zwlr_output_manager_v1::Request::Stop => {
                management.managers.retain(|kept| &kept.manager != manager);
                manager.finished();
            }
            _ => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, manager: &ZwlrOutputManagerV1) {
        state
            .output_management
            .managers
            .retain(|kept| &kept.manager != manager);
    }
}

impl Dispatch2<ZwlrOutputHeadV1, State> for Heads {
    /// `release` only: the head stays tracked, and a configuration need not name it.
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _head: &ZwlrOutputHeadV1,
        _request: zwlr_output_head_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch2<ZwlrOutputModeV1, State> for ModeOf {
    /// `release` only.
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _mode: &ZwlrOutputModeV1,
        _request: zwlr_output_mode_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch2<ZwlrOutputConfigurationV1, State> for Heads {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        configuration: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use zwlr_output_configuration_v1::Request;
        let (head, id) = match request {
            Request::EnableHead { id, head } => (head, Some(id)),
            Request::DisableHead { head } => (head, None),
            Request::Apply => return state.finish_configuration(configuration, true),
            Request::Test => return state.finish_configuration(configuration, false),
            _ => return,
        };
        let Some(pending) = state
            .output_management
            .configurations
            .iter_mut()
            .find(|pending| &pending.configuration == configuration)
        else {
            return;
        };
        if pending.used {
            configuration.post_error(
                zwlr_output_configuration_v1::Error::AlreadyUsed,
                "the configuration was applied or tested already",
            );
            return;
        }
        if pending.heads.iter().any(|(named, _)| *named == head) {
            configuration.post_error(
                zwlr_output_configuration_v1::Error::AlreadyConfiguredHead,
                "the head is configured already",
            );
            return;
        }
        let change = id.map(|id| Change {
            head: data_init.init(id, Heads),
            mode: None,
            position: None,
            transform: None,
            scale: None,
            adaptive_sync: None,
            mode_gone: false,
        });
        pending.heads.push((head, change));
    }

    fn destroyed(
        &self,
        state: &mut State,
        _client: ClientId,
        configuration: &ZwlrOutputConfigurationV1,
    ) {
        state
            .output_management
            .configurations
            .retain(|pending| &pending.configuration != configuration);
    }
}

impl Dispatch2<ZwlrOutputConfigurationHeadV1, State> for Heads {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        resource: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use zwlr_output_configuration_head_v1::{Error, Request};
        let management = &mut state.output_management;
        let Some((named, change)) = management
            .configurations
            .iter_mut()
            .flat_map(|pending| pending.heads.iter_mut())
            .find_map(|(named, change)| {
                change
                    .as_mut()
                    .filter(|change| &change.head == resource)
                    .map(|change| (named.clone(), change))
            })
        else {
            return;
        };
        let refuse = |error: Error, message: &str| resource.post_error(error, message.to_owned());
        match request {
            Request::SetMode { mode } => {
                let own = management
                    .managers
                    .iter()
                    .flat_map(|manager| manager.heads.iter())
                    .find(|head| head.head == named)
                    .and_then(|head| head.modes.iter().find(|(_, sent)| *sent == mode))
                    .map(|(mode, _)| *mode);
                let sent_for_head = mode.data::<ModeOf>().is_some_and(|of| of.0 == named);
                match (change.mode.is_some() || change.mode_gone, own) {
                    (true, _) => refuse(Error::AlreadySet, "the mode is set already"),
                    (false, Some(own)) => change.mode = Some(own),
                    // Finished since: the client raced a hot-plug, which is no protocol error.
                    (false, None) if sent_for_head => change.mode_gone = true,
                    (false, None) => refuse(Error::InvalidMode, "the mode is not the head's"),
                }
            }
            Request::SetCustomMode {
                width,
                height,
                refresh,
            } => {
                if change.mode.is_some() || change.mode_gone {
                    refuse(Error::AlreadySet, "the mode is set already")
                } else if width <= 0 || height <= 0 || refresh < 0 {
                    refuse(Error::InvalidCustomMode, "a custom mode needs a size")
                } else {
                    change.mode = Some(Mode {
                        size: (width, height).into(),
                        refresh,
                    })
                }
            }
            Request::SetPosition { x, y } => match change.position {
                Some(_) => refuse(Error::AlreadySet, "the position is set already"),
                None => change.position = Some((x, y).into()),
            },
            Request::SetTransform { transform } => match (change.transform, transform) {
                (Some(_), _) => refuse(Error::AlreadySet, "the transform is set already"),
                (None, WEnum::Value(transform)) => change.transform = Some(transform),
                (None, WEnum::Unknown(_)) => refuse(Error::InvalidTransform, "no such transform"),
            },
            Request::SetScale { scale } => match change.scale {
                Some(_) => refuse(Error::AlreadySet, "the scale is set already"),
                None if !(scale.is_finite() && scale > 0.0) => {
                    refuse(Error::InvalidScale, "a scale must be positive")
                }
                None => change.scale = Some(scale),
            },
            Request::SetAdaptiveSync { state } => match (change.adaptive_sync, state) {
                (Some(_), _) => refuse(Error::AlreadySet, "adaptive sync is set already"),
                (None, WEnum::Value(state)) => change.adaptive_sync = Some(state),
                (None, WEnum::Unknown(_)) => refuse(
                    Error::InvalidAdaptiveSyncState,
                    "no such adaptive sync state",
                ),
            },
            _ => {}
        }
    }
}
