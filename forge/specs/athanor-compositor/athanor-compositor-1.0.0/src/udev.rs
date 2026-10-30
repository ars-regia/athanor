//! The DRM backend (doc_own_stack.md, C1 task 8), after Smithay's anvil: the compositor runs in
//! a session of the seat through libseat, draws each monitor of the seat's primary GPU through
//! KMS, and reads input through libinput. A monitor plugged in or out adds or removes its
//! output; a switch to another virtual terminal pauses the session, and the switch back resumes
//! it and draws every output again. An output draws only when something may have changed (a
//! client's request or commit, input, an output or the lock), at most once a refresh, and not
//! at all while idle.

use crate::capture::Target;
use crate::cursor::{pointer_elements, Cursor, PointerElement};
use crate::headless::{gles, report_frame_times};
use crate::state::{Backend, OutputGlobal, State, BACKGROUND, FRAME_THROTTLE};
use crate::Error;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::{FrameFlags, PrimaryPlaneElement, RenderFrameError};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements};
use smithay::backend::drm::{
    DrmAccessError, DrmDevice, DrmDeviceFd, DrmError, DrmEvent, DrmEventMetadata, DrmEventTime,
    DrmNode, NodeType,
};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{
    LibinputInputBackend, LibinputSessionInterface, PointerScrollAxis,
};
use smithay::backend::renderer::damage::Error as DamageError;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{Kind, RenderElementStates};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::GpuManager;
use smithay::backend::renderer::ImportDma;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{primary_gpu, UdevBackend, UdevEvent};
use smithay::backend::SwapBuffersError;
use smithay::desktop::layer_map_for_output;
use smithay::desktop::space::{space_render_elements, SpaceRenderElements};
use smithay::desktop::utils::OutputPresentationFeedback;
use smithay::desktop::Window;
use smithay::output::{Mode, Output};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, RegistrationToken};
use smithay::reexports::drm::control::{connector, crtc, ModeTypeFlags};
use smithay::reexports::drm::Device as _;
use smithay::reexports::input::event::gesture::{
    GestureHoldEvent, GesturePinchEvent, GestureSwipeEvent,
};
use smithay::reexports::input::event::{
    DeviceEvent, EventTrait, GestureEvent, KeyboardEvent, PointerEvent, SwitchEvent,
    TabletToolEvent,
};
use smithay::reexports::input::{
    Device as InputDevice, Event as LibinputEvent, Libinput, SendEventsMode,
};
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_server::Display;
use smithay::utils::{DeviceFd, Transform};
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use smithay::wayland::drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjState};
use smithay::wayland::presentation::Refresh;
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The formats tried for each output's swapchain, in order: 8 bits a channel.
// ponytail: no 10-bit formats; anvil tries them first when a monitor needs deep colour.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Abgr8888, Fourcc::Argb8888];

type Gpus = GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>;
type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Heads = DrmOutputManager<Allocator, Exporter, OutputPresentationFeedback, DrmDeviceFd>;
type HeadOutput = DrmOutput<Allocator, Exporter, OutputPresentationFeedback, DrmDeviceFd>;

smithay::backend::renderer::element::render_elements! {
    Element<=GlesRenderer>;
    Space=SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Pointer=PointerElement<GlesRenderer>,
}

/// The session, the GPU and its monitors, and the input devices.
// ponytail: the seat's primary GPU only; a second GPU, and its monitors, are not used.
pub struct Drm {
    session: LibSeatSession,
    libinput: Libinput,
    gpus: Gpus,
    /// The GPU's render node, which draws.
    render_node: DrmNode,
    /// The GPU's primary node, whose udev events say a monitor came or went.
    node: DrmNode,
    heads: Heads,
    scanner: DrmScanner,
    outputs: HashMap<crtc::Handle, Head>,
    /// The monitors that could not be set up, with the modes and the CRTC they failed with.
    failed: HashMap<connector::Handle, (connector::Info, crtc::Handle)>,
    /// The overlay planes are left alone on NVIDIA's driver, as anvil does.
    nvidia: bool,
    cursor: Cursor,
    /// The devices that can tap, which the touchpad key turns off and on (D16).
    touchpads: Vec<InputDevice>,
    /// Whether the touchpad key has turned them off.
    touchpads_off: bool,
}

/// What a scheduled draw of an output is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wake {
    /// A draw that is due.
    Draw,
    /// The frame callbacks of a surface shown on no output, at `FRAME_THROTTLE`: a request to
    /// draw does not wait for it.
    Throttle,
}

/// A monitor: the output and what draws it.
struct Head {
    output: Output,
    drm_output: HeadOutput,
    /// A frame is queued and waits for its VBlank.
    queued: bool,
    /// The VBlanks still owed for frames queued before the output was turned off: each comes
    /// and presents nothing (`power_output`). A session switch drops them (`resume`).
    stale_vblanks: u8,
    /// Whether its frames showed the lock.
    lock: LockFrames,
    /// The next draw, when one is scheduled, and what it is for.
    timer: Option<(RegistrationToken, Wake)>,
    /// Whether its draws keep failing, so that the log says so once (`FailureLog`).
    failing: FailureLog,
    /// Something may have changed since the last draw (`request_redraw`).
    wanted: bool,
    /// The earliest next draw: six tenths of a period after a VBlank, a period after a draw
    /// that had nothing new to show.
    not_before: Option<Instant>,
    /// Whether it is on: an output the power client turned off draws nothing (`power_output`).
    powered: bool,
}

/// Turns `device`'s events on or off; a device that cannot is logged.
fn send_events(device: &InputDevice, on: bool) {
    let mode = if on {
        SendEventsMode::ENABLED
    } else {
        SendEventsMode::DISABLED
    };
    if let Err(error) = device.config_send_events_set_mode(mode) {
        tracing::warn!(
            ?error,
            device = device.sysname(),
            on,
            "a touchpad's events could not be switched"
        );
    }
}

/// A libinput event as `LibinputInputBackend` hands it to `State::process_input_event`, for the
/// devices, the switches, the gestures, the tablet tools and the kinds that handler acts on;
/// `None` for the rest, which it takes only as activity. A kind the handler starts to act on is added here as well (`State::take_queued_input`).
fn backend_event(event: LibinputEvent) -> Option<InputEvent<LibinputInputBackend>> {
    Some(match event {
        LibinputEvent::Device(DeviceEvent::Added(added)) => InputEvent::DeviceAdded {
            device: added.device(),
        },
        LibinputEvent::Device(DeviceEvent::Removed(removed)) => InputEvent::DeviceRemoved {
            device: removed.device(),
        },
        LibinputEvent::Switch(SwitchEvent::Toggle(event)) => InputEvent::SwitchToggle { event },
        LibinputEvent::Keyboard(KeyboardEvent::Key(event)) => InputEvent::Keyboard { event },
        LibinputEvent::Pointer(PointerEvent::Motion(event)) => InputEvent::PointerMotion { event },
        LibinputEvent::Pointer(PointerEvent::MotionAbsolute(event)) => {
            InputEvent::PointerMotionAbsolute { event }
        }
        LibinputEvent::Pointer(PointerEvent::Button(event)) => InputEvent::PointerButton { event },
        LibinputEvent::Pointer(PointerEvent::ScrollWheel(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Wheel(event),
        },
        LibinputEvent::Pointer(PointerEvent::ScrollFinger(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Finger(event),
        },
        LibinputEvent::Pointer(PointerEvent::ScrollContinuous(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Continuous(event),
        },
        LibinputEvent::Gesture(gesture) => match gesture {
            GestureEvent::Swipe(GestureSwipeEvent::Begin(event)) => {
                InputEvent::GestureSwipeBegin { event }
            }
            GestureEvent::Swipe(GestureSwipeEvent::Update(event)) => {
                InputEvent::GestureSwipeUpdate { event }
            }
            GestureEvent::Swipe(GestureSwipeEvent::End(event)) => {
                InputEvent::GestureSwipeEnd { event }
            }
            GestureEvent::Pinch(GesturePinchEvent::Begin(event)) => {
                InputEvent::GesturePinchBegin { event }
            }
            GestureEvent::Pinch(GesturePinchEvent::Update(event)) => {
                InputEvent::GesturePinchUpdate { event }
            }
            GestureEvent::Pinch(GesturePinchEvent::End(event)) => {
                InputEvent::GesturePinchEnd { event }
            }
            GestureEvent::Hold(GestureHoldEvent::Begin(event)) => {
                InputEvent::GestureHoldBegin { event }
            }
            GestureEvent::Hold(GestureHoldEvent::End(event)) => {
                InputEvent::GestureHoldEnd { event }
            }
            _ => return None,
        },
        LibinputEvent::Tablet(tablet) => match tablet {
            TabletToolEvent::Axis(event) => InputEvent::TabletToolAxis { event },
            TabletToolEvent::Proximity(event) => InputEvent::TabletToolProximity { event },
            TabletToolEvent::Tip(event) => InputEvent::TabletToolTip { event },
            TabletToolEvent::Button(event) => InputEvent::TabletToolButton { event },
            _ => return None,
        },
        _ => return None,
    })
}

/// Starts the DRM backend in a new session of the seat, on the seat's primary GPU.
pub fn start(
    event_loop: &mut EventLoop<'static, State>,
    display: Display<State>,
) -> Result<State, Error> {
    let (mut session, notifier) = LibSeatSession::new()?;
    let seat = session.seat();
    let path = primary_gpu(&seat)?.ok_or_else(|| format!("no GPU on {seat}"))?;
    let node = DrmNode::from_path(&path)?;
    let render_node = node
        .node_with_type(NodeType::Render)
        .transpose()?
        .ok_or_else(|| format!("{} has no render node", path.display()))?;
    let fd = session.open(
        &path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (device, drm_events) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd.clone())?;
    let nvidia = device.get_driver().is_ok_and(|driver| {
        let name = |s: &std::ffi::OsStr| s.to_string_lossy().to_lowercase().contains("nvidia");
        name(driver.name()) || name(driver.description())
    });

    let mut api = GbmGlesBackend::default();
    api.add_node(render_node, gbm.clone())?;
    let mut gpus = GpuManager::new(api)?;
    let renderer = gles(&mut gpus, &render_node)?;
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();
    let dmabuf_formats = renderer.dmabuf_formats();
    let heads = DrmOutputManager::new(
        device,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        GbmFramebufferExporter::new(gbm.clone(), render_node.into()),
        Some(gbm),
        COLOR_FORMATS,
        render_formats,
    );

    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| format!("libinput could not take the devices of {seat}"))?;
    let udev = UdevBackend::new(&seat)?;

    let drm = Drm {
        session,
        libinput: libinput.clone(),
        gpus,
        render_node,
        node,
        heads,
        scanner: DrmScanner::new(),
        outputs: HashMap::new(),
        failed: HashMap::new(),
        nvidia,
        cursor: Cursor::from_env(),
        touchpads: Vec::new(),
        touchpads_off: false,
    };
    let dh = display.handle();
    let mut state = State::new(event_loop, display, Backend::Drm(Box::new(drm)))?;
    state.session_calls = crate::session_calls::start(state.lock_epoch.clone());
    state.keyboard_monitor.events = crate::keyboard_monitor::start(&event_loop.handle());
    tracing::info!(gpu = %path.display(), %render_node, %seat, nvidia, "drawing on the GPU through KMS");

    let feedback = DmabufFeedbackBuilder::new(render_node.dev_id(), dmabuf_formats).build()?;
    state
        .dmabuf_state
        .create_global_with_default_feedback::<State>(&dh, &feedback);
    if supports_syncobj_eventfd(&fd) {
        state.syncobj_state = Some(DrmSyncobjState::new::<State>(&dh, fd));
        tracing::info!("explicit sync: linux-drm-syncobj-v1 on the GPU");
    } else {
        tracing::info!("explicit sync: none, the GPU cannot wait on a syncobj");
    }
    report_frame_times(event_loop, &mut state)?;

    let handle = event_loop.handle();
    handle
        .insert_source(drm_events, |event, metadata, state| match event {
            DrmEvent::VBlank(crtc) => state.vblank(crtc, metadata.as_ref()),
            DrmEvent::Error(error) => tracing::warn!(%error, "the GPU reported an error"),
        })
        .map_err(|e| e.error)?;
    handle
        .insert_source(LibinputInputBackend::new(libinput), |event, _, state| {
            state.track_touchpads(&event);
            state.process_input_event(event)
        })
        .map_err(|e| e.error)?;
    state.take_queued_input()?;
    handle
        .insert_source(notifier, |event, _, state| match event {
            SessionEvent::PauseSession => state.pause(),
            SessionEvent::ActivateSession => state.resume(),
        })
        .map_err(|e| e.error)?;
    handle
        .insert_source(udev, |event, _, state| match event {
            UdevEvent::Changed { device_id } => state.scan(device_id),
            UdevEvent::Added { device_id, path } => {
                tracing::info!(device_id, path = %path.display(), "a GPU was added; only the primary one is used")
            }
            UdevEvent::Removed { device_id } => state.gpu_removed(device_id),
        })
        .map_err(|e| e.error)?;

    let device_id = node.dev_id();
    state.scan(device_id);
    // A monitor plugged in later is set up then; one plugged in that cannot be set up would
    // leave a compositor that shows nothing.
    if let Backend::Drm(drm) = &state.backend {
        let plugged = drm
            .scanner
            .connectors()
            .values()
            .any(|info| info.state() == connector::State::Connected);
        if drm.outputs.is_empty() && plugged {
            return Err("no monitor plugged in could be set up".into());
        }
    }
    Ok(state)
}

impl Drm {
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> Result<(), Error> {
        gles(&mut self.gpus, &self.render_node)?.import_dmabuf(dmabuf, None)?;
        Ok(())
    }

    /// Draws what a capture source names off screen and reads it back (`capture::read_back`).
    pub fn read_back(
        &mut self,
        what: &Target,
        space: &smithay::desktop::Space<Window>,
    ) -> Result<Vec<u8>, Error> {
        crate::capture::read_back::<_, GlesTexture>(
            gles(&mut self.gpus, &self.render_node)?,
            what,
            space,
        )
    }
}

impl State {
    /// Ctrl+Alt+F`vt`: to that virtual terminal, which pauses the session. Nested or headless,
    /// there is none; while the session is paused, another terminal has the seat, and nothing
    /// here switches it.
    pub fn switch_vt(&mut self, vt: i32) {
        if let Backend::Drm(drm) = &mut self.backend {
            if !drm.session.is_active() {
                return;
            }
            if let Err(error) = drm.session.change_vt(vt) {
                tracing::warn!(%error, vt, "the virtual terminal could not be switched");
            }
        }
    }

    /// Keeps the touchpads, the devices that can tap, for the touchpad key (D16); one added
    /// while the key has turned them off starts off.
    fn track_touchpads(&mut self, event: &InputEvent<LibinputInputBackend>) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        match event {
            InputEvent::DeviceAdded { device } if device.config_tap_finger_count() > 0 => {
                if drm.touchpads_off {
                    send_events(device, false);
                }
                drm.touchpads.push(device.clone());
            }
            InputEvent::DeviceRemoved { device } => {
                drm.touchpads.retain(|touchpad| touchpad != device);
            }
            _ => {}
        }
    }

    /// The touchpad key (D16): every touchpad stops sending events, or starts again, for the
    /// session; cosmic-comp's setting of it is not read. Nested or headless, there is none.
    pub(crate) fn toggle_touchpads(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            tracing::debug!("no touchpads outside a session of the seat");
            return;
        };
        drm.touchpads_off = !drm.touchpads_off;
        tracing::info!(
            off = drm.touchpads_off,
            touchpads = drm.touchpads.len(),
            "the touchpad key"
        );
        for touchpad in &drm.touchpads {
            send_events(touchpad, !drm.touchpads_off);
        }
    }

    /// Whether the session is paused, another virtual terminal having the seat. Nested or
    /// headless, never.
    pub fn session_paused(&self) -> bool {
        matches!(&self.backend, Backend::Drm(drm) if !drm.session.is_active())
    }

    /// Something on screen may have changed: each output draws, at once or a refresh after its
    /// last frame, unless it draws already. While the session is paused nothing draws, and its
    /// resumption draws every output. Nested or headless, the backend draws on its own.
    // ponytail: every output, not only those the change shows on; per-output damage if the
    // extra empty draws of a second monitor show up in a profile.
    pub fn request_redraw(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        if !drm.session.is_active() {
            return;
        }
        let now = Instant::now();
        let due: Vec<(crtc::Handle, Duration, Wake)> = drm
            .outputs
            .iter_mut()
            .filter_map(|(crtc, head)| {
                head.wanted = true;
                if !head.powered {
                    // Off, it only sends its surfaces' frame callbacks (`draw`).
                    return head
                        .timer
                        .is_none()
                        .then_some((*crtc, FRAME_THROTTLE, Wake::Throttle));
                }
                next_draw(head, now).map(|delay| (*crtc, delay, Wake::Draw))
            })
            .collect();
        for (crtc, delay, wake) in due {
            self.wake(crtc, delay, wake);
        }
    }

    /// The session is paused, on a switch to another virtual terminal: the GPU and the input
    /// devices are given up until it resumes.
    pub(crate) fn pause(&mut self) {
        if let Backend::Drm(drm) = &mut self.backend {
            tracing::info!("the session is paused");
            drm.libinput.suspend();
            drm.heads.pause();
            // Before the record of kept keys goes: suspending releases the keys still held, and
            // the release of a key the record kept, such as the F1 of Ctrl+Alt+F1, is swallowed
            // instead of reaching a client that never saw it pressed.
            if let Err(error) = self.take_queued_input() {
                self.fail(format!("libinput could not be read: {error}"));
            }
        }
        // A key kept before the switch is released on the other terminal, if at all: the record
        // would swallow the next release of that key (`key_input`).
        self.kept_keys.clear();
        self.super_tap = None;
    }

    /// Hands `process_input_event` what libinput queued without its file descriptor turning
    /// readable: the devices it opened on taking the seat or on a resume, those it closed on a
    /// pause, and the initial state of their switches, such as a closed lid. The backend reads
    /// libinput only when that descriptor is readable, so these waited for the next key, a
    /// hot-plug or a resume: in the laptop's console hour the log showed no input device for an
    /// hour, though every device was open (C1 task 9, T9-2). Reading libinput also takes what
    /// the devices have sent meanwhile, which goes the same way.
    fn take_queued_input(&mut self) -> std::io::Result<()> {
        let Backend::Drm(drm) = &self.backend else {
            return Ok(());
        };
        let mut libinput = drm.libinput.clone();
        libinput.dispatch()?;
        for event in &mut libinput {
            match &event {
                LibinputEvent::Device(DeviceEvent::Added(added)) => {
                    tracing::info!(device = added.device().sysname(), "input device added");
                }
                LibinputEvent::Device(DeviceEvent::Removed(removed)) => {
                    tracing::info!(device = removed.device().sysname(), "input device removed");
                }
                _ => {}
            }
            match backend_event(event) {
                Some(event) => {
                    self.track_touchpads(&event);
                    self.process_input_event(event)
                }
                // As `process_input_event` does for every event: a touch, a gesture or a
                // tablet event keeps the session awake although the handler does not act on it.
                None => self.activity(),
            }
        }
        Ok(())
    }

    /// The session resumes: the input devices and the GPU are taken back, the monitors scanned
    /// again, since they may have changed meanwhile, and every output drawn.
    fn resume(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        tracing::info!("the session resumes");
        // A session with no input, or with a GPU it cannot use, is dead and shows nothing wrong:
        // the compositor ends with an error; under greetd the session ends and the greeter returns.
        if drm.libinput.resume().is_err() {
            self.fail("libinput could not take the input devices back".into());
            return;
        }
        // Not reset: the frames that follow find out whether another master left the
        // device in a state they cannot use, and reset it then (`draw`).
        let activated = drm
            .heads
            .lock()
            .activate(false)
            .map_err(|error| error.to_string());
        if let Err(error) = activated {
            self.fail(format!("the GPU could not be taken back: {error}"));
            return;
        }
        for head in drm.outputs.values_mut() {
            head.queued = false;
            head.stale_vblanks = 0;
            // The switch was activity: the idle daemon decides again from it (output power
            // plan, D8).
            if !head.powered {
                head.powered = true;
                head.drm_output.reset_buffers();
            }
        }
        self.kept_keys.clear();
        self.super_tap = None;
        // A failure may have been the switch's: the monitors that failed are tried again.
        drm.failed.clear();
        let device_id = drm.node.dev_id();
        self.power_restored();
        self.scan(device_id);
        let crtcs: Vec<crtc::Handle> = match &self.backend {
            Backend::Drm(drm) => drm.outputs.keys().copied().collect(),
            _ => Vec::new(),
        };
        for crtc in crtcs {
            self.schedule(crtc, Duration::ZERO);
        }
        if let Err(error) = self.take_queued_input() {
            self.fail(format!("libinput could not be read: {error}"));
        }
    }

    /// Ends the loop; `main` then exits with `reason` as the error.
    pub(crate) fn fail(&mut self, reason: String) {
        tracing::error!("{reason}");
        self.fatal.get_or_insert(reason);
        self.loop_signal.stop();
    }

    /// A GPU went: the compositor draws on one only, so when it is that one nothing can be shown
    /// and the compositor ends with an error. Any other GPU is not used.
    fn gpu_removed(&mut self, device_id: u64) {
        let Backend::Drm(drm) = &self.backend else {
            return;
        };
        if device_id == drm.node.dev_id() {
            self.fail("the GPU the compositor draws on was removed".into());
        } else {
            tracing::warn!(device_id, "a GPU was removed; only the primary one is used");
        }
    }

    /// The monitors of device `device_id`, when it is the GPU: those plugged in are added,
    /// those unplugged removed.
    fn scan(&mut self, device_id: u64) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        if device_id != drm.node.dev_id() {
            return;
        }
        let events = match drm.scanner.scan_connectors(drm.heads.device()) {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(%error, "the monitors could not be scanned");
                return;
            }
        };
        for event in events {
            match event {
                DrmScanEvent::Connected {
                    connector,
                    crtc: Some(crtc),
                } => self.connected(&connector, crtc),
                DrmScanEvent::Disconnected {
                    connector,
                    crtc: Some(crtc),
                } => self.disconnected(&connector, crtc),
                // The modes changed while it stayed plugged in: a monitor that could not be set
                // up with the modes it first reported is set up now, and one whose preferred
                // mode is another is set up again in it.
                DrmScanEvent::Changed {
                    connector,
                    crtc: Some(crtc),
                } => self.changed(&connector, crtc),
                DrmScanEvent::Connected { crtc: None, .. }
                | DrmScanEvent::Disconnected { crtc: None, .. }
                | DrmScanEvent::Changed { crtc: None, .. } => {}
            }
        }
        self.retry_dark_monitors();
    }

    /// The monitors the scanner holds plugged in that no output shows: one that had no CRTC to
    /// drive it when it was plugged in and has one since, or one that could not be set up and
    /// whose modes or CRTC are not what failed (`connected` skips those).
    fn retry_dark_monitors(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let plugged = |info: &connector::Info| info.state() == connector::State::Connected;
        let connectors = drm.scanner.connectors();
        drm.failed
            .retain(|handle, _| connectors.get(handle).is_some_and(plugged));
        let dark: Vec<(connector::Info, crtc::Handle)> = drm
            .scanner
            .crtcs()
            .filter(|(info, crtc)| plugged(info) && !drm.outputs.contains_key(crtc))
            .map(|(info, crtc)| (info.clone(), crtc))
            .collect();
        for info in drm.scanner.connectors().values() {
            if plugged(info) && drm.scanner.crtc_for_connector(&info.handle()).is_none() {
                tracing::debug!(
                    connector = ?info.handle(),
                    "a monitor is plugged in with no CRTC left to drive it"
                );
            }
        }
        for (info, crtc) in dark {
            self.connected(&info, crtc);
        }
    }

    /// The modes of the monitor on `connector`, driven by `crtc`, changed: set up when it has no
    /// output; else its output goes to the preferred mode when it shows another. The new mode is
    /// tried first, and when the GPU refuses it the output keeps the mode it has.
    fn changed(&mut self, connector: &connector::Info, crtc: crtc::Handle) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.get_mut(&crtc) else {
            return self.connected(connector, crtc);
        };
        let Some(wanted) = preferred_mode(connector) else {
            return;
        };
        let wl_mode = Mode::from(wanted);
        if head.output.current_mode() == Some(wl_mode) {
            return;
        }
        let used = match gles(&mut drm.gpus, &drm.render_node) {
            Ok(renderer) => head
                .drm_output
                .use_mode::<_, Element>(wanted, renderer, &DrmOutputRenderElements::default())
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = used {
            tracing::warn!(%error, output = head.output.name(), "the new mode was refused: the output keeps its own");
            return;
        }
        // A refused test makes Smithay commit a frame with the new mode, which lights the CRTC
        // of an output the power client turned off.
        if !head.powered {
            if let Err(error) = head.drm_output.with_compositor(|compositor| compositor.clear()) {
                tracing::warn!(%error, output = head.output.name(), "the output could not be kept off");
            }
        }
        let output = head.output.clone();
        tracing::info!(
            output = output.name(),
            size = ?wl_mode.size,
            refresh_mhz = wl_mode.refresh,
            "a monitor changed mode"
        );
        output.change_current_state(Some(wl_mode), None, None, None);
        output.set_preferred(wl_mode);
        layer_map_for_output(&output).arrange();
        self.arrange_outputs();
        self.request_redraw();
    }

    /// The outputs side by side again, in the order they were, after one changed size.
    pub(crate) fn arrange_outputs(&mut self) {
        let mut outputs: Vec<(i32, Output)> = self
            .space
            .outputs()
            .map(|output| {
                let x = self
                    .space
                    .output_geometry(output)
                    .map_or(0, |area| area.loc.x);
                (x, output.clone())
            })
            .collect();
        outputs.sort_by_key(|(x, _)| *x);
        let mut x = 0;
        for (_, output) in outputs {
            self.space.map_output(&output, (x, 0));
            x += self
                .space
                .output_geometry(&output)
                .map_or(0, |area| area.size.w);
        }
        self.fit_to_outputs();
    }

    /// A monitor plugged in on `connector`, driven by `crtc`: an output in its preferred mode,
    /// to the right of the others. One that could not be set up is remembered with the modes and
    /// the CRTC it failed with, and is tried again only when either changes.
    fn connected(&mut self, connector: &connector::Info, crtc: crtc::Handle) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        if drm.outputs.contains_key(&crtc) {
            return;
        }
        let failed_with = (connector.clone(), crtc);
        if drm.failed.get(&connector.handle()) == Some(&failed_with) {
            tracing::debug!(connector = ?connector.handle(), "a monitor that failed is not tried again as it was");
            return;
        }
        let name = format!(
            "{}-{}",
            connector.interface().as_str(),
            connector.interface_id()
        );
        match self.set_up(connector, crtc, &name) {
            Ok(()) => {
                if let Backend::Drm(drm) = &mut self.backend {
                    drm.failed.remove(&connector.handle());
                }
            }
            Err(error) => {
                // Tried again when its modes or its CRTC change (`retry_dark_monitors`); at start,
                // the compositor ends if no monitor could be set up (`start`).
                tracing::error!(%error, output = name, "a monitor could not be set up");
                if let Backend::Drm(drm) = &mut self.backend {
                    drm.failed.insert(connector.handle(), failed_with);
                }
            }
        }
    }

    /// The output of `connector` on `crtc`, in the preferred mode. Its global is created only
    /// once the GPU has accepted the mode, so a monitor that cannot be set up is never seen by a
    /// client.
    fn set_up(
        &mut self,
        connector: &connector::Info,
        crtc: crtc::Handle,
        name: &str,
    ) -> Result<(), String> {
        let Backend::Drm(drm) = &mut self.backend else {
            return Ok(());
        };
        let mode = preferred_mode(connector).ok_or("the monitor has no mode")?;
        let wl_mode = Mode::from(mode);
        let output = crate::state::build_output(
            name,
            "DRM",
            wl_mode.size,
            wl_mode.refresh,
            Transform::Normal,
            1.0,
        );
        let mut planes = drm
            .heads
            .device()
            .planes(&crtc)
            .map_err(|error| format!("the planes of the CRTC could not be read: {error}"))?;
        if drm.nvidia {
            planes.overlay.clear();
        }
        let renderer = gles(&mut drm.gpus, &drm.render_node).map_err(|error| error.to_string())?;
        let drm_output = drm
            .heads
            .lock()
            .initialize_output::<_, Element>(
                crtc,
                mode,
                &[connector.handle()],
                &output,
                Some(planes),
                renderer,
                &DrmOutputRenderElements::default(),
            )
            .map_err(|error| error.to_string())?;
        crate::state::publish_output(&self.display_handle, &output);
        tracing::info!(
            output = name,
            size = ?wl_mode.size,
            refresh_mhz = wl_mode.refresh,
            "a monitor was plugged in"
        );
        drm.outputs.insert(
            crtc,
            Head {
                output: output.clone(),
                drm_output,
                queued: false,
                stale_vblanks: 0,
                lock: LockFrames::default(),
                failing: FailureLog::default(),
                timer: None,
                wanted: true,
                not_before: None,
                powered: true,
            },
        );
        self.map_output(&output);
        self.schedule(crtc, Duration::ZERO);
        Ok(())
    }

    /// The monitor driven by `crtc` unplugged: its output goes, with its global and its layer
    /// surfaces, and the windows only it showed move onto the first output left.
    fn disconnected(&mut self, connector: &connector::Info, crtc: crtc::Handle) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        // Bandwidth or a CRTC may be free now: the monitors that failed are tried again.
        drm.failed.clear();
        let Some(head) = drm.outputs.remove(&crtc) else {
            return;
        };
        if let Some((timer, _)) = head.timer {
            self.loop_handle.remove(timer);
        }
        let output = head.output.clone();
        drop(head);
        if let Ok(renderer) = gles(&mut drm.gpus, &drm.render_node) {
            // The other outputs may now use the modifiers the gone one's bandwidth ruled out.
            if let Err(error) = drm.heads.lock().try_to_restore_modifiers::<_, Element>(
                renderer,
                &DrmOutputRenderElements::default(),
            ) {
                tracing::warn!(%error, "the other outputs keep their modifiers");
            }
        }
        tracing::info!(
            output = output.name(),
            connector = ?connector.handle(),
            "a monitor was unplugged"
        );
        self.remove_output(&output);
        remove_global(&self.display_handle, &output);
        // The monitor that was the only one not yet showing the lock, or the last one, is gone.
        self.confirm_lock();
        self.request_redraw();
    }

    /// Draws the output of `crtc` after `delay`, in place of a draw scheduled before.
    fn schedule(&mut self, crtc: crtc::Handle, delay: Duration) {
        self.wake(crtc, delay, Wake::Draw);
    }

    /// Schedules the next draw of `crtc` after `delay`, in place of the one scheduled.
    fn wake(&mut self, crtc: crtc::Handle, delay: Duration, wake: Wake) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.get_mut(&crtc) else {
            return;
        };
        if let Some((timer, _)) = head.timer.take() {
            self.loop_handle.remove(timer);
        }
        let timer = Timer::from_duration(delay);
        match self.loop_handle.insert_source(timer, move |_, _, state| {
            if let Backend::Drm(drm) = &mut state.backend {
                if let Some(head) = drm.outputs.get_mut(&crtc) {
                    head.timer = None;
                }
            }
            state.draw(crtc);
            TimeoutAction::Drop
        }) {
            Ok(timer) => head.timer = Some((timer, wake)),
            Err(error) => {
                tracing::warn!(error = %error.error, "the next frame could not be scheduled")
            }
        }
    }

    /// The frame queued on `crtc` is on screen: its presentation feedback is sent and, when
    /// something changed meanwhile, the next frame is drawn after six tenths of a period, which
    /// leaves the clients most of the period to draw theirs, as anvil does.
    fn vblank(&mut self, crtc: crtc::Handle, metadata: Option<&DrmEventMetadata>) {
        let now = self.clock.now();
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.get_mut(&crtc) else {
            return;
        };
        if head.stale_vblanks > 0 {
            head.stale_vblanks -= 1;
            // The frame `clear` dropped: the frame queued since the output came back on, if any,
            // is not on screen yet, and a lock must not count it as shown.
            return;
        }
        head.queued = false;
        let Some(mode) = head.output.current_mode() else {
            return;
        };
        let period = Duration::from_secs_f64(1_000.0 / f64::from(mode.refresh.max(1)));
        let hardware = metadata.and_then(|metadata| match metadata.time {
            DrmEventTime::Monotonic(time) if !time.is_zero() => Some((time, metadata.sequence)),
            _ => None,
        });
        let (presented, sequence, flags) = match hardware {
            Some((time, sequence)) => (
                time.into(),
                sequence,
                wp_presentation_feedback::Kind::Vsync
                    | wp_presentation_feedback::Kind::HwClock
                    | wp_presentation_feedback::Kind::HwCompletion,
            ),
            None => (now, 0, wp_presentation_feedback::Kind::Vsync),
        };
        match head.drm_output.frame_submitted() {
            Ok(feedback) => {
                if let Some(mut feedback) = feedback {
                    feedback.presented(
                        presented,
                        Refresh::fixed(period),
                        u64::from(sequence),
                        flags,
                    );
                }
                head.lock.presented();
            }
            Err(error) => {
                // The whole frame is drawn and queued again, so that a lock frame reaches the
                // screen.
                tracing::warn!(%error, "a frame was not presented");
                head.drm_output.reset_buffers();
                head.wanted = true;
            }
        }
        head.not_before = Some(Instant::now() + period.mul_f64(0.6));
        let next = next_draw(head, Instant::now());
        let idle = next.is_none() && head.timer.is_none();
        self.confirm_lock();
        if let Some(delay) = next {
            self.schedule(crtc, delay);
        } else if idle && self.frame_callbacks_waiting() {
            // Only a draw sends a frame callback, and a request may have taken the throttle's
            // place: with no draw to follow, a surface still waiting is drawn for at the throttle.
            self.wake(crtc, FRAME_THROTTLE, Wake::Throttle);
        }
        // `locked` and the presentation feedback are sent here, and nothing else flushes an idle
        // seat.
        self.flush();
    }

    fn flush(&mut self) {
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
    }

    /// A lock was taken. A frame of before, queued or on screen, drew another lock or none, so
    /// no output shows this one yet; with no output at all, there is nothing to wait for.
    pub fn lock_taken(&mut self) {
        if let Backend::Drm(drm) = &mut self.backend {
            for head in drm.outputs.values_mut() {
                head.lock = LockFrames::default();
            }
            self.confirm_lock();
        }
    }

    /// Turns `output` on or off at the power client's request, and says whether it is on
    /// afterwards (output power plan, D5 and D6). Off, its CRTC is disabled and nothing is
    /// drawn; on, its next frame is drawn whole, and queuing it enables the CRTC again.
    /// Nested or headless, the mode is only recorded.
    pub fn power_output(&mut self, output: &Output, on: bool) -> bool {
        let Backend::Drm(drm) = &mut self.backend else {
            return on;
        };
        let Some((&crtc, head)) = drm
            .outputs
            .iter_mut()
            .find(|(_, head)| &head.output == output)
        else {
            return on;
        };
        if on {
            head.powered = true;
            head.drm_output.reset_buffers();
            head.wanted = true;
            self.schedule(crtc, Duration::ZERO);
            return true;
        }
        if let Err(error) = head.drm_output.with_compositor(|compositor| compositor.clear()) {
            tracing::warn!(%error, output = output.name(), "the output could not be turned off");
            return true;
        }
        head.powered = false;
        if std::mem::take(&mut head.queued) {
            head.stale_vblanks = head.stale_vblanks.saturating_add(1);
        }
        if let Some((timer, _)) = head.timer.take() {
            self.loop_handle.remove(timer);
        }
        tracing::info!(output = output.name(), "the output is off");
        // The lock may have waited only for this output.
        self.confirm_lock();
        if self.frame_callbacks_waiting() {
            self.wake(crtc, FRAME_THROTTLE, Wake::Throttle);
        }
        false
    }

    /// A lock is confirmed only once every output presented a frame drawn locked.
    fn confirm_lock(&mut self) {
        if let Backend::Drm(drm) = &self.backend {
            if lock_shown(drm.outputs.values().map(|head| (head.powered, &head.lock))) {
                self.locked_drawn();
            }
        }
    }

    /// Draws the output of `crtc` and queues the frame, unless one is queued already. With
    /// nothing new to show, nothing is queued and the output waits for the next request; while a
    /// surface still waits for a frame callback another frame's throttle sends, it draws again
    /// after that throttle.
    fn draw(&mut self, crtc: crtc::Handle) {
        let status = self.shown_cursor();
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.get_mut(&crtc) else {
            return;
        };
        if head.queued {
            return;
        }
        head.wanted = false;
        let output = head.output.clone();
        if !head.powered {
            // No element is on it, so its surfaces lose it as their primary output and get
            // their frame callbacks at the throttle, as a surface shown on no output does.
            let now = self.clock.now();
            self.frame_done(
                &output,
                &RenderElementStates::default(),
                false,
                now,
                FRAME_THROTTLE,
            );
            if self.frame_callbacks_waiting() {
                self.wake(crtc, FRAME_THROTTLE, Wake::Throttle);
            }
            self.flush();
            return;
        }
        let Some(mode) = output.current_mode() else {
            return;
        };
        let period = Duration::from_secs_f64(1_000.0 / f64::from(mode.refresh.max(1)));
        let frame_target = self.clock.now() + period;
        let started = Instant::now();
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| (pointer.current_location(), status));
        let popups = self.composing.popups_on(&self.space, &output);
        let drawn = match gles(&mut drm.gpus, &drm.render_node) {
            Ok(renderer) => render(
                renderer,
                head,
                &self.space,
                self.lock.as_ref(),
                &popups,
                pointer.as_ref(),
                &mut drm.cursor,
            ),
            Err(error) => {
                tracing::error!(%error, "no renderer: the output draws again in a second");
                self.schedule(crtc, Duration::from_secs(1));
                return;
            }
        };
        let (rendered, states) = match drawn {
            Ok(drawn) => drawn,
            Err(error) => {
                self.draw_failed(crtc, error, period);
                return;
            }
        };
        if let Some(stats) = &mut self.frame_stats {
            stats.drew(started.elapsed());
        }
        self.frame_done(&output, &states, false, frame_target, period);
        if rendered {
            let feedback = self.presentation_feedback(&output, &states);
            let locked = self.lock.is_some();
            if let Backend::Drm(drm) = &mut self.backend {
                if let Some(head) = drm.outputs.get_mut(&crtc) {
                    match head.drm_output.queue_frame(feedback) {
                        Ok(()) => {
                            head.queued = true;
                            head.lock.queued(locked);
                            head.failing.recovered(&head.output);
                        }
                        Err(error) => {
                            self.draw_failed(crtc, SwapBuffersError::from(error), period);
                            return;
                        }
                    }
                }
            }
        } else {
            let locked = self.lock.is_some();
            let waiting = self.frame_callbacks_waiting();
            let mut redraw = false;
            if let Backend::Drm(drm) = &mut self.backend {
                if let Some(head) = drm.outputs.get_mut(&crtc) {
                    head.not_before = Some(Instant::now() + period);
                    head.failing.recovered(&head.output);
                    // Nothing new does not confirm a lock: only a frame drawn locked that
                    // reached the screen does, so the whole frame is drawn again.
                    if locked && !head.lock.shown {
                        head.drm_output.reset_buffers();
                        redraw = true;
                    }
                }
            }
            if redraw {
                self.schedule(crtc, period);
            } else if waiting {
                self.wake(crtc, FRAME_THROTTLE, Wake::Throttle);
            }
        }
        self.serve_captures();
        self.space.refresh();
        self.send_toplevel_states();
        self.popups.cleanup();
        self.flush();
    }

    /// A frame that could not be drawn or queued. While the session is paused nothing is
    /// drawn until it resumes; a frame KMS refused after another master changed the device
    /// resets it; anything else is tried again a period later.
    fn draw_failed(&mut self, crtc: crtc::Handle, error: SwapBuffersError, period: Duration) {
        // The next frame is drawn whole, so that a lock frame lost here reaches the screen.
        if let Backend::Drm(drm) = &self.backend {
            if let Some(head) = drm.outputs.get(&crtc) {
                head.drm_output.reset_buffers();
            }
        }
        fn inactive(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
            match error.downcast_ref::<DrmError>() {
                Some(DrmError::DeviceInactive) => true,
                Some(DrmError::Access(DrmAccessError { source, .. })) => {
                    source.kind() == std::io::ErrorKind::PermissionDenied
                }
                _ => false,
            }
        }
        match &error {
            SwapBuffersError::TemporaryFailure(cause) if inactive(cause.as_ref()) => {
                tracing::debug!(%error, "no frame while the session is paused");
                return;
            }
            SwapBuffersError::ContextLost(cause)
                if matches!(
                    cause.downcast_ref::<DrmError>(),
                    Some(DrmError::TestFailed(_))
                ) =>
            {
                if let Backend::Drm(drm) = &mut self.backend {
                    if let Err(error) = drm.heads.device_mut().reset_state() {
                        tracing::warn!(%error, "the GPU's state could not be reset");
                    }
                }
            }
            _ => {}
        }
        // A failure that persists is retried every period: logged at the first, not at each.
        let first = match &mut self.backend {
            Backend::Drm(drm) => drm
                .outputs
                .get_mut(&crtc)
                .is_none_or(|head| head.failing.failed()),
            _ => true,
        };
        if first {
            tracing::warn!(%error, "a frame could not be drawn; tried again each refresh");
        } else {
            tracing::debug!(%error, "a frame could not be drawn");
        }
        self.schedule(crtc, period);
    }
}

/// Whether the draws of a head keep failing, for the log: the first failure is logged, the
/// repeats are not, and the recovery is.
#[derive(Clone, Copy, Debug, Default)]
struct FailureLog {
    failing: bool,
}

impl FailureLog {
    /// A draw failed; whether it is the first of a run, which is the one to log.
    fn failed(&mut self) -> bool {
        !std::mem::replace(&mut self.failing, true)
    }

    /// A draw worked; whether a run of failures ends, which is logged.
    fn worked(&mut self) -> bool {
        std::mem::replace(&mut self.failing, false)
    }

    fn recovered(&mut self, output: &Output) {
        if self.worked() {
            tracing::info!(output = output.name(), "frames are drawn again");
        }
    }
}

/// The mode a monitor is set up in: the one it prefers, else its first.
fn preferred_mode(connector: &connector::Info) -> Option<smithay::reexports::drm::control::Mode> {
    let modes = connector.modes();
    modes
        .iter()
        .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| modes.first())
        .copied()
}

/// What the frames of a head showed of the lock. Only a frame on screen counts: a frame queued
/// and then not presented, or a draw with nothing new to show, changes nothing.
#[derive(Clone, Copy, Debug, Default)]
struct LockFrames {
    /// The queued frame was drawn locked.
    queued: bool,
    /// The frame on screen was drawn locked.
    shown: bool,
}

impl LockFrames {
    /// A frame was queued, drawn `locked` or not.
    fn queued(&mut self, locked: bool) {
        self.queued = locked;
        // An unlocked frame on its way: the screen may show the space before the lock is
        // asked for again, so what it showed before no longer confirms a lock.
        if !locked {
            self.shown = false;
        }
    }

    /// The queued frame reached the screen.
    fn presented(&mut self) {
        self.shown = self.queued;
    }
}

/// Whether every head of `heads`, each with whether it is on, shows the lock: a head that is
/// off shows nothing, so it hides the space as well as the lock does (output power plan, D7).
fn lock_shown<'a>(heads: impl IntoIterator<Item = (bool, &'a LockFrames)>) -> bool {
    heads
        .into_iter()
        .all(|(powered, frames)| !powered || frames.shown)
}

/// The delay of the next draw of `head`, when it must be scheduled at `now`: when something
/// may have changed and neither a queued frame (whose VBlank draws the next) nor a scheduled
/// draw will draw it. A wake-up at the throttle does not count: it would hold the request back
/// for up to a second.
fn next_draw(head: &Head, now: Instant) -> Option<Duration> {
    pacing(
        head.wanted,
        head.queued,
        head.timer.map(|(_, wake)| wake),
        head.not_before,
        now,
    )
}

/// `next_draw`, of what it reads of the head: `wanted`, a frame queued, what the scheduled
/// draw is for, and the earliest next draw.
fn pacing(
    wanted: bool,
    queued: bool,
    scheduled: Option<Wake>,
    not_before: Option<Instant>,
    now: Instant,
) -> Option<Duration> {
    let busy = queued || scheduled == Some(Wake::Draw);
    (wanted && !busy).then(|| {
        not_before.map_or(Duration::ZERO, |not_before| {
            not_before.saturating_duration_since(now)
        })
    })
}

/// Draws the frame of `head`: the space, or the lock surface while `lock` holds the session,
/// with the pointer on top, the cursor its status names, when it is over the output. Whether there was anything new to
/// show, and the elements' states.
fn render(
    renderer: &mut GlesRenderer,
    head: &mut Head,
    space: &smithay::desktop::Space<Window>,
    lock: Option<&crate::lock::Lock>,
    popups: &[crate::input_method::PlacedPopup],
    pointer: Option<&(
        smithay::utils::Point<f64, smithay::utils::Logical>,
        smithay::input::pointer::CursorImageStatus,
    )>,
    cursor: &mut Cursor,
) -> Result<(bool, RenderElementStates), SwapBuffersError> {
    let output = &head.output;
    let scale = output.current_scale().fractional_scale();
    let mut elements: Vec<Element> = Vec::new();
    if let (Some((location, status)), Some(area)) = (pointer, space.output_geometry(output)) {
        if area.to_f64().contains(*location) {
            let at = *location - area.loc.to_f64();
            elements.extend(
                pointer_elements(renderer, cursor, status, at, scale)
                    .into_iter()
                    .map(Element::Pointer),
            );
        }
    }
    match lock {
        Some(lock) => {
            if let Some(surface) = lock.surface_on(output) {
                elements.extend(
                    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                        renderer,
                        surface,
                        (0, 0),
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(Element::Surface),
                );
            }
        }
        None => {
            for (surface, at) in popups {
                elements.extend(
                    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                        renderer,
                        surface,
                        at.to_physical_precise_round(scale),
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(Element::Surface),
                );
            }
            let space_elements = space_render_elements(renderer, [space], output, 1.0)
                .map_err(|error| SwapBuffersError::TemporaryFailure(Box::new(error)))?;
            elements.extend(space_elements.into_iter().map(Element::Space));
        }
    }
    let result = head
        .drm_output
        .render_frame(renderer, &elements, BACKGROUND, FrameFlags::DEFAULT)
        .map_err(|error| match error {
            RenderFrameError::PrepareFrame(error) => SwapBuffersError::from(error),
            RenderFrameError::RenderFrame(DamageError::Rendering(error)) => {
                SwapBuffersError::from(error)
            }
            RenderFrameError::RenderFrame(DamageError::OutputNoMode(error)) => {
                SwapBuffersError::TemporaryFailure(Box::new(error))
            }
        })?;
    // KMS waits for the frame through the plane's IN_FENCE_FD when the sync point can be
    // exported; when it cannot, the frame is waited for here, before its frame callbacks invite
    // the clients to reuse their buffers.
    if result.needs_sync() {
        if let PrimaryPlaneElement::Swapchain(element) = &result.primary_element {
            element
                .sync
                .wait()
                .map_err(|error| SwapBuffersError::TemporaryFailure(Box::new(error)))?;
        }
    }
    Ok((!result.is_empty, result.states))
}

/// Withdraws the global of `output`, which clients bound to it see go.
fn remove_global(dh: &smithay::reexports::wayland_server::DisplayHandle, output: &Output) {
    output.leave_all();
    if let Some(OutputGlobal(global)) = output.user_data().get::<OutputGlobal>() {
        dh.remove_global::<State>(global.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{lock_shown, pacing, FailureLog, LockFrames, Wake};
    use std::time::{Duration, Instant};

    #[test]
    fn an_output_draws_only_when_asked_and_at_most_once_a_refresh() {
        let now = Instant::now();
        let period = Duration::from_millis(16);
        assert_eq!(pacing(false, false, None, None, now), None, "idle: no draw");
        assert_eq!(
            pacing(true, false, None, None, now),
            Some(Duration::ZERO),
            "asked: at once"
        );
        assert_eq!(
            pacing(true, true, None, None, now),
            None,
            "a queued frame draws it"
        );
        assert_eq!(
            pacing(true, false, Some(Wake::Draw), None, now),
            None,
            "a scheduled draw draws it"
        );
        assert_eq!(
            pacing(true, false, Some(Wake::Throttle), None, now),
            Some(Duration::ZERO),
            "a wake-up at the throttle does not hold a request back"
        );
        assert_eq!(
            pacing(false, false, Some(Wake::Throttle), None, now),
            None,
            "nor draws unasked"
        );
        assert_eq!(
            pacing(true, false, None, Some(now + period), now),
            Some(period),
            "not before a refresh after the last draw"
        );
        assert_eq!(
            pacing(true, false, None, Some(now), now + period),
            Some(Duration::ZERO),
            "at once when that refresh is over"
        );
    }

    /// The lock client is told `locked` only once every output presented a frame drawn locked:
    /// a frame whose submission failed, or a draw with nothing new, does not count.
    #[test]
    fn the_lock_is_confirmed_only_by_locked_frames_on_every_screen() {
        let (mut first, mut second) = (LockFrames::default(), LockFrames::default());
        assert!(!lock_shown([(true, &first), (true, &second)]), "unlocked frames on screen");
        first.queued(true);
        second.queued(true);
        second.presented();
        // The first output's frame failed to present, then a draw had nothing new: neither
        // reaches it.
        assert!(
            !lock_shown([(true, &first), (true, &second)]),
            "the first screen is not locked yet"
        );
        first.queued(true);
        first.presented();
        assert!(lock_shown([(true, &first), (true, &second)]), "both screens show the lock");
        second.queued(false);
        assert!(
            !lock_shown([(true, &first), (true, &second)]),
            "an unlocked frame on its way: a lock asked for now is not shown by the old frame"
        );
        second.queued(true);
        assert!(!lock_shown([(true, &first), (true, &second)]), "a locked frame only queued");
        second.presented();
        assert!(lock_shown([(true, &first), (true, &second)]), "locked again on both");
        // A lock taken forgets what an earlier one showed.
        first = LockFrames::default();
        assert!(!lock_shown([(true, &first), (true, &second)]));
    }

    /// An output that is off shows the lock: cosmic-idle locks after it blanks the outputs, and
    /// waits for `locked`.
    #[test]
    fn an_output_that_is_off_shows_the_lock() {
        let (off, mut on) = (LockFrames::default(), LockFrames::default());
        assert!(lock_shown([(false, &off), (false, &on)]), "both off");
        assert!(
            !lock_shown([(false, &off), (true, &on)]),
            "the other shows the space"
        );
        on.queued(true);
        on.presented();
        assert!(
            lock_shown([(false, &off), (true, &on)]),
            "the other shows the lock"
        );
    }

    /// With no output there is nothing to wait for: a lock is confirmed at once.
    #[test]
    fn a_lock_with_no_output_is_shown() {
        assert!(lock_shown([]));
    }

    #[test]
    fn a_failure_that_persists_is_logged_once() {
        let mut log = FailureLog::default();
        assert!(!log.worked(), "nothing to recover from");
        assert!(log.failed(), "the first failure is logged");
        assert!(!log.failed(), "the repeats are not");
        assert!(!log.failed());
        assert!(log.worked(), "the recovery is logged");
        assert!(!log.worked());
        assert!(log.failed(), "a new run logs again");
    }
}
