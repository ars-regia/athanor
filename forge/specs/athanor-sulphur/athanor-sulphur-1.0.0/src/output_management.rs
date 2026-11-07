//! wlr-output-management (doc_own_stack.md OS4 and OS5; open question 10 of the OS9 plan;
//! doc_session_daemons.md SD23): the outputs as heads, enabled or disabled, with their modes,
//! position, transform, scale and adaptive sync, and configurations that set any of them. It is
//! privileged: the allow-list grants it to the control center. The pinned Smithay has no server
//! for it, so the compositor serves it here.
//!
//! A configuration must name every head, as the protocol asks. One the outputs cannot take, or
//! one that leaves no output enabled, fails and changes nothing: a mode the output does not
//! offer, a scale outside 1 to 4 as `--scale` takes them, another transform of the nested
//! output, or a step the backend refuses. Every head is checked before anything is applied, and
//! when the backend refuses a step halfway, the steps it took are undone (`take_all`). An
//! applied configuration is announced to every manager once, each head as its output now is.
//! Positions are taken as given, overlapping or apart; an output a configuration gives no
//! position keeps its own, and one it enables again goes to the right of the others
//! (maintainer decision, 2026-11-07).

use crate::identity::Privilege;
use crate::input::{turn, RightsTheFrame};
use crate::state::{granted, publish_output, withdraw_output, Backend, State};
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
use std::cell::Cell;

/// The version offered: 4, with adaptive sync.
const VERSION: u32 = 4;

/// The scales a configuration may set, as `--scale` takes them.
const SCALES: std::ops::RangeInclusive<f64> = 1.0..=4.0;

/// How far from the origin a configuration may place an output, on either axis. A mode is at
/// most 65535 pixels a side (DRM's `u16`), so a corner is within 2^24 + 2^16; the space adds
/// and doubles such sums (centring, overlaps, the bounding box of every output) and the
/// renderer scales them by up to 4, which stays far below `i32::MAX`, 2^31 - 1, where an
/// overflow would abort the session.
const REACH: i32 = 1 << 24;

/// Why the nested backend refuses every step.
const NESTED_REFUSES: &str = "the nested output cannot change so";

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
    /// The outputs a configuration disabled: out of the space and without a global, still
    /// heads, until one enables them again or their monitor is unplugged.
    pub(crate) disabled: Vec<Output>,
    /// A configuration is being applied: the managers hear of it once, at the end.
    applying: bool,
}

impl OutputManagement {
    /// Whether a configuration disabled `output`.
    pub(crate) fn is_disabled(&self, output: &Output) -> bool {
        self.disabled.contains(output)
    }
}

/// Whether an output refreshes with adaptive sync, kept in its user data by the backend that
/// set it; an output that never had it set has it off.
struct AdaptiveSync(Cell<bool>);

/// Whether `output` has adaptive sync on.
fn adaptive_sync(output: &Output) -> bool {
    output
        .user_data()
        .get::<AdaptiveSync>()
        .is_some_and(|sync| sync.0.get())
}

/// Records that `output` has adaptive sync `on`.
pub(crate) fn set_adaptive_sync(output: &Output, on: bool) {
    output
        .user_data()
        .get_or_insert(|| AdaptiveSync(Cell::new(false)))
        .0
        .set(on);
}

/// Whether an output can take adaptive sync, kept in its user data by the backend that drives
/// it; an output it was never set for, the nested one, cannot.
struct CanSync(Cell<bool>);

/// Whether `output` can take adaptive sync.
fn can_sync(output: &Output) -> bool {
    output
        .user_data()
        .get::<CanSync>()
        .is_some_and(|can| can.0.get())
}

/// Records whether `output` can take adaptive sync.
pub(crate) fn set_can_sync(output: &Output, can: bool) {
    output
        .user_data()
        .get_or_insert(|| CanSync(Cell::new(false)))
        .0
        .set(can);
}

/// A change a backend makes to an output, which `undo` takes back.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Stops driving the output, which was in `mode` with adaptive sync `sync`.
    Disable {
        output: Output,
        mode: Mode,
        sync: bool,
    },
    /// Drives the output again, in `mode` with adaptive sync `sync`.
    Enable {
        output: Output,
        mode: Mode,
        sync: bool,
    },
    /// Sets the mode of an output driven in `from`.
    Mode {
        output: Output,
        from: Mode,
        to: Mode,
    },
    /// Turns adaptive sync on or off.
    AdaptiveSync { output: Output, on: bool },
}

impl Step {
    /// The output the step changes.
    pub fn output(&self) -> &Output {
        match self {
            Step::Disable { output, .. }
            | Step::Enable { output, .. }
            | Step::Mode { output, .. }
            | Step::AdaptiveSync { output, .. } => output,
        }
    }

    /// The step that takes this one back.
    fn undo(&self) -> Step {
        match self.clone() {
            Step::Disable { output, mode, sync } => Step::Enable { output, mode, sync },
            Step::Enable { output, mode, sync } => Step::Disable { output, mode, sync },
            Step::Mode { output, from, to } => Step::Mode {
                output,
                from: to,
                to: from,
            },
            Step::AdaptiveSync { output, on } => Step::AdaptiveSync { output, on: !on },
        }
    }
}

/// The steps of `plans` for the backend, in the order they are taken.
fn steps(plans: &[Plan]) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    // Disabled first: the bandwidth they free may be what another output's mode needs.
    for plan in plans
        .iter()
        .filter(|plan| plan.now.enabled && plan.wanted.is_none())
    {
        let mode = plan.now.mode.ok_or("an output to disable has no mode")?;
        steps.push(Step::Disable {
            output: plan.output.clone(),
            mode,
            sync: plan.now.adaptive_sync,
        });
    }
    for plan in plans.iter().filter(|plan| plan.now.enabled) {
        let Some(wanted) = &plan.wanted else {
            continue;
        };
        if Some(wanted.mode) != plan.now.mode {
            let from = plan.now.mode.ok_or("an output to change has no mode")?;
            steps.push(Step::Mode {
                output: plan.output.clone(),
                from,
                to: wanted.mode,
            });
        }
        if wanted.adaptive_sync != plan.now.adaptive_sync {
            steps.push(Step::AdaptiveSync {
                output: plan.output.clone(),
                on: wanted.adaptive_sync,
            });
        }
    }
    for plan in plans.iter().filter(|plan| !plan.now.enabled) {
        if let Some(wanted) = &plan.wanted {
            steps.push(Step::Enable {
                output: plan.output.clone(),
                mode: wanted.mode,
                sync: wanted.adaptive_sync,
            });
        }
    }
    Ok(steps)
}

/// Takes `steps` in order. When one fails, those taken are undone, the last first, and its
/// error is returned; an undo that fails too is logged, since nothing else can be done then.
fn take_all<S>(
    steps: &[S],
    undo: impl Fn(&S) -> S,
    mut take: impl FnMut(&S) -> Result<(), String>,
) -> Result<(), String> {
    for (taken, step) in steps.iter().enumerate() {
        let Err(error) = take(step) else {
            continue;
        };
        for step in steps[..taken].iter().rev() {
            if let Err(undone) = take(&undo(step)) {
                tracing::error!(error = %undone, "a step of an output configuration could not be undone");
            }
        }
        return Err(error);
    }
    Ok(())
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
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sent {
    enabled: bool,
    mode: Option<Mode>,
    position: Point<i32, Logical>,
    transform: wl_output::Transform,
    scale: f64,
    adaptive_sync: bool,
}

impl Sent {
    /// What `output` has now, placed in `space` unless it is disabled.
    fn of(output: &Output, space: &Space<Window>, enabled: bool) -> Self {
        Self {
            enabled,
            mode: output.current_mode(),
            position: space
                .output_geometry(output)
                .map_or_else(Point::default, |area| area.loc),
            transform: turn(output).into(),
            scale: output.current_scale().fractional_scale(),
            adaptive_sync: adaptive_sync(output),
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

/// What a configuration head asks for. A custom mode with a refresh of 0 takes the refresh the
/// output has at that size.
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

/// What a configuration asks of an output it enables, checked against what the output takes.
struct Wanted {
    mode: Mode,
    /// Where it asks the output to be, if anywhere.
    position: Option<Point<i32, Logical>>,
    /// A transform to set; none for the nested output, whose own transform stays.
    transform: Option<wl_output::Transform>,
    scale: Option<f64>,
    adaptive_sync: bool,
}

/// What a configuration does to one output: what it is now, and what it is asked to be, or none
/// to be disabled.
struct Plan {
    output: Output,
    now: Sent,
    wanted: Option<Wanted>,
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
        for (output, now) in state.heads() {
            match announce(dh, client, &manager, &output, now) {
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
    now: Sent,
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
        sent: now,
    };
    sent.send_modes(dh, client)?;
    let preferred = output.preferred_mode();
    for (mode, resource) in &sent.modes {
        if Some(*mode) == preferred {
            resource.preferred();
        }
    }
    sent.send(None, false);
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

    /// Sends the properties the head has that differ from `was`, all of them when the head is
    /// new or enabled again; a disabled head is sent none but that it is disabled.
    fn send(&self, was: Option<Sent>, modes_changed: bool) {
        let now = self.sent;
        if was.is_none_or(|was| was.enabled != now.enabled) {
            self.head.enabled(i32::from(now.enabled));
        }
        if !now.enabled {
            return;
        }
        let was = was.filter(|was| was.enabled);
        if was.is_none_or(|was| was.mode != now.mode) || modes_changed {
            self.send_current_mode();
        }
        if was.is_none_or(|was| was.position != now.position) {
            self.head.position(now.position.x, now.position.y);
        }
        if was.is_none_or(|was| was.transform != now.transform) {
            self.head.transform(now.transform);
        }
        if was.is_none_or(|was| was.scale != now.scale) {
            self.head.scale(now.scale);
        }
        if self.head.version() >= 4 && was.is_none_or(|was| was.adaptive_sync != now.adaptive_sync)
        {
            self.head.adaptive_sync(if now.adaptive_sync {
                AdaptiveSyncState::Enabled
            } else {
                AdaptiveSyncState::Disabled
            });
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
        self.send(Some(was), modes_changed);
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
    /// Every head with what it shows: the outputs of the space, then those a configuration
    /// disabled.
    fn heads(&self) -> Vec<(Output, Sent)> {
        let enabled = self.space.outputs().map(|output| (output, true));
        let disabled = self
            .output_management
            .disabled
            .iter()
            .map(|output| (output, false));
        enabled
            .chain(disabled)
            .map(|(output, on)| (output.clone(), Sent::of(output, &self.space, on)))
            .collect()
    }

    /// The outputs changed: every manager is sent its new heads, the properties that changed and
    /// the heads gone, then a new serial, which cancels the configurations made on the old one.
    /// While a configuration is applied, only once it is.
    pub fn output_heads_changed(&mut self) {
        if self.output_management.applying {
            return;
        }
        let dh = self.display_handle.clone();
        let outputs = self.heads();
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
                match announce(&dh, &client, &manager.manager, output, *now) {
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
        let heads = self.heads();
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
        let mut plans = Vec::new();
        for (named, change) in &pending.heads {
            let Some((output, now)) = manager
                .heads
                .iter()
                .find(|head| head.head == *named)
                .and_then(|head| heads.iter().find(|(output, _)| *output == head.output))
                .cloned()
            else {
                configuration.failed();
                return;
            };
            let wanted = match change {
                None => None,
                Some(change) => match change.wanted(&output, now) {
                    Some(wanted) => Some(wanted),
                    None => {
                        configuration.failed();
                        return;
                    }
                },
            };
            plans.push(Plan {
                output,
                now,
                wanted,
            });
        }
        if plans.iter().all(|plan| plan.wanted.is_none()) {
            configuration.failed();
            return;
        }
        // `test` and `apply` ask the backend the same: what it would refuse fails both.
        let steps = match steps(&plans) {
            Ok(steps) => steps,
            Err(error) => {
                tracing::warn!(%error, "an output configuration was refused");
                configuration.failed();
                return;
            }
        };
        if let Err(error) = steps.iter().try_for_each(|step| self.backend_takes(step)) {
            tracing::info!(%error, "an output configuration the backend cannot take was refused");
            configuration.failed();
            return;
        }
        if apply {
            if let Err(error) = self.apply_plans(&plans, &steps) {
                tracing::warn!(%error, "an output configuration was refused and undone");
                // A refused DRM test may have committed frames of its own, as `changed` notes.
                self.request_redraw();
                configuration.failed();
                return;
            }
        }
        configuration.succeeded();
    }

    /// Applies `plans`, every one checked already: first `steps`, what the backend does, which
    /// it may refuse, and is then undone whole; then the outputs' state, the space and the heads.
    fn apply_plans(&mut self, plans: &[Plan], steps: &[Step]) -> Result<(), String> {
        take_all(steps, Step::undo, |step| self.backend_step(step))?;
        // Only now, with every step taken: the lock may have waited only for an output disabled.
        self.confirm_lock();

        self.output_management.applying = true;
        for plan in plans {
            let output = &plan.output;
            let Some(wanted) = &plan.wanted else {
                if plan.now.enabled {
                    self.remove_output(output);
                    withdraw_output(&self.display_handle, output);
                    self.output_management.disabled.push(output.clone());
                }
                continue;
            };
            let mode = (Some(wanted.mode) != plan.now.mode).then_some(wanted.mode);
            let transform = wanted.transform.map(Into::into);
            let scale = wanted.scale.map(Scale::Fractional);
            if mode.is_some() || transform.is_some() || scale.is_some() {
                output.change_current_state(mode, transform, scale, None);
            }
            set_adaptive_sync(output, wanted.adaptive_sync);
            if !plan.now.enabled {
                self.output_management
                    .disabled
                    .retain(|kept| kept != output);
                publish_output(&self.display_handle, output);
                self.map_output(output);
            }
            layer_map_for_output(output).arrange();
        }
        // An output given no place keeps its own; one enabled again went to the right above.
        for plan in plans {
            if let Some(at) = plan.wanted.as_ref().and_then(|wanted| wanted.position) {
                self.space.map_output(&plan.output, at);
            }
        }
        self.fit_to_outputs();
        self.output_management.applying = false;
        self.output_heads_changed();
        self.request_redraw();
        Ok(())
    }

    /// Whether the backend would take `step`, asked without changing anything. The nested
    /// output is the parent compositor's window: it takes no mode, no disable and no adaptive
    /// sync. A monitor may still refuse a step this takes, which only applying it tells.
    fn backend_takes(&self, step: &Step) -> Result<(), String> {
        if let Backend::Nested(_) = self.backend {
            return Err(NESTED_REFUSES.into());
        }
        match step {
            Step::AdaptiveSync { output, on: true }
            | Step::Enable {
                output, sync: true, ..
            } if !can_sync(output) => Err("the output has no adaptive sync".into()),
            _ => Ok(()),
        }
    }

    /// Has the backend take `step`, which `backend_takes` took.
    fn backend_step(&mut self, step: &Step) -> Result<(), String> {
        match &self.backend {
            Backend::Nested(_) => Err(NESTED_REFUSES.into()),
            Backend::Headless(_) => self.headless_step(step),
            Backend::Drm(_) => self.drm_step(step),
        }
    }
}

impl Change {
    /// What this asks of `output`, which has `now`, if the output takes it: a mode it offers, a
    /// scale the compositor takes and a transform, but on the nested output only the transform
    /// it has.
    fn wanted(&self, output: &Output, now: Sent) -> Option<Wanted> {
        if self.mode_gone {
            return None;
        }
        let mode = match self.mode {
            None => now.mode?,
            Some(wanted) if wanted.refresh == 0 => {
                now.mode.filter(|mode| mode.size == wanted.size)?
            }
            Some(wanted) => output.modes().contains(&wanted).then_some(wanted)?,
        };
        let fixed = output.user_data().get::<RightsTheFrame>().is_some();
        if fixed && self.transform.is_some_and(|wanted| wanted != now.transform) {
            return None;
        }
        if self.scale.is_some_and(|scale| !SCALES.contains(&scale)) {
            return None;
        }
        let reach = -REACH..=REACH;
        if self
            .position
            .is_some_and(|at| !reach.contains(&at.x) || !reach.contains(&at.y))
        {
            return None;
        }
        Some(Wanted {
            mode,
            position: self.position,
            // A head shows what `turn` reads: the frame's own transform stays.
            transform: self.transform.filter(|_| !fixed),
            scale: self.scale,
            adaptive_sync: self
                .adaptive_sync
                .map_or(now.adaptive_sync, |sync| sync == AdaptiveSyncState::Enabled),
        })
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

#[cfg(test)]
mod tests {
    use super::take_all;
    use std::cell::RefCell;

    /// A step the backend refuses halfway undoes those it took, the last first, and fails the
    /// whole; steps it all takes are not undone.
    #[test]
    fn a_refused_step_undoes_those_taken() {
        let log = RefCell::new(Vec::new());
        let taken = &log;
        let take = |refused: i32| {
            move |step: &i32| {
                taken.borrow_mut().push(*step);
                if *step == refused {
                    Err(format!("{step} refused"))
                } else {
                    Ok(())
                }
            }
        };
        let undo = |step: &i32| -step;

        assert_eq!(take_all(&[1, 2, 3], undo, take(3)), Err("3 refused".into()));
        assert_eq!(*taken.borrow(), [1, 2, 3, -2, -1]);

        taken.borrow_mut().clear();
        assert_eq!(take_all(&[1, 2, 3], undo, take(0)), Ok(()));
        assert_eq!(*taken.borrow(), [1, 2, 3]);
    }
}
