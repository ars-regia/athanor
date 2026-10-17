//! The headless backend (doc_own_stack.md OS6): virtual outputs of any number, size and scale,
//! drawn off screen and read back for comparison. A render node draws them through GBM, EGL and
//! GLES, and offers linux-dmabuf with its feedback and linux-drm-syncobj-v1 when the node can
//! wait on a syncobj; without one, Pixman draws them on the CPU, which needs no device at all.

use crate::state::{new_output, Backend, State, BACKGROUND};
use crate::Error;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::{DrmDeviceFd, DrmNode};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::RenderElementStates;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{ApiDevice, GpuManager};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{
    Bind, ExportMem, ImportAll, ImportDma, Offscreen, Renderer, Texture,
};
use smithay::desktop::space::render_output;
use smithay::desktop::{Space, Window};
use smithay::output::Output;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::pixman::Image;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::Display;
use smithay::utils::{Buffer as BufferCoord, DeviceFd, Rectangle, Size, Transform};
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use smithay::wayland::drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjState};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::{Duration, Instant};

/// Every virtual output refreshes at 60 Hz, in mHz.
const REFRESH: i32 = 60_000;

/// How often the clients' frame times are logged.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// The pixel format of the off-screen buffers and of a frame read back: R, G, B, A bytes.
pub const FRAME_FORMAT: Fourcc = Fourcc::Abgr8888;

/// A virtual output: its size in pixels and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualOutput {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

/// What draws the virtual outputs, and one off-screen buffer per output.
enum Gpu {
    Gbm {
        // The manager creates the EGL display and the GLES renderer for the node without the
        // unsafe calls doing it by hand would take.
        manager: GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>,
        node: DrmNode,
        buffers: Vec<GlesTexture>,
    },
    Pixman {
        renderer: PixmanRenderer,
        buffers: Vec<Image<'static, 'static>>,
    },
}

pub struct Headless {
    gpu: Gpu,
    outputs: Vec<Virtual>,
}

struct Virtual {
    output: Output,
    damage_tracker: OutputDamageTracker,
    /// The buffer holds this output's previous frame: only the damage is drawn again.
    drawn: bool,
}

/// A frame: what was drawn and, when asked for, its pixels in `FRAME_FORMAT`.
struct Drawn {
    states: RenderElementStates,
    damaged: bool,
    // Read back only by the fixture, through `State::capture`.
    #[cfg_attr(not(test), allow(dead_code))]
    pixels: Option<Vec<u8>>,
}

/// Starts the headless backend: on the render node at `node`, or on Pixman without one.
pub fn start(
    event_loop: &mut EventLoop<'static, State>,
    display: Display<State>,
    node: Option<&Path>,
    outputs: &[VirtualOutput],
) -> Result<State, Error> {
    let dh = display.handle();
    let (gpu, device) = match node {
        Some(path) => {
            let (gpu, fd) = gbm(path)?;
            (gpu, Some((path, fd)))
        }
        None => (
            Gpu::Pixman {
                renderer: PixmanRenderer::new()?,
                buffers: Vec::new(),
            },
            None,
        ),
    };
    let headless = Headless {
        gpu,
        outputs: Vec::new(),
    };
    let mut state = State::new(event_loop, display, Backend::Headless(Box::new(headless)))?;
    for (index, spec) in outputs.iter().enumerate() {
        state.add_virtual_output(index, *spec)?;
    }

    match device {
        Some((path, fd)) => {
            let Backend::Headless(headless) = &mut state.backend else {
                return Err("the headless backend lost its render node".into());
            };
            let Gpu::Gbm { manager, node, .. } = &mut headless.gpu else {
                return Err("the headless backend lost its render node".into());
            };
            let formats = gles(manager, node)?.dmabuf_formats();
            let feedback = DmabufFeedbackBuilder::new(node.dev_id(), formats).build()?;
            state
                .dmabuf_state
                .create_global_with_default_feedback::<State>(&dh, &feedback);
            if supports_syncobj_eventfd(&fd) {
                state.syncobj_state = Some(DrmSyncobjState::new::<State>(&dh, fd));
                tracing::info!(node = %path.display(), "explicit sync: linux-drm-syncobj-v1 on the render node");
            } else {
                tracing::info!(node = %path.display(), "explicit sync: none, the render node cannot wait on a syncobj");
            }
        }
        // ponytail: no linux-dmabuf on Pixman, whose clients draw into wl_shm; offer it when a
        // client of the fixture needs to import through a mapping.
        None => tracing::info!("explicit sync: none, Pixman draws without a render node"),
    }

    state.frame_stats = Some(FrameStats::default());
    let period = Duration::from_secs_f64(1_000.0 / f64::from(REFRESH));
    event_loop
        .handle()
        .insert_source(Timer::from_duration(period), move |_, _, state| {
            state.tick(period);
            TimeoutAction::ToDuration(period)
        })
        .map_err(|e| e.error)?;
    event_loop
        .handle()
        .insert_source(Timer::from_duration(REPORT_EVERY), |_, _, state| {
            if let Some(stats) = &mut state.frame_stats {
                stats.report();
            }
            TimeoutAction::ToDuration(REPORT_EVERY)
        })
        .map_err(|e| e.error)?;
    Ok(state)
}

/// Opens the render node at `path` and its GBM device under the GPU manager.
fn gbm(path: &Path) -> Result<(Gpu, DrmDeviceFd), Error> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    let fd = DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(file)));
    let node = DrmNode::from_file(&fd)?;
    let mut api = GbmGlesBackend::default();
    api.add_node(node, GbmDevice::new(fd.clone())?)?;
    let mut manager = GpuManager::new(api)?;
    gles(&mut manager, &node)?;
    tracing::info!(node = %path.display(), "drawing on the render node through GBM and GLES");
    Ok((
        Gpu::Gbm {
            manager,
            node,
            buffers: Vec::new(),
        },
        fd,
    ))
}

/// The GLES renderer of `node`.
fn gles<'a>(
    manager: &'a mut GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>,
    node: &DrmNode,
) -> Result<&'a mut GlesRenderer, Error> {
    manager
        .devices_mut()?
        .find(|device| device.node() == node)
        .map(ApiDevice::renderer_mut)
        .ok_or_else(|| format!("no renderer for {node}").into())
}

impl State {
    /// Adds virtual output `index`, to the right of the others, with its off-screen buffer.
    fn add_virtual_output(&mut self, index: usize, spec: VirtualOutput) -> Result<(), Error> {
        let Backend::Headless(headless) = &mut self.backend else {
            return Err("virtual outputs need the headless backend".into());
        };
        let size = Size::<i32, BufferCoord>::from((spec.width, spec.height));
        match &mut headless.gpu {
            Gpu::Gbm {
                manager,
                node,
                buffers,
            } => buffers.push(gles(manager, node)?.create_buffer(FRAME_FORMAT, size)?),
            Gpu::Pixman { renderer, buffers } => {
                buffers.push(renderer.create_buffer(FRAME_FORMAT, size)?)
            }
        }
        let output = new_output(
            &self.display_handle,
            &format!("headless-{index}"),
            "Headless",
            (spec.width, spec.height).into(),
            REFRESH,
            Transform::Normal,
            spec.scale,
        );
        headless.outputs.push(Virtual {
            damage_tracker: OutputDamageTracker::from_output(&output),
            output: output.clone(),
            drawn: false,
        });
        self.map_output(&output);
        Ok(())
    }

    /// Draws every output, then sends presentation feedback and frame callbacks.
    fn tick(&mut self, period: Duration) {
        let Backend::Headless(headless) = &mut self.backend else {
            return;
        };
        let frame_target = self.clock.now() + period;
        let mut frames = Vec::with_capacity(headless.outputs.len());
        for index in 0..headless.outputs.len() {
            let started = Instant::now();
            match headless.draw(index, &self.space, false) {
                Ok(drawn) => {
                    frames.push((headless.outputs[index].output.clone(), drawn));
                    if let Some(stats) = &mut self.frame_stats {
                        stats.drew(started.elapsed());
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, output = index, "an output could not be drawn")
                }
            }
        }
        for (output, drawn) in frames {
            self.frame_done(&output, &drawn.states, drawn.damaged, frame_target, period);
        }
        self.space.refresh();
        self.popups.cleanup();
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
    }
}

impl Headless {
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> Result<(), Error> {
        match &mut self.gpu {
            Gpu::Gbm { manager, node, .. } => {
                gles(manager, node)?.import_dmabuf(dmabuf, None)?;
            }
            Gpu::Pixman { renderer, .. } => {
                renderer.import_dmabuf(dmabuf, None)?;
            }
        }
        Ok(())
    }

    /// Draws output `index`, and reads its pixels back when `read`.
    fn draw(&mut self, index: usize, space: &Space<Window>, read: bool) -> Result<Drawn, Error> {
        let virt = self
            .outputs
            .get_mut(index)
            .ok_or_else(|| format!("no output {index}"))?;
        let missing = || format!("no buffer for output {index}");
        let drawn = match &mut self.gpu {
            Gpu::Gbm {
                manager,
                node,
                buffers,
            } => {
                let buffer = buffers.get_mut(index).ok_or_else(missing)?;
                draw(gles(manager, node)?, buffer, virt, space, read)?
            }
            Gpu::Pixman { renderer, buffers } => {
                let buffer = buffers.get_mut(index).ok_or_else(missing)?;
                draw(renderer, buffer, virt, space, read)?
            }
        };
        virt.drawn = true;
        Ok(drawn)
    }
}

/// Draws the space on `virt` into `buffer`, and reads the frame back when `read`.
fn draw<R, T>(
    renderer: &mut R,
    buffer: &mut T,
    virt: &mut Virtual,
    space: &Space<Window>,
    read: bool,
) -> Result<Drawn, Error>
where
    R: Renderer + ImportAll + ExportMem + Bind<T>,
    R::TextureId: Clone + Texture + 'static,
    R::Error: 'static,
{
    let age = usize::from(virt.drawn);
    let mut target = renderer.bind(buffer)?;
    let result = render_output::<_, WaylandSurfaceRenderElement<R>, _, _>(
        &virt.output,
        renderer,
        &mut target,
        1.0,
        age,
        [space],
        &[],
        &mut virt.damage_tracker,
        BACKGROUND,
    )
    .map_err(|error| format!("{error:?}"))?;
    // Smithay releases a client's buffer when the renderer drops its texture, not when the GPU
    // has finished sampling it: the frame is waited for here, before its frame callbacks invite
    // the clients to reuse their buffers. Pixman's sync point has already signalled; on GLES
    // this blocks the loop for one frame of GPU work, which only the headless backend can
    // afford. The DRM backend (task 8) must not drop the sync point either: Smithay's
    // DrmCompositor passes it to the KMS commit as the plane's IN_FENCE_FD when the fence can
    // be exported, and waits as here when it cannot (as anvil does for its swapchain frame).
    result.sync.wait()?;
    let damaged = result.damage.is_some();
    let states = result.states;
    let pixels = if read {
        let size = virt
            .output
            .current_mode()
            .ok_or("the output has no mode")?
            .size;
        let region = Rectangle::from_size(Size::<i32, BufferCoord>::from((size.w, size.h)));
        let mapping = renderer.copy_framebuffer(&target, region, FRAME_FORMAT)?;
        Some(renderer.map_texture(&mapping)?.to_vec())
    } else {
        None
    };
    Ok(Drawn {
        states,
        damaged,
        pixels,
    })
}

/// Each client surface's commits of a new buffer: how often, through dmabuf, with an acquire
/// point of linux-drm-syncobj-v1; and how long the outputs take to draw. Logged every
/// `REPORT_EVERY` and reset.
#[derive(Default)]
pub struct FrameStats {
    surfaces: HashMap<ObjectId, Commits>,
    draws: Timing,
}

#[derive(Default)]
struct Commits {
    pid: Option<i32>,
    last: Option<Instant>,
    dmabuf: u64,
    syncobj: u64,
    gaps: Timing,
}

/// A count, a sum and a maximum of durations, and the number above two frames of 60 Hz.
#[derive(Default)]
struct Timing {
    count: u64,
    total: Duration,
    max: Duration,
    late: u64,
}

impl Timing {
    fn add(&mut self, value: Duration) {
        self.count += 1;
        self.total += value;
        self.max = self.max.max(value);
        if value > Duration::from_micros(33_334) {
            self.late += 1;
        }
    }

    fn mean_ms(&self) -> f64 {
        match self.count {
            0 => 0.0,
            n => self.total.as_secs_f64() * 1_000.0 / n as f64,
        }
    }
}

impl FrameStats {
    /// A commit of a new buffer on `surface`.
    pub fn commit(&mut self, surface: ObjectId, pid: Option<i32>, dmabuf: bool, syncobj: bool) {
        let now = Instant::now();
        let commits = self.surfaces.entry(surface).or_default();
        commits.pid = pid;
        if let Some(last) = commits.last.replace(now) {
            commits.gaps.add(now - last);
        }
        commits.dmabuf += u64::from(dmabuf);
        commits.syncobj += u64::from(syncobj);
    }

    fn drew(&mut self, took: Duration) {
        self.draws.add(took);
    }

    fn report(&mut self) {
        tracing::info!(
            draws = self.draws.count,
            mean_ms = format!("{:.2}", self.draws.mean_ms()),
            max_ms = format!("{:.2}", self.draws.max.as_secs_f64() * 1_000.0),
            "frame times: outputs drawn"
        );
        // A surface with no new buffer for a whole period, gone or idle, is forgotten.
        self.surfaces.retain(|_, commits| commits.gaps.count > 0);
        for (surface, commits) in &mut self.surfaces {
            tracing::info!(
                %surface,
                pid = ?commits.pid,
                frames = commits.gaps.count,
                dmabuf = commits.dmabuf,
                syncobj = commits.syncobj,
                mean_ms = format!("{:.2}", commits.gaps.mean_ms()),
                max_ms = format!("{:.2}", commits.gaps.max.as_secs_f64() * 1_000.0),
                over_two_frames = commits.gaps.late,
                "frame times: a client's commits"
            );
            *commits = Commits {
                pid: commits.pid,
                last: commits.last,
                ..Commits::default()
            };
        }
        self.draws = Timing::default();
    }
}

// Test-only code comes last: `verify.py panics` stops reading a file where it begins.
#[cfg(test)]
impl State {
    /// The pixels of output `index` as the space now stands, `FRAME_FORMAT`, row by row from
    /// the top.
    pub fn capture(&mut self, index: usize) -> Result<Vec<u8>, Error> {
        let Backend::Headless(headless) = &mut self.backend else {
            return Err("only the headless backend reads frames back".into());
        };
        headless
            .draw(index, &self.space, true)?
            .pixels
            .ok_or_else(|| "the frame was not read back".into())
    }
}
