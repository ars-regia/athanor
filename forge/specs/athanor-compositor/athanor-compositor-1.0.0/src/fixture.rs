//! The in-process fixture of doc_own_stack.md OS6: the compositor on its headless backend and a
//! Wayland client in one process, joined by a socket pair. The client lists the globals, maps a
//! window from a wl_shm buffer and waits for its frame callback; the compositor's frames are then
//! read back and their pixels compared.

use crate::handlers::MAX_HELD_COMMITS;
use crate::headless::{self, VirtualOutput};
use crate::identity::{AllowList, FileId, Process};
use crate::state::{State, Under, BACKGROUND};
use crate::Error;
use smithay::backend::allocator::dmabuf::{Dmabuf, DmabufFlags};
use smithay::backend::allocator::{Fourcc, Modifier};
use smithay::desktop::layer_map_for_output;
use smithay::input::keyboard::KeyboardSource;
use smithay::reexports::calloop::{EventLoop, Interest};
use smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::__interfaces::{
    ZWP_VIRTUAL_KEYBOARD_MANAGER_V1_INTERFACE, ZWP_VIRTUAL_KEYBOARD_V1_INTERFACE,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::utils::{Point, SERIAL_COUNTER};
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
use wayland_client::backend::protocol::{Argument, Message};
use wayland_client::backend::{Backend, ObjectData, ObjectId, WaylandError};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
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
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1, wp_security_context_v1,
};
use wayland_protocols::xdg::shell::client::{
    xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1, zwlr_output_configuration_v1, zwlr_output_head_v1,
    zwlr_output_manager_v1, zwlr_output_mode_v1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

/// The window: its top half red, its bottom half blue, so that a frame read upside down fails.
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
const GLOBALS: [&str; 13] = [
    "wl_compositor",
    "wl_subcompositor",
    "wl_shm",
    "wl_seat",
    "xdg_wm_base",
    "zwlr_layer_shell_v1",
    "zxdg_output_manager_v1",
    "wp_viewporter",
    "wp_fractional_scale_manager_v1",
    "wp_presentation",
    "wp_cursor_shape_manager_v1",
    "xdg_activation_v1",
    "wl_data_device_manager",
];

#[derive(Default)]
struct Client {
    /// Name, interface and version of each global.
    globals: Vec<(u32, String, u32)>,
    configure: Option<u32>,
    /// The serial, width and height of the last configure of a layer surface.
    layer_configure: Option<(u32, u32, u32)>,
    /// A layer surface was closed by the compositor.
    layer_closed: bool,
    frame_done: bool,
    /// Roundtrips answered (`Local::roundtrip`).
    synced: u32,
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
    /// The evdev code of each key pressed while the client had the keyboard.
    keys: Vec<u32>,
    /// The evdev codes of the keys held, as the last `enter` told them.
    entered_keys: Vec<u32>,
    /// The keymaps the client was sent.
    keymaps: usize,
    /// The heads of wlr-output-management, as the client has seen them.
    heads: Vec<OutputHead>,
    /// The serial of the output manager's last `done`.
    output_serial: Option<u32>,
    /// The answer to the last output configuration: `succeeded`, `failed` or `cancelled`.
    configured: Option<&'static str>,
    /// The last preferred scale of wp-fractional-scale, in 120ths.
    fractional_scale: Option<u32>,
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
delegate_noop!(Client: ignore xdg_toplevel::XdgToplevel);
delegate_noop!(Client: xdg_positioner::XdgPositioner);
delegate_noop!(Client: ignore xdg_popup::XdgPopup);
delegate_noop!(Client: ignore wl_seat::WlSeat);
delegate_noop!(Client: ignore wl_output::WlOutput);
delegate_noop!(Client: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(Client: ext_image_capture_source_v1::ExtImageCaptureSourceV1);
delegate_noop!(Client: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1);
delegate_noop!(Client: ext_session_lock_manager_v1::ExtSessionLockManagerV1);
delegate_noop!(Client: ext_idle_notifier_v1::ExtIdleNotifierV1);
delegate_noop!(Client: wp_security_context_manager_v1::WpSecurityContextManagerV1);
delegate_noop!(Client: wp_security_context_v1::WpSecurityContextV1);
delegate_noop!(Client: ignore zwlr_output_mode_v1::ZwlrOutputModeV1);
delegate_noop!(Client: zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1);
delegate_noop!(Client: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(Client: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
delegate_noop!(Client: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

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
            } => client.keys.push(key),
            wl_keyboard::Event::Enter { keys, .. } => {
                let (keys, _) = keys.as_chunks::<4>();
                client.entered_keys = keys.iter().copied().map(u32::from_ne_bytes).collect();
            }
            wl_keyboard::Event::Keymap { .. } => client.keymaps += 1,
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
            Event::Workspace { workspace } => client.workspaces.push((workspace.id(), None)),
            Event::Done => client.workspaces_done = true,
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
        if let ext_workspace_handle_v1::Event::State {
            state: WEnum::Value(state),
        } = event
        {
            if let Some((_, last)) = client
                .workspaces
                .iter_mut()
                .find(|(id, _)| *id == workspace.id())
            {
                *last = Some(state);
            }
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
    ext_image_copy_capture_manager_v1 ext_session_lock_manager_v1 ext_workspace_manager_v1 \
    zwp_virtual_keyboard_manager_v1 zwlr_virtual_pointer_manager_v1\n\
    /usr/bin/athanor-control-center athanor-control-center.service zwlr_layer_shell_v1 \
    zwlr_output_manager_v1\n";

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

    // Output 0 at scale 1: the window centred, red above blue, the background around it.
    let first = state.capture(0).expect("frame of output 0");
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    assert_eq!(first.len(), (w * h * 4) as usize);
    let (cx, cy) = (w / 2, h / 2);
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

    // Centred in what the zone leaves: 50 lower by half the zone, at y 375 rather than 350.
    steps.send(Step::Window).expect("the shell's client");
    let (x, y) = ((OUTPUTS[0].width - WIDTH) / 2, 50 + (750 - HEIGHT) / 2);
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
/// surface's, and asks for nothing. Super+Shift+Escape is the window's. A lock alone escapes
/// nobody.
#[test]
fn task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock() {
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);
    let window_client = window.as_ref().and_then(|s| s.client()).map(|c| c.id());

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
    // is not the window's; Super+Shift+Escape is.
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
    run.state
        .key_input(escape, KeyState::Pressed, now(), KeyboardSource::Physical);
    let escape_sent = run.state.grab_serials.key_press.clone();
    for key in [escape, shift, logo] {
        run.state
            .key_input(key, KeyState::Released, now(), KeyboardSource::Physical);
    }
    assert_eq!(
        escape_sent.map(|(_, to)| to),
        window_client,
        "Super+Shift+Escape is the window's"
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
/// on the first output, where it was placed at first.
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
    let placed = run.state.space.element_location(&window);
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
        placed,
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
        };
        local.roundtrip(run);
        local
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

/// The bar sees every global of `data/globals` once, but `wl_output`, once per output, and
/// `zwlr_output_manager_v1`, which its line does not name, not at all; a client inside a
/// security context sees only the standard and layer-gated ones (spike Q4: the privileged ones
/// are refused to sandboxes).
#[test]
fn task4_standard_globals_are_offered() {
    let mut run = Running::start(None);
    for global in rows(|_| true) {
        let count = run.globals.iter().filter(|g| *g == global).count();
        let once = usize::from(global != "zwlr_output_manager_v1");
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
        f64::from(h) / 2.0 - f64::from(HEIGHT) / 4.0,
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
        pixel(&first, w, w / 2, h / 2 - HEIGHT / 4),
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
        pixel(&first, w, w / 2, h / 2 - HEIGHT / 4),
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

/// Each output is a workspace group with one workspace, active, that the client's own
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
        assert_eq!(workspaces_in.len(), 1, "one workspace per group");
        let state = client
            .workspaces
            .iter()
            .find(|(id, _)| *id == workspaces_in[0])
            .and_then(|(_, state)| *state);
        assert_eq!(state, Some(ext_workspace_handle_v1::State::Active));
    }
    let bound: Vec<ObjectId> = outputs.iter().map(Proxy::id).collect();
    assert!(
        bound.iter().all(|id| entered.contains(id)),
        "each output has its group"
    );
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
    offered(&bar, "the bar", expected);

    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    let standard = rows(|class| matches!(class, Class::Standard | Class::LayerGated));
    offered(&sandboxed, "a client inside a security context", standard);

    let control_center = Local::control_center(&mut run);
    let mut expected = unprivileged.clone();
    expected.insert("zwlr_output_manager_v1");
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
/// keys only: the seat's is the same afterwards, and the same codes on the hardware keyboard, A
/// and S, ask for nothing.
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
    assert_eq!(
        seat_keymap(&mut run.state),
        keymap,
        "the seat's keymap is the seat's again"
    );
    chord(&mut run.state, &[31, 30]);
    assert_eq!(
        run.state.lock_requests, 1,
        "S and A on the hardware keyboard are S and A"
    );

    drop(steps);
    shell
        .join()
        .expect("the shell's thread")
        .expect("the shell's client");
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
    // The red half of the window, centred on the first output.
    let (w, h) = (f64::from(OUTPUTS[0].width), f64::from(OUTPUTS[0].height));
    let target = Point::from((w / 2.0, h / 2.0 - f64::from(HEIGHT) / 4.0));
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
    // The red half of the window, centred on the first output.
    let (w, h) = (f64::from(OUTPUTS[0].width), f64::from(OUTPUTS[0].height));
    let target = Point::from((w / 2.0, h / 2.0 - f64::from(HEIGHT) / 4.0));
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
