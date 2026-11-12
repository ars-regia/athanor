//! athanor-material-v1 (doc_own_stack.md decision 35): each output's material tier, M0, M1 or
//! M2, measured as `doc_visual_language.md` VL13 says. At each output enable, mode change and
//! GPU change a probe draws off screen, on a thread and in a GL context of its own, and VL13's
//! thresholds turn its times into the tier; while an output is in M2 its real frames are timed
//! too, and a slow median moves it to M1 until its next probe. Software rendering is M0 without
//! a probe; a probe that fails or misses its deadline is M0, and so is an output until its probe
//! has run. The description is Athanor's own,
//! `system/athanor-sulphur-client/protocols/athanor-material-v1.xml`. The manager is privileged
//! (OS4) by a grant on each shell surface's line.

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_material_manager_v1::{self, AthanorMaterialManagerV1};
use protocol::athanor_material_v1::{self, AthanorMaterialV1};
use smithay::backend::drm::DrmNode;
use smithay::output::{Mode, Output};
use smithay::reexports::calloop::channel;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{Interest, Mode as Trigger, PostAction, RegistrationToken};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New};
use smithay::utils::{Physical, Rectangle, Size};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The server side of the description, laid out as `wayland-protocols` lays out its own.
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
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-sulphur-client/protocols/athanor-material-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-sulphur-client/protocols/athanor-material-v1.xml"
    );
}

/// How long a probe may take (VL13).
const DEADLINE: Duration = Duration::from_secs(3);
/// The live check's window: the last damaged frames of an output in M2 (VL13).
const LIVE_FRAMES: usize = 120;

/// A GPU, by the path of its render node: its probes run one at a time.
type Gpu = PathBuf;
/// What a probe's thread sends when it ends: its GPU, its generation and its result.
type Outcome = (Gpu, u64, Result<Probed, String>);
/// A probe: of the scene, on the render node at the path, ended by the deadline.
pub(crate) type Prober =
    Arc<dyn Fn(&Path, &probe::Scene, Instant) -> Result<Probed, String> + Send + Sync>;

/// In tests, a GPU that is not there: its path, the probe that stands for it, and the probes'
/// deadline.
#[cfg(test)]
pub(crate) struct Fake {
    pub gpu: Gpu,
    pub prober: Prober,
    pub deadline: Duration,
}

/// The material tiers of VL13.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Opaque.
    M0,
    /// Glass over the wallpaper blurred once.
    M1,
    /// Live blur, drawn by the compositor.
    M2,
}

impl From<Tier> for athanor_material_v1::Tier {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::M0 => Self::M0,
            Tier::M1 => Self::M1,
            Tier::M2 => Self::M2,
        }
    }
}

/// The 95th percentile of each of the probe's three cases.
#[derive(Clone, Copy, Debug)]
pub struct Probed {
    /// The output's scene.
    pub scene: Duration,
    /// The scene, then the dock's region copied out and blurred.
    pub dock: Duration,
    /// The scene, then the whole output copied out and blurred.
    pub full: Duration,
}

/// VL13's thresholds, with `interval` the output's frame interval T: M2 when the dock's case
/// is at most T/4 and the whole output's at most T/2, M1 otherwise when the scene is at most
/// T/2, M0 otherwise. `None` is software rendering, which is M0 without a measurement.
pub fn tier(probed: Option<Probed>, interval: Duration) -> Tier {
    let Some(probed) = probed else {
        return Tier::M0;
    };
    if probed.dock <= interval / 4 && probed.full <= interval / 2 {
        Tier::M2
    } else if probed.scene <= interval / 2 {
        Tier::M1
    } else {
        Tier::M0
    }
}

/// An output's measurement.
#[derive(Debug)]
enum Measure {
    /// Waiting for its GPU, on which another output's probe runs: M0 meanwhile.
    Waiting,
    /// The probe of `generation` runs, until `deadline`: the output is M0 meanwhile.
    Probing { generation: u64, deadline: Instant },
    /// The tier, and, in M2, the times of the last damaged frames.
    Settled {
        tier: Tier,
        live: VecDeque<Duration>,
    },
}

impl Measure {
    fn settled(tier: Tier) -> Self {
        Self::Settled {
            tier,
            live: VecDeque::new(),
        }
    }

    fn tier(&self) -> Tier {
        match self {
            Self::Waiting | Self::Probing { .. } => Tier::M0,
            Self::Settled { tier, .. } => *tier,
        }
    }

    /// The result of the probe of `generation`, arrived at `now`: its tier, or M0 when it
    /// failed or arrived past its deadline. The result of an older probe changes nothing.
    fn probed(
        &mut self,
        generation: u64,
        result: Result<Probed, String>,
        now: Instant,
        interval: Duration,
    ) {
        let Self::Probing {
            generation: running,
            deadline,
        } = *self
        else {
            return;
        };
        if running != generation {
            return;
        }
        let tier = match result {
            Ok(_) if now > deadline => {
                tracing::warn!("the material probe finished past its deadline: tier M0");
                Tier::M0
            }
            Ok(probed) => {
                let tier = tier(Some(probed), interval);
                tracing::info!(?probed, ?interval, ?tier, "the material probe has run");
                tier
            }
            Err(error) => {
                tracing::warn!(%error, "the material probe failed: tier M0");
                Tier::M0
            }
        };
        *self = Self::settled(tier);
    }

    /// The deadline of the probe of `generation` has passed: M0, whatever it sends later.
    fn expired(&mut self, generation: u64) {
        if matches!(*self, Self::Probing { generation: running, .. } if running == generation) {
            tracing::warn!("the material probe did not finish by its deadline: tier M0");
            *self = Self::settled(Tier::M0);
        }
    }

    /// A damaged frame drawn in `time`: in M2, when the median of the last `LIVE_FRAMES`
    /// exceeds half the interval, the output falls to M1, which lasts until the next probe.
    fn frame(&mut self, time: Duration, interval: Duration) {
        let Self::Settled {
            tier: tier @ Tier::M2,
            live,
        } = self
        else {
            return;
        };
        live.push_back(time);
        if live.len() > LIVE_FRAMES {
            live.pop_front();
        }
        if live.len() == LIVE_FRAMES && median(live) > interval / 2 {
            tracing::info!(
                ?interval,
                "the live check is over half the interval: tier M1"
            );
            *tier = Tier::M1;
            live.clear();
        }
    }
}

/// The median of `times`, not empty: the mean of the two middle ones of an even count.
fn median(times: &VecDeque<Duration>) -> Duration {
    let mut sorted: Vec<Duration> = times.iter().copied().collect();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    match (sorted.get(middle.wrapping_sub(1)), sorted.get(middle)) {
        (Some(low), Some(high)) if sorted.len().is_multiple_of(2) => (*low + *high) / 2,
        (_, Some(middle)) => *middle,
        _ => Duration::ZERO,
    }
}

/// What a measurement holds for: the output's mode and the GPU that draws it. A change of
/// either, or a disable, starts the measurement again.
type Signature = (Option<Mode>, Option<Gpu>);

struct Record {
    output: Output,
    signature: Signature,
    /// The output's frame interval T.
    interval: Duration,
    measure: Measure,
    /// The watch on the fence of the frame being timed: one at a time.
    timing: Option<RegistrationToken>,
}

/// The outputs' measurements and the tier objects, in `State::materials`.
#[derive(Default)]
pub struct Materials {
    records: Vec<Record>,
    /// The tier objects, their output and the tier last sent to each.
    objects: Vec<(AthanorMaterialV1, Option<Output>, Tier)>,
    generation: u64,
    /// Where the probes' threads send their outcomes.
    results: Option<channel::Sender<Outcome>>,
    /// The GPUs a probe's thread runs on, until it has sent its outcome.
    busy: Vec<Gpu>,
    #[cfg(test)]
    pub(crate) fake: Option<Fake>,
}

impl Materials {
    pub(crate) fn tier_of(&self, output: Option<&Output>) -> Tier {
        output
            .and_then(|output| self.records.iter().find(|record| record.output == *output))
            .map_or(Tier::M0, |record| record.measure.tier())
    }

    /// How many probes have been started.
    #[cfg(test)]
    pub(crate) fn probes(&self) -> u64 {
        self.generation
    }

    /// Whether `output`'s measurement has settled.
    #[cfg(test)]
    pub(crate) fn settled(&self, output: &Output) -> bool {
        self.records.iter().any(|record| {
            record.output == *output && matches!(record.measure, Measure::Settled { .. })
        })
    }

    /// The watch on the fence of `output`'s frame being timed.
    fn timing(&mut self, output: &Output) -> Option<&mut Option<RegistrationToken>> {
        self.records
            .iter_mut()
            .find(|record| record.output == *output)
            .map(|record| &mut record.timing)
    }

    /// Whether a probe's thread runs.
    pub(crate) fn probing(&self) -> bool {
        !self.busy.is_empty()
    }

    /// Sends each object of `output` its tier, when it changed.
    fn send(&mut self, output: &Output) {
        let tier = self.tier_of(Some(output));
        for (object, of, sent) in &mut self.objects {
            if of.as_ref() == Some(output) && *sent != tier {
                object.tier(tier.into());
                *sent = tier;
            }
        }
    }

    /// The probe: VL13's, or in tests the fake's.
    fn prober(&self) -> Prober {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake.prober.clone();
        }
        Arc::new(|path: &Path, scene: &probe::Scene, deadline: Instant| {
            probe::run(path, scene, deadline).map_err(|error| error.to_string())
        })
    }

    /// How long a probe may take: VL13's `DEADLINE`, or in tests the fake's.
    fn deadline(&self) -> Duration {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake.deadline;
        }
        DEADLINE
    }
}

/// The user data of the manager's global, of every manager and of every tier object.
pub struct MaterialTiers;

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorMaterialManagerV1, _>(1, MaterialTiers);
}

impl smithay::wayland::GlobalDispatch2<AthanorMaterialManagerV1, State> for MaterialTiers {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorMaterialManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, MaterialTiers);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::Material)
    }
}

impl smithay::wayland::Dispatch2<AthanorMaterialManagerV1, State> for MaterialTiers {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _manager: &AthanorMaterialManagerV1,
        request: athanor_material_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_material_manager_v1::Request;
        match request {
            Request::GetTier { id, output } => {
                let object = data_init.init(id, MaterialTiers);
                // An output already withdrawn is M0, and stays so.
                let output = Output::from_resource(&output);
                let tier = state.materials.tier_of(output.as_ref());
                object.tier(tier.into());
                state.materials.objects.push((object, output, tier));
            }
            Request::Destroy => {}
        }
    }
}

impl smithay::wayland::Dispatch2<AthanorMaterialV1, State> for MaterialTiers {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _object: &AthanorMaterialV1,
        request: athanor_material_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        let athanor_material_v1::Request::Destroy = request;
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorMaterialV1) {
        state.materials.objects.retain(|(of, _, _)| of != object);
    }
}

impl State {
    /// Measures again each output shown whose mode or GPU changed, or which was just enabled,
    /// and forgets the outputs no longer shown, which are M0. Called after every frame.
    pub(crate) fn refresh_materials(&mut self) {
        let gpu = self.material_gpu();
        let shown: Vec<Output> = self
            .space
            .outputs()
            .filter(|output| !self.output_management.is_disabled(output))
            .cloned()
            .collect();
        let mut changed = Vec::new();
        let records = std::mem::take(&mut self.materials.records);
        for record in records {
            if shown.contains(&record.output)
                && record.signature.0 == record.output.current_mode()
                && record.signature.1 == gpu
            {
                self.materials.records.push(record);
            } else {
                // A probe of the record that runs still ends; its result is for no record.
                if let Some(token) = record.timing {
                    self.loop_handle.remove(token);
                }
                changed.push(record.output);
            }
        }
        for output in shown {
            if !self
                .materials
                .records
                .iter()
                .any(|record| record.output == output)
            {
                self.measure(&output, gpu.clone());
                changed.push(output);
            }
        }
        self.start_probes();
        for output in changed {
            self.materials.send(&output);
        }
    }

    /// A damaged frame of `output`, drawn in `time` from its first GL call to its sync point:
    /// the live check of an output in M2. Frames drawn while a probe runs do not count.
    pub(crate) fn material_frame(&mut self, output: &Output, time: Duration) {
        if self.materials.probing() {
            return;
        }
        let Some(record) = self
            .materials
            .records
            .iter_mut()
            .find(|record| record.output == *output)
        else {
            return;
        };
        let before = record.measure.tier();
        record.measure.frame(time, record.interval);
        if record.measure.tier() != before {
            self.materials.send(output);
        }
    }

    /// Whether `output`'s frames are timed: in M2, outside a probe.
    pub(crate) fn times_frames(&self, output: &Output) -> bool {
        !self.materials.probing() && self.materials.tier_of(Some(output)) == Tier::M2
    }

    /// Times a frame of `output` started at `started` through its sync point, without
    /// blocking: at once when it has signalled, at its fence otherwise; a fence that cannot be
    /// exported leaves the frame untimed.
    pub(crate) fn time_frame(
        &mut self,
        output: &Output,
        started: Instant,
        sync: smithay::backend::renderer::sync::SyncPoint,
    ) {
        if !self.times_frames(output) {
            return;
        }
        if sync.is_reached() {
            self.material_frame(output, started.elapsed());
            return;
        }
        // One frame of an output is timed at a time: a fence that never signals holds one
        // watch, until the output's measurement starts again.
        if !matches!(self.materials.timing(output), Some(None)) {
            return;
        }
        let Some(fence) = sync.export() else {
            return;
        };
        let watched = output.clone();
        let source = Generic::new(fence, Interest::READ, Trigger::OneShot);
        // ponytail: timed when the loop runs the callback, which adds the loop's latency to
        // the frame and errs towards M1; the sync file's own signal time (SYNC_IOC_FILE_INFO)
        // would remove it.
        let inserted = self.loop_handle.insert_source(source, move |_, _, state| {
            if let Some(token) = state.materials.timing(&watched) {
                *token = None;
            }
            state.material_frame(&watched, started.elapsed());
            Ok(PostAction::Remove)
        });
        match inserted {
            Ok(token) => {
                if let Some(slot) = self.materials.timing(output) {
                    *slot = Some(token);
                }
            }
            Err(error) => tracing::warn!(%error, "a frame's fence could not be watched"),
        }
    }

    /// The render node that draws the outputs; none in software, or nested.
    // ponytail: nested, the parent's GPU is not known, so a nested output stays M0.
    fn render_node(&self) -> Option<DrmNode> {
        match &self.backend {
            crate::state::Backend::Headless(headless) => headless.render_node(),
            crate::state::Backend::Drm(drm) => Some(drm.render_node()),
            crate::state::Backend::Nested(_) => None,
        }
    }

    /// The GPU that draws the outputs, by its render node's path; none in software, or
    /// nested.
    fn material_gpu(&self) -> Option<Gpu> {
        #[cfg(test)]
        if let Some(fake) = &self.materials.fake {
            return Some(fake.gpu.clone());
        }
        self.render_node().and_then(|node| node.dev_path())
    }

    /// Starts the measurement of `output`: M0 in software, a probe on its GPU otherwise, which
    /// waits for the GPU's probe that runs.
    fn measure(&mut self, output: &Output, gpu: Option<Gpu>) {
        let mode = output.current_mode();
        let refresh = mode.map_or(60_000, |mode| mode.refresh).max(1);
        let interval = Duration::from_secs_f64(1_000.0 / f64::from(refresh));
        let measure = if gpu.is_some() {
            Measure::Waiting
        } else {
            if let Some(node) = self.render_node() {
                tracing::warn!(%node, "the render node has no path: tier M0");
            }
            Measure::settled(Tier::M0)
        };
        self.materials.records.push(Record {
            output: output.clone(),
            signature: (mode, gpu),
            interval,
            measure,
            timing: None,
        });
    }

    /// Starts the probe of the first output waiting on each GPU that no probe runs on: one
    /// probe at a time on a GPU, so that they do not slow each other.
    fn start_probes(&mut self) {
        loop {
            let busy = &self.materials.busy;
            let Some((index, output, gpu)) =
                self.materials
                    .records
                    .iter()
                    .enumerate()
                    .find_map(
                        |(index, record)| match (&record.measure, &record.signature.1) {
                            (Measure::Waiting, Some(gpu)) if !busy.contains(gpu) => {
                                Some((index, record.output.clone(), gpu.clone()))
                            }
                            _ => None,
                        },
                    )
            else {
                return;
            };
            // Either the GPU is busy now or the record has settled: the loop ends.
            let measure = self.probe(&output, gpu);
            if let Some(record) = self.materials.records.get_mut(index) {
                record.measure = measure;
            }
        }
    }

    /// Starts a probe of `output` on `gpu`, and the timer of its deadline, by which its thread
    /// ends.
    fn probe(&mut self, output: &Output, gpu: Gpu) -> Measure {
        let Some(scene) = self.scene_of(output) else {
            tracing::warn!(
                output = output.name(),
                "the output has no area to probe: tier M0"
            );
            return Measure::settled(Tier::M0);
        };
        let Some(results) = self.material_results() else {
            return Measure::settled(Tier::M0);
        };
        self.materials.generation += 1;
        let generation = self.materials.generation;
        let deadline = Instant::now() + self.materials.deadline();
        let prober = self.materials.prober();
        let on = gpu.clone();
        let spawned = std::thread::Builder::new()
            .name("material-probe".into())
            .spawn(move || {
                let result = prober(&on, &scene, deadline);
                if results.send((on, generation, result)).is_err() {
                    tracing::debug!("the material probe ended after the event loop");
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "the material probe could not be started: tier M0");
            return Measure::settled(Tier::M0);
        }
        self.materials.busy.push(gpu);
        let timer = Timer::from_deadline(deadline);
        let armed = self.loop_handle.insert_source(timer, move |_, _, state| {
            state.material_expired(generation);
            TimeoutAction::Drop
        });
        if let Err(error) = armed {
            tracing::warn!(%error, "the material probe's deadline could not be armed: tier M0");
            return Measure::settled(Tier::M0);
        }
        Measure::Probing {
            generation,
            deadline,
        }
    }

    /// The channel the probes send their results to, made with the first probe.
    fn material_results(&mut self) -> Option<channel::Sender<Outcome>> {
        if let Some(results) = &self.materials.results {
            return Some(results.clone());
        }
        let (results, received) = channel::channel();
        let inserted = self.loop_handle.insert_source(received, |event, _, state| {
            if let channel::Event::Msg((gpu, generation, result)) = event {
                state.material_probed(gpu, generation, result);
            }
        });
        if let Err(error) = inserted {
            tracing::warn!(%error, "the material probes' channel was not added to the loop");
            return None;
        }
        self.materials.results = Some(results.clone());
        Some(results)
    }

    /// A probe's thread has ended on `gpu`: its result settles its output, unless a new
    /// measurement superseded it, and the next probe waiting on the GPU starts.
    fn material_probed(&mut self, gpu: Gpu, generation: u64, result: Result<Probed, String>) {
        let now = Instant::now();
        self.materials.busy.retain(|busy| *busy != gpu);
        let settled = self
            .materials
            .records
            .iter_mut()
            .find(|record| {
                matches!(record.measure, Measure::Probing { generation: running, .. } if running == generation)
            })
            .map(|record| {
                record
                    .measure
                    .probed(generation, result, now, record.interval);
                record.output.clone()
            });
        if let Some(output) = settled {
            self.materials.send(&output);
        }
        self.start_probes();
    }

    fn material_expired(&mut self, generation: u64) {
        let Some(record) = self.materials.records.iter_mut().find(|record| {
            matches!(record.measure, Measure::Probing { generation: running, .. } if running == generation)
        }) else {
            return;
        };
        record.measure.expired(generation);
        let output = record.output.clone();
        self.materials.send(&output);
    }

    /// What the probe draws for `output`, in its physical pixels: its size, the windows of its
    /// current workspace, and the dock's region (`dock_region`).
    fn scene_of(&self, output: &Output) -> Option<probe::Scene> {
        let size = output.current_mode()?.size;
        let area = self.space.output_geometry(output)?;
        let scale = output.current_scale().fractional_scale();
        let current = self.active_workspace(output);
        let windows = self
            .space
            .elements()
            .filter(|window| self.workspace_of(window) == current)
            .filter_map(|window| self.space.element_geometry(window))
            .filter_map(|geometry| geometry.intersection(area))
            .map(|geometry| {
                Rectangle::new(geometry.loc - area.loc, geometry.size)
                    .to_physical_precise_round(scale)
            })
            .collect();
        let dock = dock_region(
            size,
            self.dock_rectangle(output)
                .map(|rectangle| rectangle.to_physical_precise_round(scale)),
        )?;
        Some(probe::Scene {
            size,
            windows,
            dock,
        })
    }
}

/// The region the probe blurs as the dock's, on an output of `size`: the dock's rectangle on
/// the output or, without a dock or when its rectangle lies off the output, as after a mode
/// change, spike VL-S5's 1280 x 96 at the bottom; none when the output has no area.
fn dock_region(
    size: Size<i32, Physical>,
    rectangle: Option<Rectangle<i32, Physical>>,
) -> Option<Rectangle<i32, Physical>> {
    let usable = |region: &Rectangle<i32, Physical>| region.size.w > 0 && region.size.h > 0;
    let on = rectangle.and_then(|rectangle| rectangle.intersection(Rectangle::from_size(size)));
    if let Some(on) = on.filter(usable) {
        return Some(on);
    }
    if rectangle.is_some() {
        tracing::warn!(
            ?rectangle,
            ?size,
            "the dock's rectangle is off the output: the probe blurs the default region"
        );
    }
    let (width, height) = (1280.min(size.w), 96.min(size.h));
    Some(Rectangle::new(
        ((size.w - width) / 2, size.h - height).into(),
        (width, height).into(),
    ))
    .filter(usable)
}

/// The probe: off screen, in a GL context of its own, on the probe's thread.
pub mod probe {
    use super::Probed;
    use crate::Error;
    use smithay::backend::allocator::gbm::GbmDevice;
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::drm::{DrmDeviceFd, DrmNode};
    use smithay::backend::egl::fence::EGLFence;
    use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
    use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
    use smithay::backend::renderer::multigpu::{ApiDevice, GpuManager};
    use smithay::backend::renderer::sync::SyncPoint;
    use smithay::backend::renderer::{Bind, Color32F, Frame, ImportMem, Offscreen, Renderer};
    use smithay::utils::{Buffer, DeviceFd, Physical, Rectangle, Size, Transform};
    use std::fs::OpenOptions;
    use std::os::fd::OwnedFd;
    use std::path::Path;
    use std::time::{Duration, Instant};

    const FORMAT: Fourcc = Fourcc::Abgr8888;
    const WARM_UP: usize = 30;
    const ROUNDS: usize = 60;

    /// What a round draws, in the output's physical pixels.
    #[derive(Debug)]
    pub struct Scene {
        pub size: Size<i32, Physical>,
        pub windows: Vec<Rectangle<i32, Physical>>,
        pub dock: Rectangle<i32, Physical>,
    }

    /// Draws `WARM_UP` rounds and then `ROUNDS` measured ones of the three cases, taking turns,
    /// each timed from its first GL call to its sync point, and gives each case's 95th
    /// percentile; at `deadline` it gives up, a wait included.
    pub fn run(path: &Path, scene: &Scene, deadline: Instant) -> Result<Probed, Error> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let fd = DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(file)));
        let node = DrmNode::from_file(&fd)?;
        let mut api = GbmGlesBackend::<GlesRenderer, DrmDeviceFd>::default();
        api.add_node(node, GbmDevice::new(fd)?)?;
        let mut manager = GpuManager::new(api)?;
        let mut devices = manager.devices_mut()?;
        let renderer = devices
            .find(|device| device.node() == &node)
            .map(ApiDevice::renderer_mut)
            .ok_or("no renderer for the render node")?;

        let size = scene.size;
        let wallpaper = pattern(renderer, size.w, size.h, 255, 0)?;
        let window = pattern(renderer, 256, 256, 235, 1)?;
        let mut target: GlesTexture =
            Offscreen::<GlesTexture>::create_buffer(renderer, FORMAT, (size.w, size.h).into())?;
        let mut dock = crate::blur::Blur::new(renderer, scene.dock.size)?;
        let mut full = crate::blur::Blur::new(renderer, size)?;
        let whole = Rectangle::from_size(size);

        let mut times = [Vec::new(), Vec::new(), Vec::new()];
        for round in 0..WARM_UP + ROUNDS {
            for (case, times) in times.iter_mut().enumerate() {
                if Instant::now() > deadline {
                    return Err("the probe is past its deadline".into());
                }
                let drawn = Instant::now();
                let mut sync = draw(renderer, &mut target, scene, &wallpaper, &window, round)?;
                if case == 1 {
                    sync = dock.blur(renderer, &mut target, scene.dock)?;
                } else if case == 2 {
                    sync = full.blur(renderer, &mut target, whole)?;
                }
                wait(&sync, deadline)?;
                if round >= WARM_UP {
                    times.push(drawn.elapsed());
                }
            }
        }
        let [scene, dock, full] = times.map(|mut times| p95(&mut times));
        Ok(Probed { scene, dock, full })
    }

    /// Waits for `sync` until `deadline` at most. A frame of the GLES renderer ends on an EGL
    /// fence, or with glFinish and no fence.
    fn wait(sync: &SyncPoint, deadline: Instant) -> Result<(), Error> {
        let reached = match sync.get::<EGLFence>() {
            Some(fence) => fence.client_wait(
                Some(deadline.saturating_duration_since(Instant::now())),
                false,
            )?,
            None => sync.is_reached(),
        };
        if reached {
            Ok(())
        } else {
            Err("the probe's frame did not finish by its deadline".into())
        }
    }

    /// The 95th percentile of `times`.
    fn p95(times: &mut [Duration]) -> Duration {
        times.sort_unstable();
        let at = times.len() * 95 / 100;
        times
            .get(at.min(times.len().saturating_sub(1)))
            .copied()
            .unwrap_or(Duration::MAX)
    }

    /// A texture of `width` by `height` with a pattern a blur cannot skip, at `alpha`,
    /// premultiplied.
    fn pattern(
        renderer: &mut GlesRenderer,
        width: i32,
        height: i32,
        alpha: u8,
        seed: u32,
    ) -> Result<GlesTexture, Error> {
        let (w, h) = (width.max(1).unsigned_abs(), height.max(1).unsigned_abs());
        let mut data = Vec::with_capacity(usize::try_from(w * h * 4)?);
        for y in 0..h {
            for x in 0..w {
                let v = x
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(y.wrapping_mul(40_503))
                    .wrapping_add(seed.wrapping_mul(97));
                let a = u32::from(alpha);
                for channel in [v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff] {
                    data.push(u8::try_from(channel * a / 255)?);
                }
                data.push(alpha);
            }
        }
        let size = Size::<i32, Buffer>::from((i32::try_from(w)?, i32::try_from(h)?));
        Ok(renderer.import_memory(&data, FORMAT, size, false)?)
    }

    /// The scene, whole: the wallpaper and the output's windows, moved by `round` so that no
    /// round repeats the last.
    fn draw(
        renderer: &mut GlesRenderer,
        target: &mut GlesTexture,
        scene: &Scene,
        wallpaper: &GlesTexture,
        window: &GlesTexture,
        round: usize,
    ) -> Result<SyncPoint, Error> {
        let size = scene.size;
        let mut fb = renderer.bind(target)?;
        let mut frame = renderer.render(&mut fb, size, Transform::Normal)?;
        let all = [Rectangle::from_size(size)];
        frame.clear(Color32F::new(0.15, 0.16, 0.16, 1.0), &all)?;
        let source =
            Rectangle::<f64, Buffer>::from_size((f64::from(size.w), f64::from(size.h)).into());
        frame.render_texture_from_to(
            wallpaper,
            source,
            all[0],
            &all,
            &all,
            Transform::Normal,
            1.0,
            None,
            &[],
        )?;
        let shift = i32::try_from(round % 8)?;
        for placed in &scene.windows {
            let mut placed = *placed;
            placed.loc.x += shift;
            frame.render_texture_from_to(
                window,
                Rectangle::from_size((256.0, 256.0).into()),
                placed,
                &[Rectangle::from_size(placed.size)],
                &[],
                Transform::Normal,
                1.0,
                None,
                &[],
            )?;
        }
        Ok(frame.finish()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT_60: Duration = Duration::from_micros(16_667);
    const AT_120: Duration = Duration::from_micros(8_333);
    const EPSILON: Duration = Duration::from_micros(10);

    fn probed(scene: Duration, dock: Duration, full: Duration) -> Option<Probed> {
        Some(Probed { scene, dock, full })
    }

    /// VL13's thresholds at 60 and 120 Hz, on either side of T/4 and T/2.
    #[test]
    fn vl13_the_thresholds_at_60_and_120_hz() {
        for interval in [AT_60, AT_120] {
            let (quarter, half) = (interval / 4, interval / 2);
            let fast = Duration::from_micros(100);
            assert_eq!(tier(probed(fast, quarter, half), interval), Tier::M2);
            assert_eq!(
                tier(probed(fast, quarter + EPSILON, half), interval),
                Tier::M1
            );
            assert_eq!(
                tier(probed(fast, quarter, half + EPSILON), interval),
                Tier::M1
            );
            assert_eq!(tier(probed(half, half, half), interval), Tier::M1);
            assert_eq!(tier(probed(half + EPSILON, half, half), interval), Tier::M0);
        }
        // The UHD 620 of spike VL-S5 at 1920 x 1080: M2 at 60 Hz, M1 at 120 Hz.
        let uhd = probed(
            Duration::from_micros(3_000),
            Duration::from_micros(3_600),
            Duration::from_micros(7_660),
        );
        assert_eq!(tier(uhd, AT_60), Tier::M2);
        assert_eq!(tier(uhd, AT_120), Tier::M1);
    }

    /// The probe's dock region: the dock's rectangle, clipped to the output; the default
    /// region without a dock or when its rectangle lies off the output, as after a mode change
    /// from 2560 x 1440 to 1920 x 1080, or has no area; none on an output without area.
    #[test]
    fn vl13_a_dock_rectangle_off_the_output_gives_the_default_region() {
        let size = Size::<i32, Physical>::from((1920, 1080));
        let at = |x, y, w, h| {
            Some(Rectangle::<i32, Physical>::new(
                (x, y).into(),
                (w, h).into(),
            ))
        };
        let default = at(320, 984, 1280, 96);
        assert_eq!(dock_region(size, None), default, "no dock");
        assert_eq!(
            dock_region(size, at(0, 1016, 1920, 64)),
            at(0, 1016, 1920, 64)
        );
        assert_eq!(
            dock_region(size, at(1800, 1016, 1920, 64)),
            at(1800, 1016, 120, 64),
            "clipped"
        );
        assert_eq!(
            dock_region(size, at(0, 1376, 2560, 64)),
            default,
            "the old mode's"
        );
        assert_eq!(dock_region(size, at(0, 1016, 0, 64)), default, "no area");
        assert_eq!(
            dock_region(size, at(i32::MAX, i32::MAX, i32::MAX, i32::MAX)),
            default,
            "far off"
        );
        assert_eq!(
            dock_region(Size::from((0, 0)), None),
            None,
            "an output without area"
        );
    }

    /// Software rendering is M0, without a measurement.
    #[test]
    fn vl13_software_rendering_is_m0() {
        assert_eq!(tier(None, AT_60), Tier::M0);
    }

    fn probing() -> (Measure, Instant) {
        let deadline = Instant::now() + DEADLINE;
        (
            Measure::Probing {
                generation: 7,
                deadline,
            },
            deadline,
        )
    }

    fn quick() -> Result<Probed, String> {
        let fast = Duration::from_micros(100);
        Ok(Probed {
            scene: fast,
            dock: fast,
            full: fast,
        })
    }

    /// An output is M0 until its probe has run; a probe that fails, that finishes past its
    /// deadline or that has not finished by it gives M0; the result of an older probe changes
    /// nothing.
    #[test]
    fn vl13_a_probe_that_fails_or_misses_its_deadline_is_m0() {
        let (mut measure, deadline) = probing();
        assert_eq!(measure.tier(), Tier::M0, "until the probe has run");
        measure.probed(6, quick(), deadline, AT_60);
        assert_eq!(measure.tier(), Tier::M0, "an older probe");
        measure.probed(7, quick(), deadline, AT_60);
        assert_eq!(measure.tier(), Tier::M2, "in time");

        let (mut measure, deadline) = probing();
        measure.probed(7, quick(), deadline + EPSILON, AT_60);
        assert_eq!(measure.tier(), Tier::M0, "past its deadline");

        let (mut measure, deadline) = probing();
        measure.probed(7, Err("no renderer".into()), deadline, AT_60);
        assert_eq!(measure.tier(), Tier::M0, "failed");

        let (mut measure, deadline) = probing();
        measure.expired(7);
        measure.probed(7, quick(), deadline, AT_60);
        assert_eq!(measure.tier(), Tier::M0, "not finished by its deadline");
    }

    /// A live median above T/2 over the last 120 damaged frames moves an M2 output to M1,
    /// which stays until a new probe; a median at T/2 does not.
    #[test]
    fn vl13_a_slow_live_median_moves_m2_to_m1_until_the_next_probe() {
        let half = AT_60 / 2;
        let mut measure = Measure::settled(Tier::M2);
        for _ in 0..LIVE_FRAMES * 2 {
            measure.frame(half, AT_60);
        }
        assert_eq!(measure.tier(), Tier::M2, "a median at T/2");
        for _ in 0..LIVE_FRAMES / 2 - 1 {
            measure.frame(half + EPSILON, AT_60);
        }
        assert_eq!(
            measure.tier(),
            Tier::M2,
            "fewer than half the window over T/2"
        );
        measure.frame(half + EPSILON, AT_60);
        assert_eq!(measure.tier(), Tier::M1, "the median over T/2");
        for _ in 0..LIVE_FRAMES * 2 {
            measure.frame(Duration::from_micros(100), AT_60);
        }
        assert_eq!(measure.tier(), Tier::M1, "M1 sticks");

        let mut measure = Measure::settled(Tier::M1);
        for _ in 0..LIVE_FRAMES {
            measure.frame(AT_60, AT_60);
        }
        assert_eq!(measure.tier(), Tier::M1, "the live check runs in M2 only");
    }

    /// No code path reads the GPU's vendor or name (VL13): the tier is decided by the measured
    /// frame time, not by the GPU's brand.
    #[test]
    fn vl13_no_code_reads_the_gpu_vendor_or_name() {
        let sources = [include_str!("material.rs"), include_str!("blur.rs")];
        let forbidden = [
            concat!("GL_", "VENDOR"),
            concat!("GL_", "RENDERER"),
            concat!("vendor", "_id"),
            concat!("driver", "_name"),
            concat!("EGL_", "VENDOR"),
            concat!("glGet", "String"),
        ];
        for source in sources {
            for word in forbidden {
                assert!(!source.contains(word), "{word} is read");
            }
        }
    }
}
