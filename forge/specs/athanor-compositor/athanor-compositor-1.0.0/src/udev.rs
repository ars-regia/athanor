//! The DRM backend (doc_own_stack.md, C1 task 8), after Smithay's anvil: the compositor runs in
//! a session of the seat through libseat, draws each monitor of the seat's primary GPU through
//! KMS, and reads input through libinput. A monitor plugged in or out adds or removes its
//! output; a switch to another virtual terminal pauses the session, and the switch back resumes
//! it and draws every output again. An output draws only when something may have changed (a
//! client's request or commit, input, an output or the lock), at most once a refresh, and not
//! at all while idle.

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
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::damage::Error as DamageError;
use smithay::backend::renderer::element::solid::{SolidColorBuffer, SolidColorRenderElement};
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
use smithay::desktop::space::{space_render_elements, SpaceRenderElements};
use smithay::desktop::utils::OutputPresentationFeedback;
use smithay::desktop::Window;
use smithay::output::{Mode, Output};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, RegistrationToken};
use smithay::reexports::drm::control::{connector, crtc, ModeTypeFlags};
use smithay::reexports::drm::Device as _;
use smithay::reexports::input::Libinput;
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

/// The pointer, a square of this side in logical pixels.
// ponytail: a solid square, not the cursor surface a client sets or a themed cursor; the
// console session (C1 task 9) draws those.
const POINTER_SIDE: i32 = 12;

type Gpus = GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>;
type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Heads = DrmOutputManager<Allocator, Exporter, OutputPresentationFeedback, DrmDeviceFd>;
type HeadOutput = DrmOutput<Allocator, Exporter, OutputPresentationFeedback, DrmDeviceFd>;

smithay::backend::renderer::element::render_elements! {
    Element<=GlesRenderer>;
    Space=SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Pointer=SolidColorRenderElement,
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
    /// The overlay planes are left alone on NVIDIA's driver, as anvil does.
    nvidia: bool,
    pointer: SolidColorBuffer,
}

/// A monitor: the output and what draws it.
struct Head {
    output: Output,
    drm_output: HeadOutput,
    /// A frame is queued and waits for its VBlank.
    queued: bool,
    /// Whether its frames showed the lock.
    lock: LockFrames,
    /// The next draw, when one is scheduled.
    timer: Option<RegistrationToken>,
    /// Something may have changed since the last draw (`request_redraw`).
    wanted: bool,
    /// The earliest next draw: six tenths of a period after a VBlank, a period after a draw
    /// that had nothing new to show.
    not_before: Option<Instant>,
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
        nvidia,
        pointer: SolidColorBuffer::new((POINTER_SIDE, POINTER_SIDE), [0.9, 0.9, 0.9, 1.0]),
    };
    let dh = display.handle();
    let mut state = State::new(event_loop, display, Backend::Drm(Box::new(drm)))?;
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
            state.process_input_event(event)
        })
        .map_err(|e| e.error)?;
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
            UdevEvent::Removed { device_id } => {
                tracing::warn!(device_id, "a GPU was removed")
            }
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

    /// Draws `window` off screen at scale 1 and reads it back (`capture::read_window`).
    pub fn read_window(&mut self, window: &Window) -> Result<Vec<u8>, Error> {
        crate::capture::read_window::<_, GlesTexture>(
            gles(&mut self.gpus, &self.render_node)?,
            window,
        )
    }
}

impl State {
    /// Ctrl+Alt+F`vt`: to that virtual terminal, which pauses the session. Nested or headless,
    /// there is none.
    pub fn switch_vt(&mut self, vt: i32) {
        if let Backend::Drm(drm) = &mut self.backend {
            if let Err(error) = drm.session.change_vt(vt) {
                tracing::warn!(%error, vt, "the virtual terminal could not be switched");
            }
        }
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
        let due: Vec<(crtc::Handle, Duration)> = drm
            .outputs
            .iter_mut()
            .filter_map(|(crtc, head)| {
                head.wanted = true;
                next_draw(head, now).map(|delay| (*crtc, delay))
            })
            .collect();
        for (crtc, delay) in due {
            self.schedule(crtc, delay);
        }
    }

    /// The session is paused, on a switch to another virtual terminal: the GPU and the input
    /// devices are given up until it resumes.
    fn pause(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        tracing::info!("the session is paused");
        drm.libinput.suspend();
        drm.heads.pause();
    }

    /// The session resumes: the input devices and the GPU are taken back, the monitors scanned
    /// again, since they may have changed meanwhile, and every output drawn.
    fn resume(&mut self) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        tracing::info!("the session resumes");
        if drm.libinput.resume().is_err() {
            tracing::warn!("libinput could not take the input devices back");
        }
        // Not reset: the frames that follow find out whether another master left the
        // device in a state they cannot use, and reset it then (`draw`).
        if let Err(error) = drm.heads.lock().activate(false) {
            tracing::warn!(%error, "the GPU could not be taken back");
        }
        for head in drm.outputs.values_mut() {
            head.queued = false;
        }
        let device_id = drm.node.dev_id();
        self.scan(device_id);
        let crtcs: Vec<crtc::Handle> = match &self.backend {
            Backend::Drm(drm) => drm.outputs.keys().copied().collect(),
            _ => Vec::new(),
        };
        for crtc in crtcs {
            self.schedule(crtc, Duration::ZERO);
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
                DrmScanEvent::Connected {
                    connector,
                    crtc: None,
                } => tracing::warn!(
                    connector = ?connector.handle(),
                    "a monitor was plugged in with no CRTC left to drive it"
                ),
                // ponytail: a monitor whose modes change keeps the mode it has; anvil does the same.
                DrmScanEvent::Disconnected { .. } | DrmScanEvent::Changed { .. } => {}
            }
        }
    }

    /// A monitor plugged in on `connector`, driven by `crtc`: an output in its preferred mode,
    /// to the right of the others.
    fn connected(&mut self, connector: &connector::Info, crtc: crtc::Handle) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let name = format!(
            "{}-{}",
            connector.interface().as_str(),
            connector.interface_id()
        );
        let modes = connector.modes();
        let Some(mode) = modes
            .iter()
            .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| modes.first())
            .copied()
        else {
            tracing::warn!(output = name, "a monitor has no mode");
            return;
        };
        let wl_mode = Mode::from(mode);
        let output = crate::state::new_output(
            &self.display_handle,
            &name,
            "DRM",
            wl_mode.size,
            wl_mode.refresh,
            Transform::Normal,
            1.0,
        );
        let mut planes = match drm.heads.device().planes(&crtc) {
            Ok(planes) => planes,
            Err(error) => {
                tracing::warn!(%error, output = name, "the planes of a CRTC could not be read");
                remove_global(&self.display_handle, &output);
                return;
            }
        };
        if drm.nvidia {
            planes.overlay.clear();
        }
        let initialized = match gles(&mut drm.gpus, &drm.render_node) {
            Ok(renderer) => drm
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
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        let drm_output = match initialized {
            Ok(drm_output) => drm_output,
            Err(error) => {
                // Tried again when it is plugged in again; at start, the compositor ends if no
                // monitor could be set up (`start`).
                tracing::error!(%error, output = name, "a monitor could not be set up");
                remove_global(&self.display_handle, &output);
                return;
            }
        };
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
                lock: LockFrames::default(),
                timer: None,
                wanted: true,
                not_before: None,
            },
        );
        self.map_output(&output);
        self.schedule(crtc, Duration::ZERO);
    }

    /// The monitor driven by `crtc` unplugged: its output goes, with its global and its layer
    /// surfaces, and the windows only it showed move onto the first output left.
    fn disconnected(&mut self, connector: &connector::Info, crtc: crtc::Handle) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.remove(&crtc) else {
            return;
        };
        if let Some(timer) = head.timer {
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
        self.request_redraw();
    }

    /// Draws the output of `crtc` after `delay`, in place of a draw scheduled before.
    fn schedule(&mut self, crtc: crtc::Handle, delay: Duration) {
        let Backend::Drm(drm) = &mut self.backend else {
            return;
        };
        let Some(head) = drm.outputs.get_mut(&crtc) else {
            return;
        };
        if let Some(timer) = head.timer.take() {
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
            Ok(timer) => head.timer = Some(timer),
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
        self.confirm_lock();
        if let Some(delay) = next {
            self.schedule(crtc, delay);
        }
    }

    /// A lock is confirmed only once every output presented a frame drawn locked.
    fn confirm_lock(&mut self) {
        if let Backend::Drm(drm) = &self.backend {
            if lock_shown(drm.outputs.values().map(|head| &head.lock)) {
                self.locked_drawn();
            }
        }
    }

    /// Draws the output of `crtc` and queues the frame, unless one is queued already. With
    /// nothing new to show, nothing is queued and the output waits for the next request; while a
    /// surface still waits for a frame callback another frame's throttle sends, it draws again
    /// after that throttle.
    fn draw(&mut self, crtc: crtc::Handle) {
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
        let Some(mode) = output.current_mode() else {
            return;
        };
        let period = Duration::from_secs_f64(1_000.0 / f64::from(mode.refresh.max(1)));
        let frame_target = self.clock.now() + period;
        let started = Instant::now();
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location());
        let drawn = match gles(&mut drm.gpus, &drm.render_node) {
            Ok(renderer) => render(
                renderer,
                head,
                &self.space,
                self.lock.as_ref(),
                pointer,
                &drm.pointer,
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
                self.schedule(crtc, FRAME_THROTTLE);
            }
        }
        self.serve_captures();
        self.space.refresh();
        self.popups.cleanup();
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
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
        tracing::warn!(%error, "a frame could not be drawn");
        self.schedule(crtc, period);
    }
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
    }

    /// The queued frame reached the screen.
    fn presented(&mut self) {
        self.shown = self.queued;
    }
}

/// Whether every head of `heads` shows the lock.
fn lock_shown<'a>(heads: impl IntoIterator<Item = &'a LockFrames>) -> bool {
    heads.into_iter().all(|frames| frames.shown)
}

/// The delay of the next draw of `head`, when it must be scheduled at `now`: when something
/// may have changed and neither a queued frame (whose VBlank draws the next) nor a scheduled
/// draw will draw it.
fn next_draw(head: &Head, now: Instant) -> Option<Duration> {
    pacing(
        head.wanted,
        head.queued || head.timer.is_some(),
        head.not_before,
        now,
    )
}

/// `next_draw`, of what it reads of the head: `wanted`, a frame queued or a draw scheduled
/// (`busy`), and the earliest next draw.
fn pacing(wanted: bool, busy: bool, not_before: Option<Instant>, now: Instant) -> Option<Duration> {
    (wanted && !busy).then(|| {
        not_before.map_or(Duration::ZERO, |not_before| {
            not_before.saturating_duration_since(now)
        })
    })
}

/// Draws the frame of `head`: the space, or the lock surface while `lock` holds the session,
/// with the pointer on top when it is over the output. Whether there was anything new to
/// show, and the elements' states.
fn render(
    renderer: &mut GlesRenderer,
    head: &mut Head,
    space: &smithay::desktop::Space<Window>,
    lock: Option<&crate::lock::Lock>,
    pointer: Option<smithay::utils::Point<f64, smithay::utils::Logical>>,
    pointer_buffer: &SolidColorBuffer,
) -> Result<(bool, RenderElementStates), SwapBuffersError> {
    let output = &head.output;
    let scale = output.current_scale().fractional_scale();
    let mut elements: Vec<Element> = Vec::new();
    if let (Some(location), Some(area)) = (pointer, space.output_geometry(output)) {
        if area.to_f64().contains(location) {
            let at = (location - area.loc.to_f64())
                .to_physical(scale)
                .to_i32_round();
            elements.push(Element::Pointer(SolidColorRenderElement::from_buffer(
                pointer_buffer,
                at,
                scale,
                1.0,
                Kind::Cursor,
            )));
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
    use super::{lock_shown, pacing, LockFrames};
    use std::time::{Duration, Instant};

    #[test]
    fn an_output_draws_only_when_asked_and_at_most_once_a_refresh() {
        let now = Instant::now();
        let period = Duration::from_millis(16);
        assert_eq!(pacing(false, false, None, now), None, "idle: no draw");
        assert_eq!(
            pacing(true, false, None, now),
            Some(Duration::ZERO),
            "asked: at once"
        );
        assert_eq!(
            pacing(true, true, None, now),
            None,
            "a queued frame or a scheduled draw draws it"
        );
        assert_eq!(
            pacing(true, false, Some(now + period), now),
            Some(period),
            "not before a refresh after the last draw"
        );
        assert_eq!(
            pacing(true, false, Some(now), now + period),
            Some(Duration::ZERO),
            "at once when that refresh is over"
        );
    }

    /// The lock client is told `locked` only once every output presented a frame drawn locked:
    /// a frame whose submission failed, or a draw with nothing new, does not count.
    #[test]
    fn the_lock_is_confirmed_only_by_locked_frames_on_every_screen() {
        let (mut first, mut second) = (LockFrames::default(), LockFrames::default());
        assert!(!lock_shown([&first, &second]), "unlocked frames on screen");
        first.queued(true);
        second.queued(true);
        second.presented();
        // The first output's frame failed to present, then a draw had nothing new: neither
        // reaches it.
        assert!(
            !lock_shown([&first, &second]),
            "the first screen is not locked yet"
        );
        first.queued(true);
        first.presented();
        assert!(lock_shown([&first, &second]), "both screens show the lock");
        second.queued(false);
        assert!(
            lock_shown([&first, &second]),
            "an unlocked frame only queued"
        );
        second.presented();
        assert!(
            !lock_shown([&first, &second]),
            "an unlocked frame on screen"
        );
    }
}
