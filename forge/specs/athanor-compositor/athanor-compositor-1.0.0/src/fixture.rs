//! The in-process fixture of doc_own_stack.md OS6: the compositor on its headless backend and a
//! Wayland client in one process, joined by a socket pair. The client lists the globals, maps a
//! window from a wl_shm buffer and waits for its frame callback; the compositor's frames are then
//! read back and their pixels compared.

use crate::grabs::{MoveGrab, ResizeGrab};
use crate::handlers::MAX_HELD_COMMITS;
use crate::headless::{self, VirtualOutput};
use crate::identity::{AllowList, FileId, Process};
use crate::state::{State, Under, BACKGROUND};
use crate::Error;
use smithay::backend::allocator::dmabuf::{Dmabuf, DmabufFlags};
use smithay::backend::allocator::{Fourcc, Modifier};
use smithay::backend::input::{ButtonState, InputTime};
use smithay::desktop::layer_map_for_output;
use smithay::input::keyboard::{KeyboardSource, XkbConfig};
use smithay::input::pointer::CursorImageStatus;
use smithay::reexports::calloop::{EventLoop, Interest};
use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::__interfaces::{
    ZWP_TEXT_INPUT_MANAGER_V3_INTERFACE, ZWP_TEXT_INPUT_V3_INTERFACE,
};
use smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::__interfaces::{
    ZWP_INPUT_METHOD_KEYBOARD_GRAB_V2_INTERFACE, ZWP_INPUT_METHOD_MANAGER_V2_INTERFACE,
    ZWP_INPUT_METHOD_V2_INTERFACE, ZWP_INPUT_POPUP_SURFACE_V2_INTERFACE,
};
use smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::__interfaces::{
    ZWP_VIRTUAL_KEYBOARD_MANAGER_V1_INTERFACE, ZWP_VIRTUAL_KEYBOARD_V1_INTERFACE,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::utils::{Logical, Point, Size, SERIAL_COUNTER};
use smithay::wayland::security_context::SecurityContext;
use std::collections::BTreeSet;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::backend::protocol::{Argument, Interface, Message};
use wayland_client::backend::{Backend, ObjectData, ObjectId, WaylandError};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry,
    wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{
    delegate_noop, event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
    ext_output_image_capture_source_manager_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1,
    ext_image_copy_capture_session_v1,
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};
use wayland_protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1, wp_security_context_v1,
};
use wayland_protocols::xdg::activation::v1::client::{xdg_activation_token_v1, xdg_activation_v1};
use wayland_protocols::xdg::shell::client::{
    xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1, zwlr_output_configuration_v1, zwlr_output_head_v1,
    zwlr_output_manager_v1, zwlr_output_mode_v1,
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1, zwlr_output_power_v1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

/// The client side of athanor-toplevel-state-v1, laid out as the client crate's `generated!`.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
mod athanor_toplevel {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-client re-exports.
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml"
    );
}

/// The client side of athanor-keyboard-group-v1.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
mod athanor_keyboard_group {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-client re-exports.
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml"
    );
}

/// The window: its top half red, its bottom half blue, so that a frame read upside down fails.
/// The left button's evdev code.
const BTN_LEFT: u32 = 0x110;
const WIDTH: i32 = 200;
const HEIGHT: i32 = 100;

const OUTPUTS: [VirtualOutput; 2] = [
    VirtualOutput {
        width: 1280,
        height: 800,
        scale: 1.0,
    },
    VirtualOutput {
        width: 800,
        height: 600,
        scale: 1.5,
    },
];

/// Pixels as `headless::FRAME_FORMAT` reads them back: R, G, B, A.
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
/// The background, `state::BACKGROUND` in bytes: whole multiples of 1/255, which Pixman and
/// GLES both read back exactly.
fn background(px: &[u8; 4]) -> bool {
    px == &BACKGROUND.map(|c| (c * 255.0).round() as u8)
}

/// How long the compositor waits for the client.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Every global the compositor offers on every backend.
const GLOBALS: [&str; 16] = [
    "wl_compositor",
    "wl_subcompositor",
    "wl_shm",
    "wl_seat",
    "xdg_wm_base",
    "zwlr_layer_shell_v1",
    "zxdg_output_manager_v1",
    "wp_viewporter",
    "wp_single_pixel_buffer_manager_v1",
    "wp_fractional_scale_manager_v1",
    "wp_presentation",
    "wp_cursor_shape_manager_v1",
    "xdg_activation_v1",
    "wl_data_device_manager",
    "zwp_idle_inhibit_manager_v1",
    "zwp_primary_selection_device_manager_v1",
];

#[derive(Default)]
struct Client {
    /// Name, interface and version of each global.
    globals: Vec<(u32, String, u32)>,
    configure: Option<u32>,
    /// The width, height and states of the last configure of a toplevel.
    toplevel_configure: Option<(i32, i32, Vec<u32>)>,
    /// The serial, width and height of the last configure of a layer surface.
    layer_configure: Option<(u32, u32, u32)>,
    /// A layer surface was closed by the compositor.
    layer_closed: bool,
    frame_done: bool,
    /// Roundtrips answered (`Local::roundtrip`).
    synced: u32,
    /// How many `toplevel_states` events had arrived when the last roundtrip was answered.
    states_at_sync: usize,
    /// The windows of the foreign toplevel list.
    listed: Vec<Listed>,
    /// The buffer size of the last capture session, once its constraints are done.
    capture_size: Option<(u32, u32)>,
    /// The capture sessions stopped.
    stopped: Vec<ObjectId>,
    /// The last capture frame: ready, or failed with its reason.
    captured: Option<Result<(), u32>>,
    locked: bool,
    /// The serial, width and height of the last configure of a lock surface.
    lock_configure: Option<(u32, u32, u32)>,
    /// `idled` and `resumed`, in order.
    idle: Vec<&'static str>,
    /// Each workspace group: the outputs and the workspaces that entered it.
    groups: Vec<(ObjectId, Vec<ObjectId>, Vec<ObjectId>)>,
    /// Each workspace and its last state.
    workspaces: Vec<(ObjectId, Option<ext_workspace_handle_v1::State>)>,
    workspaces_done: bool,
    /// The `done` events of the workspace manager.
    workspace_dones: usize,
    /// Each workspace's handle, as the manager announced it.
    workspace_handles: Vec<ext_workspace_handle_v1::ExtWorkspaceHandleV1>,
    /// Each workspace's last coordinates.
    coordinates: Vec<(ObjectId, Vec<u32>)>,
    /// The workspaces the compositor removed.
    workspaces_removed: Vec<ObjectId>,
    /// The leave, removed and done events of ext-workspace-v1, in order.
    workspace_log: Vec<&'static str>,
    /// The evdev code of each key pressed while the client had the keyboard.
    keys: Vec<u32>,
    /// The evdev code of each key released while the client had the keyboard.
    released: Vec<u32>,
    /// The evdev codes of the keys held, as the last `enter` told them.
    entered_keys: Vec<u32>,
    /// The keymaps the client was sent.
    keymaps: usize,
    /// The size of the last keymap the client was sent, which tells two keymaps apart.
    keymap_size: u32,
    /// The size of the keymap each key of `keys` was read under.
    keys_under: Vec<u32>,
    /// The depressed modifiers of each `wl_keyboard.modifiers` the client was sent.
    modifiers: Vec<u32>,
    /// The group of each `wl_keyboard.modifiers` the client was sent.
    modifier_groups: Vec<u32>,
    /// The heads of wlr-output-management, as the client has seen them.
    heads: Vec<OutputHead>,
    /// The serial of the output manager's last `done`.
    output_serial: Option<u32>,
    /// The answer to the last output configuration: `succeeded`, `failed` or `cancelled`.
    configured: Option<&'static str>,
    /// The last preferred scale of wp-fractional-scale, in 120ths.
    fractional_scale: Option<u32>,
    /// The serial of the last `wl_pointer.enter` the client was sent.
    pointer_enter: Option<u32>,
    /// The serial of the last `wl_pointer.button` press the client was sent.
    button: Option<u32>,
    /// The events of every athanor_toplevel_state_v1 object, in order.
    toplevel_states: Vec<(ObjectId, StateEvent)>,
    /// The token of the last xdg_activation_v1 token done.
    activation_token: Option<String>,
    /// The toplevels the compositor asked to close.
    toplevel_closed: Vec<ObjectId>,
    /// The `group` events of every athanor_keyboard_group_v1 object, in order.
    keyboard_groups: Vec<(ObjectId, u32)>,
    /// The events of every zwlr_output_power_v1 object, in order.
    power: Vec<(ObjectId, PowerEvent)>,
}

/// An event of a zwlr_output_power_v1 object.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PowerEvent {
    On,
    Off,
    Failed,
}

/// An event of an athanor_toplevel_state_v1 object.
#[derive(Clone, Debug, PartialEq)]
enum StateEvent {
    State(u32),
    Enter(ObjectId),
    Leave(ObjectId),
    Done,
}

/// A head of wlr-output-management as the client has seen it: its name, its last scale and its
/// modes.
struct OutputHead {
    head: zwlr_output_head_v1::ZwlrOutputHeadV1,
    name: String,
    scale: f64,
    /// Every mode sent for the head, the finished ones included.
    modes: Vec<zwlr_output_mode_v1::ZwlrOutputModeV1>,
}

/// A window of the foreign toplevel list as the client has seen it.
struct Listed {
    handle: ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    title: String,
    app_id: String,
    done: bool,
    closed: bool,
}

/// The user data of a `wl_display.sync` that counts as a roundtrip.
struct Synced;

impl Client {
    fn bind<I>(
        &self,
        registry: &wl_registry::WlRegistry,
        qh: &QueueHandle<Self>,
        version: u32,
    ) -> Result<I, Error>
    where
        I: wayland_client::Proxy + 'static,
        Self: Dispatch<I, ()>,
    {
        let interface = I::interface().name;
        let (name, _, offered) = self
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .ok_or_else(|| format!("no {interface}"))?;
        Ok(registry.bind(*name, version.min(*offered), qh, ()))
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            client.globals.push((name, interface, version));
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Client {
    fn event(
        client: &mut Self,
        _: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            client.configure = Some(serial);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            client.frame_done = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for Client {
    fn event(
        client: &mut Self,
        toplevel: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let states = states
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|state| u32::from_ne_bytes(*state))
                    .collect();
                client.toplevel_configure = Some((width, height, states));
            }
            xdg_toplevel::Event::Close => client.toplevel_closed.push(toplevel.id()),
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => client.layer_configure = Some((serial, width, height)),
            zwlr_layer_surface_v1::Event::Closed => client.layer_closed = true,
            _ => {}
        }
    }
}

delegate_noop!(Client: wl_compositor::WlCompositor);
delegate_noop!(Client: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(Client: wl_shm_pool::WlShmPool);
delegate_noop!(Client: ignore wl_surface::WlSurface);
delegate_noop!(Client: ignore wl_shm::WlShm);
delegate_noop!(Client: ignore wl_buffer::WlBuffer);
delegate_noop!(Client: xdg_positioner::XdgPositioner);
delegate_noop!(Client: ignore xdg_popup::XdgPopup);
delegate_noop!(Client: ignore wl_seat::WlSeat);
delegate_noop!(Client: ignore wl_output::WlOutput);
delegate_noop!(Client: zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1);

impl Dispatch<zwlr_output_power_v1::ZwlrOutputPowerV1, ()> for Client {
    fn event(
        client: &mut Self,
        object: &zwlr_output_power_v1::ZwlrOutputPowerV1,
        event: zwlr_output_power_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            zwlr_output_power_v1::Event::Mode {
                mode: WEnum::Value(zwlr_output_power_v1::Mode::On),
            } => PowerEvent::On,
            zwlr_output_power_v1::Event::Mode { .. } => PowerEvent::Off,
            zwlr_output_power_v1::Event::Failed => PowerEvent::Failed,
            _ => return,
        };
        client.power.push((object.id(), event));
    }
}
delegate_noop!(Client: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(Client: ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(Client: ext_image_capture_source_v1::ExtImageCaptureSourceV1);
delegate_noop!(Client: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1);
delegate_noop!(Client: ext_session_lock_manager_v1::ExtSessionLockManagerV1);
delegate_noop!(Client: ext_idle_notifier_v1::ExtIdleNotifierV1);
delegate_noop!(Client: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1);
delegate_noop!(Client: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1);
delegate_noop!(Client: wp_security_context_manager_v1::WpSecurityContextManagerV1);
delegate_noop!(Client: wp_security_context_v1::WpSecurityContextV1);
delegate_noop!(Client: ignore zwlr_output_mode_v1::ZwlrOutputModeV1);
delegate_noop!(Client: zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1);
delegate_noop!(Client: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(Client: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
delegate_noop!(Client: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);
delegate_noop!(Client: athanor_toplevel::athanor_toplevel_manager_v1::AthanorToplevelManagerV1);
delegate_noop!(Client: athanor_toplevel::athanor_toplevel_actions_v1::AthanorToplevelActionsV1);
delegate_noop!(Client: xdg_activation_v1::XdgActivationV1);
delegate_noop!(
    Client: athanor_keyboard_group::athanor_keyboard_group_manager_v1::AthanorKeyboardGroupManagerV1
);

impl Dispatch<athanor_keyboard_group::athanor_keyboard_group_v1::AthanorKeyboardGroupV1, ()>
    for Client
{
    fn event(
        client: &mut Self,
        object: &athanor_keyboard_group::athanor_keyboard_group_v1::AthanorKeyboardGroupV1,
        event: athanor_keyboard_group::athanor_keyboard_group_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let athanor_keyboard_group::athanor_keyboard_group_v1::Event::Group { group } = event;
        client.keyboard_groups.push((object.id(), group));
    }
}

impl Dispatch<athanor_toplevel::athanor_toplevel_state_v1::AthanorToplevelStateV1, ()> for Client {
    fn event(
        client: &mut Self,
        object: &athanor_toplevel::athanor_toplevel_state_v1::AthanorToplevelStateV1,
        event: athanor_toplevel::athanor_toplevel_state_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_toplevel::athanor_toplevel_state_v1::Event;
        let event = match event {
            Event::State {
                state: WEnum::Value(state),
            } => StateEvent::State(state.bits()),
            Event::State {
                state: WEnum::Unknown(state),
            } => StateEvent::State(state),
            Event::OutputEnter { output } => StateEvent::Enter(output.id()),
            Event::OutputLeave { output } => StateEvent::Leave(output.id()),
            Event::Done => StateEvent::Done,
        };
        client.toplevel_states.push((object.id(), event));
    }
}

impl Dispatch<xdg_activation_token_v1::XdgActivationTokenV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &xdg_activation_token_v1::XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token } = event {
            client.activation_token = Some(token);
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                client.keys.push(key);
                client.keys_under.push(client.keymap_size);
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Released),
                ..
            } => client.released.push(key),
            wl_keyboard::Event::Enter { keys, .. } => {
                let (keys, _) = keys.as_chunks::<4>();
                client.entered_keys = keys.iter().copied().map(u32::from_ne_bytes).collect();
            }
            wl_keyboard::Event::Keymap { size, .. } => {
                client.keymaps += 1;
                client.keymap_size = size;
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                group,
                ..
            } => {
                client.modifiers.push(mods_depressed);
                client.modifier_groups.push(group);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { serial, .. } => client.pointer_enter = Some(serial),
            wl_pointer::Event::Button {
                serial,
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => client.button = Some(serial),
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, Synced> for Client {
    fn event(
        client: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &Synced,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        client.synced += 1;
        client.states_at_sync = client.toplevel_states.len();
    }
}

impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            client.listed.push(Listed {
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                done: false,
                closed: false,
            });
        }
    }

    event_created_child!(Client, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        let Some(listed) = client.listed.iter_mut().find(|l| &l.handle == handle) else {
            return;
        };
        match event {
            Event::Title { title } => listed.title = title,
            Event::AppId { app_id } => listed.app_id = app_id,
            Event::Done => listed.done = true,
            Event::Closed => listed.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, ()> for Client {
    fn event(
        client: &mut Self,
        session: &ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => client.capture_size = Some((width, height)),
            Event::Stopped => client.stopped.push(session.id()),
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Ready => client.captured = Some(Ok(())),
            Event::Failed { reason } => {
                let reason = match reason {
                    WEnum::Value(reason) => u32::from(reason),
                    WEnum::Unknown(reason) => reason,
                };
                client.captured = Some(Err(reason));
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_session_lock_v1::ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_v1::Event::Locked = event {
            client.locked = true;
        }
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            client.lock_configure = Some((serial, width, height));
        }
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => client.idle.push("idled"),
            ext_idle_notification_v1::Event::Resumed => client.idle.push("resumed"),
            _ => {}
        }
    }
}

impl Dispatch<ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_manager_v1::Event;
        match event {
            Event::WorkspaceGroup { workspace_group } => {
                client
                    .groups
                    .push((workspace_group.id(), Vec::new(), Vec::new()));
            }
            Event::Workspace { workspace } => {
                client.workspaces.push((workspace.id(), None));
                client.workspace_handles.push(workspace);
            }
            Event::Done => {
                client.workspaces_done = true;
                client.workspace_dones += 1;
                client.workspace_log.push("done");
            }
            _ => {}
        }
    }

    event_created_child!(Client, ext_workspace_manager_v1::ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        group: &ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_group_handle_v1::Event;
        let Some((_, outputs, workspaces)) = client
            .groups
            .iter_mut()
            .find(|(id, _, _)| *id == group.id())
        else {
            return;
        };
        match event {
            Event::OutputEnter { output } => outputs.push(output.id()),
            Event::WorkspaceEnter { workspace } => workspaces.push(workspace.id()),
            Event::WorkspaceLeave { workspace } => {
                workspaces.retain(|id| *id != workspace.id());
                client.workspace_log.push("workspace_leave");
            }
            Event::Removed => client.workspace_log.push("group removed"),
            _ => {}
        }
    }
}

impl Dispatch<ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        workspace: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_handle_v1::Event;
        match event {
            Event::State {
                state: WEnum::Value(state),
            } => {
                if let Some((_, last)) = client
                    .workspaces
                    .iter_mut()
                    .find(|(id, _)| *id == workspace.id())
                {
                    *last = Some(state);
                }
            }
            Event::Coordinates { coordinates } => {
                let values = coordinates
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| u32::from_ne_bytes(*bytes))
                    .collect();
                client.coordinates.retain(|(id, _)| *id != workspace.id());
                client.coordinates.push((workspace.id(), values));
            }
            Event::Removed => {
                client.workspaces_removed.push(workspace.id());
                client.workspace_log.push("removed");
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_output_manager_v1::ZwlrOutputManagerV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwlr_output_manager_v1::ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => client.heads.push(OutputHead {
                head,
                name: String::new(),
                scale: 0.0,
                modes: Vec::new(),
            }),
            zwlr_output_manager_v1::Event::Done { serial } => client.output_serial = Some(serial),
            _ => {}
        }
    }

    event_created_child!(Client, zwlr_output_manager_v1::ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (zwlr_output_head_v1::ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<zwlr_output_head_v1::ZwlrOutputHeadV1, ()> for Client {
    fn event(
        client: &mut Self,
        head: &zwlr_output_head_v1::ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(seen) = client.heads.iter_mut().find(|seen| &seen.head == head) else {
            return;
        };
        match event {
            zwlr_output_head_v1::Event::Name { name } => seen.name = name,
            zwlr_output_head_v1::Event::Scale { scale } => seen.scale = scale,
            zwlr_output_head_v1::Event::Mode { mode } => seen.modes.push(mode),
            _ => {}
        }
    }

    event_created_child!(Client, zwlr_output_head_v1::ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (zwlr_output_mode_v1::ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<zwlr_output_configuration_v1::ZwlrOutputConfigurationV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwlr_output_configuration_v1::ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        client.configured = match event {
            zwlr_output_configuration_v1::Event::Succeeded => Some("succeeded"),
            zwlr_output_configuration_v1::Event::Failed => Some("failed"),
            zwlr_output_configuration_v1::Event::Cancelled => Some("cancelled"),
            _ => None,
        };
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            client.fractional_scale = Some(scale);
        }
    }
}

/// A new file, already unlinked, for a client's buffers.
fn unlinked() -> Result<std::fs::File, Error> {
    let path = std::env::temp_dir().join(format!(
        "athanor-compositor-fixture-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    Ok(file)
}

/// An unlinked file holding the window's pixels, XRGB8888 in memory order B, G, R, X.
fn pixels() -> Result<std::fs::File, Error> {
    let mut file = unlinked()?;
    let row = |bgrx: [u8; 4]| bgrx.repeat(WIDTH as usize);
    for y in 0..HEIGHT {
        let colour = if y < HEIGHT / 2 {
            [0, 0, 255, 255]
        } else {
            [255, 0, 0, 255]
        };
        file.write_all(&row(colour))?;
    }
    Ok(file)
}

/// The client's side: lists the globals, maps the window, waits for its first frame callback,
/// reports the globals' interfaces and stays connected until `quit` is dropped, sending a
/// request at each message on it.
fn client(
    stream: UnixStream,
    report: &mpsc::Sender<Result<Vec<String>, String>>,
    quit: &mpsc::Receiver<()>,
) -> Result<(), Error> {
    let connection = Connection::from_socket(stream)?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let mut state = Client::default();
    queue.roundtrip(&mut state)?;

    let compositor: wl_compositor::WlCompositor = state.bind(&registry, &qh, 4)?;
    let shm: wl_shm::WlShm = state.bind(&registry, &qh, 1)?;
    let wm_base: xdg_wm_base::XdgWmBase = state.bind(&registry, &qh, 1)?;
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("fixture".into());
    surface.commit();
    while state.configure.is_none() {
        queue.blocking_dispatch(&mut state)?;
    }
    xdg.ack_configure(state.configure.ok_or("no configure")?);

    let file = pixels()?;
    let pool = shm.create_pool(file.as_fd(), WIDTH * HEIGHT * 4, &qh, ());
    let buffer = pool.create_buffer(
        0,
        WIDTH,
        HEIGHT,
        WIDTH * 4,
        wl_shm::Format::Xrgb8888,
        &qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, WIDTH, HEIGHT);
    surface.frame(&qh, ());
    surface.commit();
    while !state.frame_done {
        queue.blocking_dispatch(&mut state)?;
    }

    report.send(Ok(state
        .globals
        .iter()
        .map(|(_, i, _)| i.clone())
        .collect()))?;
    // The window lives while the connection does; the compositor reads its frames meanwhile.
    // A message asks for a request, which wakes the compositor to read from its clients; the
    // compositor drops its sender to say it is done, which ends the wait.
    while quit.recv().is_ok() {
        connection.display().sync(&qh, ());
        connection.flush()?;
    }
    Ok(())
}

/// The pixel at `(x, y)` of a frame `width` pixels wide.
fn pixel(frame: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    let mut px = [0; 4];
    px.copy_from_slice(&frame[at..at + 4]);
    px
}

/// The compositor on two outputs, and a client that has mapped its window and had its frame.
struct Running {
    event_loop: EventLoop<'static, State>,
    state: State,
    /// The interfaces of the globals the client saw.
    globals: Vec<String>,
    /// A message makes the client send a request; dropped, it lets the client go.
    quit: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
    /// The executable and cgroup of the next client to connect: the bar's at the start.
    identity: Arc<Mutex<(&'static str, &'static str)>>,
    /// The file its executable opens: the bar's at the start.
    executable_file: Arc<Mutex<FileId>>,
    /// How many times the compositor has read the file a client's executable opens.
    executable_stats: Arc<AtomicUsize>,
}

/// The bar, as the fixture's clients connect by default: its executable and its cgroup.
const BAR: (&str, &str) = (
    "/usr/bin/athanor-bar",
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service\n",
);

/// The control center, as a fixture's client connects when a test names it: its executable and
/// its cgroup.
const CONTROL_CENTER: (&str, &str) = (
    "/usr/bin/athanor-control-center",
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-control-center.service\n",
);

/// What the fixture grants: the bar every privileged global but the output manager, so that the
/// tests of the earlier tasks run as the bar; the control center, as its real line does, its top
/// layers and the output manager.
const FIXTURE_ALLOW_LIST: &str = "/usr/bin/athanor-bar athanor-bar.service zwlr_layer_shell_v1 \
    ext_foreign_toplevel_list_v1 ext_foreign_toplevel_image_capture_source_manager_v1 \
    ext_output_image_capture_source_manager_v1 ext_image_copy_capture_manager_v1 \
    ext_session_lock_manager_v1 ext_workspace_manager_v1 zwp_virtual_keyboard_manager_v1 \
    zwlr_virtual_pointer_manager_v1 zwp_input_method_manager_v2 athanor_toplevel_manager_v1 \
    athanor_toplevel_actions_v1 athanor_keyboard_group_manager_v1 \
    zwlr_output_power_manager_v1\n\
    /usr/bin/athanor-control-center athanor-control-center.service zwlr_layer_shell_v1 \
    zwlr_output_manager_v1 athanor_toplevel_manager_v1\n";

/// The bar's executable file, as the compositor sees it.
const BAR_FILE: FileId = FileId { dev: 1, ino: 1 };

/// The control center's executable file, as the compositor sees it.
const CONTROL_CENTER_FILE: FileId = FileId { dev: 1, ino: 3 };

/// The process at the other end of the fixture's sockets, which is the test's own, as whatever
/// executable, file and cgroup the test sets: the pidfd is read as the compositor reads it. The
/// compositor's file system holds the bar's and the control center's executables only.
struct Fake(
    Arc<Mutex<(&'static str, &'static str)>>,
    Arc<Mutex<FileId>>,
    Arc<AtomicUsize>,
);

impl Process for Fake {
    fn executable(&self, _pid: i32) -> std::io::Result<PathBuf> {
        Ok(self.0.lock().expect("identity").0.into())
    }
    fn executable_file(&self, _pid: i32) -> std::io::Result<FileId> {
        self.2.fetch_add(1, Ordering::SeqCst);
        Ok(*self.1.lock().expect("file"))
    }
    fn cgroup(&self, _pid: i32) -> std::io::Result<String> {
        Ok(self.0.lock().expect("identity").1.into())
    }
    fn file(&self, path: &Path) -> std::io::Result<FileId> {
        if path == Path::new(BAR.0) {
            Ok(BAR_FILE)
        } else if path == Path::new(CONTROL_CENTER.0) {
            Ok(CONTROL_CENTER_FILE)
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
}

impl Running {
    /// Starts the compositor on `node`, or on Pixman, and runs it until the client has had
    /// its frame.
    fn start(node: Option<&Path>) -> Self {
        let mut event_loop: EventLoop<State> = EventLoop::try_new().expect("event loop");
        let display: Display<State> = Display::new().expect("display");
        let mut state =
            headless::start(&mut event_loop, display, node, &OUTPUTS).expect("headless backend");

        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        let identity = Arc::new(Mutex::new(BAR));
        let executable_file = Arc::new(Mutex::new(BAR_FILE));
        let executable_stats = Arc::new(AtomicUsize::new(0));
        state.allow_list = AllowList::parse(FIXTURE_ALLOW_LIST).expect("allow-list");
        state.process = Box::new(Fake(
            identity.clone(),
            executable_file.clone(),
            executable_stats.clone(),
        ));
        let client_state = state.client_state(&server_end, None);
        state
            .display_handle
            .insert_client(server_end, client_state)
            .expect("client");
        let (globals_tx, globals_rx) = mpsc::channel();
        let (quit, quit_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            if let Err(error) = client(client_end, &globals_tx, &quit_rx) {
                // The compositor's loop below stops on this.
                drop(globals_tx.send(Err(error.to_string())));
            }
        });

        let started = Instant::now();
        let globals = loop {
            event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut state)
                .expect("dispatch");
            match globals_rx.try_recv() {
                Ok(result) => break result.expect("the client failed"),
                Err(mpsc::TryRecvError::Empty) => {
                    assert!(
                        started.elapsed() < TIMEOUT,
                        "the client had no frame in time"
                    )
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("the client ended without a result")
                }
            }
        };
        Self {
            event_loop,
            state,
            globals,
            quit,
            thread,
            identity,
            executable_file,
            executable_stats,
        }
    }

    /// Runs the compositor until `done` holds of its state.
    fn until(&mut self, what: &str, done: impl Fn(&State) -> bool) {
        let started = Instant::now();
        while !done(&self.state) {
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            self.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut self.state)
                .expect("dispatch");
        }
    }

    /// An xdg_activation_v1 token that `client` asks for with `serial`, done.
    fn activation_token(&mut self, client: &mut Local, serial: u32) -> String {
        let activation: xdg_activation_v1::XdgActivationV1 = client.bind(1);
        let seat: wl_seat::WlSeat = client.bind(1);
        let token = activation.get_activation_token(&client.queue.handle(), ());
        token.set_serial(serial, &seat);
        client.client.activation_token = None;
        token.commit();
        client.pump(self, "the token is done", |client, _| {
            client.activation_token.is_some()
        });
        client.client.activation_token.take().expect("a token")
    }

    fn stop(self) {
        drop(self.quit);
        self.thread.join().expect("client thread");
    }
}

/// Starts the compositor on `node`, or on Pixman, with two outputs and one client, and checks
/// the globals, the window and the frames of both outputs.
fn fixture(node: Option<&Path>) {
    let mut run = Running::start(node);
    let (globals, state) = (&run.globals, &mut run.state);

    for global in GLOBALS {
        assert!(globals.iter().any(|g| g == global), "{global} missing");
    }
    let count = |name: &str| globals.iter().filter(|g| *g == name).count();
    assert_eq!(count("wl_output"), OUTPUTS.len());
    // linux-dmabuf and explicit sync come with a render node, and syncobj only when it can wait.
    assert_eq!(count("zwp_linux_dmabuf_v1"), usize::from(node.is_some()));
    assert_eq!(
        count("wp_linux_drm_syncobj_manager_v1"),
        usize::from(state.syncobj_state.is_some())
    );
    eprintln!(
        "explicit sync on this backend: {}",
        state.syncobj_state.is_some()
    );

    assert_eq!(state.space.elements().count(), 1, "one window");
    let window = state.space.elements().next().expect("the window").clone();
    let area = state.space.element_geometry(&window).expect("mapped");
    assert_eq!((area.size.w, area.size.h), (WIDTH, HEIGHT));

    // Output 0 at scale 1: the window centred across and an eighth down, red above blue, the
    // background around it.
    let first = state.capture(0).expect("frame of output 0");
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    assert_eq!(first.len(), (w * h * 4) as usize);
    let (cx, cy) = (w / 2, h / 8 + HEIGHT / 2);
    assert_eq!(pixel(&first, w, cx, cy - HEIGHT / 4), RED, "top half");
    assert_eq!(pixel(&first, w, cx, cy + HEIGHT / 4), BLUE, "bottom half");
    for (x, y, place) in [(0, 0, "corner"), (cx, cy - HEIGHT, "above the window")] {
        let px = pixel(&first, w, x, y);
        assert!(background(&px), "{place}: {px:?}");
    }

    // Output 1, at scale 1.5 to the right of output 0, shows no window.
    let second = state.capture(1).expect("frame of output 1");
    let (w, h) = (OUTPUTS[1].width, OUTPUTS[1].height);
    assert_eq!(second.len(), (w * h * 4) as usize);
    assert!(
        second.as_chunks::<4>().0.iter().all(background),
        "output 1 is background only"
    );

    run.stop();
}

/// A commit held on a fence that has not signalled: a one-plane dmabuf on a pipe with nothing
/// to read, which polls as a fence still pending until a byte is written.
fn hold(state: &mut State, surface: &WlSurface) -> std::io::PipeWriter {
    let (reader, writer) = std::io::pipe().expect("pipe");
    let mut builder = Dmabuf::builder(
        (1, 1),
        Fourcc::Argb8888,
        Modifier::Linear,
        DmabufFlags::empty(),
    );
    assert!(builder.add_plane(OwnedFd::from(reader), 0, 4));
    let dmabuf = builder.build().expect("dmabuf");
    let (blocker, source) = dmabuf
        .generate_blocker(Interest::READ)
        .expect("an empty pipe is a pending fence");
    state.hold_commit(surface, blocker, source);
    writer
}

/// A client holds commits on fences up to the bound; a fence that signals frees its place; the
/// commit past the bound is not held but disconnects the client, and its held commits go with it.
#[test]
fn a_client_holding_too_many_commits_is_disconnected() {
    let mut run = Running::start(None);
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let surface = window.toplevel().expect("a toplevel").wl_surface().clone();

    let mut fences: Vec<_> = (0..MAX_HELD_COMMITS)
        .map(|_| hold(&mut run.state, &surface))
        .collect();
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS);

    fences[0].write_all(&[0]).expect("signal a fence");
    run.until("a signalled fence frees its commit", |state| {
        state.held.len() == MAX_HELD_COMMITS - 1
    });
    fences.push(hold(&mut run.state, &surface));
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS);
    assert!(surface.is_alive(), "within the bound the client stays");

    fences.push(hold(&mut run.state, &surface));
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS, "past the bound");
    // The client is killed, not gone: the compositor drops it the next time it reads from its
    // clients, which a request from the client, still connected, makes it do.
    run.quit.send(()).expect("the client is listening");
    run.until("the client past the bound is disconnected", |state| {
        !surface.is_alive() && state.held.is_empty()
    });
    run.stop();
}

#[test]
fn headless_on_pixman() {
    fixture(None);
}

/// The same on a GPU: `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn headless_on_a_render_node() {
    let node = PathBuf::from(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    );
    fixture(Some(&node));
}

/// The background is Calmo's dark `bg1` as the generator derives it from tokens.toml; the
/// generator's own check keeps the sheet in step with the tokens.
#[test]
fn the_background_is_calmo_bg1() {
    let sheet = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../system/athanor-style/calmo/generated/css/calmo-dark.css");
    let sheet = std::fs::read_to_string(sheet).expect("calmo-dark.css");
    let hex = sheet
        .lines()
        .find_map(|line| {
            line.strip_prefix("@define-color ath_bg1 #")?
                .strip_suffix(';')
        })
        .expect("ath_bg1 in calmo-dark.css");
    let bytes = BACKGROUND.map(|c| (c * 255.0).round() as u8);
    assert_eq!(
        format!("{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2]),
        hex
    );
}
/// What the test asks its second client, the shell's stand-in, to do next.
enum Step {
    /// A top-layer surface along the output's top edge, 50 high, with an exclusive zone of 50
    /// and exclusive keyboard interactivity.
    Exclusive,
    /// A window, placed in what the zone leaves.
    Window,
    /// A top-layer surface along the output's bottom edge, 40 high, that takes the keyboard on
    /// demand.
    OnDemand,
    /// The exclusive surface commits a null buffer.
    UnmapExclusive,
    /// The exclusive surface opens a popup that grabs the seat with this serial.
    Popup(u32),
    /// A top-layer bar along the output's top edge, 30 high, with no exclusive zone, that never
    /// takes the keyboard.
    Bar,
    /// The bar opens a popup that grabs the seat with this serial.
    BarPopup(u32),
    /// The exclusive surface commits again, unchanged; the sender is told once the compositor
    /// has handled it.
    Recommit(mpsc::Sender<()>),
    /// The exclusive surface, unmapped, is mapped again as it was first; the sender is told
    /// once the compositor has handled it.
    RemapExclusive(mpsc::Sender<()>),
    /// The sender is told, once the compositor has handled every step before, the keys the
    /// shell's client was sent.
    Keys(mpsc::Sender<Vec<u32>>),
}

/// The shell's stand-in: its connection's queue and state, and the globals it binds.
struct Shell {
    queue: wayland_client::EventQueue<Client>,
    client: Client,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    wm_base: xdg_wm_base::XdgWmBase,
    seat: wl_seat::WlSeat,
    /// The buffers' files, open while their buffers are.
    files: Vec<std::fs::File>,
}

impl Shell {
    /// A buffer of `width` by `height` pixels, black.
    fn buffer(&mut self, width: i32, height: i32) -> Result<wl_buffer::WlBuffer, Error> {
        let qh = &self.queue.handle();
        let file = unlinked()?;
        file.set_len(u64::try_from(width * height * 4)?)?;
        let pool = self
            .shm
            .create_pool(file.as_fd(), width * height * 4, qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Xrgb8888,
            qh,
            (),
        );
        pool.destroy();
        self.files.push(file);
        Ok(buffer)
    }

    /// Maps a layer surface `height` high along the `anchor` edge of the first output.
    fn layer(
        &mut self,
        namespace: &str,
        anchor: zwlr_layer_surface_v1::Anchor,
        height: u32,
        zone: i32,
        keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Result<
        (
            wl_surface::WlSurface,
            zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        ),
        Error,
    > {
        let qh = self.queue.handle();
        let surface = self.compositor.create_surface(&qh, ());
        let layer = self.layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            namespace.into(),
            &qh,
            (),
        );
        self.map_layer(&surface, &layer, anchor, height, zone, keyboard)?;
        Ok((surface, layer))
    }

    /// Maps the layer surface `layer` of `surface`, new or unmapped, which forgets its state:
    /// its state, a commit, the configure acknowledged and a black buffer of its size.
    fn map_layer(
        &mut self,
        surface: &wl_surface::WlSurface,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        anchor: zwlr_layer_surface_v1::Anchor,
        height: u32,
        zone: i32,
        keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Result<(), Error> {
        layer.set_layer(zwlr_layer_shell_v1::Layer::Top);
        layer.set_anchor(anchor);
        layer.set_size(0, height);
        layer.set_exclusive_zone(zone);
        layer.set_keyboard_interactivity(keyboard);
        self.client.layer_configure = None;
        surface.commit();
        while self.client.layer_configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        let (serial, width, height) = self.client.layer_configure.ok_or("no configure")?;
        layer.ack_configure(serial);
        let buffer = self.buffer(i32::try_from(width)?, i32::try_from(height)?)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }

    /// Maps a popup of the fixture's size on the layer surface `parent`, grabbing the seat with
    /// `serial`.
    fn popup(
        &mut self,
        parent: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        serial: u32,
    ) -> Result<(), Error> {
        let qh = self.queue.handle();
        let positioner = self.wm_base.create_positioner(&qh, ());
        positioner.set_size(WIDTH, HEIGHT);
        positioner.set_anchor_rect(0, 0, 1, 1);
        let surface = self.compositor.create_surface(&qh, ());
        let xdg = self.wm_base.get_xdg_surface(&surface, &qh, ());
        let popup = xdg.get_popup(None, &positioner, &qh, ());
        parent.get_popup(&popup);
        popup.grab(&self.seat, serial);
        self.client.configure = None;
        surface.commit();
        while self.client.configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        xdg.ack_configure(self.client.configure.ok_or("no configure")?);
        let buffer = self.buffer(WIDTH, HEIGHT)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }

    /// Maps a window of the fixture's size.
    fn window(&mut self) -> Result<(), Error> {
        let qh = self.queue.handle();
        let surface = self.compositor.create_surface(&qh, ());
        let xdg = self.wm_base.get_xdg_surface(&surface, &qh, ());
        xdg.get_toplevel(&qh, ());
        self.client.configure = None;
        surface.commit();
        while self.client.configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        xdg.ack_configure(self.client.configure.ok_or("no configure")?);
        let buffer = self.buffer(WIDTH, HEIGHT)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }
}

/// The shell's stand-in: does each step it is sent, until the sender is dropped.
fn shell_client(stream: UnixStream, steps: &mpsc::Receiver<Step>) -> Result<(), Error> {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity};
    let connection = Connection::from_socket(stream)?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let mut client = Client::default();
    queue.roundtrip(&mut client)?;
    let mut shell = Shell {
        compositor: client.bind(&registry, &qh, 4)?,
        shm: client.bind(&registry, &qh, 1)?,
        layer_shell: client.bind(&registry, &qh, 4)?,
        wm_base: client.bind(&registry, &qh, 1)?,
        seat: client.bind(&registry, &qh, 1)?,
        files: Vec::new(),
        queue,
        client,
    };
    let _keyboard = shell.seat.get_keyboard(&qh, ());
    let mut exclusive = None;
    let mut bar = None;
    while let Ok(step) = steps.recv() {
        match step {
            Step::Exclusive => {
                exclusive = Some(shell.layer(
                    "exclusive",
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    50,
                    50,
                    KeyboardInteractivity::Exclusive,
                )?);
            }
            Step::Window => shell.window()?,
            Step::OnDemand => {
                shell.layer(
                    "on-demand",
                    Anchor::Bottom | Anchor::Left | Anchor::Right,
                    40,
                    0,
                    KeyboardInteractivity::OnDemand,
                )?;
            }
            Step::UnmapExclusive => {
                let (surface, _) = exclusive.as_ref().ok_or("no exclusive surface")?;
                surface.attach(None, 0, 0);
                surface.commit();
            }
            Step::Popup(serial) => {
                let (_, layer) = exclusive.clone().ok_or("no exclusive surface")?;
                shell.popup(&layer, serial)?;
            }
            Step::Bar => {
                bar = Some(shell.layer(
                    "bar",
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    30,
                    0,
                    KeyboardInteractivity::None,
                )?);
            }
            Step::BarPopup(serial) => {
                let (_, layer) = bar.clone().ok_or("no bar")?;
                shell.popup(&layer, serial)?;
            }
            Step::Recommit(done) => {
                let (surface, _) = exclusive.as_ref().ok_or("no exclusive surface")?;
                surface.commit();
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(()).map_err(|_| "the test is gone")?;
            }
            Step::RemapExclusive(done) => {
                let (surface, layer) = exclusive.clone().ok_or("no exclusive surface")?;
                shell.map_layer(
                    &surface,
                    &layer,
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    50,
                    50,
                    KeyboardInteractivity::Exclusive,
                )?;
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(()).map_err(|_| "the test is gone")?;
            }
            Step::Keys(done) => {
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(shell.client.keys.clone())
                    .map_err(|_| "the test is gone")?;
            }
        }
        connection.flush()?;
    }
    Ok(())
}

/// The keyboard follows the layer surfaces' interactivity (register row 1 of CO3): an
/// exclusive top-layer surface takes it while mapped, and a window mapped meanwhile neither
/// takes it nor overlaps the surface's exclusive zone; a click on an on-demand surface does
/// not take it from the exclusive one; unmapping the exclusive surface hands it back to the
/// top window; a click then gives it to the on-demand surface.
#[test]
fn layer_surfaces_take_and_give_back_the_keyboard() {
    let mut run = Running::start(None);
    let first = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    run.state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client");
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    let layer = |state: &State, namespace: &str| {
        let output = state.space.outputs().next()?;
        let map = layer_map_for_output(output);
        let layer = map.layers().find(|layer| layer.namespace() == namespace)?;
        Some(layer.wl_surface().clone())
    };

    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        let exclusive = layer(state, "exclusive");
        exclusive.is_some() && focus(state) == exclusive
    });
    let exclusive = layer(&run.state, "exclusive");

    // Placed in what the zone leaves: an eighth of its 750 below the zone, at y 143 rather
    // than 100.
    steps.send(Step::Window).expect("the shell's client");
    let (x, y) = ((OUTPUTS[0].width - WIDTH) / 2, 50 + 750 / 8);
    run.until("the window is placed below the zone", |state| {
        state
            .space
            .elements()
            .any(|w| *w != first && state.space.element_location(w) == Some((x, y).into()))
    });
    assert_eq!(focus(&run.state), exclusive, "the window does not take it");
    let second = run
        .state
        .space
        .elements()
        .find(|w| **w != first)
        .expect("the second window")
        .clone();
    let second = second.toplevel().expect("a toplevel").wl_surface().clone();

    let bottom = Point::from((
        f64::from(OUTPUTS[0].width) / 2.0,
        f64::from(OUTPUTS[0].height) - 20.0,
    ));
    steps.send(Step::OnDemand).expect("the shell's client");
    run.until("the on-demand surface is under the pointer", |state| {
        matches!(state.under(bottom), Some((Under::Layer(l), _)) if l.namespace() == "on-demand")
    });
    run.state.click_focus(bottom);
    assert_eq!(
        focus(&run.state),
        exclusive,
        "a click does not take it from the exclusive surface"
    );

    steps
        .send(Step::UnmapExclusive)
        .expect("the shell's client");
    run.until("unmapping hands the keyboard back", |state| {
        focus(state).as_ref() == Some(&second)
    });

    run.state.click_focus(bottom);
    assert_eq!(
        focus(&run.state),
        layer(&run.state, "on-demand"),
        "a click on demand takes it"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// An exclusive surface's own popup keeps the keyboard grab it was given: the surface
/// committing again, as a bar does each frame, does not take the keyboard back from the popup
/// and end the grab.
#[test]
fn an_exclusive_surface_keeps_the_keyboard_in_its_own_popup() {
    let mut run = Running::start(None);
    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        focus(state)
            .is_some_and(|surface| surface.client().map(|c| c.id()) == Some(shell_id.clone()))
    });
    let exclusive = focus(&run.state).expect("the exclusive surface");

    // A key pressed while the exclusive surface holds the keyboard, as the bar's popover opens.
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    let in_popup = |state: &State| {
        focus(state).is_some_and(|surface| {
            surface != exclusive && state.popups.find_popup(&surface).is_some()
        })
    };
    run.until("the popup's grab takes the keyboard", in_popup);
    let popup = focus(&run.state);

    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::Recommit(done))
        .expect("the shell's client");
    run.until("the recommit is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(focus(&run.state), popup, "the popup keeps the keyboard");
    assert!(
        run.state
            .seat
            .get_keyboard()
            .is_some_and(|k| k.is_grabbed()),
        "the popup's grab goes on"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// A click on a bar, whose interactivity is `none`, opens a popup that grabs the pointer but
/// not the keyboard: the grab's end would hand the keyboard to the bar, which refuses it, so
/// the window the user types in keeps it.
#[test]
fn a_bar_popup_leaves_the_keyboard_to_the_window() {
    use smithay::input::pointer::MotionEvent;
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    steps.send(Step::Bar).expect("the shell's client");
    let top = Point::from((f64::from(OUTPUTS[0].width) / 2.0, 15.0));
    run.until(
        "the bar is under the pointer",
        |state| matches!(state.under(top), Some((Under::Layer(l), _)) if l.namespace() == "bar"),
    );
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    let under = run.state.surface_under(top);
    let motion = MotionEvent {
        location: top,
        serial: SERIAL_COUNTER.next_serial(),
        time: smithay::backend::input::InputTime::now(),
    };
    pointer.motion(&mut run.state, under, &motion);
    pointer.frame(&mut run.state);

    // A button pressed on the bar, while the window holds the keyboard.
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.button = Some((serial, shell_id));
    steps
        .send(Step::BarPopup(serial.into()))
        .expect("the shell's client");
    run.until("the popup's grab takes the pointer", |_| {
        pointer.is_grabbed()
    });
    assert_eq!(focus(&run.state), window, "the window keeps the keyboard");
    assert!(
        run.state
            .seat
            .get_keyboard()
            .is_some_and(|k| !k.is_grabbed()),
        "the grab leaves the keyboard alone"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// Presses the keys of evdev codes `codes` in order and releases them in reverse, as the
/// backend hands them to the seat.
fn chord(state: &mut State, codes: &[u32]) {
    use smithay::backend::input::{InputTime, KeyState};
    // An xkb keycode is the evdev code plus 8.
    for code in codes {
        state.key_input(
            (code + 8).into(),
            KeyState::Pressed,
            InputTime::now(),
            KeyboardSource::Physical,
        );
    }
    for code in codes.iter().rev() {
        state.key_input(
            (code + 8).into(),
            KeyState::Released,
            InputTime::now(),
            KeyboardSource::Physical,
        );
    }
}

/// Left Super and Escape, as evdev numbers them.
const ESCAPE_CHORD: [u32; 2] = [125, 1];

/// The keys the shell's client was sent, once the compositor has handled every step before.
fn shell_keys(run: &mut Running, steps: &mpsc::Sender<Step>) -> Vec<u32> {
    let (sent, keys) = mpsc::channel();
    steps.send(Step::Keys(sent)).expect("the shell's client");
    let got = std::cell::RefCell::new(None);
    run.until("the shell's client tells its keys", |_| {
        if let Ok(keys) = keys.try_recv() {
            got.replace(Some(keys));
        }
        got.borrow().is_some()
    });
    got.into_inner().expect("the keys")
}

/// The key the compositor keeps for itself (C1 task 8): Super+Escape takes the keyboard back at
/// the press from a granted client's exclusive surface, and ends its popup's grab, with no lock
/// client to take the lock, and asks for the lock; the client never sees the Escape. The escape
/// holds for the identity the grant went to: its surface committing again, as a bar does each
/// frame, mapped again, a new one, or one of a second connection does not take the keyboard
/// back. A click on a layer surface of the client that takes the keyboard gives it back; one on
/// its window, or on its bar, which takes none, does not. With no exclusive surface the key asks
/// for the lock all the same and is not the window's; while the session is locked it is the lock
/// surface's, and asks for nothing. Super+Shift+Escape, the log-out key, asks for no lock. A
/// lock alone escapes nobody.
#[test]
fn task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock() {
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });
    let shell_has_it = {
        let id = shell_id.clone();
        move |state: &State| {
            focus(state).is_some_and(|surface| surface.client().map(|c| c.id()) == Some(id.clone()))
        }
    };

    // A popup of the exclusive surface, opened with the keyboard's grab: the key ends the grab.
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", &shell_has_it);
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    run.until("the popup's grab takes the keyboard", |state| {
        state.seat.get_keyboard().is_some_and(|k| k.is_grabbed())
    });
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    chord(&mut run.state, &ESCAPE_CHORD);
    assert_eq!(run.state.lock_requests, 1, "Super+Escape asks for the lock");
    // No lock client runs, as in a session with none, or logind failed: the keyboard is back at
    // the press, and the popup's grab is over.
    assert_eq!(
        focus(&run.state),
        window,
        "the press takes the keyboard back"
    );
    assert!(!keyboard.is_grabbed(), "the press ends the popup's grab");
    assert_eq!(
        run.state.escaped.len(),
        1,
        "the identity whose surface held the keyboard is escaped"
    );
    // A second connection of the same identity maps an exclusive surface: it does not take the
    // keyboard either.
    {
        let mut second = Local::connect(&mut run, None);
        let compositor: wl_compositor::WlCompositor = second.bind(4);
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = second.bind(4);
        let qh = second.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            "second".into(),
            &qh,
            (),
        );
        layer.set_size(100, 100);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
        assert!(second.answer(&mut run, &surface), "the surface is mapped");
        assert!(
            run.state.exclusive_layer().is_none(),
            "a second connection of an escaped identity holds no exclusive surface"
        );
        assert_eq!(focus(&run.state), window, "and does not take the keyboard");
    }
    // The escape does not decay when the identity's last connection goes: it ends only by a click.
    run.until("the second connection is gone", |state| {
        state.escaped.len() == 1
    });
    // A lock client takes the lock that follows: it changes nothing for the escape.
    let mut local = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let lock = locks.lock(&local.queue.handle(), ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    lock.unlock_and_destroy();
    local.pump(
        &mut run,
        "the unlock gives the window the keyboard",
        |_, state| focus(state) == window,
    );
    assert!(
        run.state.kept_keys.is_empty(),
        "each key the compositor kept is released"
    );
    assert_eq!(
        shell_keys(&mut run, &steps),
        [ESCAPE_CHORD[0]],
        "the client saw Super go down, never the Escape"
    );

    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::Recommit(done))
        .expect("the shell's client");
    run.until("the recommit is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(focus(&run.state), window, "a commit does not take it back");
    steps
        .send(Step::UnmapExclusive)
        .expect("the shell's client");
    // The unmap handled before the client maps the surface again: a configure the compositor
    // answered the unmap with would be the first the client reads after its new initial commit.
    run.until("the unmap is handled", |state| {
        state.space.outputs().any(|output| {
            layer_map_for_output(output).layers().any(|layer| {
                layer.namespace() == "exclusive" && !crate::layer::mapped(layer.wl_surface())
            })
        })
    });
    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::RemapExclusive(done))
        .expect("the shell's client");
    run.until("the remap is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(
        focus(&run.state),
        window,
        "mapped again, it does not take it back"
    );
    steps.send(Step::Exclusive).expect("the shell's client");
    shell_keys(&mut run, &steps);
    assert_eq!(focus(&run.state), window, "a new surface does not either");

    // No exclusive surface holds the keyboard: Super+Escape asks for the lock all the same, and
    // is not the window's; Super+Shift+Escape asks for no lock.
    use smithay::backend::input::{InputTime, KeyState};
    use smithay::input::keyboard::Keycode;
    let [logo, escape] = ESCAPE_CHORD.map(|code| Keycode::from(code + 8));
    let now = InputTime::now;
    let requests = run.state.lock_requests;
    run.state
        .key_input(logo, KeyState::Pressed, now(), KeyboardSource::Physical);
    let logo_sent = run.state.grab_serials.key_press.clone();
    run.state
        .key_input(escape, KeyState::Pressed, now(), KeyboardSource::Physical);
    let escape_sent = run.state.grab_serials.key_press.clone();
    run.state
        .key_input(escape, KeyState::Released, now(), KeyboardSource::Physical);
    run.state
        .key_input(logo, KeyState::Released, now(), KeyboardSource::Physical);
    assert_eq!(escape_sent, logo_sent, "the Escape is not sent on");
    assert_eq!(
        run.state.lock_requests,
        requests + 1,
        "it asks for the lock"
    );
    let shift = Keycode::from(42u32 + 8);
    run.state
        .key_input(logo, KeyState::Pressed, now(), KeyboardSource::Physical);
    run.state
        .key_input(shift, KeyState::Pressed, now(), KeyboardSource::Physical);
    let shift_sent = run.state.grab_serials.key_press.clone();
    run.state
        .key_input(escape, KeyState::Pressed, now(), KeyboardSource::Physical);
    let escape_sent = run.state.grab_serials.key_press.clone();
    for key in [escape, shift, logo] {
        run.state
            .key_input(key, KeyState::Released, now(), KeyboardSource::Physical);
    }
    assert_eq!(
        escape_sent, shift_sent,
        "Super+Shift+Escape is the log-out key, not the window's"
    );
    assert_eq!(
        run.state.fired.last(),
        Some(&crate::bindings::Action::Run(&crate::session_calls::LOG_OUT))
    );
    assert_eq!(
        run.state.lock_requests,
        requests + 1,
        "and asks for nothing"
    );

    // A click on a window of the client, or on its surface that takes no keyboard, does not
    // end the escape.
    steps.send(Step::Window).expect("the shell's client");
    steps.send(Step::Bar).expect("the shell's client");
    shell_keys(&mut run, &steps);
    let shell_window = run
        .state
        .space
        .elements()
        .find(|window| {
            window
                .toplevel()
                .and_then(|toplevel| toplevel.wl_surface().client())
                .map(|client| client.id())
                == Some(shell_id.clone())
        })
        .cloned()
        .expect("the shell's window");
    let window_area = run
        .state
        .space
        .element_geometry(&shell_window)
        .expect("mapped");
    let output = run.state.space.outputs().next().expect("an output").clone();
    let bar_area = {
        let map = layer_map_for_output(&output);
        let bar = map
            .layers()
            .find(|layer| layer.namespace() == "bar")
            .expect("the bar");
        let mut area = map.layer_geometry(bar).expect("placed");
        area.loc += run
            .state
            .space
            .output_geometry(&output)
            .expect("placed")
            .loc;
        area
    };
    for (area, what) in [(window_area, "its window"), (bar_area, "its bar")] {
        let centre = area.to_f64().loc
            + Point::from((f64::from(area.size.w) / 2.0, f64::from(area.size.h) / 2.0));
        run.state.click_focus(centre);
        assert_eq!(
            run.state.escaped.len(),
            1,
            "a click on {what} keeps the escape"
        );
    }

    // A click on the client's surface that takes the keyboard gives it back.
    run.state
        .click_focus(Point::from((f64::from(OUTPUTS[0].width) / 2.0, 25.0)));
    assert!(run.state.escaped.is_empty(), "a click ends the escape");
    assert!(
        focus(&run.state).is_some() && focus(&run.state) == run.state.exclusive_layer(),
        "a click gives it back"
    );

    // Locked, the key is the lock surface's and asks for nothing. The exclusive surface held the
    // keyboard when the lock was taken and nobody pressed the key: it has it again at the
    // unlock.
    let mut local = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let lock = locks.lock(&local.queue.handle(), ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    let requests = run.state.lock_requests;
    chord(&mut run.state, &ESCAPE_CHORD);
    assert_eq!(
        run.state.lock_requests, requests,
        "the key asks for nothing while locked"
    );
    assert!(run.state.escaped.is_empty(), "a lock alone escapes nobody");
    lock.unlock_and_destroy();
    local.pump(
        &mut run,
        "the unlock gives the exclusive surface the keyboard",
        |_, state| shell_has_it(state),
    );
    assert!(
        !shell_keys(&mut run, &steps).contains(&ESCAPE_CHORD[1]),
        "the client never saw the Escape"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// A virtual-terminal switch kept a key press, and the release comes on the other terminal: the
/// session pausing forgets the record, so the next release of that key reaches the client.
#[test]
fn task8_a_pause_forgets_the_keys_the_compositor_kept() {
    use smithay::backend::input::{InputTime, KeyState};
    use smithay::input::keyboard::Keycode;
    let mut run = Running::start(None);
    let [ctrl, alt, f1] = [29u32, 56, 59].map(|code| Keycode::from(code + 8));
    let now = InputTime::now;
    for key in [ctrl, alt, f1] {
        run.state
            .key_input(key, KeyState::Pressed, now(), KeyboardSource::Physical);
    }
    assert_eq!(run.state.kept_keys, [f1], "Ctrl+Alt+F1 is kept");
    run.state.pause();
    assert!(run.state.kept_keys.is_empty(), "the pause forgets it");
    run.state.grab_serials.key_release = None;
    run.state
        .key_input(f1, KeyState::Released, now(), KeyboardSource::Physical);
    assert!(
        run.state.grab_serials.key_release.is_some(),
        "a later release of F1 reaches the client"
    );
    run.stop();
}

/// A failure the backend cannot go on after ends the loop with the reason, which `main` turns
/// into the exit status.
#[test]
fn task8_a_fatal_failure_is_kept_for_main() {
    let mut run = Running::start(None);
    run.state.fail("the GPU was removed".into());
    run.state.fail("a second reason".into());
    assert_eq!(run.state.fatal.as_deref(), Some("the GPU was removed"));
    run.stop();
}

/// An output that goes takes no window with it (C1 task 8): a window only it showed is centred
/// on the first output.
#[test]
fn task8_a_window_on_an_output_that_goes_moves_onto_the_first() {
    let mut run = Running::start(None);
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let centred = run
        .state
        .window_area()
        .map(|area| crate::state::centred(area, window.geometry()));
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .expect("the second output")
        .clone();
    let area = run
        .state
        .space
        .output_geometry(&second)
        .expect("the second output's area");
    run.state
        .space
        .relocate_element(&window, area.loc + Point::from((10, 10)));
    run.state.remove_output(&second);
    assert_eq!(
        run.state.space.element_location(&window),
        centred,
        "centred on the first output"
    );
    run.stop();
}

/// An output that takes another size (C1 task 8): the outputs side by side again leave no window
/// outside them and no pointer outside them.
#[test]
fn task8_an_output_that_shrinks_strands_no_window_and_no_pointer() {
    let mut run = Running::start(None);
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let mut outputs = run.state.space.outputs().cloned();
    let (first, second) = (
        outputs.next().expect("the first output"),
        outputs.next().expect("the second output"),
    );
    drop(outputs);
    let area = run.state.space.output_geometry(&second).expect("placed");
    let inside = area.loc + Point::from((area.size.w - 20, area.size.h - 20));
    run.state.space.relocate_element(&window, inside);
    run.state
        .pointer_to(inside.to_f64(), smithay::backend::input::InputTime::now());
    let small = smithay::output::Mode {
        size: (200, 150).into(),
        refresh: 60_000,
    };
    first.change_current_state(Some(small), None, None, None);
    run.state.arrange_outputs();
    let shown = |area: smithay::utils::Rectangle<i32, smithay::utils::Logical>| {
        run.state
            .space
            .outputs()
            .filter_map(|output| run.state.space.output_geometry(output))
            .any(|output| output.overlaps(area))
    };
    let moved = run
        .state
        .space
        .element_geometry(&window)
        .expect("the window is mapped");
    assert!(shown(moved), "the window is on an output");
    let at = run
        .state
        .seat
        .get_pointer()
        .expect("a pointer")
        .current_location();
    assert!(
        run.state
            .space
            .outputs()
            .filter_map(|output| run.state.space.output_geometry(output))
            .any(|output| output.to_f64().contains(at)),
        "the pointer is on an output"
    );
    run.stop();
}

/// A client in the test's own thread, for the protocols of C1 task 4: the test runs the
/// compositor's loop and the client's queue in turn (`pump`).
struct Local {
    connection: Connection,
    queue: wayland_client::EventQueue<Client>,
    registry: wl_registry::WlRegistry,
    client: Client,
    /// Its id in the compositor, when the fixture inserted it.
    id: Option<ClientId>,
    /// The buffers' files, open while their buffers are.
    files: Vec<std::fs::File>,
    /// The seat and its pointer, once `toplevel` has bound them.
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    /// The surface of the window `toplevel` mapped.
    window: Option<(wl_surface::WlSurface, xdg_surface::XdgSurface)>,
}

impl Local {
    /// Connects to the compositor, inside `context` if any, and lists the globals.
    fn connect(run: &mut Running, context: Option<SecurityContext>) -> Self {
        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        let data = run.state.client_state(&server_end, context);
        let id = run
            .state
            .display_handle
            .insert_client(server_end, data)
            .expect("client")
            .id();
        Self::over(run, client_end, Some(id))
    }

    /// A client on `stream`, connected already, and its id in the compositor if the fixture
    /// inserted it; lists the globals.
    fn over(run: &mut Running, stream: UnixStream, id: Option<ClientId>) -> Self {
        let connection = Connection::from_socket(stream).expect("connection");
        let queue = connection.new_event_queue();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let mut local = Self {
            connection,
            queue,
            registry,
            client: Client::default(),
            id,
            files: Vec::new(),
            seat: None,
            pointer: None,
            window: None,
        };
        local.roundtrip(run);
        local
    }

    /// Binds the seat and its pointer, maps a `WIDTH`x`HEIGHT` window and gives it the keyboard.
    /// The fixture's own window holds the keyboard, so `place` leaves the new window at the
    /// bottom, exactly under the fixture's: raised, the new window is the one under the pointer,
    /// and `fixture_window`, the bottom element, is the fixture's again.
    fn toplevel(&mut self, run: &mut Running) -> xdg_toplevel::XdgToplevel {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shm: wl_shm::WlShm = self.bind(1);
        let wm_base: xdg_wm_base::XdgWmBase = self.bind(1);
        let seat: wl_seat::WlSeat = self.bind(1);
        let qh = self.queue.handle();
        self.pointer = Some(seat.get_pointer(&qh, ()));
        self.seat = Some(seat);
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        self.client.configure = None;
        surface.commit();
        self.pump(run, "the window is configured", |client, _| {
            client.configure.is_some()
        });
        xdg.ack_configure(self.client.configure.expect("a configure"));
        let (buffer, _) = self.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        let id = self.id.clone().expect("an id");
        // The window is in the space from its first commit; it is placed once it has a size.
        self.pump(run, "the window is placed", |_, state| {
            state
                .space
                .elements()
                .any(|window| owned_by(window, &id) && !window.geometry().is_empty())
        });
        let window = window_of(run, &id);
        run.state.focus(&window);
        // The configure that activates it is not one a test waits for.
        self.roundtrip(run);
        self.client.configure = None;
        self.window = Some((surface, xdg));
        toplevel
    }

    /// Waits for the next configure of the window `toplevel` mapped, acknowledges it and draws
    /// a buffer of the size it names, or of the fixture's size when it leaves the size to the
    /// client: the configure's width, height and states.
    fn redraw(&mut self, run: &mut Running) -> (i32, i32, Vec<u32>) {
        self.pump(run, "the window is configured", |client, _| {
            client.configure.is_some()
        });
        let (surface, xdg) = self.window.clone().expect("a window");
        xdg.ack_configure(self.client.configure.take().expect("a configure"));
        let (width, height, states) = self
            .client
            .toplevel_configure
            .take()
            .expect("a toplevel configure");
        let shm: wl_shm::WlShm = self.bind(1);
        let (buffer, _) = self.buffer(
            &shm,
            if width > 0 { width } else { WIDTH },
            if height > 0 { height } else { HEIGHT },
            [0, 0, 0, 255],
        );
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        self.roundtrip(run);
        (width, height, states)
    }

    /// Maps a bar: a surface on the top layer along the output's top edge, `height` high, with
    /// an exclusive zone of its height.
    fn bar(&mut self, run: &mut Running, height: i32) -> wl_surface::WlSurface {
        use zwlr_layer_surface_v1::Anchor;
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shm: wl_shm::WlShm = self.bind(1);
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind(4);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            "bar".into(),
            &qh,
            (),
        );
        layer.set_anchor(Anchor::Top | Anchor::Left | Anchor::Right);
        layer.set_size(0, height.unsigned_abs());
        layer.set_exclusive_zone(height);
        assert!(self.answer(run, &surface), "the bar is configured");
        let (serial, width, _) = self.client.layer_configure.expect("a configure");
        layer.ack_configure(serial);
        let width = i32::try_from(width).expect("a width");
        let (buffer, _) = self.buffer(&shm, width, height, [0, 0, 0, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        self.roundtrip(run);
        surface
    }

    /// Runs the compositor and the client in turn until `done` holds.
    fn pump(&mut self, run: &mut Running, what: &str, done: impl Fn(&Client, &State) -> bool) {
        let started = Instant::now();
        loop {
            self.connection.flush().expect("flush");
            if let Some(guard) = self.queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("{what}: {error}"),
                }
            }
            self.queue
                .dispatch_pending(&mut self.client)
                .expect("client dispatch");
            if done(&self.client, &run.state) {
                return;
            }
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            run.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut run.state)
                .expect("dispatch");
            // As `main`'s loop does after every dispatch.
            run.state.refresh_idle_inhibit();
        }
    }

    /// Until the compositor has handled every request sent so far.
    fn roundtrip(&mut self, run: &mut Running) {
        let synced = self.client.synced;
        self.connection.display().sync(&self.queue.handle(), Synced);
        self.pump(run, "a roundtrip", |client, _| client.synced > synced);
    }

    fn bind<I>(&self, version: u32) -> I
    where
        I: Proxy + 'static,
        Client: Dispatch<I, ()>,
    {
        self.client
            .bind(&self.registry, &self.queue.handle(), version)
            .expect("bind")
    }

    /// Whether the client sees the global `interface`.
    fn sees(&self, interface: &str) -> bool {
        self.client.globals.iter().any(|(_, i, _)| i == interface)
    }

    /// The interfaces of the globals the client sees.
    fn listed(&self) -> BTreeSet<&str> {
        self.client
            .globals
            .iter()
            .map(|(_, interface, _)| interface.as_str())
            .collect()
    }

    /// A buffer of `width` by `height` pixels in XRGB8888, each `bgrx`, and its file.
    fn buffer(
        &mut self,
        shm: &wl_shm::WlShm,
        width: i32,
        height: i32,
        bgrx: [u8; 4],
    ) -> (wl_buffer::WlBuffer, std::fs::File) {
        let mut file = unlinked().expect("buffer file");
        let pixels = usize::try_from(width * height).expect("size");
        file.write_all(&bgrx.repeat(pixels)).expect("pixels");
        let qh = self.queue.handle();
        let pool = shm.create_pool(file.as_fd(), width * height * 4, &qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Xrgb8888,
            &qh,
            (),
        );
        pool.destroy();
        let kept = file.try_clone().expect("buffer file");
        self.files.push(file);
        (buffer, kept)
    }
}

/// Who receives a global: the classes of `data/globals`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Offered to every client.
    Standard,
    /// Offered to every client outside a security context.
    Unconfined,
    /// Offered to every client; its top and overlay layers only to the allow-list's.
    LayerGated,
    /// Offered only to the allow-list's clients, each by its own line.
    Privileged,
}

/// Every global the compositor can offer, with its class, from `data/globals`.
fn classified() -> Vec<(&'static str, Class)> {
    include_str!("../data/globals")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [interface, class] = fields[..] else {
                panic!("not `<interface> <class>`: {line}");
            };
            let class = match class {
                "standard" => Class::Standard,
                "unconfined" => Class::Unconfined,
                "layer-gated" => Class::LayerGated,
                "privileged" => Class::Privileged,
                other => panic!("{interface}: no class {other}"),
            };
            (interface, class)
        })
        .collect()
}

/// The globals offered only on a render node: the tests below run on Pixman, without one
/// (`fixture` checks them on a node).
const RENDER_NODE_GLOBALS: [&str; 2] = ["zwp_linux_dmabuf_v1", "wp_linux_drm_syncobj_manager_v1"];

/// The globals of `data/globals` whose class `admitted` holds, as Pixman offers them.
fn rows(admitted: impl Fn(Class) -> bool) -> BTreeSet<&'static str> {
    classified()
        .into_iter()
        .filter(|(interface, class)| admitted(*class) && !RENDER_NODE_GLOBALS.contains(interface))
        .map(|(interface, _)| interface)
        .collect()
}

/// A security context as Flatpak creates one, made by the fixture's own client.
fn sandbox(run: &Running) -> SecurityContext {
    let (_, creator) = fixture_window(run);
    SecurityContext {
        sandbox_engine: Some("org.flatpak".into()),
        app_id: Some("org.example.App".into()),
        instance_id: None,
        creator_client_id: creator,
    }
}

/// The fixture's window and the id of its client.
fn fixture_window(run: &Running) -> (smithay::desktop::Window, ClientId) {
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let id = window
        .toplevel()
        .and_then(|t| t.wl_surface().client())
        .expect("the fixture's client")
        .id();
    (window, id)
}

/// The fixture's window is placed as cosmic-comp places a new floating window: centred across,
/// and an eighth of the output's height down rather than centred (#738).
#[test]
fn the_first_window_is_placed_as_cosmic_comp_places_it() {
    let run = Running::start(None);
    let (window, _) = fixture_window(&run);
    assert_eq!(
        run.state.space.element_location(&window),
        Some(((OUTPUTS[0].width - WIDTH) / 2, OUTPUTS[0].height / 8).into())
    );
    run.stop();
}

/// At scale 1.5 a window is placed in the layer map's 853 logical pixels of the first output's
/// 1280, not the space's 854: the chooser's 420 at 216, as on cosmic-comp's golden
/// chooser-light-1.5-en (#738).
#[test]
fn a_window_is_placed_in_the_layer_maps_area_at_a_fractional_scale() {
    let mut run = Running::start(None);
    let first = run
        .state
        .space
        .outputs()
        .next()
        .expect("the first output")
        .clone();
    first.change_current_state(
        None,
        None,
        Some(smithay::output::Scale::Fractional(1.5)),
        None,
    );
    layer_map_for_output(&first).arrange();
    run.state.arrange_outputs();
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();
    local.pump(&mut run, "the initial configure", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.take().expect("a configure"));
    // A shadow of 15 by 12 around the 420 px window, as GTK draws the chooser's.
    let (buffer, _) = local.buffer(&shm, 450, HEIGHT + 24, [0, 0, 0, 255]);
    xdg.set_window_geometry(15, 12, 420, HEIGHT);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(&mut run);
    let window = local.server_window(&run, &surface);
    // 853 / 2 - 420 / 2 across; 533 / 8 down, above 533 / 2 - 100 / 2. The window's
    // geometry is there, its buffer 15 by 12 up and to the left.
    assert_eq!(
        run.state.space.element_location(&window),
        Some((216, 66).into())
    );
    run.stop();
}

/// Whether `window` belongs to the client `id`.
fn owned_by(window: &smithay::desktop::Window, id: &ClientId) -> bool {
    window
        .toplevel()
        .and_then(|t| t.wl_surface().client())
        .is_some_and(|client| client.id() == *id)
}

/// The mapped window of the client `id`.
fn window_of(run: &Running, id: &ClientId) -> smithay::desktop::Window {
    run.state
        .space
        .elements()
        .find(|window| owned_by(window, id))
        .expect("the client's window")
        .clone()
}

/// Where the window of the client `id` is in the space.
fn location_of(run: &Running, id: &ClientId) -> Point<i32, Logical> {
    run.state
        .space
        .element_location(&window_of(run, id))
        .expect("a location")
}

/// Whether a `MoveGrab` holds the pointer: any press installs Smithay's click grab, so
/// `is_grabbed` cannot tell a move from a click.
fn moving(run: &Running) -> bool {
    run.state
        .seat
        .get_pointer()
        .and_then(|pointer| pointer.with_grab(|_, grab| grab.is::<MoveGrab>()))
        .unwrap_or(false)
}

/// Moves the pointer to `at` and presses the left button.
fn press(run: &mut Running, at: Point<f64, Logical>) {
    run.state.pointer_to(at, InputTime::now());
    run.state
        .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
}

/// Releases the left button.
fn release(run: &mut Running) {
    run.state
        .button_input(BTN_LEFT, ButtonState::Released, InputTime::now());
}

/// A client's window, moved on the press it was sent: the client's id and where the press was.
fn moved(run: &mut Running, local: &mut Local) -> (ClientId, Point<f64, Logical>) {
    let toplevel = local.toplevel(run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let at = location_of(run, &id).to_f64() + Point::from((20.0, 10.0));
    press(run, at);
    local.pump(run, "the press", |client, _| client.button.is_some());
    toplevel._move(&seat, local.client.button.expect("a press"));
    local.roundtrip(run);
    assert!(moving(run), "the client's own press grants the move");
    (id, at)
}

/// A move is granted only for the last press, sent to the client that asks, while it is held
/// (OS12).
#[test]
fn a_move_needs_the_clients_own_button_press() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let at = location_of(&run, &id).to_f64() + Point::from((20.0, 10.0));

    // A serial no press sent the client.
    toplevel._move(&seat, SERIAL_COUNTER.next_serial().into());
    local.roundtrip(&mut run);
    assert!(!moving(&run), "a serial no press sent is refused");

    // An older press's serial.
    press(&mut run, at);
    release(&mut run);
    local.pump(&mut run, "the first press", |client, _| {
        client.button.is_some()
    });
    let older = local.client.button.take().expect("a press");
    press(&mut run, at);
    local.pump(&mut run, "the second press", |client, _| {
        client.button.is_some()
    });
    toplevel._move(&seat, older);
    local.roundtrip(&mut run);
    assert!(!moving(&run), "an older press's serial is refused");

    // The current press's serial, recorded as sent to another client.
    let current = local.client.button.expect("a press");
    let (_, other) = fixture_window(&run);
    assert_ne!(other, id, "the fixture's window belongs to another client");
    run.state.grab_serials.button = Some((current.into(), other));
    toplevel._move(&seat, current);
    local.roundtrip(&mut run);
    assert!(!moving(&run), "a press sent to another client is refused");

    // The current press, sent to this client: granted.
    run.state.grab_serials.button = Some((current.into(), id));
    toplevel._move(&seat, current);
    local.roundtrip(&mut run);
    assert!(moving(&run), "the client's own press grants the move");
    run.stop();
}

/// The window follows the pointer until the button is up, and stays (OS12).
#[test]
fn a_window_follows_the_pointer_while_moved() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let before = location_of(&run, &id);
    run.state
        .pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());
    assert_eq!(location_of(&run, &id), before + Point::from((100, 50)));
    release(&mut run);
    assert!(!moving(&run), "the release ends the move");
    run.state.pointer_to(at, InputTime::now());
    assert_eq!(
        location_of(&run, &id),
        before + Point::from((100, 50)),
        "the window stays"
    );
    run.stop();
}

/// A window dragged off the area keeps its top edge, and 32 pixels across, inside it (OS12).
#[test]
fn a_moved_window_keeps_its_top_edge_reachable() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let area = run.state.window_area().expect("an area");
    run.state
        .pointer_to(at - Point::from((10_000.0, 10_000.0)), InputTime::now());
    let location = location_of(&run, &id);
    assert_eq!(location.y, area.loc.y, "the top edge stays in the area");
    let width = window_of(&run, &id).geometry().size.w;
    assert!(
        location.x + width >= area.loc.x + 32,
        "32 pixels stay in the area"
    );
    run.stop();
}

/// A window dragged from the first output to the middle of the second follows the pointer
/// there, unclamped, and the release puts it on the second output's active workspace (#832).
#[test]
fn dragging_a_window_reaches_the_second_output() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let before = location_of(&run, &id);
    let second = output_at(&run, 1);
    let shown = run.state.space.output_geometry(&second).expect("placed");
    let target = (shown.loc + Point::from((shown.size.w / 2, shown.size.h / 2))).to_f64();
    run.state.pointer_to(target, InputTime::now());
    assert_eq!(
        location_of(&run, &id),
        before + (target - at).to_i32_round(),
        "the window follows the pointer onto the second output"
    );
    // Below the second output, which is shorter than the first, the pointer is on no output:
    // the second, nearer than the first, still keeps the window.
    let gap = Point::from((target.x, f64::from(shown.loc.y + shown.size.h) + 100.0));
    run.state.pointer_to(gap, InputTime::now());
    let kept = location_of(&run, &id);
    assert_eq!(
        kept.x,
        (before + (gap - at).to_i32_round()).x,
        "in the gap, the second output keeps the window"
    );
    assert!(
        kept.y < shown.loc.y + shown.size.h,
        "its top edge stays on the second output"
    );
    run.state.pointer_to(target, InputTime::now());
    release(&mut run);
    assert_eq!(
        run.state.workspace_of(&window_of(&run, &id)),
        run.state.active_workspace(&second),
        "dropped on the second output's active workspace"
    );
    run.stop();
}

/// A popup of a window on the second output is kept inside that output, not the first: one
/// opened at the window's right edge, by the output's right edge, slides back onto it (#845).
#[test]
fn a_popup_is_kept_inside_its_windows_output() {
    use xdg_positioner::{Anchor, ConstraintAdjustment, Gravity};
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let parent = local.another(&mut run, "parent");
    let window = local.server_window(&run, &parent.surface);
    let second = output_at(&run, 1);
    let shown = run.state.space.output_geometry(&second).expect("placed");
    let at = shown.loc + Point::from((shown.size.w - WIDTH - 10, 10));
    run.state.space.relocate_element(&window, at);
    run.state.settle(&window);
    assert_eq!(
        run.state.workspace_of(&window),
        run.state.active_workspace(&second),
        "the window is on the second output"
    );

    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let positioner = wm_base.create_positioner(&qh, ());
    positioner.set_size(WIDTH, HEIGHT);
    positioner.set_anchor_rect(WIDTH - 1, 0, 1, 1);
    positioner.set_anchor(Anchor::TopRight);
    positioner.set_gravity(Gravity::BottomRight);
    positioner.set_constraint_adjustment(ConstraintAdjustment::SlideX);
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    xdg.get_popup(Some(&parent.xdg), &positioner, &qh, ());
    local.client.configure = None;
    surface.commit();
    local.pump(&mut run, "the popup is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.take().expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(&mut run);

    let root = window.toplevel().expect("a toplevel").wl_surface().clone();
    let (_, offset) = smithay::desktop::PopupManager::popups_for_surface(&root)
        .next()
        .expect("the popup");
    let geometry = run.state.space.element_geometry(&window).expect("shown");
    let popup = smithay::utils::Rectangle::new(geometry.loc + offset, (WIDTH, HEIGHT).into());
    assert!(
        shown.contains_rect(popup),
        "the popup {popup:?} stays on the second output {shown:?}"
    );
    run.stop();
}

/// The lock ends a move: the pointer goes to the lock, and the window stays (OS12).
#[test]
fn the_lock_ends_a_move() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let before = location_of(&run, &id);
    let mut locker = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let _lock = locks.lock(&locker.queue.handle(), ());
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    assert!(!moving(&run), "the lock ends the move");
    run.state
        .pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());
    assert_eq!(location_of(&run, &id), before, "the window stays");
    run.stop();
}

/// A resize sends configures with the `resizing` state and the size the pointer asks for, and
/// one without it at the release; from the top-left corner the bottom-right one stays (OS12).
#[test]
fn a_resize_sends_resizing_configures() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let resizing = xdg_toplevel::State::Resizing as u32;
    let geometry = |run: &Running| {
        run.state
            .space
            .element_geometry(&window_of(run, &id))
            .expect("mapped")
    };

    let at = location_of(&run, &id).to_f64() + Point::from((20.0, 10.0));
    press(&mut run, at);
    local.pump(&mut run, "the press", |client, _| client.button.is_some());
    let serial = local.client.button.take().expect("a press");
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(&mut run);
    run.state
        .pointer_to(at + Point::from((50.0, 20.0)), InputTime::now());
    let (width, height, states) = local.redraw(&mut run);
    assert!(states.contains(&resizing), "the configure is resizing");
    assert_eq!((width, height), (WIDTH + 50, HEIGHT + 20));
    release(&mut run);
    let (_, _, states) = local.redraw(&mut run);
    assert!(!states.contains(&resizing), "the release ends the resize");

    let before = geometry(&run);
    let at = before.loc.to_f64() + Point::from((5.0, 5.0));
    press(&mut run, at);
    local.pump(&mut run, "the press", |client, _| client.button.is_some());
    let serial = local.client.button.take().expect("a press");
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::TopLeft);
    local.roundtrip(&mut run);
    run.state
        .pointer_to(at - Point::from((30.0, 40.0)), InputTime::now());
    let (width, height, _) = local.redraw(&mut run);
    assert_eq!((width, height), (before.size.w + 30, before.size.h + 40));
    let after = geometry(&run);
    assert_eq!(
        (after.loc.x + after.size.w, after.loc.y + after.size.h),
        (before.loc.x + before.size.w, before.loc.y + before.size.h),
        "the bottom-right corner stays"
    );
    release(&mut run);
    run.stop();
}

/// A window maximised while it is resized ends the resize and takes the area, and the pointer
/// no longer moves it (OS12).
#[test]
fn maximising_ends_a_resize() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let area = run.state.window_area().expect("an area");
    let resizing = |run: &Running| {
        run.state
            .seat
            .get_pointer()
            .and_then(|pointer| pointer.with_grab(|_, grab| grab.is::<ResizeGrab>()))
            .unwrap_or(false)
    };

    let at = location_of(&run, &id).to_f64() + Point::from((20.0, 10.0));
    press(&mut run, at);
    local.pump(&mut run, "the press", |client, _| client.button.is_some());
    let serial = local.client.button.take().expect("a press");
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(&mut run);
    assert!(resizing(&run), "the client's own press grants the resize");
    run.state
        .pointer_to(at + Point::from((50.0, 20.0)), InputTime::now());
    local.redraw(&mut run);

    toplevel.set_maximized();
    local.roundtrip(&mut run);
    assert!(!resizing(&run), "maximising ends the resize");
    let (width, height, states) = local.redraw(&mut run);
    assert!(states.contains(&(xdg_toplevel::State::Maximized as u32)));
    assert_eq!((width, height), (area.size.w, area.size.h));
    assert_eq!(location_of(&run, &id), area.loc);
    run.state
        .pointer_to(at + Point::from((90.0, 60.0)), InputTime::now());
    local.roundtrip(&mut run);
    assert!(
        local.client.configure.is_none(),
        "no resizing configure follows"
    );
    assert_eq!(location_of(&run, &id), area.loc, "the window stays");
    release(&mut run);
    run.stop();
}

/// A maximised window takes the area the bar's zone leaves at the commit that draws that size,
/// is fitted again when the zone changes, and goes back where it was, at its size, when
/// restored (OS12).
#[test]
fn a_window_is_maximised_and_restored() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let before = location_of(&run, &id);
    let area = run.state.window_area().expect("an area");
    let maximized = xdg_toplevel::State::Maximized as u32;

    toplevel.set_maximized();
    let (width, height, states) = local.redraw(&mut run);
    assert!(states.contains(&maximized), "the configure is maximised");
    assert_eq!((width, height), (area.size.w, area.size.h));
    assert_eq!(location_of(&run, &id), area.loc);

    let mut shell = Local::connect(&mut run, None);
    let _bar = shell.bar(&mut run, 32);
    let fitted = run.state.window_area().expect("an area");
    assert_ne!(fitted, area, "the bar's zone changes the area");
    let (width, height, states) = local.redraw(&mut run);
    assert!(states.contains(&maximized), "still maximised");
    assert_eq!((width, height), (fitted.size.w, fitted.size.h));
    assert_eq!(location_of(&run, &id), fitted.loc);

    toplevel.unset_maximized();
    let (width, height, states) = local.redraw(&mut run);
    assert!(
        !states.contains(&maximized),
        "the configure is not maximised"
    );
    assert_eq!((width, height), (WIDTH, HEIGHT));
    assert_eq!(location_of(&run, &id), before);
    run.stop();
}

/// A client with its own window and the pointer over it: the serial of the pointer's enter.
fn pointed_at(run: &mut Running, local: &mut Local) -> u32 {
    local.toplevel(run);
    let id = local.id.clone().expect("an id");
    let at = location_of(run, &id).to_f64() + Point::from((20.0, 10.0));
    run.state.pointer_to(at, InputTime::now());
    local.pump(run, "the pointer enters the window", |client, _| {
        client.pointer_enter.is_some()
    });
    local.client.pointer_enter.expect("an enter")
}

/// A cursor a client sets is shown only while that client holds the pointer's focus: over
/// another client's window, and over the background, the default is drawn (OS15).
#[test]
fn another_clients_cursor_is_not_shown() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let serial = pointed_at(&mut run, &mut local);
    let pointer = local.pointer.clone().expect("a pointer");
    pointer.set_cursor(serial, None, 0, 0);
    local.roundtrip(&mut run);
    let id = local.id.clone().expect("an id");
    assert_eq!(
        run.state.cursor_status,
        (CursorImageStatus::Hidden, Some(id.clone())),
        "a cursor without a surface belongs to the client the pointer is over"
    );
    assert_eq!(
        run.state.shown_cursor(),
        CursorImageStatus::Hidden,
        "over its own window"
    );

    // The client's window moves off the fixture's, which the pointer is then over.
    let area = run.state.window_area().expect("an area");
    run.state
        .space
        .map_element(window_of(&run, &id), area.loc, true);
    let (other, _) = fixture_window(&run);
    let over = run
        .state
        .space
        .element_location(&other)
        .expect("a location");
    run.state
        .pointer_to(over.to_f64() + Point::from((20.0, 10.0)), InputTime::now());
    // Smithay resets the cursor when the focus changes: the client's is put back, as if it had
    // been set late, here and over the background.
    run.state.cursor_status = (CursorImageStatus::Hidden, Some(id.clone()));
    assert_eq!(
        run.state.shown_cursor(),
        CursorImageStatus::default_named(),
        "over another client's window"
    );

    let corner = (area.loc + area.size - Size::from((1, 1))).to_f64();
    assert!(run.state.surface_under(corner).is_none(), "the background");
    run.state.pointer_to(corner, InputTime::now());
    run.state.cursor_status = (CursorImageStatus::Hidden, Some(id));
    assert_eq!(
        run.state.shown_cursor(),
        CursorImageStatus::default_named(),
        "over the background"
    );
    run.stop();
}

/// A cursor surface is sent its frame callbacks while the pointer is over its output, as a
/// window is.
#[test]
fn task_pointer_a_cursor_surface_gets_its_frame_callbacks() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let serial = pointed_at(&mut run, &mut local);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let cursor = compositor.create_surface(&qh, ());
    let (buffer, _) = local.buffer(&shm, 16, 16, [255, 255, 255, 255]);
    let pointer = local.pointer.clone().expect("a pointer");
    pointer.set_cursor(serial, Some(&cursor), 0, 0);
    cursor.attach(Some(&buffer), 0, 0);
    cursor.frame(&qh, ());
    local.client.frame_done = false;
    cursor.commit();
    local.pump(&mut run, "the cursor's frame callback", |client, _| {
        client.frame_done
    });
    run.stop();
}

/// The bar sees every global of `data/globals` once, but `wl_output`, once per output, and
/// `zwlr_output_manager_v1`, which its line does not name, not at all; a client inside a
/// security context sees only the standard and layer-gated ones (spike Q4: the privileged ones
/// are refused to sandboxes).
#[test]
fn task4_standard_globals_are_offered() {
    let mut run = Running::start(None);
    for global in rows(|_| true) {
        let count = run.globals.iter().filter(|g| *g == global).count();
        // The bar is granted neither the output manager nor data control.
        let once = usize::from(!matches!(
            global,
            "zwlr_output_manager_v1" | "ext_data_control_manager_v1" | "zwlr_data_control_manager_v1"
        ));
        let expected = if global == "wl_output" {
            OUTPUTS.len()
        } else {
            once
        };
        assert_eq!(count, expected, "{global}");
    }
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    for (global, class) in classified() {
        let expected = matches!(class, Class::Standard | Class::LayerGated)
            && !RENDER_NODE_GLOBALS.contains(&global);
        assert_eq!(
            sandboxed.sees(global),
            expected,
            "{global} inside a context"
        );
    }
    run.stop();
}

/// A window appears in the foreign toplevel list with its title and app id, follows a new
/// title, and is closed when the window is destroyed; the others stay.
#[test]
fn task4_a_window_is_listed_and_unlisted() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    local.pump(&mut run, "the fixture's window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "fixture")
    });

    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("Probe".into());
    toplevel.set_app_id("org.athanor.Probe".into());
    surface.commit();
    local.pump(&mut run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let named = |title: &'static str| {
        move |client: &Client, _: &State| {
            client
                .listed
                .iter()
                .any(|l| l.done && l.title == title && l.app_id == "org.athanor.Probe")
        }
    };
    local.pump(&mut run, "the window is listed", named("Probe"));

    toplevel.set_title("Probe 2".into());
    local.pump(&mut run, "the new title is listed", named("Probe 2"));

    toplevel.destroy();
    xdg.destroy();
    surface.destroy();
    local.pump(&mut run, "the window is unlisted", |client, _| {
        client
            .listed
            .iter()
            .any(|l| l.title == "Probe 2" && l.closed)
    });
    let fixture = local.client.listed.iter().find(|l| l.title == "fixture");
    assert!(
        fixture.is_some_and(|l| !l.closed),
        "the fixture's window stays"
    );
    run.stop();
}

/// A window is captured into the client's wl_shm buffer, red above blue as it draws itself;
/// locking the session stops the capture, and a new session is refused while locked (OS4).
#[test]
fn task4_a_window_is_captured_and_refused_while_locked() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    local.pump(&mut run, "the fixture's window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "fixture")
    });
    let qh = local.queue.handle();
    let handle = local
        .client
        .listed
        .iter()
        .find(|l| l.title == "fixture")
        .map(|l| l.handle.clone())
        .expect("the fixture's handle");
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    assert_eq!(
        local.client.capture_size,
        Some((WIDTH as u32, HEIGHT as u32))
    );

    let (buffer, file) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, WIDTH, HEIGHT);
    frame.capture();
    local.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(local.client.captured, Some(Ok(())), "the frame is ready");
    let at = |x: i32, y: i32| {
        let mut px = [0; 4];
        let offset = u64::try_from((y * WIDTH + x) * 4).expect("offset");
        file.read_exact_at(&mut px, offset).expect("the buffer");
        px
    };
    // XRGB8888 in memory: B, G, R, X.
    assert_eq!(at(WIDTH / 2, HEIGHT / 4)[..3], [0, 0, 255], "top half red");
    assert_eq!(
        at(WIDTH / 2, HEIGHT * 3 / 4)[..3],
        [255, 0, 0],
        "bottom half blue"
    );
    frame.destroy();

    let _lock = locks.lock(&qh, ());
    let stopped = session.id();
    local.pump(&mut run, "locking stops the session", |client, _| {
        client.stopped.contains(&stopped)
    });
    let refused = copy.create_session(&source, Options::empty(), &qh, ());
    let refused = refused.id();
    local.pump(
        &mut run,
        "a session is refused while locked",
        |client, _| client.stopped.contains(&refused),
    );
    run.stop();
}

/// The lock surface of output 0, as the compositor holds it.
fn surface_on_server(state: &State) -> WlSurface {
    let output = state.space.outputs().next().expect("an output");
    state
        .lock
        .as_ref()
        .and_then(|lock| lock.surface_on(output))
        .expect("a lock surface")
        .clone()
}

/// A window that closes while it is captured stops the session: the next frame fails with
/// `stopped` and the session receives `stopped`, as the failure reason promises.
#[test]
fn task4_a_closed_window_stops_its_capture() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("Probe".into());
    surface.commit();
    local.pump(&mut run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.pump(&mut run, "the window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "Probe")
    });
    let handle = local
        .client
        .listed
        .iter()
        .find(|l| l.title == "Probe")
        .map(|l| l.handle.clone())
        .expect("the window's handle");
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });

    toplevel.destroy();
    xdg.destroy();
    surface.destroy();
    local.roundtrip(&mut run);
    let (target, _) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&target);
    frame.capture();
    let stopped = session.id();
    local.pump(&mut run, "the session is stopped", |client, _| {
        client.stopped.contains(&stopped)
    });
    // ext_image_copy_capture_frame_v1.failure_reason.stopped
    assert_eq!(
        local.client.captured,
        Some(Err(2)),
        "the frame fails stopped"
    );
    run.stop();
}

/// While locked only the lock surface is drawn, on its output, and only it has the keyboard and
/// the pointer, whatever is clicked or focused and though the pointer has not moved; unlocking
/// gives both back to the window.
#[test]
fn task4_the_lock_hides_and_takes_the_keyboard() {
    use smithay::input::pointer::MotionEvent;
    let mut run = Running::start(None);
    let (window, _) = fixture_window(&run);
    let window_surface = window.toplevel().expect("a toplevel").wl_surface().clone();
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).as_ref() == Some(&window_surface)
    });
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    let centre = Point::from((
        f64::from(w) / 2.0,
        f64::from(h) / 8.0 + f64::from(HEIGHT) / 4.0,
    ));
    // The pointer rests on the window, and does not move again.
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    let under = run.state.surface_under(centre);
    let motion = MotionEvent {
        location: centre,
        serial: SERIAL_COUNTER.next_serial(),
        time: smithay::backend::input::InputTime::now(),
    };
    pointer.motion(&mut run.state, under, &motion);
    pointer.frame(&mut run.state);
    assert_eq!(pointer.current_focus().as_ref(), Some(&window_surface));

    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let output: wl_output::WlOutput = local.bind(4);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let lock = locks.lock(&qh, ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    assert_eq!(focus(&run.state), None, "no surface has the keyboard");

    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    local.pump(&mut run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = local.client.lock_configure.expect("a configure");
    assert_eq!(
        (width, height),
        (OUTPUTS[0].width as u32, OUTPUTS[0].height as u32)
    );
    lock_surface.ack_configure(serial);
    let (buffer, _) = local.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let id = local.id.clone().expect("a client the fixture inserted");
    let locker_has_it = move |state: &State| {
        focus(state).and_then(|s| s.client()).map(|c| c.id()) == Some(id.clone())
    };
    local.pump(&mut run, "the lock surface has the keyboard", |_, state| {
        locker_has_it(state)
    });
    // Its buffer committed.
    local.roundtrip(&mut run);
    assert_eq!(
        pointer.current_focus().as_ref(),
        Some(&surface_on_server(&run.state)),
        "a click goes to the lock surface, not the window under it"
    );

    let first = run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        pixel(&first, w, w / 2, h / 8 + HEIGHT / 4),
        [0, 0, 0, 255],
        "the window is hidden"
    );
    let second = run.state.capture(1).expect("frame of output 1");
    assert!(
        second.as_chunks::<4>().0.iter().all(background),
        "an output without a lock surface shows the background only"
    );
    assert!(
        run.state.under(centre).is_none(),
        "nothing under the pointer"
    );
    run.state.click_focus(centre);
    run.state.focus(&window);
    run.state.focus_top();
    assert!(
        locker_has_it(&run.state),
        "the keyboard stays with the lock surface"
    );

    lock.unlock_and_destroy();
    local.pump(&mut run, "unlocking gives the keyboard back", |_, state| {
        focus(state).as_ref() == Some(&window_surface)
    });
    assert_eq!(
        pointer.current_focus().as_ref(),
        Some(&window_surface),
        "and the pointer"
    );
    let first = run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        pixel(&first, w, w / 2, h / 8 + HEIGHT / 4),
        RED,
        "the window is back"
    );
    run.stop();
}

/// The idle notification fires after its timeout, and input resumes it.
#[test]
fn task4_idle_fires_and_input_resets_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = local.bind(1);
    let started = Instant::now();
    let _notification = notifier.get_idle_notification(100, &seat, &local.queue.handle(), ());
    local.pump(&mut run, "the user is idle", |client, _| {
        client.idle == ["idled"]
    });
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "not before the timeout"
    );
    run.state.activity();
    local.pump(&mut run, "input resumes", |client, _| {
        client.idle == ["idled", "resumed"]
    });
    run.stop();
}

/// A client with a shown window that holds an idle inhibitor on it, and a client with a
/// 100 ms idle notification.
fn inhibited_window(
    run: &mut Running,
) -> (
    Local,
    xdg_toplevel::XdgToplevel,
    zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
    Local,
) {
    let mut player = Local::connect(run, None);
    let toplevel = player.toplevel(run);
    let surface = player.window.as_ref().expect("the window").0.clone();
    let inhibitors: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 = player.bind(1);
    let inhibitor = inhibitors.create_inhibitor(&surface, &player.queue.handle(), ());
    player.roundtrip(run);
    let mut daemon = Local::connect(run, None);
    let seat: wl_seat::WlSeat = daemon.bind(1);
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = daemon.bind(1);
    let _notification = notifier.get_idle_notification(100, &seat, &daemon.queue.handle(), ());
    let started = Instant::now();
    daemon.pump(run, "three timeouts", |_, _| {
        started.elapsed() >= Duration::from_millis(300)
    });
    assert!(
        daemon.client.idle.is_empty(),
        "no idle while the inhibiting window is shown"
    );
    (player, toplevel, inhibitor, daemon)
}

/// An inhibitor holds the idle notification off while its window is shown: minimised, the
/// timer runs; restored, it is held again; destroyed, the timer runs.
#[test]
fn an_idle_inhibitor_holds_while_its_window_is_shown() {
    let mut run = Running::start(None);
    let (mut player, toplevel, inhibitor, mut daemon) = inhibited_window(&mut run);
    toplevel.set_minimized();
    player.roundtrip(&mut run);
    daemon.pump(&mut run, "minimised, the user is idle", |client, _| {
        client.idle == ["idled"]
    });
    let window = run.state.minimised[0].0.clone();
    assert!(run.state.unminimise(&window), "restored");
    daemon.pump(&mut run, "restored, the inhibitor holds", |client, _| {
        client.idle == ["idled", "resumed"]
    });
    inhibitor.destroy();
    player.roundtrip(&mut run);
    daemon.pump(&mut run, "without the inhibitor, the user is idle", |client, _| {
        client.idle == ["idled", "resumed", "idled"]
    });
    run.stop();
}

/// An inhibitor on a layer surface counts once the surface draws, not while it is only in the
/// layer map.
#[test]
fn an_idle_inhibitor_on_a_layer_surface_needs_a_buffer() {
    use zwlr_layer_surface_v1::Anchor;
    let mut run = Running::start(None);
    let mut panel = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = panel.bind(4);
    let shm: wl_shm::WlShm = panel.bind(1);
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = panel.bind(4);
    let inhibitors: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 = panel.bind(1);
    let qh = panel.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let layer = shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Bottom,
        "panel".into(),
        &qh,
        (),
    );
    layer.set_anchor(Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer.set_size(0, 20);
    assert!(panel.answer(&mut run, &surface), "the panel is configured");
    let (serial, width, _) = panel.client.layer_configure.expect("a configure");
    layer.ack_configure(serial);
    let _inhibitor = inhibitors.create_inhibitor(&surface, &qh, ());
    panel.roundtrip(&mut run);
    let mut daemon = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = daemon.bind(1);
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = daemon.bind(1);
    let _notification = notifier.get_idle_notification(100, &seat, &daemon.queue.handle(), ());
    daemon.pump(&mut run, "nothing drawn, the user is idle", |client, _| {
        client.idle == ["idled"]
    });
    let width = i32::try_from(width).expect("a width");
    let (buffer, _) = panel.buffer(&shm, width, 20, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    panel.roundtrip(&mut run);
    daemon.pump(&mut run, "drawn, the inhibitor holds", |client, _| {
        client.idle == ["idled", "resumed"]
    });
    run.stop();
}

/// The lock lets the idle timer run under an inhibiting window, and the unlock holds it again;
/// a client that leaves takes its inhibitor with it.
#[test]
fn an_idle_inhibitor_yields_to_the_lock_and_leaves_with_its_client() {
    let mut run = Running::start(None);
    let (player, _toplevel, _inhibitor, mut daemon) = inhibited_window(&mut run);
    let (mut locker, lock) = lock_session(&mut run);
    daemon.pump(&mut run, "locked, the user is idle", |client, _| {
        client.idle == ["idled"]
    });
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    daemon.pump(&mut run, "unlocked, the inhibitor holds", |client, _| {
        client.idle == ["idled", "resumed"]
    });
    drop(player);
    daemon.pump(&mut run, "its client gone, the user is idle", |client, _| {
        client.idle == ["idled", "resumed", "idled"]
    });
    run.stop();
}

/// Each output is a workspace group with one workspace active, that the client's own
/// `wl_output` of that output has entered.
#[test]
fn task4_each_output_has_an_active_workspace() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let qh = local.queue.handle();
    let outputs: Vec<wl_output::WlOutput> = local
        .client
        .globals
        .iter()
        .filter(|(_, interface, _)| interface == "wl_output")
        .map(|(name, _, version)| local.registry.bind(*name, (*version).min(4), &qh, ()))
        .collect();
    assert_eq!(outputs.len(), OUTPUTS.len());
    local.roundtrip(&mut run);
    let _manager: ext_workspace_manager_v1::ExtWorkspaceManagerV1 = local.bind(1);
    local.pump(&mut run, "the workspaces are sent", |client, _| {
        client.workspaces_done
    });

    let client = &local.client;
    assert_eq!(client.groups.len(), OUTPUTS.len(), "one group per output");
    let mut entered: Vec<ObjectId> = Vec::new();
    for (_, outputs_in, workspaces_in) in &client.groups {
        assert_eq!(outputs_in.len(), 1, "a group stands for one output");
        entered.extend(outputs_in.iter().cloned());
        let active = workspaces_in
            .iter()
            .filter(|workspace| {
                client
                    .workspaces
                    .iter()
                    .find(|(id, _)| id == *workspace)
                    .and_then(|(_, state)| *state)
                    == Some(ext_workspace_handle_v1::State::Active)
            })
            .count();
        assert_eq!(active, 1, "one workspace of each group is active");
    }
    let bound: Vec<ObjectId> = outputs.iter().map(Proxy::id).collect();
    assert!(
        bound.iter().all(|id| entered.contains(id)),
        "each output has its group"
    );
    run.stop();
}

/// Output `index` of the space.
fn output_at(run: &Running, index: usize) -> smithay::output::Output {
    run.state
        .space
        .outputs()
        .nth(index)
        .expect("the output")
        .clone()
}

/// The workspaces of output `index`, in order.
fn workspaces_of(run: &Running, index: usize) -> Vec<crate::workspace::WorkspaceId> {
    run.state.workspaces(&output_at(run, index)).to_vec()
}

/// A client of ext-workspace-v1, as the bar binds it, once it has been sent every workspace.
fn workspace_client(
    run: &mut Running,
) -> (Local, ext_workspace_manager_v1::ExtWorkspaceManagerV1) {
    let mut local = Local::connect(run, None);
    let manager = local.bind(1);
    local.pump(run, "the workspaces are sent", |client, _| {
        client.workspaces_done
    });
    (local, manager)
}

/// The workspaces of group `group` as the client last heard them, in the order of their
/// coordinates: each one's id, coordinate and state.
fn heard(
    client: &Client,
    group: usize,
) -> Vec<(ObjectId, u32, Option<ext_workspace_handle_v1::State>)> {
    let (_, _, members) = &client.groups[group];
    let mut heard: Vec<_> = members
        .iter()
        .map(|id| {
            let at = client
                .coordinates
                .iter()
                .find(|(workspace, _)| workspace == id)
                .and_then(|(_, values)| values.first().copied())
                .expect("coordinates");
            let state = client
                .workspaces
                .iter()
                .find(|(workspace, _)| workspace == id)
                .and_then(|(_, state)| *state);
            (id.clone(), at, state)
        })
        .collect();
    heard.sort_by_key(|(_, at, _)| *at);
    heard
}

/// The client's handle of the workspace `id`.
fn workspace_handle(
    client: &Client,
    id: &ObjectId,
) -> ext_workspace_handle_v1::ExtWorkspaceHandleV1 {
    client
        .workspace_handles
        .iter()
        .find(|handle| handle.id() == *id)
        .cloned()
        .expect("the workspace's handle")
}

/// Each output starts with one workspace, empty and active (OS2); the output a window is mapped
/// on keeps an empty one after the window's.
#[test]
fn each_output_starts_with_one_empty_workspace() {
    let mut event_loop: EventLoop<State> = EventLoop::try_new().expect("event loop");
    let display: Display<State> = Display::new().expect("display");
    let state =
        headless::start(&mut event_loop, display, None, &OUTPUTS).expect("headless backend");
    assert_eq!(state.space.outputs().count(), OUTPUTS.len());
    for output in state.space.outputs() {
        let list = state.workspaces(output);
        assert_eq!(list.len(), 1, "one workspace");
        assert_eq!(state.active_workspace(output), Some(list[0]), "active");
    }
    drop(state);

    let run = Running::start(None);
    let (window, _) = fixture_window(&run);
    let first = workspaces_of(&run, 0);
    assert_eq!(first.len(), 2, "the window's workspace, then an empty one");
    assert_eq!(run.state.workspace_of(&window), Some(first[0]));
    assert_eq!(
        run.state.active_workspace(&output_at(&run, 0)),
        Some(first[0])
    );
    assert_eq!(workspaces_of(&run, 1).len(), 1, "the other output's is empty");
    run.stop();
}

/// A window maps on the active workspace of its output; on the last, empty one, it adds an
/// empty one after it, which the clients are sent.
#[test]
fn a_window_on_the_last_workspace_adds_an_empty_one() {
    let mut run = Running::start(None);
    let (mut local, _manager) = workspace_client(&mut run);
    let before = workspaces_of(&run, 0);
    assert_eq!(heard(&local.client, 0).len(), 2);
    local.another(&mut run, "first");
    assert_eq!(workspaces_of(&run, 0), before, "the first workspace was not empty");

    assert!(run.state.activate_workspace(before[1]));
    let second = local.another(&mut run, "second");
    let window = local.server_window(&run, &second.surface);
    assert_eq!(
        run.state.workspace_of(&window),
        Some(before[1]),
        "the window is on the active workspace"
    );
    let after = workspaces_of(&run, 0);
    assert_eq!(after.len(), 3, "an empty one is added");
    assert_eq!(after[..2], before[..]);
    local.roundtrip(&mut run);
    assert_eq!(heard(&local.client, 0).len(), 3, "and sent");
    run.stop();
}

/// `activate` applies at the manager's `commit`: the windows of the workspace left go out of
/// the space, those of the one activated come back where they were, and the clients are sent
/// both states. While the session is locked a commit changes nothing.
#[test]
fn activate_and_commit_switch_the_workspace() {
    use ext_workspace_handle_v1::State as Shown;
    let mut run = Running::start(None);
    let (fixture, _) = fixture_window(&run);
    let (mut local, manager) = workspace_client(&mut run);
    let first = local.another(&mut run, "first");
    let window = local.server_window(&run, &first.surface);
    let at = run.state.space.element_location(&window);
    assert!(at.is_some());
    let output = output_at(&run, 0);
    let list = workspaces_of(&run, 0);
    let order = heard(&local.client, 0);
    assert_eq!(order.len(), 2);
    let (home, empty) = (
        workspace_handle(&local.client, &order[0].0),
        workspace_handle(&local.client, &order[1].0),
    );

    empty.activate();
    local.roundtrip(&mut run);
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[0]),
        "nothing before the commit"
    );
    let dones = local.client.workspace_dones;
    manager.commit();
    local.pump(&mut run, "the switch is sent", |client, _| {
        client.workspace_dones > dones
    });
    assert_eq!(run.state.active_workspace(&output), Some(list[1]));
    assert_eq!(run.state.space.element_location(&window), None, "left");
    assert_eq!(run.state.space.element_location(&fixture), None, "left");
    let now = heard(&local.client, 0);
    assert_eq!(now[0].2, Some(Shown::empty()), "the first is inactive");
    assert_eq!(now[1].2, Some(Shown::Active), "the second active");

    home.activate();
    let dones = local.client.workspace_dones;
    manager.commit();
    local.pump(&mut run, "the switch back is sent", |client, _| {
        client.workspace_dones > dones
    });
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    assert_eq!(
        run.state.space.element_location(&window),
        at,
        "back where it was"
    );
    assert!(run.state.space.element_location(&fixture).is_some());
    let now = heard(&local.client, 0);
    assert_eq!(now[0].2, Some(Shown::Active));
    assert_eq!(now[1].2, Some(Shown::empty()));

    let (_locker, _lock) = lock_session(&mut run);
    empty.activate();
    manager.commit();
    local.roundtrip(&mut run);
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[0]),
        "nothing while the session is locked"
    );
    run.stop();
}

/// A workspace emptied that is neither the last nor the active one is removed, and its handle
/// with it; the active one emptied stays until it is left, as under cosmic-comp.
#[test]
fn an_emptied_workspace_that_is_not_last_is_removed() {
    let mut run = Running::start(None);
    let (mut watcher, _manager) = workspace_client(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert_eq!(list.len(), 2);

    assert!(run.state.move_window_to(&window, list[1]));
    let three = workspaces_of(&run, 0);
    assert_eq!(three.len(), 3, "the last one taken, an empty one is added");
    assert_eq!(three[..2], list[..]);
    assert_eq!(
        run.state.space.element_location(&window),
        None,
        "on an inactive workspace the window leaves the space"
    );
    watcher.roundtrip(&mut run);
    let order = heard(&watcher.client, 0);
    assert_eq!(order.len(), 3);

    assert!(run.state.move_window_to(&window, list[0]));
    assert_eq!(
        workspaces_of(&run, 0),
        [list[0], three[2]],
        "the emptied second workspace is removed"
    );
    assert!(run.state.space.element_location(&window).is_some());
    watcher.roundtrip(&mut run);
    assert!(
        watcher.client.workspaces_removed.contains(&order[1].0),
        "and its handle"
    );

    let last = three[2];
    assert!(run.state.move_window_to(&window, last));
    assert!(run.state.activate_workspace(last));
    assert!(run.state.move_window_to(&window, list[0]));
    let kept = workspaces_of(&run, 0);
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[1], last, "the active workspace stays while it is active");
    assert!(run.state.activate_workspace(list[0]));
    assert_eq!(
        workspaces_of(&run, 0),
        [list[0], kept[2]],
        "and goes once left"
    );
    run.stop();
}

/// Each workspace's coordinates are its place in its group, sent again when one before it is
/// removed.
#[test]
fn coordinates_follow_the_order() {
    let mut run = Running::start(None);
    let (mut watcher, _manager) = workspace_client(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    watcher.roundtrip(&mut run);
    let order = heard(&watcher.client, 0);
    let at: Vec<u32> = order.iter().map(|(_, at, _)| *at).collect();
    assert_eq!(at, [0, 1, 2]);

    assert!(run.state.move_window_to(&window, list[0]));
    watcher.roundtrip(&mut run);
    let after: Vec<(ObjectId, u32)> = heard(&watcher.client, 0)
        .into_iter()
        .map(|(id, at, _)| (id, at))
        .collect();
    assert_eq!(
        after,
        [(order[0].0.clone(), 0), (order[2].0.clone(), 1)],
        "the last takes the removed one's place"
    );
    let second: Vec<u32> = heard(&watcher.client, 1)
        .iter()
        .map(|(_, at, _)| *at)
        .collect();
    assert_eq!(second, [0], "the other output's group has its own");
    run.stop();
}

/// A window of an inactive workspace keeps its thumbnail, drawn from its last buffer, as a
/// minimised one does.
#[test]
fn a_window_on_an_inactive_workspace_can_be_captured() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let thumb = local.another(&mut run, "thumbnail");
    let handle = local.handle(&mut run, "thumbnail");
    let qh = local.queue.handle();
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    let window = local.server_window(&run, &thumb.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.activate_workspace(list[1]));
    assert_eq!(run.state.space.element_location(&window), None);

    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, WIDTH, HEIGHT);
    frame.capture();
    local.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(local.client.captured, Some(Ok(())), "the frame is ready");
    assert_eq!(
        local.client.capture_size,
        Some((WIDTH as u32, HEIGHT as u32))
    );
    run.stop();
}

/// The dock finds a window of an inactive workspace by its handle: activating it, or restoring
/// it minimised, activates its workspace and gives it the keyboard on top.
#[test]
fn a_window_on_an_inactive_workspace_is_found_and_activated() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let first = app.another(&mut run, "first");
    let window = app.server_window(&run, &first.surface);
    let handle = dock.local.handle(&mut run, "first");
    let output = output_at(&run, 0);
    let list = workspaces_of(&run, 0);

    assert!(run.state.activate_workspace(list[1]));
    assert_eq!(run.state.space.element_location(&window), None);
    dock.actions.activate(&handle, &dock.seat);
    dock.local.roundtrip(&mut run);
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    assert_eq!(run.state.space.elements().last(), Some(&window), "raised");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );

    dock.actions.set_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(run.state.is_minimised(&window));
    assert!(run.state.activate_workspace(list[1]));
    dock.actions.unset_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(!run.state.is_minimised(&window), "restored");
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    assert_eq!(run.state.space.elements().last(), Some(&window), "on top");
    assert_eq!(keyboard_focus(&run), Some(surface_of(&window)));
    run.stop();
}

/// The privileged globals the fixture's allow-list grants the bar: every one of
/// `data/globals` its line names.
fn bar_privileges() -> BTreeSet<&'static str> {
    let named: Vec<&str> = FIXTURE_ALLOW_LIST
        .lines()
        .find(|line| line.trim_start().starts_with(BAR.0))
        .expect("the bar's line")
        .split_whitespace()
        .collect();
    rows(|class| class == Class::Privileged)
        .into_iter()
        .filter(|global| named.contains(global))
        .collect()
}

/// Another executable, and another unit: a shell started in an application scope.
const SH: &str = "/usr/bin/sh";
const OTHER_UNIT: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-sh-1.scope\n";

impl Local {
    /// Whether the client sees the privileged globals the bar is granted, all of them (true), or
    /// no privileged global (false); a mix fails.
    fn privileged(&self) -> bool {
        let seen: BTreeSet<&str> = rows(|class| class == Class::Privileged)
            .into_iter()
            .filter(|g| self.sees(g))
            .collect();
        assert!(
            seen.is_empty() || seen == bar_privileges(),
            "some privileged globals but not the bar's: {seen:?}"
        );
        assert!(
            [
                "wl_compositor",
                "zwlr_layer_shell_v1",
                "ext_idle_notifier_v1"
            ]
            .iter()
            .all(|g| self.sees(g)),
            "the standard globals stay"
        );
        !seen.is_empty()
    }

    /// Connects as the control center; the clients connecting after it do as well, until the
    /// test names another identity.
    fn control_center(run: &mut Running) -> Self {
        *run.identity.lock().expect("identity") = CONTROL_CENTER;
        *run.executable_file.lock().expect("file") = CONTROL_CENTER_FILE;
        Self::connect(run, None)
    }

    /// Sends an output configuration on `serial` that enables every head and calls `change` on
    /// the configuration of the head named `name`, applies it and returns the compositor's
    /// answer.
    fn configure(
        &mut self,
        run: &mut Running,
        manager: &zwlr_output_manager_v1::ZwlrOutputManagerV1,
        serial: u32,
        name: &str,
        change: impl Fn(&zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1),
    ) -> &'static str {
        let qh = self.queue.handle();
        let configuration = manager.create_configuration(serial, &qh, ());
        for seen in &self.client.heads {
            let head = configuration.enable_head(&seen.head, &qh, ());
            if seen.name == name {
                change(&head);
            }
        }
        self.client.configured = None;
        configuration.apply();
        self.pump(run, "an answer to the configuration", |client, _| {
            client.configured.is_some()
        });
        configuration.destroy();
        self.client.configured.expect("an answer")
    }

    /// Lists the globals again, on a new registry.
    fn relist(&mut self, run: &mut Running) {
        self.client.globals.clear();
        self.registry = self
            .connection
            .display()
            .get_registry(&self.queue.handle(), ());
        self.roundtrip(run);
    }

    /// A layer surface on `layer`, committed: whether the compositor configures it (true) or
    /// closes it (false).
    fn layer_kept(
        &mut self,
        run: &mut Running,
        layer: zwlr_layer_shell_v1::Layer,
    ) -> (
        bool,
        zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wl_surface::WlSurface,
    ) {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind(4);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(&surface, None, layer, "task5".into(), &qh, ());
        layer.set_size(100, 100);
        (self.answer(run, &surface), layer, surface)
    }

    /// Commits `surface`, a layer surface: whether it is configured (true) or closed (false).
    fn answer(&mut self, run: &mut Running, surface: &wl_surface::WlSurface) -> bool {
        self.client.layer_configure = None;
        self.client.layer_closed = false;
        surface.commit();
        self.pump(run, "a configure or a close", |client, _| {
            client.layer_configure.is_some() || client.layer_closed
        });
        !self.client.layer_closed
    }
}

/// The bar's executable in the bar's unit is granted the privileged globals of the allow-list.
#[test]
fn task5_bar_unit_and_executable_get_privileged_globals() {
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.privileged(), "the bar sees the privileged globals");
    run.stop();
}

/// Another executable in the bar's unit is granted nothing.
#[test]
fn task5_other_executable_in_the_bar_unit_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.identity.lock().expect("identity") = (SH, BAR.1);
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "another executable in the bar's unit");
    run.stop();
}

/// The bar's executable in another unit is granted nothing.
#[test]
fn task5_other_unit_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.identity.lock().expect("identity") = (BAR.0, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "the bar's executable in another unit");
    run.stop();
}

/// A file at the bar's path that is not the bar's file, as a sandbox with its own `/usr` has
/// it, is granted nothing.
#[test]
fn task5_another_file_at_the_bar_s_path_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.executable_file.lock().expect("file") = FileId { dev: 1, ino: 2 };
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "another file at the bar's path");
    run.stop();
}

/// Only a client that names an allow-listed executable and unit has its executable's file read:
/// that read can block on the client's own file system, a stalled FUSE mount, and the
/// compositor with it.
#[test]
fn task5_only_an_allow_listed_name_has_its_file_read() {
    let mut run = Running::start(None);
    let stats = Arc::clone(&run.executable_stats);
    let before = stats.load(Ordering::SeqCst);
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "another executable, another unit"
    );
    *run.identity.lock().expect("identity") = (SH, BAR.1);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "another executable, the bar's unit"
    );
    *run.identity.lock().expect("identity") = (BAR.0, OTHER_UNIT);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "the bar's executable, another unit"
    );
    *run.identity.lock().expect("identity") = BAR;
    Local::connect(&mut run, None);
    assert_eq!(stats.load(Ordering::SeqCst), before + 1, "the bar");
    run.stop();
}

/// The bar itself, connecting through a security context, is granted nothing.
#[test]
fn task5_security_context_gets_standard_globals() {
    let mut run = Running::start(None);
    let (_, creator) = fixture_window(&run);
    let context = SecurityContext {
        sandbox_engine: Some("org.flatpak".into()),
        app_id: Some("org.example.App".into()),
        instance_id: None,
        creator_client_id: creator,
    };
    let sandboxed = Local::connect(&mut run, Some(context));
    for global in rows(|class| class == Class::Privileged) {
        assert!(!sandboxed.sees(global), "{global} inside a context");
    }
    run.stop();
}

/// The bar's own process, connecting through a security context's listener as a sandboxed
/// application does, is granted nothing: the path from `wp_security_context_manager_v1` to the
/// listener's clients.
#[test]
fn task5_a_security_context_listener_gets_standard_globals() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let path = std::env::temp_dir().join(format!("athanor-context-{}.sock", std::process::id()));
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("listen");
    // The compositor listens until the write end closes.
    let (close, _open) = std::io::pipe().expect("pipe");
    let manager: wp_security_context_manager_v1::WpSecurityContextManagerV1 = bar.bind(1);
    let qh = bar.queue.handle();
    let context = manager.create_listener(listener.as_fd(), close.as_fd(), &qh, ());
    context.set_sandbox_engine("org.flatpak".into());
    context.set_app_id("org.example.App".into());
    context.commit();
    bar.roundtrip(&mut run);

    let stream = UnixStream::connect(&path).expect("connect");
    std::fs::remove_file(&path).expect("remove");
    let sandboxed = Local::over(&mut run, stream, None);
    assert!(sandboxed.sees("wl_compositor"), "the core");
    for global in rows(|class| class == Class::Privileged) {
        assert!(!sandboxed.sees(global), "{global} through the listener");
    }
    run.stop();
}

/// The identity is the one read at connect: a client that connected as the bar keeps the
/// bar's globals when its process becomes another executable, and one that connected as
/// another gains nothing when it becomes the bar.
#[test]
fn task5_exec_after_connect_keeps_the_connect_identity() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    bar.relist(&mut run);
    assert!(bar.privileged(), "connected as the bar");

    let mut other = Local::connect(&mut run, None);
    *run.identity.lock().expect("identity") = BAR;
    other.relist(&mut run);
    assert!(!other.privileged(), "connected as another");
    run.stop();
}

/// The top and overlay layers are the granted client's: another one's surface there is closed,
/// whether it asked for the layer at creation or moves there with `set_layer`; the bottom
/// layer stays every unconfined client's.
#[test]
fn task5_top_layers_are_closed_to_a_client_not_granted_them() {
    use zwlr_layer_shell_v1::Layer;
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    assert!(
        bar.layer_kept(&mut run, Layer::Top).0,
        "the bar's top layer"
    );
    assert!(
        bar.layer_kept(&mut run, Layer::Overlay).0,
        "the bar's overlay"
    );

    *run.identity.lock().expect("identity") = (SH, BAR.1);
    let mut other = Local::connect(&mut run, None);
    assert!(
        !other.layer_kept(&mut run, Layer::Top).0,
        "another's top layer"
    );
    assert!(
        !other.layer_kept(&mut run, Layer::Overlay).0,
        "another's overlay"
    );
    let (kept, layer, surface) = other.layer_kept(&mut run, Layer::Bottom);
    assert!(kept, "another's bottom layer");
    layer.set_layer(Layer::Top);
    assert!(!other.answer(&mut run, &surface), "moved to the top layer");
    run.stop();
}

/// Each client is offered exactly the globals `data/globals` gives its class of client (OS4,
/// OS5): the bar every one but the output manager, which its line does not name; the control
/// center the unprivileged ones and the output manager; a client with no line the unprivileged
/// ones; a client inside a security context the standard and layer-gated ones. An offered global
/// with no row fails with its name.
#[test]
fn os9_every_global_is_classified() {
    let classes = classified();
    let offered = |local: &Local, who: &str, expected: BTreeSet<&str>| {
        let listed = local.listed();
        for interface in &listed {
            assert!(
                classes.iter().any(|(row, _)| row == interface),
                "{interface}, offered to {who}, has no row in data/globals"
            );
        }
        assert_eq!(listed, expected, "{who}");
    };
    let unprivileged = rows(|class| class != Class::Privileged);
    let mut run = Running::start(None);

    let bar = Local::connect(&mut run, None);
    let mut expected = rows(|_| true);
    expected.remove("zwlr_output_manager_v1");
    expected.remove("ext_data_control_manager_v1");
    expected.remove("zwlr_data_control_manager_v1");
    offered(&bar, "the bar", expected);

    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    let standard = rows(|class| matches!(class, Class::Standard | Class::LayerGated));
    offered(&sandboxed, "a client inside a security context", standard);

    let control_center = Local::control_center(&mut run);
    let mut expected = unprivileged.clone();
    expected.insert("zwlr_output_manager_v1");
    expected.insert("athanor_toplevel_manager_v1");
    offered(&control_center, "the control center", expected);

    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    offered(&other, "a client with no line", unprivileged);
    run.stop();
}

/// The privileged and layer-gated rows of `data/globals` are the names of the allow-list, each
/// one a grant on a line of its own.
#[test]
fn os9_the_table_matches_the_allow_list_names() {
    let mut gated: Vec<&str> = classified()
        .into_iter()
        .filter(|(_, class)| matches!(class, Class::Privileged | Class::LayerGated))
        .map(|(interface, _)| interface)
        .collect();
    let mut names: Vec<&str> = crate::identity::names().collect();
    gated.sort_unstable();
    names.sort_unstable();
    assert_eq!(gated, names);
    for name in gated {
        let line = format!("/usr/bin/athanor-bar athanor-bar.service {name}\n");
        assert!(
            AllowList::parse(&line).is_ok(),
            "{name} on a line of its own"
        );
    }
}

/// `ext_workspace_manager_v1` is privileged (open question 7 of the OS9 plan): neither a client
/// with no line nor one inside a security context sees it; the bar, granted it, does.
#[test]
fn os9_the_workspace_list_is_privileged() {
    const WORKSPACES: &str = "ext_workspace_manager_v1";
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(WORKSPACES), "the bar, granted it");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(WORKSPACES), "the bar inside a context");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(WORKSPACES), "a client with no line");
    run.stop();
}

/// `athanor_toplevel_manager_v1` and `athanor_toplevel_actions_v1` are privileged (OS4, OS5),
/// each granted on its own: the fixture's bar, granted both, sees both; the control center,
/// granted the manager only, reads every window's state and cannot act on one, as the actions
/// are not offered to it; the bar's identity inside a security context, and a client with no
/// line, see neither.
#[test]
fn the_toplevel_manager_and_actions_are_privileged() {
    const MANAGER: &str = "athanor_toplevel_manager_v1";
    const ACTIONS: &str = "athanor_toplevel_actions_v1";
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(MANAGER), "the bar, granted it");
    assert!(bar.sees(ACTIONS), "the bar, granted it");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(MANAGER), "the bar inside a context");
    assert!(!sandboxed.sees(ACTIONS), "the bar inside a context");
    let control_center = Local::control_center(&mut run);
    assert!(control_center.sees(MANAGER), "the control center");
    assert!(!control_center.sees(ACTIONS), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    assert!(!other.sees(ACTIONS), "a client with no line");
    run.stop();
}

/// `athanor_keyboard_group_manager_v1` is privileged (OS4, OS5): the fixture's bar, granted it,
/// sees it; the bar's identity inside a security context, the control center and a client with
/// no line do not.
#[test]
fn the_keyboard_group_is_privileged() {
    const MANAGER: &str = "athanor_keyboard_group_manager_v1";
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(MANAGER), "the bar, granted it");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(MANAGER), "the bar inside a context");
    let control_center = Local::control_center(&mut run);
    assert!(!control_center.sees(MANAGER), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    run.stop();
}

impl Local {
    /// The seat's keyboard and a keyboard group object of it.
    fn keyboard_group(
        &mut self,
        run: &mut Running,
    ) -> (
        wl_keyboard::WlKeyboard,
        athanor_keyboard_group::athanor_keyboard_group_v1::AthanorKeyboardGroupV1,
    ) {
        let manager: athanor_keyboard_group::athanor_keyboard_group_manager_v1::AthanorKeyboardGroupManagerV1 =
            self.bind(1);
        let seat: wl_seat::WlSeat = self.bind(1);
        let qh = self.queue.handle();
        let keyboard = seat.get_keyboard(&qh, ());
        let object = manager.get_keyboard_group(&keyboard, &qh, ());
        self.roundtrip(run);
        (keyboard, object)
    }

    /// The groups the keyboard group object `object` was sent, in order.
    fn groups_of(
        &self,
        object: &athanor_keyboard_group::athanor_keyboard_group_v1::AthanorKeyboardGroupV1,
    ) -> Vec<u32> {
        self.client
            .keyboard_groups
            .iter()
            .filter(|(id, _)| *id == object.id())
            .map(|(_, group)| *group)
            .collect()
    }
}

/// A keyboard group object is sent the seat's group when it is made.
#[test]
fn a_keyboard_group_object_is_sent_the_group_at_once() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let (_keyboard, object) = bar.keyboard_group(&mut run);
    assert_eq!(bar.groups_of(&object), [0]);
    run.stop();
}

/// Gives the seat two layouts, us and it, switched by Alt+Shift.
fn two_layouts(run: &mut Running) {
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    let config = XkbConfig {
        layout: "us,it",
        variant: ",",
        options: Some("grp:alt_shift_toggle".into()),
        ..Default::default()
    };
    keyboard
        .set_xkb_config(&mut run.state, config)
        .expect("the us and it layouts");
}

/// `set_group` switches the seat's group: every object hears it once, and the focused window
/// is sent modifiers in the new group.
#[test]
fn set_group_switches_and_every_object_hears_it() {
    let mut run = Running::start(None);
    two_layouts(&mut run);
    let (mut window, _) = focused_window(&mut run);
    let mut first = Local::connect(&mut run, None);
    let (_first_keyboard, one) = first.keyboard_group(&mut run);
    let mut second = Local::connect(&mut run, None);
    let (_second_keyboard, two) = second.keyboard_group(&mut run);
    one.set_group(1);
    first.roundtrip(&mut run);
    second.roundtrip(&mut run);
    window.roundtrip(&mut run);
    assert_eq!(first.groups_of(&one), [0, 1], "the object that asked");
    assert_eq!(second.groups_of(&two), [0, 1], "the other object");
    assert_eq!(window.client.modifier_groups.last(), Some(&1), "the focus");
    two.set_group(0);
    second.roundtrip(&mut run);
    first.roundtrip(&mut run);
    assert_eq!(first.groups_of(&one), [0, 1, 0], "back to the first");
    run.stop();
}

/// A group at or beyond the keymap's layouts changes nothing.
#[test]
fn a_group_beyond_the_layouts_is_ignored() {
    let mut run = Running::start(None);
    two_layouts(&mut run);
    let (mut window, _) = focused_window(&mut run);
    // The modifiers of the window's enter may follow the keymap `focused_window` waits for.
    window.roundtrip(&mut run);
    let sent = window.client.modifier_groups.len();
    let mut bar = Local::connect(&mut run, None);
    let (_keyboard, object) = bar.keyboard_group(&mut run);
    object.set_group(2);
    object.set_group(u32::MAX);
    bar.roundtrip(&mut run);
    window.roundtrip(&mut run);
    assert_eq!(bar.groups_of(&object), [0], "no group is sent");
    assert_eq!(window.client.modifier_groups.len(), sent, "nor modifiers");
    assert_eq!(run.state.seat_group(), 0);
    object.set_group(1);
    bar.roundtrip(&mut run);
    assert_eq!(bar.groups_of(&object), [0, 1], "a group in range is");
    run.stop();
}

/// The keymap's own shortcut, Alt+Shift, switches the group, and the objects hear it.
#[test]
fn the_group_follows_a_shortcut() {
    const ALT: u32 = 56;
    let mut run = Running::start(None);
    two_layouts(&mut run);
    let mut bar = Local::connect(&mut run, None);
    let (_keyboard, object) = bar.keyboard_group(&mut run);
    for (code, pressed) in [(ALT, true), (SHIFT, true), (SHIFT, false), (ALT, false)] {
        key(&mut run.state, code, pressed);
    }
    bar.roundtrip(&mut run);
    assert_eq!(bar.groups_of(&object), [0, 1]);
    run.stop();
}

/// A virtual keyboard with no keymap of its own types under its own modifiers, group included,
/// and the seat's group is not moved.
#[test]
fn a_virtual_keyboards_modifiers_leave_the_group_alone() {
    let mut run = Running::start(None);
    two_layouts(&mut run);
    let (mut local, _) = focused_window(&mut run);
    let (_keyboard, object) = local.keyboard_group(&mut run);
    let typist = VirtualKeyboard::new(&mut local, &mut run, None);
    typist.group(&mut local, &mut run, 1);
    typist.key(&mut local, &mut run, 30, true);
    typist.key(&mut local, &mut run, 30, false);
    assert_eq!(local.groups_of(&object), [0], "no group is sent");
    assert_eq!(run.state.seat_group(), 0);
    run.stop();
}

/// A virtual keyboard's own keymap leaves the seat's group alone, and `set_group` while it is
/// current gives the focus the seat's keymap back before it switches.
#[test]
fn a_virtual_keymap_leaves_the_group_alone() {
    let mut run = Running::start(None);
    two_layouts(&mut run);
    let (mut local, _) = focused_window(&mut run);
    let (_keyboard, object) = local.keyboard_group(&mut run);
    let typist = VirtualKeyboard::new(&mut local, &mut run, Some(SWAPPED_KEYMAP));
    typist.key(&mut local, &mut run, 30, true);
    typist.key(&mut local, &mut run, 30, false);
    assert_eq!(local.groups_of(&object), [0], "no group is sent");
    let keymaps = local.client.keymaps;
    object.set_group(1);
    local.roundtrip(&mut run);
    assert!(
        local.client.keymaps > keymaps,
        "the seat's keymap is sent again"
    );
    assert_eq!(local.groups_of(&object), [0, 1]);
    assert_eq!(local.client.modifier_groups.last(), Some(&1));
    run.stop();
}

/// An `xkb_config` path in a directory of the test's own.
fn xkb_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("athanor-xkb-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a directory");
    dir.join("xkb_config")
}

/// Writes `text` to `file` as cosmic-config does: a rename into the directory.
fn write_raw(file: &Path, text: &str) {
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, text).expect("write");
    std::fs::rename(&tmp, file).expect("rename");
}

/// Writes the record of the layouts `layout`, each with its default variant.
fn write_xkb(file: &Path, layout: &str) {
    let variant = vec![""; layout.split(',').count()].join(",");
    write_raw(
        file,
        &format!(
            "(rules: \"\", model: \"\", layout: \"{layout}\", variant: \"{variant}\", \
             options: None, repeat_delay: 600, repeat_rate: 25)"
        ),
    );
}

/// The names of the seat's layouts.
fn layouts(state: &mut State) -> Vec<String> {
    let keyboard = state.seat.get_keyboard().expect("a keyboard");
    keyboard.with_xkb_state(state, |context| {
        let xkb = context.xkb().lock().expect("xkb");
        xkb.layouts()
            .map(|layout| xkb.layout_name(layout).to_owned())
            .collect()
    })
}

/// Whether the seat's Caps Lock is locked.
fn caps_lock(state: &State) -> bool {
    let keyboard = state.seat.get_keyboard().expect("a keyboard");
    keyboard.modifier_state().caps_lock
}

/// Caps Lock, as evdev numbers it.
const CAPS: u32 = 58;

/// The file read before the first client gives the seat its layouts and repeat: the bar is sent
/// that keymap and the first group.
#[test]
fn the_seat_starts_from_the_file() {
    let mut run = Running::start(None);
    let file = xkb_file("start");
    write_xkb(&file, "us,it");
    run.state.load_xkb_config(&file);
    let mut bar = Local::connect(&mut run, None);
    let (_keyboard, object) = bar.keyboard_group(&mut run);
    assert_eq!(layouts(&mut run.state), ["English (US)", "Italian"]);
    let size = seat_keymap(&mut run.state).len();
    assert_eq!(bar.client.keymap_size as usize, size, "the bar has that keymap");
    assert_eq!(bar.groups_of(&object), [0]);
    assert_eq!(run.state.repeat, (25, 600));
    run.stop();
}

/// A change of the file reaches every client with the new keymap; the group, clamped, and Caps
/// Lock stay, and a virtual keyboard's keymap is replaced by the new one, not the saved one.
#[test]
fn a_live_change_rebuilds_the_keymap() {
    let mut run = Running::start(None);
    let file = xkb_file("live");
    write_xkb(&file, "us,it");
    crate::xkb_config::follow(&mut run.state, &run.event_loop.handle(), file.clone());
    let (mut window, _) = focused_window(&mut run);
    let (_keyboard, object) = window.keyboard_group(&mut run);
    object.set_group(1);
    key(&mut run.state, CAPS, true);
    key(&mut run.state, CAPS, false);
    window.roundtrip(&mut run);
    let keymaps = window.client.keymaps;
    write_xkb(&file, "us,it,de");
    window.pump(&mut run, "the three layouts", |client, _| client.keymaps > keymaps);
    assert_eq!(layouts(&mut run.state).len(), 3);
    assert_eq!(run.state.seat_group(), 1, "the group stays");
    assert!(caps_lock(&run.state), "Caps Lock stays");
    assert_eq!(window.groups_of(&object), [0, 1], "no group is sent twice");
    let keymaps = window.client.keymaps;
    write_xkb(&file, "fr");
    window.pump(&mut run, "one layout", |client, _| client.keymaps > keymaps);
    window.roundtrip(&mut run);
    assert_eq!(layouts(&mut run.state), ["French"]);
    assert_eq!(window.groups_of(&object), [0, 1, 0], "the group is clamped");

    write_xkb(&file, "us,it");
    let keymaps = window.client.keymaps;
    window.pump(&mut run, "two layouts", |client, _| client.keymaps > keymaps);
    object.set_group(1);
    window.roundtrip(&mut run);
    let typist = VirtualKeyboard::new(&mut window, &mut run, Some(SWAPPED_KEYMAP));
    typist.key(&mut window, &mut run, 30, true);
    typist.key(&mut window, &mut run, 30, false);
    assert!(run.state.virtual_keymap.is_some(), "the virtual keymap is current");
    let keymaps = window.client.keymaps;
    write_xkb(&file, "us,it,de");
    window.pump(&mut run, "the file's keymap", |client, _| client.keymaps > keymaps);
    assert!(run.state.virtual_keymap.is_none(), "the virtual keymap is dropped");
    assert_eq!(layouts(&mut run.state).len(), 3, "the file's keymap, not the saved one");
    assert_eq!(run.state.seat_group(), 1, "the seat's group, not the virtual one");
    assert!(caps_lock(&run.state), "the seat's Caps Lock");
    run.stop();
}

/// A change of the file keeps a held Shift held, and an input method's grab is sent the new
/// repeat, keymap and modifiers.
#[test]
fn a_live_change_keeps_held_keys_and_reaches_the_grab() {
    let mut run = Running::start(None);
    let (_window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let file = xkb_file("held");
    write_xkb(&file, "us,it");
    key(&mut run.state, SHIFT, true);
    ime.roundtrip(&mut run);
    let before = method.grabs[0].events().len();
    run.state.load_xkb_config(&file);
    ime.roundtrip(&mut run);
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(keyboard.modifier_state().shift, "Shift stays held");
    assert_eq!(
        method.grabs[0].names_after(before),
        ["repeat_info", "keymap", "modifiers"]
    );
    assert_eq!(run.state.repeat, (25, 600));
    key(&mut run.state, SHIFT, false);
    run.stop();
}

/// A malformed, missing or uncompilable file changes nothing; a good one then applies.
#[test]
fn a_malformed_file_keeps_the_last_keymap() {
    let mut run = Running::start(None);
    let file = xkb_file("malformed");
    write_xkb(&file, "us,it");
    run.state.load_xkb_config(&file);
    let (mut window, _) = focused_window(&mut run);
    let (_keyboard, object) = window.keyboard_group(&mut run);
    object.set_group(1);
    window.roundtrip(&mut run);
    let keymaps = window.client.keymaps;
    write_raw(&file, "(rules: \"\", model: \"\", layout: \"de\"");
    run.state.load_xkb_config(&file);
    std::fs::remove_file(&file).expect("removed");
    run.state.load_xkb_config(&file);
    write_xkb(&file, "xx");
    run.state.load_xkb_config(&file);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keymaps, keymaps, "no keymap is sent");
    assert_eq!(layouts(&mut run.state).len(), 2);
    assert_eq!(run.state.seat_group(), 1, "the group stays");
    write_xkb(&file, "us,de");
    run.state.load_xkb_config(&file);
    window.roundtrip(&mut run);
    assert!(window.client.keymaps > keymaps, "a good file applies");
    assert_eq!(layouts(&mut run.state), ["English (US)", "German"]);
    run.stop();
}

/// More than four layouts are refused as a whole; four apply.
#[test]
fn more_than_four_layouts_are_refused() {
    let mut run = Running::start(None);
    let file = xkb_file("four");
    write_xkb(&file, "us,it");
    run.state.load_xkb_config(&file);
    write_xkb(&file, "us,it,de,fr,es");
    run.state.load_xkb_config(&file);
    assert_eq!(layouts(&mut run.state).len(), 2, "five are refused");
    write_xkb(&file, "us,it,de,fr");
    run.state.load_xkb_config(&file);
    assert_eq!(layouts(&mut run.state).len(), 4, "four apply");
    run.stop();
}

/// Without a file the keymap of the environment stays.
#[test]
fn without_a_file_the_environment_keymap_stays() {
    let mut run = Running::start(None);
    let before = seat_keymap(&mut run.state);
    run.state.load_xkb_config(&xkb_file("absent"));
    assert_eq!(seat_keymap(&mut run.state), before);
    assert_eq!(run.state.repeat, (25, 200));
    run.stop();
}

/// A window of a fixture client besides the one `Local::toplevel` maps: its surface, its xdg
/// surface and its toplevel.
struct Other {
    surface: wl_surface::WlSurface,
    xdg: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
}

impl Local {
    /// Maps a `WIDTH`x`HEIGHT` window titled `title`; `place` decides its place and focus.
    fn another(&mut self, run: &mut Running, title: &str) -> Other {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shm: wl_shm::WlShm = self.bind(1);
        let wm_base: xdg_wm_base::XdgWmBase = self.bind(1);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_title(title.into());
        self.client.configure = None;
        surface.commit();
        self.pump(run, "the window is configured", |client, _| {
            client.configure.is_some()
        });
        xdg.ack_configure(self.client.configure.take().expect("a configure"));
        let (buffer, _) = self.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        self.roundtrip(run);
        self.client.configure = None;
        Other {
            surface,
            xdg,
            toplevel,
        }
    }

    /// The compositor's window of this client's `surface`, mapped, minimised or on an inactive
    /// workspace.
    fn server_window(
        &self,
        run: &Running,
        surface: &wl_surface::WlSurface,
    ) -> smithay::desktop::Window {
        let id = self.id.clone().expect("an id");
        let protocol_id = surface.id().protocol_id();
        run.state
            .space
            .elements()
            .chain(run.state.minimised.iter().map(|(window, _)| window))
            .chain(run.state.parked.iter().map(|(window, _)| window))
            .find(|window| {
                owned_by(window, &id)
                    && window
                        .toplevel()
                        .is_some_and(|t| t.wl_surface().id().protocol_id() == protocol_id)
            })
            .expect("the window")
            .clone()
    }

    /// The foreign toplevel handle of the window titled `title`, once it is listed.
    fn handle(
        &mut self,
        run: &mut Running,
        title: &str,
    ) -> ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1 {
        self.pump(run, "the window is listed", |client, _| {
            client.listed.iter().any(|l| l.done && l.title == title)
        });
        self.client
            .listed
            .iter()
            .find(|l| l.title == title)
            .map(|l| l.handle.clone())
            .expect("the handle")
    }

    /// This client's `wl_output` of output `index`, bound now.
    fn output(&self, index: usize) -> wl_output::WlOutput {
        let (name, version) = self
            .client
            .globals
            .iter()
            .filter(|(_, interface, _)| interface == "wl_output")
            .nth(index)
            .map(|(name, _, version)| (*name, *version))
            .expect("the output's global");
        self.registry
            .bind(name, version.min(4), &self.queue.handle(), ())
    }

    /// The events of `object` until its next `done`, which they end with; taken.
    fn state_events(
        &mut self,
        run: &mut Running,
        object: &athanor_toplevel::athanor_toplevel_state_v1::AthanorToplevelStateV1,
    ) -> Vec<StateEvent> {
        let id = object.id();
        self.pump(run, "the state object's done", |client, _| {
            client
                .toplevel_states
                .iter()
                .any(|(of, event)| *of == id && *event == StateEvent::Done)
        });
        let events = &mut self.client.toplevel_states;
        let end = events
            .iter()
            .position(|(of, event)| *of == id && *event == StateEvent::Done)
            .expect("a done");
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for (index, (of, event)) in events.drain(..).enumerate() {
            if of == id && index <= end {
                taken.push(event);
            } else {
                kept.push((of, event));
            }
        }
        *events = kept;
        taken
    }

    /// Runs the compositor and the client for `frames` frames of the headless outputs.
    fn frames(&mut self, run: &mut Running, frames: u32) {
        let until = Instant::now() + Duration::from_millis(u64::from(frames) * 1000 / 60 + 1);
        self.pump(run, "the frames", |_, _| Instant::now() > until);
    }
}

/// The surface that has the keyboard.
fn keyboard_focus(run: &Running) -> Option<WlSurface> {
    run.state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.current_focus())
}

/// The toplevel surface of `window`.
fn surface_of(window: &smithay::desktop::Window) -> WlSurface {
    window.toplevel().expect("a toplevel").wl_surface().clone()
}

/// A window minimised leaves the space and the keyboard to the window under it; it keeps
/// committing, and its new title reaches the foreign toplevel list; a token restores it where it
/// was, on top, with the keyboard (doc_own_stack.md OS5).
#[test]
fn a_window_is_minimised_and_restored() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    local.toplevel(&mut run);
    let (first_surface, _) = local.window.clone().expect("the first window");
    let first = local.server_window(&run, &first_surface);
    let second = local.another(&mut run, "second");
    let window = local.server_window(&run, &second.surface);
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "the second is focused"
    );
    let at = run.state.space.element_location(&window).expect("mapped");

    second.toplevel.set_minimized();
    local.roundtrip(&mut run);
    assert!(
        !run.state.space.elements().any(|kept| *kept == window),
        "it leaves the space"
    );
    assert_eq!(run.state.minimised.len(), 1);
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&first)),
        "the first has the keyboard"
    );

    second.toplevel.set_title("second, renamed".into());
    second.surface.commit();
    local.handle(&mut run, "second, renamed");

    let serial = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.last_enter())
        .expect("an enter");
    let token = run.activation_token(&mut local, serial.into());
    let activation: xdg_activation_v1::XdgActivationV1 = local.bind(1);
    activation.activate(token, &second.surface);
    local.roundtrip(&mut run);
    assert!(run.state.minimised.is_empty(), "restored");
    assert_eq!(
        run.state.space.element_location(&window),
        Some(at),
        "where it was"
    );
    assert_eq!(run.state.space.elements().last(), Some(&window), "on top");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );
    run.stop();
}

/// A window that asks to be minimised before its first commit is refused: it gets its initial
/// configure and maps (Review Focus 1).
#[test]
fn a_window_minimised_before_its_first_commit_maps() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    // Nothing holds the keyboard: the window takes it when it is placed.
    run.state.set_keyboard_focus(None);
    toplevel.set_minimized();
    surface.commit();
    local.pump(&mut run, "the initial configure", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.take().expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(&mut run);
    let window = local.server_window(&run, &surface);
    assert!(
        run.state.space.elements().any(|kept| *kept == window),
        "mapped"
    );
    assert!(!window.geometry().is_empty(), "with its size");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );
    assert!(run.state.minimised.is_empty());
    run.stop();
}

/// A minimised window destroyed by its client, or whose client is gone, leaves `minimised` and
/// the list says it is closed (Review Focus 4).
#[test]
fn a_minimised_window_can_be_destroyed() {
    let closed = |title: &'static str| {
        move |client: &Client, state: &State| {
            state.minimised.is_empty() && client.listed.iter().any(|l| l.title == title && l.closed)
        }
    };
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let gone = local.another(&mut run, "destroyed");
    local.handle(&mut run, "destroyed");
    gone.toplevel.set_minimized();
    local.roundtrip(&mut run);
    assert_eq!(run.state.minimised.len(), 1);
    gone.toplevel.destroy();
    gone.xdg.destroy();
    gone.surface.destroy();
    local.pump(
        &mut run,
        "the destroyed window is closed",
        closed("destroyed"),
    );

    let mut other = Local::connect(&mut run, None);
    let dropped = other.another(&mut run, "disconnected");
    local.handle(&mut run, "disconnected");
    dropped.toplevel.set_minimized();
    other.roundtrip(&mut run);
    assert_eq!(run.state.minimised.len(), 1);
    drop(dropped);
    drop(other);
    local.pump(
        &mut run,
        "the gone client's window is closed",
        closed("disconnected"),
    );
    run.stop();
}

/// A window minimised on an output that then goes is restored centred in the window area.
#[test]
fn a_window_restored_after_its_output_left_is_centred() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let window = window_of(&run, &id);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .expect("the second output")
        .clone();
    let area = run
        .state
        .space
        .output_geometry(&second)
        .expect("the second output's area");
    run.state
        .space
        .relocate_element(&window, area.loc + Point::from((10, 10)));
    toplevel.set_minimized();
    local.roundtrip(&mut run);
    assert_eq!(run.state.minimised.len(), 1);
    run.state.remove_output(&second);
    assert!(run.state.unminimise(&window), "restored");
    let free = run.state.window_area().expect("an area");
    assert_eq!(
        run.state.space.element_location(&window),
        Some(crate::state::centred(free, window.geometry()))
    );
    run.stop();
}

/// A maximised window minimised keeps committing; restored after a bar's zone changed the area,
/// it is configured to the new area and takes it at the commit that answers (Review Focus 2).
#[test]
fn a_maximised_window_restored_fits_the_area() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let window = window_of(&run, &id);
    toplevel.set_maximized();
    local.redraw(&mut run);
    toplevel.set_minimized();
    local.roundtrip(&mut run);
    assert_eq!(run.state.minimised.len(), 1);
    // The configure that deactivates it, answered while it is minimised.
    local.redraw(&mut run);
    assert_eq!(run.state.minimised.len(), 1, "a commit does not map it");

    let mut shell = Local::connect(&mut run, None);
    let _bar = shell.bar(&mut run, 30);
    let fitted = run.state.window_area().expect("an area");
    assert!(run.state.unminimise(&window), "restored");
    let (width, height, states) = local.redraw(&mut run);
    assert!(states.contains(&(xdg_toplevel::State::Maximized as u32)));
    assert_eq!((width, height), (fitted.size.w, fitted.size.h));
    assert_eq!(location_of(&run, &id), fitted.loc);
    assert_eq!(window.geometry().size, fitted.size);
    run.stop();
}

/// A minimised window that commits a null buffer, as an app hiding into the tray does, is not
/// restored by a token: it stays minimised and the keyboard stays where it is.
#[test]
fn a_hidden_minimised_window_stays_minimised() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let (surface, _) = local.window.clone().expect("the window");
    let window = local.server_window(&run, &surface);
    toplevel.set_minimized();
    local.roundtrip(&mut run);
    surface.attach(None, 0, 0);
    surface.commit();
    local.roundtrip(&mut run);

    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    let focus = keyboard_focus(&run);
    let serial = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.last_enter())
        .expect("an enter");
    let token = run.activation_token(&mut other, serial.into());
    let activation: xdg_activation_v1::XdgActivationV1 = local.bind(1);
    activation.activate(token, &surface);
    local.roundtrip(&mut run);
    assert!(
        run.state.minimised.iter().any(|(kept, _)| *kept == window),
        "still minimised"
    );
    assert!(!run.state.space.elements().any(|kept| *kept == window));
    assert_eq!(keyboard_focus(&run), focus, "the keyboard stays");
    run.stop();
}

/// While the session is locked, a token restores no minimised window, and the keyboard stays
/// on the lock surface (L14).
#[test]
fn a_token_restores_nothing_while_locked() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let (surface, _) = local.window.clone().expect("the window");
    let window = local.server_window(&run, &surface);
    toplevel.set_minimized();
    local.roundtrip(&mut run);

    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    let serial = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.last_enter())
        .expect("an enter");
    let token = run.activation_token(&mut other, serial.into());
    let (locker, _lock) = lock_session(&mut run);
    let activation: xdg_activation_v1::XdgActivationV1 = local.bind(1);
    activation.activate(token, &surface);
    local.roundtrip(&mut run);
    assert!(
        run.state.minimised.iter().any(|(kept, _)| *kept == window),
        "still minimised"
    );
    let holder = keyboard_focus(&run)
        .and_then(|surface| surface.client())
        .map(|client| client.id());
    assert_eq!(holder, locker.id, "the lock surface keeps the keyboard");
    run.stop();
}

/// A state object is sent the window's state and outputs at once, then each change: another
/// window focused, its own focus back, and the minimise, after which its client's configure no
/// longer says activated.
#[test]
fn the_state_follows_focus_and_minimise() {
    use athanor_toplevel::athanor_toplevel_manager_v1::AthanorToplevelManagerV1;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let manager: AthanorToplevelManagerV1 = local.bind(1);
    let output = local.output(0);
    let probe = local.another(&mut run, "probe");
    let window = local.server_window(&run, &probe.surface);
    run.state.focus(&window);
    let handle = local.handle(&mut run, "probe");
    let object = manager.get_toplevel_state(&handle, &local.queue.handle(), ());
    assert_eq!(
        local.state_events(&mut run, &object),
        [
            StateEvent::State(1),
            StateEvent::Enter(output.id()),
            StateEvent::Done
        ]
    );

    let (fixture, _) = fixture_window(&run);
    run.state.focus(&fixture);
    assert_eq!(
        local.state_events(&mut run, &object),
        [StateEvent::State(0), StateEvent::Done]
    );
    run.state.focus(&window);
    assert_eq!(
        local.state_events(&mut run, &object),
        [StateEvent::State(1), StateEvent::Done]
    );

    local.client.toplevel_configure = None;
    probe.toplevel.set_minimized();
    assert_eq!(
        local.state_events(&mut run, &object),
        [
            StateEvent::State(2),
            StateEvent::Leave(output.id()),
            StateEvent::Done
        ]
    );
    let (_, _, states) = local
        .client
        .toplevel_configure
        .clone()
        .expect("a configure");
    assert!(
        !states.contains(&(xdg_toplevel::State::Activated as u32)),
        "no longer activated"
    );
    run.stop();
}

/// A window moved between outputs: the state object says it left one and entered the other,
/// once; a client that binds `wl_output` after making its state object is sent the output then
/// (Review Focus 5).
#[test]
fn outputs_follow_the_window() {
    use athanor_toplevel::athanor_toplevel_manager_v1::AthanorToplevelManagerV1;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let manager: AthanorToplevelManagerV1 = local.bind(1);
    let (first, second) = (local.output(0), local.output(1));
    let probe = local.another(&mut run, "probe");
    let window = local.server_window(&run, &probe.surface);
    run.state.focus(&window);
    let handle = local.handle(&mut run, "probe");
    let object = manager.get_toplevel_state(&handle, &local.queue.handle(), ());
    assert_eq!(
        local.state_events(&mut run, &object),
        [
            StateEvent::State(1),
            StateEvent::Enter(first.id()),
            StateEvent::Done
        ]
    );

    let area = run
        .state
        .space
        .outputs()
        .nth(1)
        .and_then(|output| run.state.space.output_geometry(output))
        .expect("the second output's area");
    run.state
        .move_window(&window, area.loc + Point::from((10, 10)));
    assert_eq!(
        local.state_events(&mut run, &object),
        [
            StateEvent::Leave(first.id()),
            StateEvent::Enter(second.id()),
            StateEvent::Done
        ]
    );
    local.frames(&mut run, 3);
    assert!(local.client.toplevel_states.is_empty(), "nothing more");

    let mut late = Local::connect(&mut run, None);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = late.bind(1);
    let manager: AthanorToplevelManagerV1 = late.bind(1);
    let handle = late.handle(&mut run, "probe");
    let object = manager.get_toplevel_state(&handle, &late.queue.handle(), ());
    assert_eq!(
        late.state_events(&mut run, &object),
        [StateEvent::State(1), StateEvent::Done]
    );
    let bound = late.output(1);
    assert_eq!(
        late.state_events(&mut run, &object),
        [StateEvent::Enter(bound.id()), StateEvent::Done]
    );
    run.stop();
}

/// A click with no motion after a minimise misses the hidden window, which no longer has the
/// pointer; restored, it has the pointer and the click again.
#[test]
fn a_click_after_minimise_misses_the_hidden_window() {
    let click = |run: &mut Running| {
        run.state
            .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
        release(run);
    };
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let window = window_of(&run, &id);
    let at = location_of(&run, &id).to_f64() + Point::from((f64::from(WIDTH) / 2.0, 10.0));
    run.state.pointer_to(at, InputTime::now());
    toplevel.set_minimized();
    local.roundtrip(&mut run);
    local.client.button = None;
    click(&mut run);
    local.roundtrip(&mut run);
    assert!(
        local.client.button.is_none(),
        "the hidden window misses the click"
    );

    assert!(run.state.unminimise(&window), "restored");
    click(&mut run);
    local.pump(
        &mut run,
        "the restored window has the click",
        |client, _| client.button.is_some(),
    );
    run.stop();
}

/// A window captured while it is minimised is still drawn, at its size, from its last buffer.
#[test]
fn a_minimised_window_keeps_its_thumbnail() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let thumb = local.another(&mut run, "thumbnail");
    let handle = local.handle(&mut run, "thumbnail");
    let qh = local.queue.handle();
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    thumb.toplevel.set_minimized();
    local.roundtrip(&mut run);
    assert_eq!(run.state.minimised.len(), 1);

    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, WIDTH, HEIGHT);
    frame.capture();
    local.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(local.client.captured, Some(Ok(())), "the frame is ready");
    assert_eq!(
        local.client.capture_size,
        Some((WIDTH as u32, HEIGHT as u32))
    );
    run.stop();
}

/// The dock, as the shell's units connect: the manager and the actions of
/// athanor-toplevel-state-v1, the foreign toplevel list and the seat.
struct Dock {
    local: Local,
    manager: athanor_toplevel::athanor_toplevel_manager_v1::AthanorToplevelManagerV1,
    actions: athanor_toplevel::athanor_toplevel_actions_v1::AthanorToplevelActionsV1,
    seat: wl_seat::WlSeat,
}

impl Dock {
    fn connect(run: &mut Running) -> Self {
        let local = Local::connect(run, None);
        let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
        let manager = local.bind(1);
        let actions = local.bind(1);
        let seat = local.bind(1);
        Self {
            local,
            manager,
            actions,
            seat,
        }
    }
}

/// The windows in the space, bottom to top.
fn stack(run: &Running) -> Vec<smithay::desktop::Window> {
    run.state.space.elements().cloned().collect()
}

/// The dock raises a window with the keyboard, minimises it, restores it with the keyboard, and
/// asks another window to close (doc_own_stack.md OS5).
#[test]
fn the_dock_raises_minimises_and_closes_a_window() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let first = app.another(&mut run, "first");
    let second = app.another(&mut run, "second");
    let window = app.server_window(&run, &first.surface);
    let handle = dock.local.handle(&mut run, "first");
    let other = dock.local.handle(&mut run, "second");

    dock.actions.activate(&handle, &dock.seat);
    dock.local.roundtrip(&mut run);
    assert_eq!(run.state.space.elements().last(), Some(&window), "raised");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );

    dock.actions.set_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(run.state.is_minimised(&window), "minimised");

    dock.actions.unset_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(!run.state.is_minimised(&window), "restored");
    assert_eq!(run.state.space.elements().last(), Some(&window), "on top");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );

    dock.actions.close(&other);
    dock.local.roundtrip(&mut run);
    let closed = second.toplevel.id();
    app.pump(
        &mut run,
        "the second window is asked to close",
        |client, _| client.toplevel_closed.contains(&closed),
    );
    run.stop();
}

/// While the session is locked the actions change neither the stack nor the keyboard
/// (L14); with a layer surface holding the keyboard exclusively, `activate` raises the window
/// and the keyboard stays with the layer.
#[test]
fn activation_respects_the_lock_and_an_exclusive_layer() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let first = app.another(&mut run, "first");
    let second = app.another(&mut run, "second");
    let window = app.server_window(&run, &first.surface);
    let shown_second = app.server_window(&run, &second.surface);
    run.state.focus(&shown_second);
    let handle = dock.local.handle(&mut run, "first");
    let other = dock.local.handle(&mut run, "second");

    let (mut locker, lock) = lock_session(&mut run);
    let (before, focus) = (stack(&run), keyboard_focus(&run));
    dock.actions.activate(&handle, &dock.seat);
    dock.actions.set_minimized(&other);
    dock.local.roundtrip(&mut run);
    assert_eq!(stack(&run), before, "the stack is unchanged while locked");
    assert_eq!(keyboard_focus(&run), focus, "and the keyboard");
    assert!(run.state.minimised.is_empty(), "nothing is minimised");
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });

    let mut shell = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = shell.bind(4);
    let shm: wl_shm::WlShm = shell.bind(1);
    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = shell.bind(4);
    let qh = shell.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "launcher".into(),
        &qh,
        (),
    );
    layer.set_size(100, 100);
    layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
    assert!(
        shell.answer(&mut run, &surface),
        "the surface is configured"
    );
    let (serial, width, height) = shell.client.layer_configure.expect("a configure");
    layer.ack_configure(serial);
    let (buffer, _) = shell.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    shell.roundtrip(&mut run);
    let exclusive = run.state.exclusive_layer().expect("an exclusive layer");
    assert_eq!(keyboard_focus(&run), Some(exclusive.clone()));
    dock.actions.activate(&handle, &dock.seat);
    dock.local.roundtrip(&mut run);
    assert_eq!(run.state.space.elements().last(), Some(&window), "raised");
    assert_eq!(
        keyboard_focus(&run),
        Some(exclusive),
        "the layer keeps the keyboard"
    );
    run.stop();
}

/// The four requests on a destroyed window's handle, and `activate` on a placed window that
/// committed a null buffer, change neither the stack nor the keyboard; `activate` on a live
/// window still raises it with the keyboard.
#[test]
fn a_closed_or_undrawn_window_is_left_alone() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let live = app.another(&mut run, "live");
    let hidden = app.another(&mut run, "hidden");
    let gone = app.another(&mut run, "gone");
    let window = app.server_window(&run, &live.surface);
    let live_handle = dock.local.handle(&mut run, "live");
    let hidden_handle = dock.local.handle(&mut run, "hidden");
    let gone_handle = dock.local.handle(&mut run, "gone");

    gone.toplevel.destroy();
    gone.xdg.destroy();
    gone.surface.destroy();
    app.roundtrip(&mut run);
    dock.local
        .pump(&mut run, "the window is closed", |client, _| {
            client.listed.iter().any(|l| l.title == "gone" && l.closed)
        });
    hidden.surface.attach(None, 0, 0);
    hidden.surface.commit();
    app.roundtrip(&mut run);

    let (before, focus) = (stack(&run), keyboard_focus(&run));
    dock.actions.activate(&gone_handle, &dock.seat);
    dock.actions.set_minimized(&gone_handle);
    dock.actions.unset_minimized(&gone_handle);
    dock.actions.close(&gone_handle);
    dock.actions.activate(&hidden_handle, &dock.seat);
    dock.actions.set_minimized(&hidden_handle);
    dock.local.roundtrip(&mut run);
    assert_eq!(stack(&run), before, "the stack is unchanged");
    assert_eq!(keyboard_focus(&run), focus, "and the keyboard");
    assert!(run.state.minimised.is_empty(), "nothing is minimised");
    assert!(app.client.toplevel_closed.is_empty(), "nothing is closed");

    dock.actions.activate(&live_handle, &dock.seat);
    dock.local.roundtrip(&mut run);
    assert_eq!(run.state.space.elements().last(), Some(&window), "raised");
    assert_eq!(
        keyboard_focus(&run),
        Some(surface_of(&window)),
        "with the keyboard"
    );
    run.stop();
}

/// The events `local` has received for `object` so far, without running the compositor.
fn received(
    local: &Local,
    object: &athanor_toplevel::athanor_toplevel_state_v1::AthanorToplevelStateV1,
) -> Vec<StateEvent> {
    let id = object.id();
    local
        .client
        .toplevel_states
        .iter()
        .filter(|(of, _)| *of == id)
        .map(|(_, event)| event.clone())
        .collect()
}

/// `get_toplevel_state` sends the window's state and done at once: they arrive before the answer
/// to the `sync` sent with it. The compositor reads the two requests together and handles them in
/// turn, so no frame comes between them, and a frame's send could only follow the answer.
#[test]
fn the_first_state_needs_no_frame() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let probe = app.another(&mut run, "probe");
    let window = app.server_window(&run, &probe.surface);
    run.state.focus(&window);
    let handle = dock.local.handle(&mut run, "probe");
    let object = dock
        .manager
        .get_toplevel_state(&handle, &dock.local.queue.handle(), ());
    dock.local.roundtrip(&mut run);
    let id = object.id();
    let answered = dock.local.client.states_at_sync;
    let before: Vec<_> = dock.local.client.toplevel_states[..answered]
        .iter()
        .filter(|(of, _)| *of == id)
        .map(|(_, event)| event.clone())
        .collect();
    assert_eq!(before, [StateEvent::State(1), StateEvent::Done]);
    run.stop();
}

/// A state object destroyed frees its place: a new one of the same window, in the same flush,
/// is sent its state and done, and the client is not disconnected.
#[test]
fn a_state_object_can_be_made_again_after_its_destroy() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    app.another(&mut run, "probe");
    let handle = dock.local.handle(&mut run, "probe");
    let qh = dock.local.queue.handle();
    let first = dock.manager.get_toplevel_state(&handle, &qh, ());
    dock.local.state_events(&mut run, &first);
    first.destroy();
    let second = dock.manager.get_toplevel_state(&handle, &qh, ());
    let events = dock.local.state_events(&mut run, &second);
    assert!(matches!(
        events.as_slice(),
        [StateEvent::State(_), StateEvent::Done]
    ));
    dock.local.roundtrip(&mut run);
    assert!(dock.local.connection.protocol_error().is_none(), "no error");
    assert_eq!(run.state.toplevel_states.len(), 1);
    run.stop();
}

/// Two clients each hold a state object of the same window: both are sent its state, and
/// neither is disconnected.
#[test]
fn two_clients_each_hold_a_state_object_of_a_window() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut bar = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    app.another(&mut run, "probe");
    let docked = dock.local.handle(&mut run, "probe");
    let barred = bar.local.handle(&mut run, "probe");
    let first = dock
        .manager
        .get_toplevel_state(&docked, &dock.local.queue.handle(), ());
    let second = bar
        .manager
        .get_toplevel_state(&barred, &bar.local.queue.handle(), ());
    for (local, object) in [(&mut dock.local, &first), (&mut bar.local, &second)] {
        let events = local.state_events(&mut run, object);
        assert!(matches!(
            events.as_slice(),
            [StateEvent::State(_), StateEvent::Done]
        ));
        local.roundtrip(&mut run);
        assert!(local.connection.protocol_error().is_none(), "no error");
    }
    assert_eq!(run.state.toplevel_states.len(), 2);
    run.stop();
}

/// A second state object of the same window from the same client is the `already_made` error,
/// which bounds the objects each frame walks.
#[test]
fn a_second_state_object_of_a_window_is_an_error() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    app.another(&mut run, "probe");
    let handle = dock.local.handle(&mut run, "probe");
    let qh = dock.local.queue.handle();
    let first = dock.manager.get_toplevel_state(&handle, &qh, ());
    dock.local.state_events(&mut run, &first);
    assert_eq!(run.state.toplevel_states.len(), 1);
    let _second = dock.manager.get_toplevel_state(&handle, &qh, ());
    dock.local.connection.flush().expect("flush");
    run.until("the client is disconnected", |state| {
        state.toplevel_states.is_empty()
    });
    let started = Instant::now();
    let error = loop {
        if let Some(error) = dock.local.connection.protocol_error() {
            break error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = dock.local.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    };
    assert_eq!(error.object_interface, "athanor_toplevel_manager_v1");
    assert_eq!(
        error.code,
        athanor_toplevel::athanor_toplevel_manager_v1::Error::AlreadyMade as u32
    );
    run.stop();
}

/// While the session is locked `unset_minimized` leaves a minimised window minimised and `close`
/// sends no close (L14); once unlocked, the next request acts.
#[test]
fn a_locked_session_restores_and_closes_nothing() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let first = app.another(&mut run, "first");
    let second = app.another(&mut run, "second");
    let window = app.server_window(&run, &first.surface);
    let handle = dock.local.handle(&mut run, "first");
    let other = dock.local.handle(&mut run, "second");
    dock.actions.set_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(run.state.is_minimised(&window), "minimised");

    let (mut locker, lock) = lock_session(&mut run);
    dock.actions.unset_minimized(&handle);
    dock.actions.close(&other);
    dock.local.roundtrip(&mut run);
    app.roundtrip(&mut run);
    assert!(
        run.state.is_minimised(&window),
        "still minimised while locked"
    );
    assert!(app.client.toplevel_closed.is_empty(), "nothing is closed");

    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    dock.actions.close(&other);
    dock.local.roundtrip(&mut run);
    let closed = second.toplevel.id();
    app.pump(
        &mut run,
        "the second window is asked to close",
        |client, _| client.toplevel_closed.contains(&closed),
    );
    dock.actions.unset_minimized(&handle);
    dock.local.roundtrip(&mut run);
    assert!(!run.state.is_minimised(&window), "restored once unlocked");
    run.stop();
}

/// A state object made on a closed handle is inert: it is never sent an event, and the
/// compositor does not keep it.
#[test]
fn a_state_object_of_a_closed_window_is_inert() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    let gone = app.another(&mut run, "gone");
    let handle = dock.local.handle(&mut run, "gone");
    gone.toplevel.destroy();
    gone.xdg.destroy();
    gone.surface.destroy();
    app.roundtrip(&mut run);
    dock.local
        .pump(&mut run, "the window is closed", |client, _| {
            client.listed.iter().any(|l| l.title == "gone" && l.closed)
        });

    let object = dock
        .manager
        .get_toplevel_state(&handle, &dock.local.queue.handle(), ());
    dock.local.roundtrip(&mut run);
    dock.local.frames(&mut run, 3);
    assert_eq!(received(&dock.local, &object), []);
    assert!(run.state.toplevel_states.is_empty(), "not kept");
    run.stop();
}

/// A state object its client destroys leaves the compositor's list.
#[test]
fn a_destroyed_state_object_is_forgotten() {
    let mut run = Running::start(None);
    let mut dock = Dock::connect(&mut run);
    let mut app = Local::connect(&mut run, None);
    app.another(&mut run, "probe");
    let handle = dock.local.handle(&mut run, "probe");
    let object = dock
        .manager
        .get_toplevel_state(&handle, &dock.local.queue.handle(), ());
    dock.local.state_events(&mut run, &object);
    assert_eq!(run.state.toplevel_states.len(), 1);
    object.destroy();
    dock.local.roundtrip(&mut run);
    assert!(run.state.toplevel_states.is_empty(), "forgotten");
    run.stop();
}

/// `zwlr_output_manager_v1` is the control center's only (open question 10 of the OS9 plan):
/// the bar, whose line does not name it, does not see it. The control center sets the first
/// output's scale through it, which the output keeps in the next frame and a window already
/// mapped there is told as its preferred scale; a configuration that asks for more than a scale
/// fails, and one on a stale serial is cancelled.
#[test]
fn os9_the_output_manager_is_granted_to_the_control_center_only() {
    const MANAGER: &str = "zwlr_output_manager_v1";
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(!bar.sees(MANAGER), "the bar");
    let mut control_center = Local::control_center(&mut run);
    assert!(control_center.sees(MANAGER), "the control center");

    let manager: zwlr_output_manager_v1::ZwlrOutputManagerV1 = control_center.bind(4);
    control_center.pump(&mut run, "the heads are sent", |client, _| {
        client.output_serial.is_some()
    });
    let names: Vec<&str> = control_center
        .client
        .heads
        .iter()
        .map(|head| head.name.as_str())
        .collect();
    assert_eq!(names, ["headless-0", "headless-1"], "one head per output");
    let serial = control_center.client.output_serial.expect("a serial");

    // A window on the first output, mapped before the change, with its fractional scale. It is
    // larger than the fixture's, centred at the same place, so that a frame shows part of it
    // whichever is on top: only a surface shown on an output is sent that output's scale.
    let compositor: wl_compositor::WlCompositor = control_center.bind(4);
    let shm: wl_shm::WlShm = control_center.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = control_center.bind(1);
    let fractional: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1 =
        control_center.bind(1);
    let qh = control_center.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let _scale = fractional.get_fractional_scale(&surface, &qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();
    control_center.pump(&mut run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(control_center.client.configure.expect("a configure"));
    let (buffer, _) = control_center.buffer(&shm, 2 * WIDTH, 2 * HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    control_center.pump(&mut run, "the window is mapped at scale 1", |client, _| {
        client.fractional_scale == Some(120)
    });

    let first = run
        .state
        .space
        .outputs()
        .find(|output| output.name() == "headless-0")
        .expect("the first output")
        .clone();

    let turned = control_center.configure(&mut run, &manager, serial, "headless-0", |head| {
        head.set_scale(1.5);
        head.set_transform(wl_output::Transform::_90);
    });
    assert_eq!(turned, "failed", "a transform");
    let stale = serial.wrapping_add(1);
    let late = control_center.configure(&mut run, &manager, stale, "headless-0", |head| {
        head.set_scale(1.5)
    });
    assert_eq!(late, "cancelled", "a stale serial");
    assert_eq!(first.current_scale().fractional_scale(), 1.0, "unchanged");

    let scaled = control_center.configure(&mut run, &manager, serial, "headless-0", |head| {
        head.set_scale(1.5)
    });
    assert_eq!(scaled, "succeeded");
    let frame = run.state.capture(0).expect("the next frame");
    assert_eq!(
        frame.len(),
        (OUTPUTS[0].width * OUTPUTS[0].height * 4) as usize
    );
    assert_eq!(
        first.current_scale().fractional_scale(),
        1.5,
        "the new scale"
    );
    let area = run.state.space.output_geometry(&first).expect("mapped");
    assert!(
        area.size.w < OUTPUTS[0].width,
        "a smaller logical size: {area:?}"
    );
    control_center.pump(&mut run, "the new scale is sent", |client, _| {
        client.output_serial != Some(serial)
            && client
                .heads
                .iter()
                .any(|head| head.name == "headless-0" && head.scale == 1.5)
    });
    control_center.pump(&mut run, "the window is told the new scale", |client, _| {
        client.fractional_scale == Some(180)
    });
    run.stop();
}

/// A mode the compositor finished between the client's last `done` and its configuration is a
/// race with a hot-plug, not a protocol error: the configuration fails and the client stays
/// connected. A mode list that loses one mode and gains another, as many as before, is still a
/// change, sent with a new serial.
#[test]
fn os9_a_mode_finished_in_a_race_fails_the_configuration() {
    let mut run = Running::start(None);
    let mut control_center = Local::control_center(&mut run);
    let manager: zwlr_output_manager_v1::ZwlrOutputManagerV1 = control_center.bind(4);
    control_center.pump(&mut run, "the heads are sent", |client, _| {
        client.output_serial.is_some()
    });
    let first = run
        .state
        .space
        .outputs()
        .find(|output| output.name() == "headless-0")
        .expect("the first output")
        .clone();
    let sent = |client: &Client| {
        client
            .heads
            .iter()
            .find(|head| head.name == "headless-0")
            .map_or(0, |head| head.modes.len())
    };
    let before = sent(&control_center.client);
    let extra = |width: i32| smithay::output::Mode {
        size: (width, OUTPUTS[0].height).into(),
        refresh: 60_000,
    };

    first.add_mode(extra(OUTPUTS[0].width + 16));
    run.state.output_heads_changed();
    control_center.pump(&mut run, "the extra mode is sent", |client, _| {
        sent(client) == before + 1
    });
    control_center.roundtrip(&mut run);
    let serial = control_center.client.output_serial.expect("a serial");
    let gone = control_center
        .client
        .heads
        .iter()
        .find(|head| head.name == "headless-0")
        .and_then(|head| head.modes.last())
        .cloned()
        .expect("the extra mode");

    first.delete_mode(extra(OUTPUTS[0].width + 16));
    first.add_mode(extra(OUTPUTS[0].width + 32));
    run.state.output_heads_changed();
    control_center.pump(&mut run, "one mode for another is a change", |client, _| {
        client.output_serial.is_some_and(|now| now != serial)
    });
    let serial = control_center.client.output_serial.expect("a serial");
    assert_eq!(
        sent(&control_center.client),
        before + 2,
        "the new mode is sent"
    );

    let raced = control_center.configure(&mut run, &manager, serial, "headless-0", |head| {
        head.set_mode(&gone)
    });
    assert_eq!(raced, "failed", "a finished mode");
    let scaled = control_center.configure(&mut run, &manager, serial, "headless-0", |head| {
        head.set_scale(1.5)
    });
    assert_eq!(scaled, "succeeded", "the client is still connected");
    run.stop();
}

/// A virtual keyboard of zwp-virtual-keyboard-v1, whose client side no crate of the workspace
/// generates: its requests are sent raw, on the interfaces Smithay's server side describes.
struct VirtualKeyboard {
    backend: Backend,
    id: ObjectId,
    /// The file of its keymap, open while the compositor may read it.
    _keymap: std::fs::File,
}

/// The object data of the raw objects: zwp-virtual-keyboard-v1 has no events.
struct Silent;

impl ObjectData for Silent {
    fn event(
        self: Arc<Self>,
        _: &Backend,
        _: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData>> {
        None
    }
    fn destroyed(&self, _: ObjectId) {}
}

impl VirtualKeyboard {
    /// Binds the manager and creates a keyboard on the seat, with `keymap`, an xkb keymap, or
    /// the seat's own keymap when none.
    fn new(local: &mut Local, run: &mut Running, keymap: Option<&str>) -> Self {
        let seat: wl_seat::WlSeat = local.bind(1);
        let manager = &ZWP_VIRTUAL_KEYBOARD_MANAGER_V1_INTERFACE;
        let (name, _, _) = local
            .client
            .globals
            .iter()
            .find(|(_, interface, _)| interface == manager.name)
            .expect("the virtual keyboard manager");
        let bind = wl_registry::Request::Bind {
            name: *name,
            id: (manager, 1),
        };
        let manager = local
            .connection
            .send_request(&local.registry, bind, Some(Arc::new(Silent)))
            .expect("bind");
        let backend = local.connection.backend();
        let create = Message {
            sender_id: manager,
            opcode: 0,
            args: [
                Argument::Object(seat.id()),
                Argument::NewId(ObjectId::null()),
            ]
            .into_iter()
            .collect(),
        };
        let id = backend
            .send_request(
                create,
                Some(Arc::new(Silent)),
                Some((&ZWP_VIRTUAL_KEYBOARD_V1_INTERFACE, 1)),
            )
            .expect("a virtual keyboard");
        let mut file = unlinked().expect("keymap file");
        // `wl_keyboard.keymap_format`: no_keymap, or xkb_v1 in a NUL-terminated string.
        let (format, size) = match keymap {
            Some(text) => {
                file.write_all(text.as_bytes()).expect("keymap");
                file.write_all(&[0]).expect("keymap");
                (1, text.len() + 1)
            }
            None => (0, 0),
        };
        let keyboard = Self {
            backend,
            id,
            _keymap: file,
        };
        keyboard.send(
            0,
            [
                Argument::Uint(format),
                Argument::Fd(std::os::fd::AsRawFd::as_raw_fd(&keyboard._keymap)),
                Argument::Uint(u32::try_from(size).expect("keymap size")),
            ],
        );
        local.roundtrip(run);
        keyboard
    }

    /// Presses the keys of evdev codes `codes` in order and releases them in reverse, as
    /// `chord` does on the hardware path.
    fn chord(&self, local: &mut Local, run: &mut Running, codes: &[u32]) {
        let presses = codes.iter().map(|code| (*code, 1));
        for (code, state) in presses.chain(codes.iter().rev().map(|code| (*code, 0))) {
            self.send(
                1,
                [
                    Argument::Uint(0),
                    Argument::Uint(code),
                    Argument::Uint(state),
                ],
            );
        }
        local.roundtrip(run);
    }

    /// Presses or releases the key of evdev code `code`.
    fn key(&self, local: &mut Local, run: &mut Running, code: u32, pressed: bool) {
        let state = u32::from(pressed);
        self.send(
            1,
            [
                Argument::Uint(0),
                Argument::Uint(code),
                Argument::Uint(state),
            ],
        );
        local.roundtrip(run);
    }

    /// Sets the group `group`, with no modifier.
    fn group(&self, local: &mut Local, run: &mut Running, group: u32) {
        self.send(
            2,
            [
                Argument::Uint(0),
                Argument::Uint(0),
                Argument::Uint(0),
                Argument::Uint(group),
            ],
        );
        local.roundtrip(run);
    }

    /// Sets the depressed modifiers `depressed`, none latched or locked, in the first group.
    fn modifiers(&self, local: &mut Local, run: &mut Running, depressed: u32) {
        self.send(
            2,
            [
                Argument::Uint(depressed),
                Argument::Uint(0),
                Argument::Uint(0),
                Argument::Uint(0),
            ],
        );
        local.roundtrip(run);
    }

    fn send<const N: usize>(&self, opcode: u16, args: [Argument<ObjectId, i32>; N]) {
        let message = Message {
            sender_id: self.id.clone(),
            opcode,
            args: args.into_iter().collect(),
        };
        self.backend
            .send_request(message, None, None)
            .expect("a virtual keyboard request");
    }
}

/// A keymap of two keys whose evdev codes are other keys in the seat's: Escape at the code of
/// A (30), and Super at the code of S (31).
const SWAPPED_KEYMAP: &str = r#"xkb_keymap {
    xkb_keycodes "swapped" { minimum = 8; maximum = 255; <ESCA> = 38; <SUPR> = 39; };
    xkb_types "swapped" { include "complete" };
    xkb_compatibility "swapped" { include "complete" };
    xkb_symbols "swapped" {
        key <ESCA> { [ Escape ] };
        key <SUPR> { symbols[Group1] = [ Super_L ], actions[Group1] = [ SetMods(modifiers=Mod4) ] };
        modifier_map Mod4 { <SUPR> };
    };
};"#;

/// The seat's keymap, as the compositor sends it to its clients.
fn seat_keymap(state: &mut State) -> String {
    let keyboard = state.seat.get_keyboard().expect("a keyboard");
    keyboard.with_xkb_state(state, |context| {
        context
            .xkb()
            .lock()
            .expect("the seat's keymap")
            .keymap_as_string()
    })
}

/// A virtual key takes the hardware path (OS9 task 6): a granted client's virtual Super+Escape,
/// typed with a keymap of its own, takes the keyboard back from an exclusive top surface and asks
/// for the lock, as `task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock` does with a
/// hardware key; the surface sees Super go down, never the Escape. The keymap applies to its own
/// keys only: the same codes on the hardware keyboard, A and S, ask for nothing, and the seat's
/// keymap is its own again.
#[test]
fn os9_a_virtual_key_takes_the_hardware_path() {
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);
    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        focus(state).and_then(|s| s.client()).map(|c| c.id()) == Some(shell_id.clone())
    });
    let keymap = seat_keymap(&mut run.state);

    let mut local = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut local, &mut run, Some(SWAPPED_KEYMAP));
    keyboard.chord(&mut local, &mut run, &[31, 30]);
    assert_eq!(run.state.lock_requests, 1, "Super+Escape asks for the lock");
    assert_eq!(
        focus(&run.state),
        window,
        "the press takes the keyboard back"
    );
    assert_eq!(
        run.state.escaped.len(),
        1,
        "the shell's identity is escaped"
    );
    assert!(
        run.state.kept_keys.is_empty(),
        "the release is kept as well"
    );
    assert_eq!(
        shell_keys(&mut run, &steps),
        [31],
        "the surface saw Super go down, never the Escape"
    );
    chord(&mut run.state, &[31, 30]);
    assert_eq!(
        run.state.lock_requests, 1,
        "S and A on the hardware keyboard are S and A"
    );
    assert_eq!(
        seat_keymap(&mut run.state),
        keymap,
        "a hardware key gives the seat its keymap back"
    );

    drop(steps);
    shell
        .join()
        .expect("the shell's thread")
        .expect("the shell's client");
    run.stop();
}

/// The keymap follows the keyboard that sent the last key, as wlroots has it (OS9 task 6 audit):
/// from a virtual key's press to its release the focused client has the virtual keyboard's
/// keymap, so that its own key repeat reads the held key under it; a second virtual key sends no
/// keymap again; a hardware key after them is sent the seat's keymap first.
#[test]
fn os9_the_keymap_follows_the_last_keyboard() {
    let mut run = Running::start(None);
    let (mut local, _) = focused_window(&mut run);
    let seat_size = local.client.keymap_size;
    let keymap = seat_keymap(&mut run.state);
    let typist = VirtualKeyboard::new(&mut local, &mut run, Some(SWAPPED_KEYMAP));
    let keymaps = local.client.keymaps;

    // Escape, in the virtual keyboard's keymap, held.
    typist.key(&mut local, &mut run, 30, true);
    let own_size = local.client.keymap_size;
    assert_ne!(own_size, seat_size, "the client has the virtual keymap");
    assert_eq!(
        local.client.keys_under,
        [own_size],
        "the press is read under it"
    );
    assert_ne!(
        seat_keymap(&mut run.state),
        keymap,
        "the seat keeps it while the key is held"
    );
    typist.key(&mut local, &mut run, 30, false);
    typist.key(&mut local, &mut run, 31, true);
    typist.key(&mut local, &mut run, 31, false);
    assert_eq!(
        local.client.keymaps,
        keymaps + 1,
        "two virtual keys in a row send the keymap once"
    );

    // A, on the hardware keyboard.
    chord(&mut run.state, &[30]);
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.keys_under,
        [own_size, own_size, seat_size],
        "a hardware key is read under the seat's keymap, sent before it"
    );
    assert_eq!(
        seat_keymap(&mut run.state),
        keymap,
        "the seat has its keymap back"
    );

    // A virtual keyboard that goes while the seat has its keymap gives the seat its own back.
    let mut leaving = Local::connect(&mut run, None);
    let gone = VirtualKeyboard::new(&mut leaving, &mut run, Some(SWAPPED_KEYMAP));
    gone.chord(&mut leaving, &mut run, &[30]);
    assert!(
        run.state.virtual_keymap.is_some(),
        "the seat has the leaving keyboard's keymap"
    );
    drop(gone);
    drop(leaving);
    run.until("the gone keyboard's keymap goes with it", |state| {
        state.virtual_keymap.is_none()
    });
    assert_eq!(
        seat_keymap(&mut run.state),
        keymap,
        "the seat has its keymap back once more"
    );
    run.stop();
}

/// A client with a mapped toplevel and a `wl_keyboard`, given the keyboard, and its window.
fn focused_window(run: &mut Running) -> (Local, smithay::desktop::Window) {
    let mut local = Local::connect(run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let seat: wl_seat::WlSeat = local.bind(1);
    let qh = local.queue.handle();
    let _keyboard = seat.get_keyboard(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();
    local.pump(run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let id = local.id.clone().expect("a client the fixture inserted");
    let ours = |state: &State| {
        state
            .space
            .elements()
            .find(|w| {
                w.toplevel()
                    .and_then(|t| t.wl_surface().client())
                    .map(|c| c.id())
                    == Some(id.clone())
            })
            .cloned()
    };
    local.pump(run, "the window is mapped", |_, state| {
        ours(state).is_some()
    });
    let window = ours(&run.state).expect("the window");
    run.state.focus(&window);
    local.pump(run, "the window has the keyboard", |client, state| {
        let focus = state.seat.get_keyboard().and_then(|k| k.current_focus());
        client.keymaps > 0 && focus.and_then(|s| s.client()).map(|c| c.id()) == Some(id.clone())
    });
    (local, window)
}

/// A virtual keyboard's modifiers reach the seat at once while its keymap is current (OS9 task 6
/// audit): a Shift it lets go of with no key after it is let go for the focused client, and a
/// client focused afterwards is not entered with Shift.
#[test]
fn os9_a_virtual_modifier_change_reaches_the_seat_at_once() {
    const SHIFT: u32 = 1;
    let mut run = Running::start(None);
    let (mut other, other_window) = focused_window(&mut run);
    let (mut local, _) = focused_window(&mut run);
    let typist = VirtualKeyboard::new(&mut local, &mut run, Some(SWAPPED_KEYMAP));
    typist.modifiers(&mut local, &mut run, SHIFT);
    typist.key(&mut local, &mut run, 30, true);
    typist.key(&mut local, &mut run, 30, false);
    assert_eq!(
        local.client.modifiers.last(),
        Some(&SHIFT),
        "the key is read with Shift"
    );
    typist.modifiers(&mut local, &mut run, 0);
    assert_eq!(
        local.client.modifiers.last(),
        Some(&0),
        "Shift is let go with no key after it"
    );
    let sent = other.client.modifiers.len();
    run.state.focus(&other_window);
    other.pump(&mut run, "the other window has the keyboard", |client, _| {
        client.modifiers.len() > sent
    });
    assert_eq!(
        other.client.modifiers.last(),
        Some(&0),
        "a client focused afterwards is not entered with Shift"
    );
    let shift = run
        .state
        .seat
        .get_keyboard()
        .expect("a keyboard")
        .modifier_state()
        .shift;
    assert!(!shift, "the seat holds no Shift");
    run.stop();
}

/// The window under the pointer and the id of its client.
fn window_under_pointer(run: &Running) -> (WlSurface, ClientId) {
    let (window, id) = fixture_window(run);
    let surface = window.toplevel().expect("a toplevel").wl_surface().clone();
    (surface, id)
}

/// Motion and a button from a granted client's virtual pointer reach the window under the
/// pointer (OS9 task 6): the window has the pointer, and the button's serial was sent to its
/// client.
#[test]
fn os9_a_virtual_pointer_moves_and_clicks() {
    let mut run = Running::start(None);
    let (surface, window_client) = window_under_pointer(&run);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = local.bind(2);
    let pointer = manager.create_virtual_pointer(Some(&seat), &local.queue.handle(), ());
    let at = run
        .state
        .seat
        .get_pointer()
        .expect("a pointer")
        .current_location();
    // The red half of the window, on the first output.
    let (w, h) = (f64::from(OUTPUTS[0].width), f64::from(OUTPUTS[0].height));
    let target = Point::from((w / 2.0, h / 8.0 + f64::from(HEIGHT) / 4.0));
    pointer.motion(0, target.x - at.x, target.y - at.y);
    pointer.frame();
    local.roundtrip(&mut run);
    let seat_pointer = run.state.seat.get_pointer().expect("a pointer");
    assert_eq!(
        seat_pointer.current_location(),
        target,
        "moved by its delta"
    );
    assert_eq!(
        seat_pointer.current_focus().as_ref(),
        Some(&surface),
        "the window has the pointer"
    );

    run.state.grab_serials.button = None;
    // BTN_LEFT.
    pointer.button(
        0,
        0x110,
        wayland_client::protocol::wl_pointer::ButtonState::Pressed,
    );
    pointer.button(
        0,
        0x110,
        wayland_client::protocol::wl_pointer::ButtonState::Released,
    );
    pointer.frame();
    local.roundtrip(&mut run);
    assert_eq!(
        run.state
            .grab_serials
            .button
            .as_ref()
            .map(|(_, client)| client),
        Some(&window_client),
        "the press went to the window's client"
    );
    run.stop();
}

/// A virtual pointer's code outside evdev's buttons (BTN_MISC to KEY_MAX) is no button (OS9 task
/// 6 audit): it reaches no client and is not counted as held, so a pointer cannot grow its held
/// set without bound; a button code goes on.
#[test]
fn os9_a_virtual_pointer_code_outside_the_buttons_is_dropped() {
    use wayland_client::protocol::wl_pointer::ButtonState;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = local.bind(2);
    let pointer = manager.create_virtual_pointer(Some(&seat), &local.queue.handle(), ());
    run.state.last_input_serial = None;
    for code in [0, 0xff, 0x300, u32::MAX] {
        pointer.button(0, code, ButtonState::Pressed);
    }
    pointer.frame();
    local.roundtrip(&mut run);
    assert!(
        run.state.last_input_serial.is_none(),
        "no press reached the seat"
    );
    assert!(
        run.state.virtual_buttons.values().all(Vec::is_empty),
        "no code is held"
    );
    // BTN_LEFT and KEY_MAX, the range's last code.
    for code in [0x110, 0x2ff] {
        pointer.button(0, code, ButtonState::Pressed);
    }
    pointer.frame();
    local.roundtrip(&mut run);
    assert!(
        run.state.last_input_serial.is_some(),
        "a button reached the seat"
    );
    assert_eq!(
        run.state
            .virtual_buttons
            .values()
            .flatten()
            .collect::<Vec<_>>(),
        [&0x110, &0x2ff],
        "both buttons are held"
    );
    run.stop();
}

/// Absolute motion to the far edge of the output a virtual pointer is bound to leaves it on that
/// output's last column (OS9 task 6 audit): x == x_extent maps to the first column of the output
/// beside it, which `confine` alone accepts. The height is one both outputs have.
#[test]
fn os9_a_virtual_pointer_bound_to_an_output_stays_on_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let output: wl_output::WlOutput = local.bind(4);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = local.bind(2);
    let pointer = manager.create_virtual_pointer_with_output(
        Some(&seat),
        Some(&output),
        &local.queue.handle(),
        (),
    );
    pointer.motion_absolute(0, 100, 10, 100, 100);
    pointer.frame();
    local.roundtrip(&mut run);
    let first = run
        .state
        .space
        .outputs()
        .find(|output| output.name() == "headless-0")
        .and_then(|output| run.state.space.output_geometry(output))
        .expect("the first output");
    let y = first.loc.y + first.size.h / 10;
    let beside = Point::from((first.loc.x + first.size.w, y));
    assert!(
        run.state
            .space
            .outputs()
            .filter_map(|output| run.state.space.output_geometry(output))
            .any(|area| area.contains(beside)),
        "the unclamped point is on the other output"
    );
    let at = run
        .state
        .seat
        .get_pointer()
        .expect("a pointer")
        .current_location();
    assert_eq!(
        at,
        Point::from((f64::from(first.loc.x + first.size.w - 1), f64::from(y))),
        "on the first output's last column"
    );
    run.stop();
}

/// Absolute and relative motion of a virtual pointer past the edge of the fixture's two outputs
/// leaves the pointer on an output (OS9 task 6), as
/// `task8_an_output_that_shrinks_strands_no_window_and_no_pointer` requires of the hardware one.
#[test]
fn os9_a_virtual_pointer_stays_inside_the_outputs() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = local.bind(2);
    let pointer = manager.create_virtual_pointer(Some(&seat), &local.queue.handle(), ());
    let on_an_output = |state: &State| {
        let at = state
            .seat
            .get_pointer()
            .expect("a pointer")
            .current_location();
        state
            .space
            .outputs()
            .filter_map(|output| state.space.output_geometry(output))
            .any(|output| output.to_f64().contains(at))
    };
    // The far corner of the outputs' bounding box, below the smaller second output.
    pointer.motion_absolute(0, 100, 100, 100, 100);
    pointer.frame();
    local.roundtrip(&mut run);
    assert!(on_an_output(&run.state), "absolute, at the far corner");
    for (dx, dy, what) in [
        (10_000.0, 10_000.0, "right and down"),
        (-10_000.0, 10_000.0, "left and down"),
        (-10_000.0, -10_000.0, "left and up"),
        (10_000.0, -10_000.0, "right and up"),
    ] {
        pointer.motion(0, dx, dy);
        pointer.frame();
        local.roundtrip(&mut run);
        assert!(on_an_output(&run.state), "relative, {what}");
    }
    run.stop();
}

/// Locks the session for a new client, with a lock surface on the first output that has the
/// keyboard, and returns the client, which records its keys, and its lock.
fn lock_session(run: &mut Running) -> (Local, ext_session_lock_v1::ExtSessionLockV1) {
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    let mut locker = Local::connect(run, None);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let shm: wl_shm::WlShm = locker.bind(1);
    let output: wl_output::WlOutput = locker.bind(4);
    let seat: wl_seat::WlSeat = locker.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let qh = locker.queue.handle();
    let _keyboard = seat.get_keyboard(&qh, ());
    let lock = locks.lock(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    locker.pump(run, "the lock surface is configured", |client, _| {
        client.locked && client.lock_configure.is_some()
    });
    let (serial, width, height) = locker.client.lock_configure.expect("a configure");
    lock_surface.ack_configure(serial);
    let (buffer, _) = locker.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let id = locker.id.clone().expect("a client the fixture inserted");
    locker.pump(run, "the lock surface has the keyboard", |_, state| {
        focus(state).and_then(|s| s.client()).map(|c| c.id()) == Some(id.clone())
    });
    (locker, lock)
}

/// A virtual keyboard while the session is locked (open question 12 of the OS9 plan): with the
/// lock surface focused, a granted client's virtual key reaches neither the lock surface nor any
/// other client; a hardware key reaches it; after unlock the same virtual key reaches the window
/// again.
#[test]
fn os9_a_virtual_key_is_dropped_while_locked() {
    let mut run = Running::start(None);
    let (window, window_client) = window_under_pointer(&run);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).as_ref() == Some(&window)
    });

    let (mut locker, lock) = lock_session(&mut run);

    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.grab_serials.key_press = None;
    keyboard.chord(&mut typist, &mut run, &[30]);
    locker.roundtrip(&mut run);
    assert!(locker.client.keys.is_empty(), "the lock surface saw no key");
    assert_eq!(
        run.state.grab_serials.key_press, None,
        "no client was sent a key"
    );

    chord(&mut run.state, &[30]);
    locker.pump(
        &mut run,
        "a hardware key reaches the lock surface",
        |client, _| client.keys == [30],
    );

    lock.unlock_and_destroy();
    locker.pump(
        &mut run,
        "unlocking gives the window the keyboard",
        |_, state| focus(state).as_ref() == Some(&window),
    );
    run.state.grab_serials.key_press = None;
    keyboard.chord(&mut typist, &mut run, &[30]);
    assert_eq!(
        run.state
            .grab_serials
            .key_press
            .as_ref()
            .map(|(_, client)| client),
        Some(&window_client),
        "after unlock the virtual key reaches the window"
    );
    assert_eq!(locker.client.keys, [30], "and not the locker");
    run.stop();
}

/// Moves `pointer` by `(dx, dy)` and clicks its left button.
fn virtual_click(
    pointer: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    local: &mut Local,
    run: &mut Running,
    (dx, dy): (f64, f64),
) {
    use wayland_client::protocol::wl_pointer::ButtonState;
    pointer.motion(0, dx, dy);
    pointer.frame();
    // BTN_LEFT.
    pointer.button(0, 0x110, ButtonState::Pressed);
    pointer.button(0, 0x110, ButtonState::Released);
    pointer.frame();
    local.roundtrip(run);
}

/// A virtual pointer while the session is locked (the maintainer's decision of 2026-10-23): a
/// granted client's virtual motion and click over the lock surface reach nothing; a hardware
/// click reaches the lock surface; after unlock the same virtual click reaches the window.
#[test]
fn os9_a_virtual_pointer_is_dropped_while_locked() {
    use smithay::backend::input::{ButtonState, InputTime};
    let mut run = Running::start(None);
    let (window, window_client) = window_under_pointer(&run);
    let mut clicker = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = clicker.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = clicker.bind(2);
    let pointer = manager.create_virtual_pointer(Some(&seat), &clicker.queue.handle(), ());
    let (mut locker, lock) = lock_session(&mut run);
    let locker_id = locker.id.clone().expect("a client the fixture inserted");
    let seat_pointer = run.state.seat.get_pointer().expect("a pointer");
    // The red half of the window, on the first output.
    let (w, h) = (f64::from(OUTPUTS[0].width), f64::from(OUTPUTS[0].height));
    let target = Point::from((w / 2.0, h / 8.0 + f64::from(HEIGHT) / 4.0));
    let to_target = |at: Point<f64, smithay::utils::Logical>| (target.x - at.x, target.y - at.y);

    let at = seat_pointer.current_location();
    run.state.grab_serials.button = None;
    virtual_click(&pointer, &mut clicker, &mut run, to_target(at));
    assert_eq!(
        seat_pointer.current_location(),
        at,
        "the pointer did not move"
    );
    assert_eq!(
        run.state.grab_serials.button, None,
        "no client was sent a button"
    );

    run.state
        .button_input(0x110, ButtonState::Pressed, InputTime::now());
    run.state
        .button_input(0x110, ButtonState::Released, InputTime::now());
    assert_eq!(
        run.state
            .grab_serials
            .button
            .as_ref()
            .map(|(_, client)| client),
        Some(&locker_id),
        "a hardware click reaches the lock surface"
    );

    lock.unlock_and_destroy();
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    locker.pump(
        &mut run,
        "unlocking gives the window the keyboard",
        |_, state| focus(state).as_ref() == Some(&window),
    );
    run.state.grab_serials.button = None;
    let at = seat_pointer.current_location();
    virtual_click(&pointer, &mut clicker, &mut run, to_target(at));
    assert_eq!(
        seat_pointer.current_location(),
        target,
        "moved by its delta"
    );
    assert_eq!(
        run.state
            .grab_serials
            .button
            .as_ref()
            .map(|(_, client)| client),
        Some(&window_client),
        "after unlock the virtual click reaches the window"
    );
    run.stop();
}

/// The keys a surface that takes the keyboard is told are held: those of a new lock surface's
/// `enter`, the session locked and unlocked again around it.
fn held_at_the_next_enter(run: &mut Running) -> Vec<u32> {
    let (mut locker, lock) = lock_session(run);
    locker.roundtrip(run);
    let held = locker.client.entered_keys.clone();
    lock.unlock_and_destroy();
    locker.roundtrip(run);
    held
}

/// Waits for the window to have the keyboard, and returns a granted client's virtual keyboard
/// holding A down in it.
fn holding_a(run: &mut Running) -> (Local, VirtualKeyboard) {
    let (window, _) = window_under_pointer(run);
    run.until("the window has the keyboard", |state| {
        state.seat.get_keyboard().and_then(|k| k.current_focus()) == Some(window.clone())
    });
    let mut typist = Local::connect(run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, run, None);
    keyboard.key(&mut typist, run, 30, true);
    (typist, keyboard)
}

/// A virtual key held when the session locks and released while it is locked is let go (the
/// OS9 task 6 audit): the lock surface is told it is held, and the next surface to take the
/// keyboard after unlock is told of no key held.
#[test]
fn os9_a_virtual_key_held_across_the_lock_is_let_go() {
    let mut run = Running::start(None);
    let (mut typist, keyboard) = holding_a(&mut run);
    let (mut locker, lock) = lock_session(&mut run);
    locker.roundtrip(&mut run);
    assert_eq!(
        locker.client.entered_keys,
        [30],
        "the lock surface sees A held"
    );
    keyboard.key(&mut typist, &mut run, 30, false);
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    assert_eq!(held_at_the_next_enter(&mut run), [0; 0], "A is let go");
    run.stop();
}

/// A virtual keyboard whose client disconnects while the session is locked, with a key held
/// from before the lock, leaves no key held after unlock (the OS9 task 6 audit).
#[test]
fn os9_a_virtual_keyboard_gone_while_locked_leaves_nothing_held() {
    let mut run = Running::start(None);
    let (typist, keyboard) = holding_a(&mut run);
    let (mut locker, lock) = lock_session(&mut run);
    drop(keyboard);
    drop(typist);
    run.until("the gone keyboard's key is let go", |state| {
        state
            .seat
            .get_keyboard()
            .is_some_and(|keyboard| keyboard.pressed_keys().is_empty())
    });
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    assert_eq!(held_at_the_next_enter(&mut run), [0; 0], "A is let go");
    run.stop();
}

/// A virtual keyboard with a keymap of its own typing while the session is locked sends its
/// keymap to no client (the OS9 task 6 audit): the lock surface's client is sent no keymap and
/// no key.
#[test]
fn os9_a_virtual_keymap_is_not_sent_while_locked() {
    let mut run = Running::start(None);
    let (mut locker, _lock) = lock_session(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(SWAPPED_KEYMAP));
    locker.roundtrip(&mut run);
    let keymaps = locker.client.keymaps;
    keyboard.chord(&mut typist, &mut run, &[30, 31]);
    locker.roundtrip(&mut run);
    assert_eq!(locker.client.keymaps, keymaps, "no keymap was sent");
    assert!(locker.client.keys.is_empty(), "no key was sent");
    run.stop();
}

/// A virtual button pressed before the session locks and released while it is locked is let go
/// (the OS9 task 6 audit): a later hardware click on the lock surface ends its grab.
#[test]
fn os9_a_virtual_button_released_while_locked_lets_a_click_end() {
    use smithay::backend::input::{ButtonState, InputTime};
    use wayland_client::protocol::wl_pointer;
    let mut run = Running::start(None);
    let _ = window_under_pointer(&run);
    let mut clicker = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = clicker.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = clicker.bind(2);
    let pointer = manager.create_virtual_pointer(Some(&seat), &clicker.queue.handle(), ());
    // BTN_LEFT, over the window.
    pointer.button(0, 0x110, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    clicker.roundtrip(&mut run);
    let seat_pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(seat_pointer.is_grabbed(), "the press grabs the pointer");

    let (_locker, _lock) = lock_session(&mut run);
    pointer.button(0, 0x110, wl_pointer::ButtonState::Released);
    pointer.frame();
    clicker.roundtrip(&mut run);
    // BTN_RIGHT, on the hardware.
    run.state
        .button_input(0x111, ButtonState::Pressed, InputTime::now());
    run.state
        .button_input(0x111, ButtonState::Released, InputTime::now());
    assert!(!seat_pointer.is_grabbed(), "the click's grab ended");
    assert!(
        run.state.virtual_buttons.values().all(Vec::is_empty),
        "no virtual button is held"
    );
    run.stop();
}

/// A virtual pointer's release while the session is locked goes on only for a button that same
/// pointer holds, and is no activity (the second OS9 task 6 audit): A holds the left button from
/// before the lock; on the lock surface the user presses it on the hardware; B's release of it
/// reaches no client and leaves the user's click held; A's release lets go, and the locked
/// session stays idle.
#[test]
fn os9_a_virtual_release_while_locked_is_the_same_pointers() {
    use smithay::backend::input::{ButtonState, InputTime};
    use wayland_client::protocol::wl_pointer;
    let mut run = Running::start(None);
    let _ = window_under_pointer(&run);
    let mut clicker = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = clicker.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = clicker.bind(2);
    let qh = clicker.queue.handle();
    let a = manager.create_virtual_pointer(Some(&seat), &qh, ());
    let b = manager.create_virtual_pointer(Some(&seat), &qh, ());
    // BTN_LEFT, over the window.
    a.button(0, 0x110, wl_pointer::ButtonState::Pressed);
    a.frame();
    clicker.roundtrip(&mut run);

    let (_locker, _lock) = lock_session(&mut run);
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = clicker.bind(1);
    let _notification = notifier.get_idle_notification(100, &seat, &qh, ());
    clicker.pump(&mut run, "the locked session is idle", |client, _| {
        client.idle == ["idled"]
    });
    let seat_pointer = run.state.seat.get_pointer().expect("a pointer");
    run.state
        .button_input(0x110, ButtonState::Pressed, InputTime::now());
    assert!(
        seat_pointer.is_grabbed(),
        "the user's click on the lock surface"
    );

    b.button(0, 0x110, wl_pointer::ButtonState::Released);
    b.frame();
    clicker.roundtrip(&mut run);
    assert!(
        seat_pointer.is_grabbed(),
        "B's release leaves the user's click held"
    );

    a.button(0, 0x110, wl_pointer::ButtonState::Released);
    a.frame();
    clicker.roundtrip(&mut run);
    assert!(!seat_pointer.is_grabbed(), "A's release lets go");
    assert!(
        run.state.virtual_buttons.values().all(Vec::is_empty),
        "no virtual button is held"
    );
    assert_eq!(clicker.client.idle, ["idled"], "the session stays idle");
    run.stop();
}

/// A virtual pointer made and dropped is no input: the idle session stays idle, locked or not.
/// A press dropped while locked leaves no entry behind.
#[test]
fn os9_a_virtual_pointer_coming_and_going_is_no_activity() {
    use wayland_client::protocol::wl_pointer;
    for locked in [true, false] {
        let mut run = Running::start(None);
        let _lock = locked.then(|| lock_session(&mut run));
        let mut clicker = Local::connect(&mut run, None);
        let seat: wl_seat::WlSeat = clicker.bind(1);
        let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = clicker.bind(2);
        let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = clicker.bind(1);
        let qh = clicker.queue.handle();
        let _notification = notifier.get_idle_notification(100, &seat, &qh, ());
        clicker.pump(&mut run, "the session is idle", |client, _| {
            client.idle == ["idled"]
        });
        let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
        if locked {
            // BTN_LEFT, dropped while locked.
            pointer.button(0, 0x110, wl_pointer::ButtonState::Pressed);
            pointer.frame();
        }
        clicker.roundtrip(&mut run);
        assert!(
            run.state.virtual_buttons.is_empty(),
            "no entry for a pointer that holds nothing (locked: {locked})"
        );
        pointer.destroy();
        clicker.roundtrip(&mut run);
        assert_eq!(
            clicker.client.idle,
            ["idled"],
            "the session stays idle (locked: {locked})"
        );
        run.stop();
    }
}

/// A capture source of output `index`, named by `local`'s own `wl_output` of it, and a session
/// of it that has its constraints.
fn output_session(
    local: &mut Local,
    run: &mut Running,
    copy: &ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    index: usize,
) -> (
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
) {
    let sources: ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1 =
        local.bind(1);
    let qh = local.queue.handle();
    let (name, version) = local
        .client
        .globals
        .iter()
        .filter(|(_, interface, _)| interface == "wl_output")
        .nth(index)
        .map(|(name, _, version)| (*name, *version))
        .expect("the output's global");
    let output: wl_output::WlOutput = local.registry.bind(name, version.min(4), &qh, ());
    let source = sources.create_source(&output, &qh, ());
    let session = copy.create_session(
        &source,
        ext_image_copy_capture_manager_v1::Options::empty(),
        &qh,
        (),
    );
    local.pump(run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    (source, session)
}

/// The whole of `file`, `len` bytes from its start.
fn contents(file: &std::fs::File, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    file.read_exact_at(&mut bytes, 0).expect("the buffer");
    bytes
}

/// A client granted `ext_output_image_capture_source_manager_v1`, the bar, captures the
/// fixture's second output by its `wl_output` (OS4, register row 10). The window is moved onto
/// that output, and the buffer then holds, byte for byte, the pixels the fixture reads back from
/// it, in the client's B, G, R, A: the window's red among them, so a buffer left as it was fails.
#[test]
fn os9_an_output_is_captured_by_a_granted_client() {
    let mut run = Running::start(None);
    let (window, _) = fixture_window(&run);
    let second = run.state.space.outputs().nth(1).expect("output 1").clone();
    let area = run.state.space.output_geometry(&second).expect("mapped");
    run.state
        .space
        .map_element(window, area.loc + Point::from((40, 40)), false);

    let mut bar = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = bar.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = bar.bind(1);
    let (_source, session) = output_session(&mut bar, &mut run, &copy, 1);
    let (w, h) = (OUTPUTS[1].width, OUTPUTS[1].height);
    assert_eq!(
        bar.client.capture_size,
        Some((w as u32, h as u32)),
        "the output's size in pixels"
    );

    let (buffer, file) = bar.buffer(&shm, w, h, [0x55; 4]);
    let frame = session.create_frame(&bar.queue.handle(), ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, w, h);
    frame.capture();
    bar.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(bar.client.captured, Some(Ok(())), "the frame is ready");

    let expected: Vec<u8> = run
        .state
        .capture(1)
        .expect("frame of output 1")
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    let captured = contents(&file, expected.len());
    assert!(
        captured == expected,
        "the buffer is output 1 as the fixture reads it back"
    );
    assert!(
        captured.as_chunks::<4>().0.contains(&[0, 0, 255, 255]),
        "the window's red is in it"
    );
    run.stop();
}

/// While the session is locked no output is captured (OS4, register row 10). A frame asked for
/// in the same flush as the lock fails, its session is stopped, and the buffer keeps its own
/// bytes; a new session of the output is refused.
#[test]
fn os9_output_capture_is_refused_while_locked() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = bar.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = bar.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = bar.bind(1);
    let (source, session) = output_session(&mut bar, &mut run, &copy, 1);
    let (w, h) = (OUTPUTS[1].width, OUTPUTS[1].height);
    let (buffer, file) = bar.buffer(&shm, w, h, [0x55; 4]);

    let qh = bar.queue.handle();
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, w, h);
    frame.capture();
    // The compositor reads both requests before it draws the next frame.
    let _lock = locks.lock(&qh, ());
    let stopped = session.id();
    bar.pump(
        &mut run,
        "the frame fails and the session stops",
        |client, _| client.captured.is_some() && client.stopped.contains(&stopped),
    );
    assert!(
        matches!(bar.client.captured, Some(Err(_))),
        "the frame fails: {:?}",
        bar.client.captured
    );
    let len = usize::try_from(w * h * 4).expect("size");
    assert!(
        contents(&file, len).iter().all(|&byte| byte == 0x55),
        "no pixel of the output reaches the buffer"
    );

    let refused = copy
        .create_session(
            &source,
            ext_image_copy_capture_manager_v1::Options::empty(),
            &qh,
            (),
        )
        .id();
    bar.pump(
        &mut run,
        "a session is refused while locked",
        |client, _| client.stopped.contains(&refused),
    );
    run.stop();
}

/// The events a raw object was sent, in order: each one's name, and its string and number
/// arguments as text.
#[derive(Default)]
struct Recorder(Mutex<Vec<(&'static str, Vec<String>)>>);

impl ObjectData for Recorder {
    fn event(
        self: Arc<Self>,
        backend: &Backend,
        message: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData>> {
        let name = backend
            .info(message.sender_id)
            .ok()
            .and_then(|info| info.interface.events.get(usize::from(message.opcode)))
            .map_or("unknown", |event| event.name);
        let args = message
            .args
            .into_iter()
            .map(|arg| match arg {
                Argument::Str(Some(text)) => text.to_string_lossy().into_owned(),
                Argument::Uint(n) => n.to_string(),
                Argument::Int(n) => n.to_string(),
                _ => String::new(),
            })
            .collect();
        self.0.lock().expect("events").push((name, args));
        None
    }
    fn destroyed(&self, _: ObjectId) {}
}

/// An object of a protocol whose client side no crate of the workspace generates
/// (zwp-input-method-v2, text-input-v3): its requests are sent raw, by name, on the interfaces
/// Smithay's server side describes, and its events are recorded.
struct Raw {
    backend: Backend,
    id: ObjectId,
    interface: &'static Interface,
    events: Arc<Recorder>,
}

impl Raw {
    /// Binds the global of `interface`, version 1.
    fn bind(local: &mut Local, interface: &'static Interface) -> Self {
        let (name, _, _) = local
            .client
            .globals
            .iter()
            .find(|(_, listed, _)| listed == interface.name)
            .unwrap_or_else(|| panic!("{} is listed", interface.name));
        let bind = wl_registry::Request::Bind {
            name: *name,
            id: (interface, 1),
        };
        let events = Arc::new(Recorder::default());
        let id = local
            .connection
            .send_request(&local.registry, bind, Some(events.clone()))
            .expect("bind");
        Self {
            backend: local.connection.backend(),
            id,
            interface,
            events,
        }
    }

    fn opcode(&self, request: &str) -> u16 {
        self.interface
            .requests
            .iter()
            .position(|desc| desc.name == request)
            .and_then(|at| u16::try_from(at).ok())
            .unwrap_or_else(|| panic!("{}.{request}", self.interface.name))
    }

    fn send(&self, request: &str, args: Vec<Argument<ObjectId, i32>>) {
        let message = Message {
            sender_id: self.id.clone(),
            opcode: self.opcode(request),
            args: args.into_iter().collect(),
        };
        self.backend
            .send_request(message, None, None)
            .unwrap_or_else(|error| panic!("{}.{request}: {error}", self.interface.name));
    }

    /// Sends `request`, whose `args` hold a placeholder `NewId`, for an object of `interface`.
    fn create(
        &self,
        request: &str,
        args: Vec<Argument<ObjectId, i32>>,
        interface: &'static Interface,
    ) -> Self {
        let message = Message {
            sender_id: self.id.clone(),
            opcode: self.opcode(request),
            args: args.into_iter().collect(),
        };
        let events = Arc::new(Recorder::default());
        let id = self
            .backend
            .send_request(message, Some(events.clone()), Some((interface, 1)))
            .unwrap_or_else(|error| panic!("{}.{request}: {error}", self.interface.name));
        Self {
            backend: self.backend.clone(),
            id,
            interface,
            events,
        }
    }

    fn events(&self) -> Vec<(&'static str, Vec<String>)> {
        self.events.0.lock().expect("events").clone()
    }

    /// The names of the events after the first `skip`.
    fn names_after(&self, skip: usize) -> Vec<&'static str> {
        self.events()
            .into_iter()
            .skip(skip)
            .map(|(name, _)| name)
            .collect()
    }
}

fn text(text: &str) -> Argument<ObjectId, i32> {
    Argument::Str(Some(Box::new(
        std::ffi::CString::new(text).expect("no NUL in the text"),
    )))
}

/// A text-input-v3 object of `local`, on its seat.
fn text_input(local: &mut Local, run: &mut Running) -> Raw {
    let seat: wl_seat::WlSeat = local.bind(1);
    let manager = Raw::bind(local, &ZWP_TEXT_INPUT_MANAGER_V3_INTERFACE);
    let text_input = manager.create(
        "get_text_input",
        vec![
            Argument::NewId(ObjectId::null()),
            Argument::Object(seat.id()),
        ],
        &ZWP_TEXT_INPUT_V3_INTERFACE,
    );
    local.roundtrip(run);
    text_input
}

/// Enables `text_input` with `surrounding` around the cursor, as a focused text field does.
fn enable(text_input: &Raw, local: &mut Local, run: &mut Running, surrounding: &str) {
    text_input.send("enable", vec![]);
    let end = Argument::Int(i32::try_from(surrounding.len()).expect("short text"));
    text_input.send(
        "set_surrounding_text",
        vec![text(surrounding), end.clone(), end],
    );
    text_input.send("commit", vec![]);
    local.roundtrip(run);
}

/// An input method of zwp-input-method-v2 on the seat of its client (OS9 item 3): it pre-edits
/// and commits text into the focused text input, and takes the keyboard. Every request is
/// answered once the compositor has handled it.
struct SimulatedInputMethod {
    method: Raw,
    /// Its keyboard grabs, each one's keys recorded.
    grabs: Vec<Raw>,
}

impl SimulatedInputMethod {
    fn new(local: &mut Local, run: &mut Running) -> Self {
        let seat: wl_seat::WlSeat = local.bind(1);
        let manager = Raw::bind(local, &ZWP_INPUT_METHOD_MANAGER_V2_INTERFACE);
        let method = manager.create(
            "get_input_method",
            vec![
                Argument::Object(seat.id()),
                Argument::NewId(ObjectId::null()),
            ],
            &ZWP_INPUT_METHOD_V2_INTERFACE,
        );
        local.roundtrip(run);
        Self {
            method,
            grabs: Vec::new(),
        }
    }

    /// Commits the pending state, with the serial the protocol asks: the `done` events received.
    fn commit(&self, local: &mut Local, run: &mut Running) {
        let done = self
            .method
            .names_after(0)
            .iter()
            .filter(|name| **name == "done")
            .count();
        let serial = u32::try_from(done).expect("serial");
        self.method.send("commit", vec![Argument::Uint(serial)]);
        local.roundtrip(run);
    }

    /// Shows `preedit` with the cursor at its end.
    fn preedit(&self, local: &mut Local, run: &mut Running, preedit: &str) {
        let end = Argument::Int(i32::try_from(preedit.len()).expect("short text"));
        self.method
            .send("set_preedit_string", vec![text(preedit), end.clone(), end]);
        self.commit(local, run);
    }

    /// Commits `string`, which replaces the preedit.
    fn commit_string(&self, local: &mut Local, run: &mut Running, string: &str) {
        self.method.send("commit_string", vec![text(string)]);
        self.commit(local, run);
    }

    /// Takes the keyboard: the keys go to the grab, not to the focused surface.
    fn grab_keyboard(&mut self, local: &mut Local, run: &mut Running) {
        self.grabs.push(self.method.create(
            "grab_keyboard",
            vec![Argument::NewId(ObjectId::null())],
            &ZWP_INPUT_METHOD_KEYBOARD_GRAB_V2_INTERFACE,
        ));
        local.roundtrip(run);
    }

    /// A popup surface of its own, on a new `wl_surface` of `local`.
    fn popup(&self, local: &mut Local, run: &mut Running) -> (wl_surface::WlSurface, Raw) {
        let compositor: wl_compositor::WlCompositor = local.bind(4);
        let surface = compositor.create_surface(&local.queue.handle(), ());
        let popup = self.method.create(
            "get_input_popup_surface",
            vec![
                Argument::NewId(ObjectId::null()),
                Argument::Object(surface.id()),
            ],
            &ZWP_INPUT_POPUP_SURFACE_V2_INTERFACE,
        );
        local.roundtrip(run);
        (surface, popup)
    }

    /// The evdev codes of the keys pressed on its grabs.
    fn keys(&self) -> Vec<u32> {
        self.key_events()
            .into_iter()
            .filter_map(|(key, pressed)| pressed.then_some(key))
            .collect()
    }

    /// Every key event of its grabs: the evdev code, and whether it is a press.
    fn key_events(&self) -> Vec<(u32, bool)> {
        self.grabs
            .iter()
            .flat_map(Raw::events)
            .filter_map(|(name, args)| match (name, &args[..]) {
                ("key", [_, _, key, state]) => Some((key.parse().ok()?, state == "1")),
                _ => None,
            })
            .collect()
    }
}

/// A focused window and an input method, granted the protocol, with the window's text input
/// enabled and the input method activated for it.
fn composing(run: &mut Running) -> (Local, Raw, Local, SimulatedInputMethod) {
    let (mut window, _) = focused_window(run);
    let mut ime = Local::connect(run, None);
    let method = SimulatedInputMethod::new(&mut ime, run);
    let field = text_input(&mut window, run);
    enable(&field, &mut window, run, "caf");
    ime.pump(run, "the input method is activated", |_, _| {
        method.method.names_after(0).contains(&"activate")
    });
    (window, field, ime, method)
}

/// OS9 item 3: an input method's preedit and commit reach the focused window's text input, in
/// order, each followed by `done`.
#[test]
fn os9_a_committed_string_reaches_the_focused_text_input() {
    let mut run = Running::start(None);
    let (mut window, field, mut ime, method) = composing(&mut run);
    let before = field.events().len();
    method.preedit(&mut ime, &mut run, "e");
    method.commit_string(&mut ime, &mut run, "è");
    window.pump(&mut run, "the text input is done twice", |_, _| {
        field
            .names_after(before)
            .iter()
            .filter(|name| **name == "done")
            .count()
            == 2
    });
    let received: Vec<(&str, Vec<String>)> = field.events().into_iter().skip(before).collect();
    let args = |list: &[&str]| list.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    assert_eq!(received[0], ("preedit_string", args(&["e", "1", "1"])));
    assert_eq!(received[1].0, "done");
    assert_eq!(received[2], ("commit_string", args(&["è"])));
    assert_eq!(received[3].0, "done");
    assert_eq!(received.len(), 4, "{received:?}");
    run.stop();
}

/// Register row 8: the lock ends the input method's keyboard grab, and a grab it takes while
/// locked does not hold; the keys typed on the lock reach the lock surface only, and after
/// unlock the grab is not restored on its own.
#[test]
fn os9_the_input_method_grab_is_dropped_while_locked() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    chord(&mut run.state, &[30]);
    ime.pump(&mut run, "the grab takes the key", |_, _| {
        method.keys() == [30]
    });
    window.roundtrip(&mut run);
    assert!(window.client.keys.is_empty(), "the window saw no key");

    let (mut locker, lock) = lock_session(&mut run);
    let grabbed = |state: &State| state.seat.get_keyboard().is_some_and(|k| k.is_grabbed());
    assert!(!grabbed(&run.state), "the lock ends the grab");
    chord(&mut run.state, &[31]);
    locker.pump(&mut run, "the lock surface takes the key", |client, _| {
        client.keys == [31]
    });
    // A grab asked for while locked does not hold either.
    method.grab_keyboard(&mut ime, &mut run);
    chord(&mut run.state, &[32]);
    locker.pump(
        &mut run,
        "the lock surface takes the next key",
        |client, _| client.keys == [31, 32],
    );
    ime.roundtrip(&mut run);
    assert_eq!(
        method.keys(),
        [30],
        "the input method saw no key typed on the lock"
    );

    lock.unlock_and_destroy();
    let id = window.id.clone().expect("a client the fixture inserted");
    locker.pump(
        &mut run,
        "unlocking gives the window the keyboard",
        |_, state| {
            state
                .seat
                .get_keyboard()
                .and_then(|k| k.current_focus())
                .and_then(|s| s.client())
                .map(|c| c.id())
                == Some(id.clone())
        },
    );
    assert!(!grabbed(&run.state), "unlocking restores no grab");
    chord(&mut run.state, &[33]);
    window.pump(&mut run, "the window takes the key", |client, _| {
        client.keys == [33]
    });
    ime.roundtrip(&mut run);
    assert_eq!(method.keys(), [30], "nor did it after unlock");
    run.stop();
}

/// While locked the input method is cut off from the lock surface: it is not activated for the
/// lock's text field nor told its text, and what it commits reaches no text input.
#[test]
fn os9_the_input_method_never_reaches_the_lock_surface() {
    let mut run = Running::start(None);
    let (mut window, field, mut ime, method) = composing(&mut run);
    let (mut locker, _lock) = lock_session(&mut run);
    let told = method.method.events().len();
    let password = text_input(&mut locker, &mut run);
    enable(&password, &mut locker, &mut run, "secret");
    method.preedit(&mut ime, &mut run, "e");
    method.commit_string(&mut ime, &mut run, "è");
    locker.roundtrip(&mut run);
    window.roundtrip(&mut run);
    let seen = method.method.events().into_iter().skip(told);
    for (name, args) in seen {
        assert!(
            !matches!(name, "activate" | "surrounding_text" | "content_type"),
            "the input method was sent {name} {args:?} while locked"
        );
    }
    let reached = |raw: &Raw| {
        raw.events()
            .into_iter()
            .filter(|(name, _)| matches!(*name, "preedit_string" | "commit_string" | "done"))
            .count()
    };
    assert_eq!(reached(&password), 0, "{:?}", password.events());
    assert_eq!(reached(&field), 0, "{:?}", field.events());
    run.stop();
}

/// Open question 13 of the OS9 plan: a client inside a security context cannot be an input
/// method; only a client the allow-list grants it, the bar here, sees the manager.
#[test]
fn os9_a_sandboxed_client_cannot_be_an_input_method() {
    let mut run = Running::start(None);
    let manager = ZWP_INPUT_METHOD_MANAGER_V2_INTERFACE.name;
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(manager), "inside a security context");
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(manager), "the bar, granted it");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(manager), "a client with no line");
    run.stop();
}

/// Open question 13 of the OS9 plan: text input is offered to every client outside a security
/// context, and to none inside one.
#[test]
fn os9_a_sandboxed_client_has_no_text_input() {
    let mut run = Running::start(None);
    let manager = ZWP_TEXT_INPUT_MANAGER_V3_INTERFACE.name;
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(manager), "inside a security context");
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(manager), "the bar");
    let control_center = Local::control_center(&mut run);
    assert!(control_center.sees(manager), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(other.sees(manager), "a client with no line");
    run.stop();
}

/// Presses the key of evdev code `code`, or releases it, as the backend hands it to the seat.
fn key(state: &mut State, code: u32, pressed: bool) {
    use smithay::backend::input::{InputTime, KeyState};
    let key_state = if pressed {
        KeyState::Pressed
    } else {
        KeyState::Released
    };
    state.key_input(
        (code + 8).into(),
        key_state,
        InputTime::now(),
        KeyboardSource::Physical,
    );
}

/// Left Shift, as evdev numbers it.
const SHIFT: u32 = 42;

/// A keyboard grab asked for while locked is sent nothing, not even the keymap, the repeat
/// rate or the modifiers held on the lock: a modifier pressed while the password is typed
/// stays unknown to the input method, when pressed and when released.
#[test]
fn os9_a_grab_taken_while_locked_is_sent_nothing() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    let (mut locker, _lock) = lock_session(&mut run);
    key(&mut run.state, SHIFT, true);
    method.grab_keyboard(&mut ime, &mut run);
    locker.roundtrip(&mut run);
    window.roundtrip(&mut run);
    ime.roundtrip(&mut run);
    assert_eq!(method.grabs[0].events(), [], "with Shift held on the lock");
    key(&mut run.state, SHIFT, false);
    chord(&mut run.state, &[30]);
    locker.pump(&mut run, "the lock surface takes the keys", |client, _| {
        client.keys == [SHIFT, 30]
    });
    ime.roundtrip(&mut run);
    assert_eq!(method.grabs[0].events(), [], "after Shift is released");
    run.stop();
}

/// After unlock a grab taken while locked stays inert, and the input method takes the keyboard
/// with a new grab, which is sent the keymap, the repeat rate and the modifiers, then the keys.
#[test]
fn os9_a_grab_taken_while_locked_stays_inert_after_unlock() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    let (mut locker, lock) = lock_session(&mut run);
    method.grab_keyboard(&mut ime, &mut run);
    lock.unlock_and_destroy();
    locker.pump(&mut run, "unlocking ends the lock", |_, state| {
        state.lock.is_none()
    });
    chord(&mut run.state, &[30]);
    window.pump(&mut run, "the window takes the key", |client, _| {
        client.keys == [30]
    });
    ime.roundtrip(&mut run);
    assert_eq!(method.grabs[0].events(), [], "the grab taken while locked");

    method.grab_keyboard(&mut ime, &mut run);
    let names = |raw: &Raw| raw.names_after(0);
    assert_eq!(
        names(&method.grabs[1]),
        ["repeat_info", "keymap", "modifiers"]
    );
    chord(&mut run.state, &[31]);
    ime.pump(&mut run, "the new grab takes the key", |_, _| {
        method.keys() == [31]
    });
    assert_eq!(method.grabs[0].events(), [], "the old grab still");
    run.stop();
}

/// An input method releasing its keyboard grab ends that grab only: a popup's grab that
/// replaced it goes on.
#[test]
fn os9_an_input_method_releasing_its_grab_leaves_a_popup_grab() {
    let mut run = Running::start(None);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let grabbed = |state: &State| state.seat.get_keyboard().is_some_and(|k| k.is_grabbed());
    assert!(grabbed(&run.state), "the input method holds the keyboard");

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        focus(state)
            .is_some_and(|surface| surface.client().map(|c| c.id()) == Some(shell_id.clone()))
    });
    let exclusive = focus(&run.state).expect("the exclusive surface");
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    run.until("the popup's grab takes the keyboard", |state| {
        focus(state).is_some_and(|surface| {
            surface != exclusive && state.popups.find_popup(&surface).is_some()
        })
    });
    let popup = focus(&run.state);

    method.grabs[0].send("release", vec![]);
    ime.roundtrip(&mut run);
    assert!(grabbed(&run.state), "the popup's grab goes on");
    assert_eq!(focus(&run.state), popup, "the popup keeps the keyboard");

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// One input method per seat: a second is told it is unavailable, as the only event it gets,
/// and the first goes on serving the focused text input.
#[test]
fn os9_a_second_input_method_is_unavailable() {
    let mut run = Running::start(None);
    let (mut window, field, mut ime, method) = composing(&mut run);
    let mut other = Local::connect(&mut run, None);
    let second = SimulatedInputMethod::new(&mut other, &mut run);
    assert_eq!(second.method.names_after(0), ["unavailable"]);
    let before = field.events().len();
    method.commit_string(&mut ime, &mut run, "è");
    window.pump(&mut run, "the text input is done", |_, _| {
        field.names_after(before).contains(&"done")
    });
    assert_eq!(field.names_after(before), ["commit_string", "done"]);
    run.stop();
}

/// A cursor rectangle of negative size, which a client may send, is taken as an empty one: the
/// input method's popup is told the cursor's place with a size of zero.
#[test]
fn os9_a_negative_cursor_rectangle_is_empty() {
    let mut run = Running::start(None);
    let (mut window, field, mut ime, method) = composing(&mut run);
    let at = |n: i32| Argument::Int(n);
    field.send("set_cursor_rectangle", vec![at(5), at(6), at(-1), at(-1)]);
    field.send("commit", vec![]);
    window.roundtrip(&mut run);
    let (_surface, popup) = method.popup(&mut ime, &mut run);
    let rectangle = ["5", "6", "0", "0"].map(String::from).to_vec();
    assert_eq!(popup.events(), [("text_input_rectangle", rectangle)]);
    run.stop();
}

/// An input method's popup whose `wl_surface` is destroyed is drawn no more, though the popup
/// object lives on.
#[test]
fn os9_a_popup_of_a_destroyed_surface_is_not_drawn() {
    let mut run = Running::start(None);
    let (_window, _field, mut ime, method) = composing(&mut run);
    let (surface, _popup) = method.popup(&mut ime, &mut run);
    let drawn = |state: &State| {
        let output = state.space.outputs().next().expect("an output");
        state.composing.popups_on(&state.space, output).len()
    };
    assert_eq!(drawn(&run.state), 1, "the popup is drawn");
    surface.destroy();
    ime.roundtrip(&mut run);
    assert_eq!(drawn(&run.state), 0, "its surface is gone");
    run.stop();
}

/// A client has at most 32 text inputs: its 33rd is inert, never entered, and its requests are
/// ignored, so enabling it activates no input method.
#[test]
fn os9_a_client_s_33rd_text_input_is_inert() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let method = SimulatedInputMethod::new(&mut ime, &mut run);
    let seat: wl_seat::WlSeat = window.bind(1);
    let manager = Raw::bind(&mut window, &ZWP_TEXT_INPUT_MANAGER_V3_INTERFACE);
    let fields: Vec<Raw> = (0..33)
        .map(|_| {
            manager.create(
                "get_text_input",
                vec![
                    Argument::NewId(ObjectId::null()),
                    Argument::Object(seat.id()),
                ],
                &ZWP_TEXT_INPUT_V3_INTERFACE,
            )
        })
        .collect();
    window.roundtrip(&mut run);
    for field in &fields[..32] {
        assert_eq!(field.names_after(0), ["enter"]);
    }
    assert_eq!(fields[32].names_after(0), [] as [&str; 0], "the 33rd");
    enable(&fields[32], &mut window, &mut run, "caf");
    ime.roundtrip(&mut run);
    assert!(
        !method.method.names_after(0).contains(&"activate"),
        "{:?}",
        method.method.events()
    );
    assert_eq!(fields[32].names_after(0), [] as [&str; 0], "the 33rd");
    run.stop();
}

/// What an input method sends waits for its `commit`, and a commit that names an old serial
/// applies none of it: a string sent for one window, then committed after the keyboard moved to
/// another client's field, reaches neither field, nor does a later commit carry it over.
#[test]
fn os9_a_stale_input_method_commit_reaches_no_field() {
    let mut run = Running::start(None);
    let (mut window, field, mut ime, method) = composing(&mut run);
    let dones = |raw: &Raw| raw.names_after(0).iter().filter(|n| **n == "done").count();
    let stale = u32::try_from(dones(&method.method)).expect("serial");
    window.roundtrip(&mut run);
    let before = field.events().len();
    method.method.send("commit_string", vec![text("x")]);
    ime.roundtrip(&mut run);

    let (mut other, _) = focused_window(&mut run);
    let other_field = text_input(&mut other, &mut run);
    enable(&other_field, &mut other, &mut run, "caf");
    ime.pump(&mut run, "the input method serves the other field", |_, _| {
        method
            .method
            .names_after(0)
            .iter()
            .filter(|name| **name == "activate")
            .count()
            == 2
    });
    let other_before = other_field.events().len();
    method.method.send("commit", vec![Argument::Uint(stale)]);
    ime.roundtrip(&mut run);
    other.roundtrip(&mut run);
    window.roundtrip(&mut run);
    assert_eq!(field.names_after(before), ["leave"], "the first field");
    assert_eq!(
        other_field.names_after(other_before),
        [] as [&str; 0],
        "the other field"
    );

    method.commit(&mut ime, &mut run);
    other.pump(&mut run, "the other field is done", |_, _| {
        other_field.names_after(other_before).contains(&"done")
    });
    assert_eq!(other_field.names_after(other_before), ["done"]);
    run.stop();
}

/// A key the input method forwards through its own virtual keyboard while it holds the
/// keyboard goes to the focused window, not back to its grab.
#[test]
fn os9_a_key_the_input_method_forwards_reaches_the_window() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, false);
    window.pump(&mut run, "the window takes the key", |client, _| {
        client.keys == [30]
    });
    ime.roundtrip(&mut run);
    assert_eq!(method.keys(), [] as [u32; 0], "the grab saw no key");
    chord(&mut run.state, &[31]);
    ime.pump(&mut run, "the grab takes a hardware key", |_, _| {
        method.keys() == [31]
    });
    run.stop();
}

/// Modifiers a virtual keyboard sets with no key after them reach the input method's grab, as
/// they reach the focused window: Shift let go is let go for the input method too.
#[test]
fn os9_a_virtual_modifier_change_reaches_the_input_method_grab() {
    const SHIFT: u32 = 1;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let mut local = Local::connect(&mut run, None);
    let typist = VirtualKeyboard::new(&mut local, &mut run, Some(SWAPPED_KEYMAP));
    typist.key(&mut local, &mut run, 30, true);
    typist.key(&mut local, &mut run, 30, false);
    ime.pump(&mut run, "the grab takes the key", |_, _| {
        method.keys() == [30]
    });
    let grab = &method.grabs[0];
    for (depressed, change) in [(SHIFT, "Shift held"), (0, "Shift let go")] {
        let before = grab.events().len();
        typist.modifiers(&mut local, &mut run, depressed);
        ime.roundtrip(&mut run);
        let sent: Vec<_> = grab.events().into_iter().skip(before).collect();
        assert!(
            matches!(&sent[..], [("modifiers", args)] if args[1] == depressed.to_string()),
            "{change}: {sent:?}"
        );
    }

    // Left Shift forwarded by the input method itself goes past its grab, which is sent the
    // modifiers it changed and not the key. A first key gives the seat its keymap back.
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    forwarder.chord(&mut ime, &mut run, &[31]);
    window.pump(&mut run, "the window takes S", |client, _| {
        client.keys.ends_with(&[31])
    });
    ime.roundtrip(&mut run);
    let before = grab.events().len();
    forwarder.key(&mut ime, &mut run, 42, true);
    window.pump(&mut run, "the window takes Shift", |client, _| {
        client.keys.ends_with(&[42])
    });
    let sent = grab.names_after(before);
    assert!(sent.contains(&"modifiers") && !sent.contains(&"key"), "{sent:?}");
    run.stop();
}

/// A key the input method forwards past its grab is read with the seat's modifiers: a hardware
/// Shift held, which only the grab was sent, reaches the window with the key.
#[test]
fn os9_a_forwarded_key_carries_the_modifiers_the_grab_saw() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    key(&mut run.state, SHIFT, true);
    ime.pump(&mut run, "the grab takes Shift", |_, _| {
        method.keys() == [SHIFT]
    });
    forwarder.key(&mut ime, &mut run, 30, true);
    window.pump(&mut run, "the window takes the key", |client, _| {
        client.keys == [30]
    });
    assert_eq!(
        window.client.modifiers.last(),
        Some(&1),
        "the key is read with Shift"
    );
    run.stop();
}

/// The input method's edits are kept only while it serves a text input: a string it sends while
/// none is active is dropped, and a field enabled afterwards never receives it.
#[test]
fn os9_an_edit_sent_with_no_active_field_is_dropped() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let method = SimulatedInputMethod::new(&mut ime, &mut run);
    let field = text_input(&mut window, &mut run);
    method.method.send("commit_string", vec![text("x")]);
    ime.roundtrip(&mut run);
    enable(&field, &mut window, &mut run, "caf");
    ime.pump(&mut run, "the input method is activated", |_, _| {
        method.method.names_after(0).contains(&"activate")
    });
    window.roundtrip(&mut run);
    let before = field.events().len();
    method.commit(&mut ime, &mut run);
    window.pump(&mut run, "the field is done", |_, _| {
        field.names_after(before).contains(&"done")
    });
    assert_eq!(field.names_after(before), ["done"]);
    run.stop();
}

/// A key the input method forwards past its grab is let go past it too, to the window that saw
/// the press: when the input method's client goes with the key held, after another input method
/// has taken the keyboard, the release reaches the window and not the new grab.
#[test]
fn os9_a_forwarded_key_held_when_the_input_method_goes_is_let_go_for_the_window() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    forwarder.key(&mut ime, &mut run, 30, true);
    window.pump(&mut run, "the window takes the press", |client, _| {
        client.keys == [30]
    });
    method.method.send("destroy", vec![]);
    ime.roundtrip(&mut run);
    let mut next = Local::connect(&mut run, None);
    let mut successor = SimulatedInputMethod::new(&mut next, &mut run);
    successor.grab_keyboard(&mut next, &mut run);

    drop(forwarder);
    drop(method);
    drop(ime);
    window.pump(&mut run, "the window sees the release", |client, _| {
        client.released == [30]
    });
    next.roundtrip(&mut run);
    let sent = successor.grabs[0].names_after(0);
    assert!(!sent.contains(&"key"), "the new grab: {sent:?}");
    assert!(
        run.state.past_grab_keys.is_empty(),
        "no key is held past the grab"
    );
    run.stop();
}

/// A key the input method forwards past its grab is kept once, however often it is pressed: a
/// press repeated before the release leaves nothing held past the grab after it.
#[test]
fn os9_a_forwarded_key_pressed_twice_is_let_go_once() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, false);
    window.pump(&mut run, "the window sees the release", |client, _| {
        client.released == [30]
    });
    assert!(
        run.state.past_grab_keys.values().all(|held| held.is_empty()),
        "no key is held past the grab"
    );
    // A code past evdev's last key, KEY_MAX, is not kept.
    forwarder.key(&mut ime, &mut run, 0x300, true);
    assert!(
        run.state.past_grab_keys.values().all(|held| held.is_empty()),
        "a code past KEY_MAX is not kept"
    );
    run.stop();
}

/// A key pressed twice past one input method's grab, then let go, leaves nothing behind that
/// would send a later release past another's: the next input method's grab, which sees the
/// first one's keyboard press the key, sees it let go too.
#[test]
fn os9_a_key_let_go_past_the_grab_is_not_let_go_past_the_next_one() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    let forwarder = VirtualKeyboard::new(&mut ime, &mut run, None);
    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, false);
    window.pump(&mut run, "the window sees the release", |client, _| {
        client.released == [30]
    });
    method.method.send("destroy", vec![]);
    ime.roundtrip(&mut run);
    let mut next = Local::connect(&mut run, None);
    let mut successor = SimulatedInputMethod::new(&mut next, &mut run);
    successor.grab_keyboard(&mut next, &mut run);

    forwarder.key(&mut ime, &mut run, 30, true);
    forwarder.key(&mut ime, &mut run, 30, false);
    next.roundtrip(&mut run);
    let states: Vec<String> = successor.grabs[0]
        .events()
        .into_iter()
        .filter_map(|(name, args)| match (name, &args[..]) {
            ("key", [_, _, key, state]) if key == "30" => Some(state.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(states, ["1", "0"], "the next grab sees the press and the release");
    window.roundtrip(&mut run);
    assert_eq!(window.client.released, [30], "the window sees no second release");
    run.stop();
}


/// A `zwlr_output_power_v1` of output `index`, made through a manager bound now.
fn power_of(
    local: &Local,
    index: usize,
) -> zwlr_output_power_v1::ZwlrOutputPowerV1 {
    let manager: zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1 = local.bind(1);
    manager.get_output_power(&local.output(index), &local.queue.handle(), ())
}

/// The events `object` was sent, in order.
fn power_events(local: &Local, object: &zwlr_output_power_v1::ZwlrOutputPowerV1) -> Vec<PowerEvent> {
    local
        .client
        .power
        .iter()
        .filter(|(id, _)| *id == object.id())
        .map(|(_, event)| *event)
        .collect()
}

/// `zwlr_output_power_manager_v1` is privileged (D1): a client the allow-list does not grant it,
/// the control center here, does not see it; the bar, which the fixture grants it, does.
#[test]
fn output_power_is_privileged() {
    const MANAGER: &str = "zwlr_output_power_manager_v1";
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(MANAGER), "the bar");
    let control_center = Local::control_center(&mut run);
    assert!(!control_center.sees(MANAGER), "the control center");
    run.stop();
}

/// An object is sent the mode when it is made and at each change, and so is every other object
/// of the same client for the same output (D4); the other output stays on.
#[test]
fn the_mode_is_sent_at_creation_and_on_change() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let first = power_of(&local, 0);
    let second = power_of(&local, 0);
    local.roundtrip(&mut run);
    assert_eq!(power_events(&local, &first), [PowerEvent::On]);
    first.set_mode(zwlr_output_power_v1::Mode::Off);
    local.roundtrip(&mut run);
    assert_eq!(power_events(&local, &first), [PowerEvent::On, PowerEvent::Off]);
    assert_eq!(power_events(&local, &second), [PowerEvent::On, PowerEvent::Off]);
    let outputs: Vec<_> = run.state.space.outputs().cloned().collect();
    assert!(!run.state.output_power.is_on(&outputs[0]), "the first output is off");
    assert!(run.state.output_power.is_on(&outputs[1]), "the second output stays on");
    second.set_mode(zwlr_output_power_v1::Mode::On);
    local.roundtrip(&mut run);
    assert_eq!(
        power_events(&local, &first),
        [PowerEvent::On, PowerEvent::Off, PowerEvent::On]
    );
    run.stop();
}

/// While one client controls an output, another client's object for it is sent `failed` and
/// does nothing; once the first client's object is gone, the second client may take it (D2).
#[test]
fn a_second_client_is_refused_while_one_controls() {
    let mut run = Running::start(None);
    let mut first = Local::connect(&mut run, None);
    let mut second = Local::connect(&mut run, None);
    let held = power_of(&first, 0);
    first.roundtrip(&mut run);
    let refused = power_of(&second, 0);
    second.roundtrip(&mut run);
    assert_eq!(power_events(&second, &refused), [PowerEvent::Failed]);
    refused.set_mode(zwlr_output_power_v1::Mode::Off);
    second.roundtrip(&mut run);
    let output = run.state.space.outputs().next().cloned().expect("an output");
    assert!(run.state.output_power.is_on(&output), "a refused object does nothing");
    let other = power_of(&second, 1);
    second.roundtrip(&mut run);
    assert_eq!(power_events(&second, &other), [PowerEvent::On], "another output is free");
    held.destroy();
    first.roundtrip(&mut run);
    let taken = power_of(&second, 0);
    second.roundtrip(&mut run);
    assert_eq!(power_events(&second, &taken), [PowerEvent::On]);
    run.stop();
}

/// A client that turned an output off and goes leaves it on (D3).
#[test]
fn the_output_turns_on_when_its_controller_goes() {
    let mut run = Running::start(None);
    let mut daemon = Local::connect(&mut run, None);
    let power = power_of(&daemon, 0);
    power.set_mode(zwlr_output_power_v1::Mode::Off);
    daemon.roundtrip(&mut run);
    let output = run.state.space.outputs().next().cloned().expect("an output");
    assert!(!run.state.output_power.is_on(&output), "off");
    drop(daemon);
    run.until("the output turns on", |state| state.output_power.is_on(&output));
    run.stop();
}

/// The objects of an output that goes are sent `failed`.
#[test]
fn a_removed_output_fails_its_objects() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let power = power_of(&local, 1);
    power.set_mode(zwlr_output_power_v1::Mode::Off);
    local.roundtrip(&mut run);
    let second = run.state.space.outputs().nth(1).cloned().expect("the second output");
    run.state.remove_output(&second);
    local.pump(&mut run, "failed", |client, _| {
        client.power.iter().any(|(_, event)| *event == PowerEvent::Failed)
    });
    assert_eq!(
        power_events(&local, &power),
        [PowerEvent::On, PowerEvent::Off, PowerEvent::Failed]
    );
    assert!(run.state.output_power.is_on(&second), "a gone output is not kept off");
    run.stop();
}

/// A `set_mode` outside the enum is the protocol error `invalid_mode` (D9). The typed request
/// cannot carry one, so the message is sent raw.
#[test]
fn an_unknown_mode_is_a_protocol_error() {
    use wayland_client::backend::protocol::{Argument, Message};
    use wayland_client::backend::smallvec::smallvec;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let power = power_of(&local, 0);
    local.roundtrip(&mut run);
    let message = Message {
        sender_id: power.id(),
        // `set_mode`, the first request of the interface.
        opcode: 0,
        args: smallvec![Argument::Uint(7)],
    };
    local
        .connection
        .backend()
        .send_request(message, None, None)
        .expect("the request");
    local.connection.flush().expect("flush");
    let started = Instant::now();
    let error = loop {
        if let Some(error) = local.connection.protocol_error() {
            break error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = local.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    };
    assert_eq!(error.object_interface, "zwlr_output_power_v1");
    assert_eq!(
        error.code,
        zwlr_output_power_v1::Error::InvalidMode as u32
    );
    run.stop();
}

// The key bindings (doc_own_stack.md OS13, `bindings.rs`), keys as evdev numbers them.
const KEY_ESC: u32 = 1;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_TAB: u32 = 15;
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_S: u32 = 31;
const KEY_T: u32 = 20;
const KEY_LEFTCTRL: u32 = 29;
const KEY_A: u32 = 30;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_X: u32 = 45;
const KEY_C: u32 = 46;
const KEY_V: u32 = 47;
const KEY_N: u32 = 49;
const KEY_M: u32 = 50;
const KEY_LEFTALT: u32 = 56;
const KEY_SPACE: u32 = 57;
const KEY_F1: u32 = 59;
const KEY_F4: u32 = 62;
const KEY_SYSRQ: u32 = 99;
const KEY_MUTE: u32 = 113;
const KEY_VOLUMEDOWN: u32 = 114;
const KEY_VOLUMEUP: u32 = 115;
const KEY_POWER: u32 = 116;
/// XF86LaunchA in xkeyboard-config's evdev keycodes.
const KEY_SCALE: u32 = 120;
const KEY_BRIGHTNESSDOWN: u32 = 224;
const KEY_BRIGHTNESSUP: u32 = 225;
const KEY_LEFTMETA: u32 = 125;
const KEY_RIGHTMETA: u32 = 126;
const KEY_NEXTSONG: u32 = 163;
const KEY_PLAYPAUSE: u32 = 164;
const KEY_PREVIOUSSONG: u32 = 165;
const KEY_KBDILLUMTOGGLE: u32 = 228;
const KEY_KBDILLUMDOWN: u32 = 229;
const KEY_KBDILLUMUP: u32 = 230;
const KEY_WLAN: u32 = 238;
const KEY_RFKILL: u32 = 247;
const KEY_MICMUTE: u32 = 248;
const KEY_TOUCHPAD_TOGGLE: u32 = 530;
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_9: u32 = 10;
const KEY_0: u32 = 11;
const KEY_H: u32 = 35;
const KEY_J: u32 = 36;
const KEY_K: u32 = 37;
const KEY_L: u32 = 38;
const KEY_UP: u32 = 103;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_DOWN: u32 = 108;

/// Presses the chord `codes` while `to` holds the keyboard: the presses and the releases `to`
/// was sent.
fn sent_to(to: &mut Local, run: &mut Running, codes: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let (pressed, released) = (to.client.keys.len(), to.client.released.len());
    chord(&mut run.state, codes);
    to.roundtrip(run);
    (
        to.client.keys[pressed..].to_vec(),
        to.client.released[released..].to_vec(),
    )
}

/// The chord `codes` reaches `to` whole, each press and each release, and the compositor does
/// nothing with it.
fn reaches(to: &mut Local, run: &mut Running, codes: &[u32], what: &str) {
    let fired = run.state.fired.len();
    let (pressed, released) = sent_to(to, run, codes);
    assert_eq!(pressed, codes, "{what}: every press reaches the client");
    let reversed: Vec<u32> = codes.iter().rev().copied().collect();
    assert_eq!(released, reversed, "{what}: and every release");
    assert_eq!(run.state.fired.len(), fired, "{what}: and nothing is done");
}

/// The chord `codes`, whose last key is bound, takes `action`, or none: `to` is sent the
/// modifiers' presses and releases, never the bound key's.
fn acts(
    to: &mut Local,
    run: &mut Running,
    codes: &[u32],
    action: Option<crate::bindings::Action>,
    what: &str,
) {
    let fired = run.state.fired.len();
    let (pressed, released) = sent_to(to, run, codes);
    let modifiers = &codes[..codes.len() - 1];
    assert_eq!(
        pressed, modifiers,
        "{what}: the client is sent no bound press"
    );
    let reversed: Vec<u32> = modifiers.iter().rev().copied().collect();
    assert_eq!(released, reversed, "{what}: nor its release");
    assert_eq!(run.state.fired[fired..], *action.as_slice(), "{what}");
}

/// Runs the compositor's loop as `main` runs it, the chord `codes` pressed in its first round:
/// the rounds it ran until it stopped, at most 50.
fn rounds_until_stopped(run: &mut Running, codes: &[u32]) -> u32 {
    let mut rounds = 0;
    run.event_loop
        .run(Duration::from_millis(1), &mut run.state, |state| {
            rounds += 1;
            if rounds == 1 {
                chord(state, codes);
            } else if rounds == 50 {
                state.loop_signal.stop();
            }
        })
        .expect("the loop");
    rounds
}

/// Super+Alt+Escape ends the compositor, and the session with it (LP10: the only exit from a
/// lock that cannot start): the loop stops in the round the key is pressed in, unlocked and
/// while a lock client holds the lock, and no client is sent the Escape.
#[test]
fn super_alt_escape_ends_the_compositor_even_locked() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let terminate = [KEY_LEFTMETA, KEY_LEFTALT, KEY_ESC];
    assert_eq!(
        rounds_until_stopped(&mut run, &terminate),
        1,
        "the loop stops"
    );
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_LEFTMETA, KEY_LEFTALT]);
    assert_eq!(window.client.released, [KEY_LEFTALT, KEY_LEFTMETA]);
    assert_eq!(run.state.fired, [Action::Terminate]);

    let (mut locker, _lock) = lock_session(&mut run);
    assert_eq!(
        rounds_until_stopped(&mut run, &terminate),
        1,
        "locked, the loop stops"
    );
    locker.roundtrip(&mut run);
    assert_eq!(locker.client.keys, [KEY_LEFTMETA, KEY_LEFTALT]);
    assert_eq!(locker.client.released, [KEY_LEFTALT, KEY_LEFTMETA]);
    assert_eq!(run.state.fired, [Action::Terminate, Action::Terminate]);
    run.stop();
}

/// Decision 28's six keys stay unbound until what they open is built: Super+W and XF86LaunchA
/// (the overview), Super+= and Super+- (the magnifier), Super+A (the application library) and
/// Super+V (the clipboard history) reach the focused client, and the compositor does nothing.
#[test]
fn decision_28_keys_reach_the_client_and_do_nothing() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_W][..], "Super+W"),
        (&[KEY_SCALE], "XF86LaunchA"),
        (&[KEY_LEFTMETA, KEY_EQUAL], "Super+="),
        (&[KEY_LEFTMETA, KEY_MINUS], "Super+-"),
        (&[KEY_LEFTMETA, KEY_A], "Super+A"),
        (&[KEY_LEFTMETA, KEY_V], "Super+V"),
    ] {
        reaches(&mut window, &mut run, codes, what);
    }
    run.stop();
}

/// The keys whose target is not built stay unbound (D12, OD15): Super+N (no
/// `ToggleNotifications`), Super+Alt+A (no entry point of the bar's accessibility menu), and the
/// keyboard-light, WLAN and RFKill keys (no `os.athanor.Osd1`).
#[test]
fn the_unbuilt_targets_do_nothing() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_N][..], "Super+N"),
        (&[KEY_LEFTMETA, KEY_LEFTALT, KEY_A], "Super+Alt+A"),
        (&[KEY_KBDILLUMTOGGLE], "XF86KbdLightOnOff"),
        (&[KEY_KBDILLUMDOWN], "XF86KbdBrightnessDown"),
        (&[KEY_KBDILLUMUP], "XF86KbdBrightnessUp"),
        (&[KEY_WLAN], "XF86WLAN"),
        (&[KEY_RFKILL], "XF86RFKill"),
    ] {
        reaches(&mut window, &mut run, codes, what);
    }
    run.stop();
}

/// COSMIC's defaults that no approved document names stay unbound (OS13, D15), Alt+Tab among
/// them (gap 8).
#[test]
fn the_unnamed_cosmic_keys_reach_the_client() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_M][..], "Super+M"),
        (&[KEY_LEFTMETA, KEY_T], "Super+T"),
        (&[KEY_LEFTALT, KEY_TAB], "Alt+Tab"),
        (&[KEY_SYSRQ], "Print"),
    ] {
        reaches(&mut window, &mut run, codes, what);
    }
    run.stop();
}

/// Super+Alt+T, the reveal key of OS4, is kept from every client, unlocked and locked, with no
/// effect until the border (D11); while locked it acts on nothing (section 1).
#[test]
fn the_reveal_key_never_reaches_a_client() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let reveal = [KEY_LEFTMETA, KEY_LEFTALT, KEY_T];
    acts(
        &mut window,
        &mut run,
        &reveal,
        Some(Action::Reveal),
        "unlocked",
    );
    let (mut locker, _lock) = lock_session(&mut run);
    acts(&mut locker, &mut run, &reveal, None, "locked");
    run.stop();
}

/// While locked only doc_lock_and_prompts.md section 1's keys act (D3): the terminal switch and
/// Super+Alt+Escape; Super+Escape, Super+C and Super+Shift+Escape reach the lock surface.
#[test]
fn only_the_lock_list_acts_while_locked() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut locker, _lock) = lock_session(&mut run);
    let requests = run.state.lock_requests;
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_ESC][..], "Super+Escape"),
        (&[KEY_LEFTMETA, KEY_C], "Super+C"),
        (
            &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_ESC],
            "Super+Shift+Escape",
        ),
    ] {
        reaches(&mut locker, &mut run, codes, what);
    }
    assert_eq!(run.state.lock_requests, requests, "no lock is asked for");
    acts(
        &mut locker,
        &mut run,
        &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_F1],
        Some(Action::Vt(1)),
        "Ctrl+Alt+F1",
    );
    acts(
        &mut locker,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTALT, KEY_ESC],
        Some(Action::Terminate),
        "Super+Alt+Escape",
    );
    run.stop();
}

/// A bound key reaches neither the focused client nor the input method's grab, which the filter
/// runs before (D2); an unbound key still goes to the grab.
#[test]
fn a_bound_key_never_reaches_the_client_nor_the_input_method() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ime = Local::connect(&mut run, None);
    let mut method = SimulatedInputMethod::new(&mut ime, &mut run);
    method.grab_keyboard(&mut ime, &mut run);
    chord(&mut run.state, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_T]);
    ime.roundtrip(&mut run);
    assert_eq!(
        method.key_events(),
        [
            (KEY_LEFTMETA, true),
            (KEY_LEFTALT, true),
            (KEY_LEFTALT, false),
            (KEY_LEFTMETA, false)
        ],
        "the grab is sent the modifiers only, neither the bound key's press nor its release"
    );
    assert_eq!(run.state.fired, [Action::Reveal]);
    chord(&mut run.state, &[KEY_A]);
    ime.pump(&mut run, "an unbound key reaches the grab", |_, _| {
        method
            .key_events()
            .ends_with(&[(KEY_A, true), (KEY_A, false)])
    });
    window.roundtrip(&mut run);
    assert!(window.client.keys.is_empty(), "the window saw no key");
    run.stop();
}

/// Super+Q and Alt+F4 close the window with the keyboard focus (D15), which sees neither key;
/// with a layer surface holding the keyboard no window is sent `close`; locked, both keys reach
/// the lock surface.
#[test]
fn super_q_and_alt_f4_close_the_focused_window() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_Q][..], "Super+Q"),
        (&[KEY_LEFTALT, KEY_F4], "Alt+F4"),
    ] {
        let closed = window.client.toplevel_closed.len();
        acts(
            &mut window,
            &mut run,
            codes,
            Some(Action::CloseFocused),
            what,
        );
        assert_eq!(
            window.client.toplevel_closed.len(),
            closed + 1,
            "{what}: the window is sent close"
        );
    }

    // A launcher's exclusive surface holds the keyboard: nothing is closed.
    let mut launcher = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = launcher.bind(4);
    let shm: wl_shm::WlShm = launcher.bind(1);
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = launcher.bind(4);
    let seat: wl_seat::WlSeat = launcher.bind(1);
    let qh = launcher.queue.handle();
    let _keyboard = seat.get_keyboard(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    let layer = shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "launcher".into(),
        &qh,
        (),
    );
    layer.set_size(100, 100);
    layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
    assert!(
        launcher.answer(&mut run, &surface),
        "the launcher is configured"
    );
    let (serial, width, height) = launcher.client.layer_configure.expect("a configure");
    layer.ack_configure(serial);
    let (buffer, _) = launcher.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    launcher.roundtrip(&mut run);
    let exclusive = run.state.exclusive_layer().expect("an exclusive layer");
    assert_eq!(
        keyboard_focus(&run),
        Some(exclusive),
        "the launcher has the keyboard"
    );
    let closed = window.client.toplevel_closed.len();
    acts(
        &mut launcher,
        &mut run,
        &[KEY_LEFTMETA, KEY_Q],
        Some(Action::CloseFocused),
        "Super+Q on the launcher",
    );
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.toplevel_closed.len(),
        closed,
        "no window is sent close"
    );

    let (mut locker, _lock) = lock_session(&mut run);
    let fired = run.state.fired.len();
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_Q][..], "Super+Q"),
        (&[KEY_LEFTALT, KEY_F4], "Alt+F4"),
    ] {
        reaches(&mut locker, &mut run, codes, what);
    }
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.toplevel_closed.len(),
        closed,
        "locked, nothing is closed"
    );
    assert_eq!(run.state.fired.len(), fired);
    run.stop();
}

/// Super+Space steps the seat's layout group, the first after the last, unlocked and locked
/// (D14): the client sees no Space, and every keyboard group object hears the group; under a
/// keymap of one layout it changes nothing.
#[test]
fn super_space_steps_the_layout_group_even_locked() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let space = [KEY_LEFTMETA, KEY_SPACE];
    acts(
        &mut window,
        &mut run,
        &space,
        Some(Action::NextGroup),
        "one layout",
    );
    assert_eq!(run.state.seat_group(), 0, "one layout: no step");
    two_layouts(&mut run);
    window.roundtrip(&mut run);
    let mut bar = Local::connect(&mut run, None);
    let (_keyboard, object) = bar.keyboard_group(&mut run);
    acts(
        &mut window,
        &mut run,
        &space,
        Some(Action::NextGroup),
        "to it",
    );
    assert_eq!(run.state.seat_group(), 1);
    acts(
        &mut window,
        &mut run,
        &space,
        Some(Action::NextGroup),
        "back to us",
    );
    assert_eq!(run.state.seat_group(), 0, "the first after the last");
    let (mut locker, _lock) = lock_session(&mut run);
    acts(
        &mut locker,
        &mut run,
        &space,
        Some(Action::NextGroup),
        "locked",
    );
    assert_eq!(run.state.seat_group(), 1, "locked, the group still steps");
    bar.roundtrip(&mut run);
    assert_eq!(
        bar.groups_of(&object),
        [0, 1, 0, 1],
        "the bar hears every step"
    );
    run.stop();
}

/// A client's surface on the top layer, `anchor`ed along an edge of the first output, 20 high,
/// with the keyboard interactivity `keyboard`.
fn top_surface(
    run: &mut Running,
    namespace: &str,
    anchor: zwlr_layer_surface_v1::Anchor,
    keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
) -> (Local, WlSurface) {
    let (local, server, _) = top_surface_on(run, namespace, anchor, keyboard, None);
    (local, server)
}

/// `top_surface` on the client's `wl_output` number `output` when given, with the client's
/// layer surface.
fn top_surface_on(
    run: &mut Running,
    namespace: &str,
    anchor: zwlr_layer_surface_v1::Anchor,
    keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    output: Option<usize>,
) -> (Local, WlSurface, zwlr_layer_surface_v1::ZwlrLayerSurfaceV1) {
    let mut local = Local::connect(run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = local.bind(4);
    let qh = local.queue.handle();
    let output = output.map(|index| {
        let (name, _, version) = local
            .client
            .globals
            .iter()
            .filter(|(_, interface, _)| interface == "wl_output")
            .nth(index)
            .cloned()
            .expect("the output");
        let bound: wl_output::WlOutput = local.registry.bind(name, version.min(4), &qh, ());
        bound
    });
    let surface = compositor.create_surface(&qh, ());
    let layer = shell.get_layer_surface(
        &surface,
        output.as_ref(),
        zwlr_layer_shell_v1::Layer::Top,
        namespace.into(),
        &qh,
        (),
    );
    layer.set_anchor(anchor);
    layer.set_size(0, 20);
    layer.set_keyboard_interactivity(keyboard);
    assert!(local.answer(run, &surface), "{namespace} is configured");
    let (serial, width, _) = local.client.layer_configure.expect("a configure");
    layer.ack_configure(serial);
    let width = i32::try_from(width).expect("a width");
    let (buffer, _) = local.buffer(&shm, width, 20, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(run);
    let server = run
        .state
        .space
        .outputs()
        .find_map(|output| {
            let map = layer_map_for_output(output);
            let layer = map.layers().find(|layer| layer.namespace() == namespace)?;
            Some(layer.wl_surface().clone())
        })
        .expect("the layer surface");
    (local, server, layer)
}

/// Ctrl+Alt+Tab gives the keyboard to the bar, then the dock, then the window (AX7): the
/// surfaces that take it on demand, those anchored to the top first; one that takes none is
/// skipped, an identity Super+Escape took the keyboard from is skipped, and nothing happens
/// while the session is locked.
#[test]
fn ctrl_alt_tab_cycles_the_bar_the_dock_and_the_window() {
    use crate::bindings::Action;
    use zwlr_layer_surface_v1::{
        Anchor,
        KeyboardInteractivity::{None as NoKeyboard, OnDemand},
    };
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    // The dock is mapped before the bar, and a surface with no keyboard sits between.
    let top = Anchor::Top | Anchor::Left | Anchor::Right;
    let (_dock, dock_surface) = top_surface(
        &mut run,
        "dock",
        Anchor::Bottom | Anchor::Left | Anchor::Right,
        OnDemand,
    );
    let (_ticker, _) = top_surface(&mut run, "ticker", top, NoKeyboard);
    let (_bar, bar_surface) = top_surface(&mut run, "bar", top, OnDemand);
    let window_surface = keyboard_focus(&run).expect("the window has the keyboard");
    let cycle = [KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB];

    // The key moves the keyboard, so the modifiers' releases go to the surface that has it
    // then; what the chord does is read from `fired` and the focus.
    let next = |run: &mut Running, expected: &WlSurface, what: &str| {
        let fired = run.state.fired.len();
        chord(&mut run.state, &cycle);
        assert_eq!(run.state.fired[fired..], [Action::FocusCycle], "{what}");
        assert_eq!(keyboard_focus(run).as_ref(), Some(expected), "{what}");
    };
    next(&mut run, &bar_surface, "to the bar");
    next(&mut run, &dock_surface, "to the dock");
    next(&mut run, &window_surface, "back to the window");
    next(&mut run, &bar_surface, "to the bar again");
    window.roundtrip(&mut run);
    assert!(
        !window.client.keys.contains(&KEY_TAB),
        "no client is sent the Tab"
    );

    // Super+Escape took the keyboard from this identity, which every client of the fixture
    // shares: its surfaces are skipped, and the window keeps the keyboard.
    let identity = bar_surface
        .client()
        .and_then(|client| crate::state::identity_of(&client))
        .expect("the fixture's identity");
    run.state.escaped.push(identity);
    next(&mut run, &window_surface, "the escaped are skipped");
    run.state.escaped.clear();

    // An exclusive surface keeps the keyboard (D10).
    let (_launcher, exclusive) = top_surface(
        &mut run,
        "launcher",
        top,
        zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
    );
    assert_eq!(keyboard_focus(&run).as_ref(), Some(&exclusive));
    chord(&mut run.state, &cycle);
    assert_eq!(
        keyboard_focus(&run).as_ref(),
        Some(&exclusive),
        "an exclusive surface keeps the keyboard"
    );

    let (mut locker, _lock) = lock_session(&mut run);
    let held = keyboard_focus(&run);
    let fired = run.state.fired.len();
    reaches(&mut locker, &mut run, &cycle, "Ctrl+Alt+Tab locked");
    assert_eq!(run.state.fired.len(), fired, "locked, nothing is taken");
    assert_eq!(keyboard_focus(&run), held, "the lock surface keeps it");
    run.stop();
}

/// With the window on the second output, Ctrl+Alt+Tab gives the keyboard to that output's
/// bar, not the first output's (#829).
#[test]
fn ctrl_alt_tab_cycles_the_focused_windows_output() {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity::OnDemand};
    let mut run = Running::start(None);
    let (_window, window) = focused_window(&mut run);
    let top = Anchor::Top | Anchor::Left | Anchor::Right;
    let (_first_bar, _, _) = top_surface_on(&mut run, "bar-1", top, OnDemand, Some(0));
    let (_second_bar, second_bar, _) = top_surface_on(&mut run, "bar-2", top, OnDemand, Some(1));
    let second = output_at(&run, 1);
    assert!(
        layer_map_for_output(&second)
            .layer_for_surface(&second_bar, smithay::desktop::WindowSurfaceType::TOPLEVEL)
            .is_some(),
        "bar-2 is on the second output"
    );
    let at = run.state.space.output_geometry(&second).expect("placed").loc;
    run.state.space.relocate_element(&window, at);
    run.state.space.refresh();
    assert!(keyboard_focus(&run).is_some(), "the window has the keyboard");
    chord(&mut run.state, &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB]);
    assert_eq!(
        keyboard_focus(&run).as_ref(),
        Some(&second_bar),
        "the bar of the window's output"
    );
    run.stop();
}

/// From a popup of the bar, Ctrl+Alt+Tab continues after the bar, to the dock, instead of
/// starting again at the bar (#829).
#[test]
fn ctrl_alt_tab_from_a_popup_continues_after_its_parent() {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity::OnDemand};
    let mut run = Running::start(None);
    let (_window, _) = focused_window(&mut run);
    let (_dock, dock_surface) = top_surface(
        &mut run,
        "dock",
        Anchor::Bottom | Anchor::Left | Anchor::Right,
        OnDemand,
    );
    let top = Anchor::Top | Anchor::Left | Anchor::Right;
    let (mut bar, bar_surface, bar_layer) = top_surface_on(&mut run, "bar", top, OnDemand, None);
    let cycle = [KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB];
    chord(&mut run.state, &cycle);
    assert_eq!(keyboard_focus(&run).as_ref(), Some(&bar_surface), "to the bar");

    // A key typed to the bar lets it open a popup that takes the keyboard.
    let serial = SERIAL_COUNTER.next_serial();
    let bar_client = bar_surface.client().expect("the bar's client").id();
    run.state.grab_serials.key_press = Some((serial, bar_client));
    let compositor: wl_compositor::WlCompositor = bar.bind(4);
    let shm: wl_shm::WlShm = bar.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = bar.bind(1);
    let seat: wl_seat::WlSeat = bar.bind(1);
    let qh = bar.queue.handle();
    let positioner = wm_base.create_positioner(&qh, ());
    positioner.set_size(40, 40);
    positioner.set_anchor_rect(0, 0, 1, 1);
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let popup = xdg.get_popup(None, &positioner, &qh, ());
    bar_layer.get_popup(&popup);
    popup.grab(&seat, serial.into());
    bar.client.configure = None;
    surface.commit();
    bar.pump(&mut run, "the popup is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(bar.client.configure.expect("a configure"));
    let (buffer, _) = bar.buffer(&shm, 40, 40, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    bar.roundtrip(&mut run);
    let held = keyboard_focus(&run);
    assert!(
        held.is_some() && held.as_ref() != Some(&bar_surface),
        "the popup has the keyboard"
    );

    chord(&mut run.state, &cycle);
    assert_eq!(
        keyboard_focus(&run).as_ref(),
        Some(&dock_surface),
        "after the bar, the dock"
    );
    assert!(
        smithay::desktop::PopupManager::popups_for_surface(&bar_surface)
            .next()
            .is_none(),
        "the popup is closed"
    );
    assert!(
        !run.state.seat.get_pointer().expect("a pointer").is_grabbed(),
        "its grab ends with it"
    );
    run.stop();
}

/// Super pressed and released alone shows the launcher, after the client was sent both (D4).
#[test]
fn super_alone_shows_the_launcher() {
    use crate::bindings::Action;
    use crate::session_calls::LAUNCHER;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let (pressed, released) = sent_to(&mut window, &mut run, &[KEY_LEFTMETA]);
    assert_eq!(
        (pressed, released),
        (vec![KEY_LEFTMETA], vec![KEY_LEFTMETA])
    );
    assert_eq!(run.state.fired, [Action::Call(&LAUNCHER)]);
    run.stop();
}

/// Super with another key, with a click, or followed by Ctrl shows nothing on its release (D4).
#[test]
fn super_with_a_key_or_a_click_shows_nothing() {
    use smithay::backend::input::KeyState;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    reaches(&mut window, &mut run, &[KEY_LEFTMETA, KEY_X], "Super+X");
    reaches(
        &mut window,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTCTRL],
        "Super then Ctrl",
    );
    let meta = smithay::input::keyboard::Keycode::from(KEY_LEFTMETA + 8);
    let key = |state: &mut State, key_state| {
        state.key_input(meta, key_state, InputTime::now(), KeyboardSource::Physical);
    };
    key(&mut run.state, KeyState::Pressed);
    run.state
        .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
    run.state
        .button_input(BTN_LEFT, ButtonState::Released, InputTime::now());
    key(&mut run.state, KeyState::Released);
    window.roundtrip(&mut run);
    assert!(run.state.fired.is_empty(), "Super with a click");
    run.stop();
}

/// Super+C toggles the control center; the client sees no C (CC9).
#[test]
fn super_c_toggles_the_control_center() {
    use crate::bindings::Action;
    use crate::session_calls::CONTROL_CENTER;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    acts(
        &mut window,
        &mut run,
        &[KEY_LEFTMETA, KEY_C],
        Some(Action::Call(&CONTROL_CENTER)),
        "Super+C",
    );
    run.stop();
}

/// While locked, Super alone reaches the lock surface and shows nothing (D4).
#[test]
fn super_alone_does_nothing_while_locked() {
    let mut run = Running::start(None);
    let (mut locker, _lock) = lock_session(&mut run);
    reaches(&mut locker, &mut run, &[KEY_LEFTMETA], "Super");
    run.stop();
}

/// Super held into a lock, or Super pressed on the lock surface, opens nothing after the unlock
/// when it is released; nor does a second Super pressed while one is held (D4).
#[test]
fn super_held_across_a_lock_or_beside_another_super_shows_nothing() {
    use crate::bindings::Action;
    use smithay::backend::input::{InputTime, KeyState};
    let key = |run: &mut Running, code: u32, key_state| {
        run.state.key_input(
            (code + 8).into(),
            key_state,
            InputTime::now(),
            KeyboardSource::Physical,
        );
    };
    let launcher = Action::Call(&crate::session_calls::LAUNCHER);
    let mut run = Running::start(None);
    let (_window, _) = focused_window(&mut run);
    for held_into_the_lock in [true, false] {
        if held_into_the_lock {
            key(&mut run, KEY_LEFTMETA, KeyState::Pressed);
        }
        let (mut locker, lock) = lock_session(&mut run);
        if !held_into_the_lock {
            key(&mut run, KEY_LEFTMETA, KeyState::Pressed);
        }
        lock.unlock_and_destroy();
        locker.pump(&mut run, "the session is unlocked", |_, state| {
            state.lock.is_none()
        });
        key(&mut run, KEY_LEFTMETA, KeyState::Released);
        assert!(
            !run.state.fired.contains(&launcher),
            "held into the lock: {held_into_the_lock}"
        );
    }
    key(&mut run, KEY_LEFTMETA, KeyState::Pressed);
    key(&mut run, KEY_RIGHTMETA, KeyState::Pressed);
    key(&mut run, KEY_RIGHTMETA, KeyState::Released);
    key(&mut run, KEY_LEFTMETA, KeyState::Released);
    assert!(!run.state.fired.contains(&launcher), "two Supers");
    run.stop();
}

/// A call is stamped with the lock epoch it was asked in: a lock moves the epoch on, so the
/// worker drops a call asked before it; after an unlock a call carries the current one.
#[test]
fn a_call_carries_the_current_lock_epoch() {
    use crate::session_calls::{Job, Request, SessionCalls, CONTROL_CENTER};
    use std::sync::atomic::Ordering;
    let mut run = Running::start(None);
    let (calls, requests) = SessionCalls::detached();
    run.state.session_calls = Some(calls);
    let (mut window, _) = focused_window(&mut run);
    let super_c = [KEY_LEFTMETA, KEY_C];
    sent_to(&mut window, &mut run, &super_c);
    let before = Request {
        job: Job::Call(&CONTROL_CENTER),
        epoch: 0,
    };
    assert_eq!(requests.try_recv().ok(), Some(before));
    let (mut locker, lock) = lock_session(&mut run);
    let epoch = run.state.lock_epoch.load(Ordering::SeqCst);
    assert_eq!(epoch, 1, "the lock moved the epoch on");
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    sent_to(&mut window, &mut run, &super_c);
    let after = Request {
        job: Job::Call(&CONTROL_CENTER),
        epoch,
    };
    assert_eq!(requests.try_recv().ok(), Some(after), "the current epoch");
    run.stop();
}

/// The media keys run their commands as transient units of the user manager (D6): mute,
/// microphone mute, play and pause, next and previous, each with the table's vector; locked,
/// only mute acts, and the others reach the lock surface.
#[test]
fn the_media_keys_run_their_commands() {
    use crate::bindings::Action;
    use crate::session_calls::{MIC_MUTE, MUTE, NEXT, PLAY_PAUSE, PREVIOUS};
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let keys = [
        (KEY_MUTE, &MUTE, "XF86AudioMute"),
        (KEY_MICMUTE, &MIC_MUTE, "XF86AudioMicMute"),
        (KEY_PLAYPAUSE, &PLAY_PAUSE, "XF86AudioPlay"),
        (KEY_NEXTSONG, &NEXT, "XF86AudioNext"),
        (KEY_PREVIOUSSONG, &PREVIOUS, "XF86AudioPrev"),
    ];
    for (code, command, what) in keys {
        acts(
            &mut window,
            &mut run,
            &[code],
            Some(Action::Run(command)),
            what,
        );
    }
    let (mut locker, _lock) = lock_session(&mut run);
    acts(
        &mut locker,
        &mut run,
        &[KEY_MUTE],
        Some(Action::Run(&MUTE)),
        "locked, XF86AudioMute",
    );
    for (code, _, what) in &keys[1..] {
        reaches(&mut locker, &mut run, &[*code], what);
    }
    run.stop();
}

/// Super+Shift+Escape and `XF86PowerOff` run cosmic-osd's log-out and power dialogs (D6);
/// neither acts while locked.
#[test]
fn the_session_keys_run_their_commands() {
    use crate::bindings::Action;
    use crate::session_calls::{LOG_OUT, POWER_OFF};
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let log_out = [KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_ESC];
    acts(
        &mut window,
        &mut run,
        &log_out,
        Some(Action::Run(&LOG_OUT)),
        "Super+Shift+Escape",
    );
    acts(
        &mut window,
        &mut run,
        &[KEY_POWER],
        Some(Action::Run(&POWER_OFF)),
        "XF86PowerOff",
    );
    let (mut locker, _lock) = lock_session(&mut run);
    reaches(
        &mut locker,
        &mut run,
        &log_out,
        "locked, Super+Shift+Escape",
    );
    reaches(&mut locker, &mut run, &[KEY_POWER], "locked, XF86PowerOff");
    run.stop();
}

/// The touchpad key, alone or with Super+Ctrl, toggles the touchpads (D16); headless there are
/// none, and the device side is checked at the console. Not while locked.
#[test]
fn the_touchpad_key_toggles_the_touchpads() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    acts(
        &mut window,
        &mut run,
        &[KEY_TOUCHPAD_TOGGLE],
        Some(Action::TouchpadToggle),
        "XF86TouchpadToggle",
    );
    acts(
        &mut window,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_TOUCHPAD_TOGGLE],
        Some(Action::TouchpadToggle),
        "Super+Ctrl+XF86TouchpadToggle",
    );
    let (mut locker, _lock) = lock_session(&mut run);
    reaches(
        &mut locker,
        &mut run,
        &[KEY_TOUCHPAD_TOGGLE],
        "locked, XF86TouchpadToggle",
    );
    run.stop();
}

/// The volume and brightness keys call cosmic-settings-daemon as cosmic-comp's session does
/// (D13, decision 29), unlocked and locked (section 1).
#[test]
fn the_volume_and_brightness_keys_call_their_daemon() {
    use crate::bindings::cosmic::{BRIGHTNESS_DOWN, BRIGHTNESS_UP, VOLUME_DOWN, VOLUME_UP};
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let keys = [
        (KEY_VOLUMEUP, &VOLUME_UP, "XF86AudioRaiseVolume"),
        (KEY_VOLUMEDOWN, &VOLUME_DOWN, "XF86AudioLowerVolume"),
        (KEY_BRIGHTNESSUP, &BRIGHTNESS_UP, "XF86MonBrightnessUp"),
        (
            KEY_BRIGHTNESSDOWN,
            &BRIGHTNESS_DOWN,
            "XF86MonBrightnessDown",
        ),
    ];
    for (code, call, what) in keys {
        acts(
            &mut window,
            &mut run,
            &[code],
            Some(Action::Call(call)),
            what,
        );
    }
    let (mut locker, _lock) = lock_session(&mut run);
    for (code, call, what) in keys {
        acts(
            &mut locker,
            &mut run,
            &[code],
            Some(Action::Call(call)),
            what,
        );
    }
    run.stop();
}

/// Super+Alt+S toggles the screen reader through cosmic-settings-daemon (D13); not while
/// locked, where it reaches the lock surface.
#[test]
fn super_alt_s_toggles_the_screen_reader() {
    use crate::bindings::cosmic::SCREEN_READER;
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let keys = [KEY_LEFTMETA, KEY_LEFTALT, KEY_S];
    acts(
        &mut window,
        &mut run,
        &keys,
        Some(Action::Call(&SCREEN_READER)),
        "Super+Alt+S",
    );
    let (mut locker, _lock) = lock_session(&mut run);
    reaches(&mut locker, &mut run, &keys, "locked, Super+Alt+S");
    run.stop();
}

/// A directory of its own for an X11 test, under the system's temporary directory.
fn x11_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("athanor-x11-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// xwayland-satellite's Wayland socket confines its every client: one lists what a client
/// inside a security context lists, though its process is here the bar's (OS4); the socket goes
/// with the compositor.
#[test]
fn x11_the_satellite_gets_the_standard_globals() {
    let dir = x11_dir("globals");
    let mut run = Running::start(None);
    let program = dir.join("satellite");
    let number =
        crate::xwayland::start(&mut run.state, &dir, &dir, program, false).expect("display");
    let wayland = dir.join(format!("athanor-x11-{number}"));
    let stream = UnixStream::connect(&wayland).expect("the satellite's socket");
    let satellite = Local::over(&mut run, stream, None);
    let standard = rows(|class| matches!(class, Class::Standard | Class::LayerGated));
    assert_eq!(satellite.listed(), standard);
    drop(satellite);
    run.stop();
    assert!(!wayland.exists(), "the satellite's socket outlives the compositor");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// The first X11 client starts the satellite with the display, its socket and its own Wayland
/// socket; the client that came while it ran is dropped when it exits, and the next one starts
/// it again, no sooner than `RESTART` after the first start.
#[test]
fn x11_the_first_client_starts_the_satellite() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    let dir = x11_dir("start");
    let program = dir.join("satellite");
    let out = dir.join("satellite.out");
    std::fs::write(
        &program,
        "#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\n{\n  printf '%s\\n' \"$@\"\n  \
         if [ -e \"/proc/self/fd/$3\" ]; then echo open; else echo closed; fi\n  \
         echo \"${WAYLAND_DISPLAY-unset}\"\n  \
         echo \"${WAYLAND_SOCKET-unset} ${DISPLAY-unset} ${NOTIFY_SOCKET-unset}\"\n\
         } > \"$0.tmp\" && mv \"$0.tmp\" \"$0.out\"\n",
    )
    .expect("program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("mode");
    // A process another test thread forks while the script is open for writing holds it until
    // its exec, and the script's own exec fails meanwhile with ETXTBSY; once one exec succeeds,
    // no such process is left.
    loop {
        match std::process::Command::new(&program).arg("--probe").status() {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            status => {
                assert!(status.expect("the script runs").success());
                break;
            }
        }
    }

    let mut run = Running::start(None);
    let number =
        crate::xwayland::start(&mut run.state, &dir, &dir, program, false).expect("display");
    let socket = dir.join(format!(".X11-unix/X{number}"));
    let wayland = dir.join(format!("athanor-x11-{number}"));
    let mut x11 = UnixStream::connect(&socket).expect("an X11 client");
    run.until("the satellite starts", |state| {
        state.x11.as_ref().is_some_and(|x11| x11.starts == 1)
    });
    run.until("the satellite exits", |state| {
        state.x11.as_ref().is_some_and(|x11| !x11.running)
    });
    let lines = std::fs::read_to_string(&out).expect("what the satellite saw");
    let lines: Vec<&str> = lines.lines().collect();
    assert_eq!(lines[..2], [format!(":{number}").as_str(), "-listenfd"]);
    let wayland_display = wayland.to_str().expect("UTF-8");
    assert_eq!(lines[3..], ["open", wayland_display, "unset unset unset"]);

    x11.set_read_timeout(Some(TIMEOUT)).expect("timeout");
    let mut byte = [0];
    assert_eq!(x11.read(&mut byte).expect("read"), 0, "the waiting client stays");

    let first = run.state.x11.as_ref().and_then(|x11| x11.last).expect("started");
    let _again = UnixStream::connect(&socket).expect("another X11 client");
    run.until("the satellite starts again", |state| {
        state.x11.as_ref().is_some_and(|x11| x11.starts == 2 && !x11.running)
    });
    let second = run.state.x11.as_ref().and_then(|x11| x11.last).expect("started");
    assert!(second - first >= crate::xwayland::RESTART, "restarted after {:?}", second - first);
    run.stop();
    assert!(!socket.exists(), "the socket outlives the compositor");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Three workspaces on the first output: the first with the fixture's window and the client's
/// first, the second with the client's second, which is active and holds the keyboard, and the
/// third empty. The client, its second window and the list.
fn three_workspaces(
    run: &mut Running,
) -> (
    Local,
    smithay::desktop::Window,
    Vec<crate::workspace::WorkspaceId>,
) {
    let (mut local, first) = focused_window(run);
    // `focused_window` returns once the window is mapped, which is before its first buffer
    // places it on a workspace.
    local.pump(run, "the first window is placed", |_, state| {
        state.workspace_of(&first).is_some()
    });
    let list = workspaces_of(run, 0);
    assert_eq!(run.state.workspace_of(&first), Some(list[0]));
    assert_eq!(list.len(), 2);
    assert!(run.state.activate_workspace(list[1]));
    let second = local.another(run, "second");
    let second = local.server_window(run, &second.surface);
    let list = workspaces_of(run, 0);
    assert_eq!(list.len(), 3, "the second workspace took a window");
    assert!(keyboard_on(run, &second));
    (local, second, list)
}

/// Whether `window` holds the keyboard.
fn keyboard_on(run: &Running, window: &smithay::desktop::Window) -> bool {
    let focus = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.current_focus());
    focus.is_some() && focus.as_ref() == window.toplevel().map(|t| t.wl_surface())
}

/// Presses the chord `codes`: the actions the compositor took.
fn fires(run: &mut Running, codes: &[u32]) -> Vec<crate::bindings::Action> {
    let fired = run.state.fired.len();
    chord(&mut run.state, codes);
    run.state.fired[fired..].to_vec()
}

/// Super+1 to Super+9 activate the n-th workspace of the output with the keyboard's focus, its
/// last when n is past it, and Super+0 the last (OV13); while locked they reach the lock
/// surface.
#[test]
fn super_digit_switches_the_workspace() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut local, _, list) = three_workspaces(&mut run);
    let output = output_at(&run, 0);
    acts(
        &mut local,
        &mut run,
        &[KEY_LEFTMETA, KEY_1],
        Some(Action::Workspace(1)),
        "Super+1",
    );
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    acts(
        &mut local,
        &mut run,
        &[KEY_LEFTMETA, KEY_2],
        Some(Action::Workspace(2)),
        "Super+2",
    );
    assert_eq!(run.state.active_workspace(&output), Some(list[1]));
    // The last is empty: the keyboard leaves the client before the keys' release.
    assert_eq!(fires(&mut run, &[KEY_LEFTMETA, KEY_9]), [Action::Workspace(9)]);
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[2]),
        "past the last, the last"
    );
    assert!(run.state.activate_workspace(list[0]));
    assert_eq!(fires(&mut run, &[KEY_LEFTMETA, KEY_0]), [Action::LastWorkspace]);
    assert_eq!(run.state.active_workspace(&output), Some(list[2]));
    assert_eq!(workspaces_of(&run, 0), list, "switching moves no window");

    assert!(run.state.activate_workspace(list[0]));
    let (mut locker, _lock) = lock_session(&mut run);
    for (codes, what) in [
        (&[KEY_LEFTMETA, KEY_2][..], "Super+2"),
        (&[KEY_LEFTMETA, KEY_0], "Super+0"),
    ] {
        reaches(&mut locker, &mut run, codes, what);
    }
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    run.stop();
}

/// Super+Shift+1 to Super+Shift+9 and Super+Shift+0 move the focused window to that workspace,
/// which is activated with the window holding the keyboard; while locked they move nothing.
#[test]
fn super_shift_digit_moves_the_window() {
    use crate::bindings::Action;
    let mut run = Running::start(None);
    let (mut local, second, list) = three_workspaces(&mut run);
    let output = output_at(&run, 0);
    acts(
        &mut local,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_1],
        Some(Action::MoveTo(1)),
        "Super+Shift+1",
    );
    assert_eq!(run.state.workspace_of(&second), Some(list[0]));
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    assert!(keyboard_on(&run, &second), "the window keeps the keyboard");
    assert!(run.state.space.element_location(&second).is_some());
    assert_eq!(
        workspaces_of(&run, 0),
        [list[0], list[2]],
        "the emptied second workspace is removed"
    );
    acts(
        &mut local,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_0],
        Some(Action::MoveToLast),
        "Super+Shift+0",
    );
    assert_eq!(run.state.workspace_of(&second), Some(list[2]));
    assert_eq!(run.state.active_workspace(&output), Some(list[2]));
    assert!(keyboard_on(&run, &second));
    assert_eq!(workspaces_of(&run, 0).len(), 3, "an empty one follows");

    let (mut locker, _lock) = lock_session(&mut run);
    reaches(
        &mut locker,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_1],
        "Super+Shift+1",
    );
    assert_eq!(run.state.workspace_of(&second), Some(list[2]));
    assert_eq!(run.state.active_workspace(&output), Some(list[2]));
    run.stop();
}

/// Super+Ctrl with Left, Up, h or k activates the workspace before the active one, with Right,
/// Down, j or l the one after it, one step and no wrap at either end; while locked nothing.
#[test]
fn super_ctrl_steps_the_workspace() {
    use crate::bindings::Action;
    use crate::workspace::Step;
    let mut run = Running::start(None);
    let (_local, _, list) = three_workspaces(&mut run);
    let output = output_at(&run, 0);
    for (key, step, to) in [
        (KEY_LEFT, Step::Previous, list[0]),
        (KEY_UP, Step::Previous, list[0]),
        (KEY_H, Step::Previous, list[0]),
        (KEY_K, Step::Previous, list[0]),
        (KEY_RIGHT, Step::Next, list[2]),
        (KEY_DOWN, Step::Next, list[2]),
        (KEY_J, Step::Next, list[2]),
        (KEY_L, Step::Next, list[2]),
    ] {
        assert!(run.state.activate_workspace(list[1]));
        assert_eq!(
            fires(&mut run, &[KEY_LEFTMETA, KEY_LEFTCTRL, key]),
            [Action::Step(step)],
            "key {key}"
        );
        assert_eq!(run.state.active_workspace(&output), Some(to), "key {key}");
    }
    assert!(run.state.activate_workspace(list[0]));
    fires(&mut run, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFT]);
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[0]),
        "no wrap at the first"
    );
    assert!(run.state.activate_workspace(list[2]));
    fires(&mut run, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_RIGHT]);
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[2]),
        "no wrap at the last"
    );
    assert_eq!(workspaces_of(&run, 0), list, "stepping moves no window");

    assert!(run.state.activate_workspace(list[1]));
    let (mut locker, _lock) = lock_session(&mut run);
    reaches(
        &mut locker,
        &mut run,
        &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFT],
        "Super+Ctrl+Left",
    );
    assert_eq!(run.state.active_workspace(&output), Some(list[1]));
    run.stop();
}

/// Super+Shift+Ctrl with the same keys carries the focused window one workspace, which is
/// activated with the window holding the keyboard; no wrap; while locked nothing.
#[test]
fn super_shift_ctrl_carries_the_window() {
    use crate::bindings::Action;
    use crate::workspace::Step;
    let mut run = Running::start(None);
    let (mut local, second, list) = three_workspaces(&mut run);
    let output = output_at(&run, 0);
    let carry = |key| [KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_LEFTCTRL, key];
    acts(
        &mut local,
        &mut run,
        &carry(KEY_K),
        Some(Action::Carry(Step::Previous)),
        "Super+Shift+Ctrl+k",
    );
    assert_eq!(run.state.workspace_of(&second), Some(list[0]));
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    assert!(keyboard_on(&run, &second));
    assert_eq!(workspaces_of(&run, 0), [list[0], list[2]]);
    acts(
        &mut local,
        &mut run,
        &carry(KEY_LEFT),
        Some(Action::Carry(Step::Previous)),
        "Super+Shift+Ctrl+Left",
    );
    assert_eq!(
        run.state.workspace_of(&second),
        Some(list[0]),
        "no wrap at the first"
    );
    assert_eq!(run.state.active_workspace(&output), Some(list[0]));
    acts(
        &mut local,
        &mut run,
        &carry(KEY_RIGHT),
        Some(Action::Carry(Step::Next)),
        "Super+Shift+Ctrl+Right",
    );
    assert_eq!(run.state.workspace_of(&second), Some(list[2]));
    assert_eq!(run.state.active_workspace(&output), Some(list[2]));
    assert!(keyboard_on(&run, &second));
    assert_eq!(workspaces_of(&run, 0).len(), 3, "an empty one follows");

    let (mut locker, _lock) = lock_session(&mut run);
    reaches(&mut locker, &mut run, &carry(KEY_H), "Super+Shift+Ctrl+h");
    assert_eq!(run.state.workspace_of(&second), Some(list[2]));
    run.stop();
}

/// Leaving a workspace dismisses the popups of its windows and ends their grab: the keyboard
/// goes to the workspace shown, not to a menu no one sees.
#[test]
fn leaving_a_workspace_ends_its_popup_grab() {
    let mut run = Running::start(None);
    let (mut local, _, list) = three_workspaces(&mut run);
    let menu = local.another(&mut run, "menu");
    let window = local.server_window(&run, &menu.surface);
    run.state.focus(&window);
    grabbing_popup(&mut local, &mut run, &menu.xdg);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    local.pump(&mut run, "the popup's grab takes the keyboard", |_, state| {
        focus(state).is_some_and(|surface| state.popups.find_popup(&surface).is_some())
    });
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(keyboard.is_grabbed(), "the popup holds the keyboard");

    assert!(run.state.activate_workspace(list[0]));
    assert!(!keyboard.is_grabbed(), "the popup's keyboard grab ends");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(!pointer.is_grabbed(), "the popup's pointer grab ends");
    let focus = focus(&run.state);
    assert!(
        run.state
            .space
            .elements()
            .any(|shown| focus.is_some() && shown.toplevel().map(|t| t.wl_surface()) == focus.as_ref()),
        "the keyboard is on a window of the workspace shown"
    );
    run.stop();
}

/// A token, which a focused client can make at any time, that names a window on an inactive
/// workspace, minimised or not, drawn or hidden, switches nothing and restores nothing: the
/// workspace shown stays, and the window stays out of the space.
#[test]
fn a_token_for_a_window_on_an_inactive_workspace_keeps_the_workspace() {
    for minimised in [true, false] {
        for hidden in [true, false] {
            a_token_for_a_window_on_an_inactive_workspace(minimised, hidden);
        }
    }
}

fn a_token_for_a_window_on_an_inactive_workspace(minimised: bool, hidden: bool) {
    let mut run = Running::start(None);
    let (mut local, _, list) = three_workspaces(&mut run);
    let tray = local.another(&mut run, "tray");
    let window = local.server_window(&run, &tray.surface);
    if minimised {
        assert!(run.state.minimise(&window));
    }
    if hidden {
        tray.surface.attach(None, 0, 0);
        tray.surface.commit();
    }
    local.roundtrip(&mut run);
    assert!(run.state.activate_workspace(list[0]));
    let output = output_at(&run, 0);
    let serial = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.last_enter())
        .expect("an enter");
    let token = run.activation_token(&mut local, serial.into());
    let activation: xdg_activation_v1::XdgActivationV1 = local.bind(1);
    activation.activate(token, &tray.surface);
    local.roundtrip(&mut run);
    assert_eq!(run.state.is_minimised(&window), minimised, "as it was");
    assert_eq!(
        run.state.active_workspace(&output),
        Some(list[0]),
        "the workspace shown stays"
    );
    assert!(
        run.state.space.element_location(&window).is_none(),
        "the window stays out of the space"
    );
    run.stop();
}

/// A window a move drops with its centre on another output goes to that output's active
/// workspace, where it was dropped: switching the first output's workspace leaves it shown.
#[test]
fn a_window_dropped_on_another_output_takes_its_workspace() {
    let mut run = Running::start(None);
    let (mut local, window) = focused_window(&mut run);
    let first = output_at(&run, 0);
    let second = output_at(&run, 1);
    local.pump(&mut run, "the window is placed", |_, state| {
        state.workspace_of(&window).is_some()
    });
    let area = run
        .state
        .space
        .output_geometry(&second)
        .expect("the second output's place");
    run.state.space.relocate_element(&window, area.loc);
    run.state.settle(&window);
    let on_second = run.state.active_workspace(&second);
    assert!(on_second.is_some());
    assert_eq!(run.state.workspace_of(&window), on_second);
    assert_eq!(
        workspaces_of(&run, 1).len(),
        2,
        "an empty workspace follows on the second output"
    );
    assert_eq!(run.state.space.element_location(&window), Some(area.loc), "where it was dropped");

    let last = *workspaces_of(&run, 0).last().expect("a workspace");
    assert!(run.state.activate_workspace(last));
    assert!(run.state.space.element_location(&window).is_some(), "still shown");
    assert_eq!(run.state.output_of_workspace(last), Some(first));
    run.stop();
}

/// Maps a popup of `parent`, a window of `local`, that grabs the seat with a key serial of
/// `local`'s, as a menu opened from the keyboard does.
fn grabbing_popup(local: &mut Local, run: &mut Running, parent: &xdg_surface::XdgSurface) {
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let seat: wl_seat::WlSeat = local.bind(1);
    let qh = local.queue.handle();
    let positioner = wm_base.create_positioner(&qh, ());
    positioner.set_size(WIDTH, HEIGHT);
    positioner.set_anchor_rect(0, 0, 1, 1);
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let popup = xdg.get_popup(Some(parent), &positioner, &qh, ());
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, local.id.clone().expect("an id")));
    popup.grab(&seat, serial.into());
    local.client.configure = None;
    surface.commit();
    local.pump(run, "the popup is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.take().expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(run);
}

/// A popup of a window the user cannot see, on an inactive workspace or minimised, grabs
/// nothing, even with a key serial of its client's: the keyboard stays where it is.
#[test]
fn a_popup_of_a_hidden_window_grabs_nothing() {
    for minimised in [false, true] {
        let mut run = Running::start(None);
        let (mut local, _, list) = three_workspaces(&mut run);
        let hidden = local.another(&mut run, "hidden");
        let window = local.server_window(&run, &hidden.surface);
        if minimised {
            assert!(run.state.minimise(&window));
        } else {
            assert!(run.state.activate_workspace(list[0]));
            assert!(run.state.is_parked(&window));
        }
        let focus = keyboard_focus(&run);
        assert_eq!(
            focus.as_ref().and_then(|surface| surface.client()).map(|c| c.id()),
            local.id.clone(),
            "a window of the popup's client has the keyboard"
        );
        grabbing_popup(&mut local, &mut run, &hidden.xdg);
        let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
        assert!(!keyboard.is_grabbed(), "no keyboard grab");
        let pointer = run.state.seat.get_pointer().expect("a pointer");
        assert!(!pointer.is_grabbed(), "no pointer grab");
        assert_eq!(keyboard_focus(&run), focus, "the keyboard stays");
        run.stop();
    }
}

/// Minimising a window dismisses its popups and ends their grab: the keyboard goes to the window
/// on top, not to a menu no one sees (#831).
#[test]
fn minimising_a_window_ends_its_popup_grab() {
    let mut run = Running::start(None);
    let (mut local, _, _) = three_workspaces(&mut run);
    let menu = local.another(&mut run, "menu");
    let window = local.server_window(&run, &menu.surface);
    run.state.focus(&window);
    grabbing_popup(&mut local, &mut run, &menu.xdg);
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(keyboard.is_grabbed(), "the popup holds the keyboard");

    assert!(run.state.minimise(&window));
    assert!(!keyboard.is_grabbed(), "the popup's keyboard grab ends");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(!pointer.is_grabbed(), "the popup's pointer grab ends");
    let top = run
        .state
        .space
        .elements()
        .rev()
        .find(|window| crate::handlers::shown(window))
        .and_then(|window| window.toplevel().map(|t| t.wl_surface().clone()));
    assert!(top.is_some(), "a window is shown");
    assert_eq!(keyboard_focus(&run), top, "the keyboard is on the window on top");
    run.stop();
}

/// A popup of a window that draws nothing, one that committed a null buffer, grabs nothing, even
/// with a key serial of its client's.
#[test]
fn a_popup_of_a_window_drawing_nothing_grabs_nothing() {
    let mut run = Running::start(None);
    let (mut local, _, _) = three_workspaces(&mut run);
    let blank = local.another(&mut run, "blank");
    let window = local.server_window(&run, &blank.surface);
    run.state.focus(&window);
    blank.surface.attach(None, 0, 0);
    blank.surface.commit();
    local.roundtrip(&mut run);
    assert!(run.state.space.element_location(&window).is_some(), "still in the space");
    let focus = keyboard_focus(&run);
    assert_eq!(
        focus.as_ref().and_then(|surface| surface.client()).map(|c| c.id()),
        local.id.clone(),
        "a window of the popup's client has the keyboard"
    );
    grabbing_popup(&mut local, &mut run, &blank.xdg);
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(!keyboard.is_grabbed(), "no keyboard grab");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(!pointer.is_grabbed(), "no pointer grab");
    run.stop();
}

/// Unplugging an output with workspaces: its windows, shown and parked, join the first output's
/// active workspace and come back on the first output, and a client hears each workspace leave
/// its group and go, then the group go, in one `done`.
#[test]
fn unplugging_an_output_rehomes_its_workspaces() {
    let mut run = Running::start(None);
    let (mut local, shown) = focused_window(&mut run);
    local.pump(&mut run, "the window is placed", |_, state| {
        state.workspace_of(&shown).is_some()
    });
    let (first, second) = (output_at(&run, 0), output_at(&run, 1));
    let area = run.state.space.output_geometry(&second).expect("placed");
    let other = local.another(&mut run, "parked");
    let parked = local.server_window(&run, &other.surface);
    for window in [&shown, &parked] {
        run.state.space.relocate_element(window, area.loc);
        run.state.settle(window);
    }
    let list = workspaces_of(&run, 1);
    assert_eq!(list.len(), 2);
    assert!(run.state.move_window_to(&parked, list[1]));
    assert!(run.state.is_parked(&parked));
    assert_eq!(workspaces_of(&run, 1).len(), 3);

    let (mut client, _manager) = workspace_client(&mut run);
    client.client.workspace_log.clear();
    run.state.remove_output(&second);
    client.roundtrip(&mut run);
    let home = run.state.active_workspace(&first);
    let within = run.state.space.output_geometry(&first).expect("placed");
    for window in [&shown, &parked] {
        assert_eq!(run.state.workspace_of(window), home, "on the first output's workspace");
        let geometry = run.state.space.element_geometry(window).expect("shown");
        assert!(within.overlaps(geometry), "on the first output");
    }
    let mut heard = ["workspace_leave", "removed"].repeat(3);
    heard.extend(["group removed", "done"]);
    assert_eq!(client.client.workspace_log, heard);
    run.stop();
}

/// An output that changes size moves the others: a window of the second output's workspaces,
/// shown or parked, whose place its output no longer shows comes back on that output, not the
/// first, and keeps its workspace.
#[test]
fn a_window_fitted_after_a_mode_change_stays_on_its_output() {
    let mut run = Running::start(None);
    let (mut local, shown) = focused_window(&mut run);
    local.pump(&mut run, "the window is placed", |_, state| {
        state.workspace_of(&shown).is_some()
    });
    let (first, second) = (output_at(&run, 0), output_at(&run, 1));
    let area = run.state.space.output_geometry(&second).expect("placed");
    let other = local.another(&mut run, "parked");
    let parked = local.server_window(&run, &other.surface);
    let edge = area.loc + Point::from((area.size.w - 20, area.size.h - 20));
    for window in [&shown, &parked] {
        run.state.space.relocate_element(window, area.loc);
        run.state.settle(window);
        run.state.space.relocate_element(window, edge);
    }
    let list = workspaces_of(&run, 1);
    assert!(run.state.move_window_to(&parked, list[1]));
    assert!(run.state.is_parked(&parked));

    let small = smithay::output::Mode {
        size: (200, 150).into(),
        refresh: 60_000,
    };
    first.change_current_state(Some(small), None, None, None);
    run.state.arrange_outputs();
    let on_second = |run: &Running, window: &smithay::desktop::Window| {
        let area = run.state.space.output_geometry(&second).expect("placed");
        run.state
            .space
            .element_geometry(window)
            .is_some_and(|geometry| area.overlaps(geometry))
    };
    assert!(on_second(&run, &shown), "the shown window is on its output");
    assert_eq!(run.state.workspace_of(&shown), Some(list[0]));
    assert!(run.state.activate_workspace(list[1]));
    assert!(on_second(&run, &parked), "the parked window comes back on its output");
    assert_eq!(run.state.workspace_of(&parked), Some(list[1]));
    run.stop();
}

/// A window of an unplugged output that a third output still shows a strip of joins the first
/// output's active workspace and comes back on the first output, where that workspace is shown.
#[test]
fn a_window_of_an_unplugged_output_comes_back_on_the_first() {
    let mut run = Running::start(None);
    let (mut local, window) = focused_window(&mut run);
    local.pump(&mut run, "the window is placed", |_, state| {
        state.workspace_of(&window).is_some()
    });
    let (first, second) = (output_at(&run, 0), output_at(&run, 1));
    let third = crate::state::build_output(
        "HEADLESS-3",
        "fixture",
        (800, 600).into(),
        60_000,
        smithay::utils::Transform::Normal,
        1.0,
    );
    run.state.map_output(&third);
    let area = run.state.space.output_geometry(&second).expect("placed");
    run.state.space.relocate_element(&window, area.loc);
    run.state.settle(&window);
    let straddling = area.loc + Point::from((area.size.w - WIDTH + 10, 0));
    run.state.space.relocate_element(&window, straddling);

    run.state.remove_output(&second);
    assert_eq!(
        run.state.workspace_of(&window),
        run.state.active_workspace(&first)
    );
    let within = run.state.space.output_geometry(&first).expect("placed");
    let geometry = run.state.space.element_geometry(&window).expect("shown");
    assert!(within.overlaps(geometry), "on the first output");
    run.stop();
}

/// A window of the second output's workspace is maximised to the second output's area, fitted
/// to it again when the outputs move, and restored on that output when its place there moved
/// off every output.
#[test]
fn a_window_is_maximised_on_its_output() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let window = window_of(&run, &id);
    let (first, second) = (output_at(&run, 0), output_at(&run, 1));
    let at = run.state.space.output_geometry(&second).expect("placed").loc;
    run.state.space.relocate_element(&window, at);
    run.state.settle(&window);
    let list = workspaces_of(&run, 1);
    assert_eq!(run.state.workspace_of(&window), Some(list[0]));
    let size = run.state.space.output_geometry(&second).expect("placed").size;
    let edge = at + Point::from((size.w - 20, size.h - 20));
    run.state.space.relocate_element(&window, edge);

    toplevel.set_maximized();
    let area = run.state.window_area_on(&second).expect("an area");
    let (width, height, _) = local.redraw(&mut run);
    assert_eq!((width, height), (area.size.w, area.size.h));
    assert_eq!(location_of(&run, &id), area.loc);

    let small = smithay::output::Mode {
        size: (200, 150).into(),
        refresh: 60_000,
    };
    first.change_current_state(Some(small), None, None, None);
    run.state.arrange_outputs();
    let moved = run.state.window_area_on(&second).expect("an area");
    assert_ne!(moved.loc, area.loc, "the second output moved");
    local.redraw(&mut run);
    assert_eq!(location_of(&run, &id), moved.loc);
    assert_eq!(run.state.workspace_of(&window), Some(list[0]));

    toplevel.unset_maximized();
    local.redraw(&mut run);
    let shown = run.state.space.output_geometry(&second).expect("placed");
    let geometry = run.state.space.element_geometry(&window).expect("shown");
    assert!(shown.overlaps(geometry), "restored on its output");
    run.stop();
}

#[cfg(test)]
mod clipboard;
