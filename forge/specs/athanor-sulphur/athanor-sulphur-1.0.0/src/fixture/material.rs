//! Decision 35 and VL13: each output's material tier, athanor-material-v1 (`material.rs`). The
//! grant, software rendering, and on a render node the probe; the thresholds, the deadline and
//! the live check are `material.rs`'s unit tests. On a GPU that is not there, whose probe the
//! tests stand in for, the probes' order on the GPU, a mode change, the deadline and an output
//! no longer shown.

use super::*;
use crate::material::probe::Scene;
use crate::material::{Fake, Probed, Tier};
use athanor_material::athanor_material_manager_v1::AthanorMaterialManagerV1;
use athanor_material::athanor_material_v1::AthanorMaterialV1;
use smithay::output::Output;

const MANAGER: &str = "athanor_material_manager_v1";

impl Local {
    /// The tier object of the first output, from a new manager.
    fn tier(&mut self, run: &mut Running) -> AthanorMaterialV1 {
        let manager: AthanorMaterialManagerV1 = self.bind(1);
        let output: wl_output::WlOutput = self.bind(4);
        let qh = self.queue.handle();
        let object = manager.get_tier(&output, &qh, ());
        self.roundtrip(run);
        object
    }

    /// The tiers `object` was sent, in order.
    fn tiers_of(&self, object: &AthanorMaterialV1) -> Vec<u32> {
        self.client
            .tiers
            .iter()
            .filter(|(id, _)| *id == object.id())
            .map(|(_, tier)| *tier)
            .collect()
    }
}

/// `athanor_material_manager_v1` is privileged (OS4, decision 35): the bar and the control
/// center, granted it on their lines, see it; the bar's identity inside a security context
/// and a client with no line do not.
#[test]
fn d35_only_the_shell_surfaces_see_the_material() {
    let mut run = Running::start(None);
    assert!(Local::connect(&mut run, None).sees(MANAGER), "the bar");
    assert!(
        Local::control_center(&mut run).sees(MANAGER),
        "the control center"
    );
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(MANAGER), "the bar inside a context");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    run.stop();
}

/// Software rendering is M0 without a measurement: under Pixman the tier is sent at once, it
/// is M0, it stays M0, and no probe is started.
#[test]
fn d35_software_rendering_is_m0_without_a_probe() {
    let mut run = Running::start(None);
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    assert_eq!(shell.tiers_of(&object), [0], "sent at once");
    shell.frames(&mut run, 5);
    assert_eq!(shell.tiers_of(&object), [0], "it stays M0");
    assert_eq!(run.state.materials.probes(), 0, "no probe");
    run.stop();
}

/// On a GPU an output is M0 until its probe has run, off the event loop; the probes of the
/// two outputs run one after the other, each by its deadline:
/// `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn d35_the_probe_runs_on_a_render_node() {
    let node = PathBuf::from(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    );
    let mut run = Running::start(Some(&node));
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    let started = Instant::now();
    assert_eq!(
        shell.tiers_of(&object)[..1],
        [0],
        "M0 until the probe has run"
    );
    assert_eq!(
        run.state.materials.probes(),
        1,
        "one probe at a time on the GPU"
    );
    shell.pump(&mut run, "both probes", |_, state| {
        state.materials.probes() == 2 && !state.materials.probing()
    });
    shell.frames(&mut run, 3);
    let tiers = shell.tiers_of(&object);
    eprintln!("tiers {tiers:?} after {:?}", started.elapsed());
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "by the deadline"
    );
    assert!(tiers.iter().all(|tier| *tier <= 2));
    run.stop();
}

/// The longest a probe may take in the tests below that do not test the deadline.
const LONG: Duration = Duration::from_secs(5);

/// Probe times that give `tier`.
fn times(tier: Tier) -> Probed {
    let (fast, slow) = (Duration::ZERO, Duration::from_secs(1));
    let (scene, dock, full) = match tier {
        Tier::M2 => (fast, fast, fast),
        Tier::M1 => (fast, slow, slow),
        Tier::M0 => (slow, slow, slow),
    };
    Probed { scene, dock, full }
}

/// Puts the outputs on a GPU that is not there, whose probe is `prober` and whose probes'
/// deadline is `deadline`, and measures them again.
fn on_a_fake_gpu(
    run: &mut Running,
    deadline: Duration,
    prober: impl Fn(&Path, &Scene, Instant) -> Result<Probed, String> + Send + Sync + 'static,
) {
    run.state.materials.fake = Some(Fake {
        gpu: PathBuf::from("/dev/dri/renderD-fake"),
        prober: Arc::new(prober),
        deadline,
    });
    run.state.refresh_materials();
}

/// The `n`th output.
fn output(run: &Running, n: usize) -> Output {
    run.state
        .space
        .outputs()
        .nth(n)
        .cloned()
        .expect("an output")
}

/// A probe that holds until the test lets it go, by its own deadline at most.
fn held() -> (mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>) {
    let (release, released) = mpsc::channel();
    (release, Mutex::new(released))
}

fn wait(released: &Mutex<mpsc::Receiver<()>>) -> Result<(), String> {
    released
        .lock()
        .map_err(|_| "poisoned")?
        .recv_timeout(LONG)
        .map_err(|error| error.to_string())
}

/// The probes of a GPU run one at a time, and each result is sent to the tier objects of its
/// output.
#[test]
fn d35_the_probes_of_a_gpu_run_one_at_a_time() {
    let mut run = Running::start(None);
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    let (running, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (now, highest) = (running.clone(), most.clone());
    on_a_fake_gpu(&mut run, LONG, move |_, _, _| {
        let at_once = now.fetch_add(1, Ordering::SeqCst) + 1;
        highest.fetch_max(at_once, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        now.fetch_sub(1, Ordering::SeqCst);
        Ok(times(Tier::M2))
    });
    shell.pump(&mut run, "both probes", |_, state| {
        state.materials.probes() == 2 && !state.materials.probing()
    });
    shell.roundtrip(&mut run);
    assert_eq!(most.load(Ordering::SeqCst), 1, "one probe at a time");
    assert_eq!(shell.tiers_of(&object), [0, 2], "the result is sent");
    let second = output(&run, 1);
    assert_eq!(run.state.materials.tier_of(Some(&second)), Tier::M2);
    run.stop();
}

/// A mode change measures the output again: the probe that runs at the change is superseded,
/// its result sent to no one, and the new probe, after it on the GPU, gives the tier.
#[test]
fn d35_a_mode_change_supersedes_the_running_probe() {
    let mut run = Running::start(None);
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let (release, released) = held();
    on_a_fake_gpu(&mut run, LONG, move |_, _, _| {
        // The first is the first output's probe at its old mode.
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            wait(&released)?;
            return Ok(times(Tier::M2));
        }
        Ok(times(Tier::M1))
    });
    assert_eq!(
        run.state.materials.probes(),
        1,
        "the first output's probe runs"
    );
    let first = output(&run, 0);
    let mode = smithay::output::Mode {
        size: (1024, 768).into(),
        refresh: 60_000,
    };
    first.change_current_state(Some(mode), None, None, None);
    run.state.arrange_outputs();
    run.state.refresh_materials();
    release.send(()).expect("the probe holds");
    shell.pump(&mut run, "three probes", |_, state| {
        state.materials.probes() == 3 && !state.materials.probing()
    });
    shell.roundtrip(&mut run);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        shell.tiers_of(&object),
        [0, 1],
        "the old mode's M2 is not sent"
    );
    run.stop();
}

/// A probe not finished by its deadline is M0 by the deadline's timer, while its thread still
/// runs; its late result is sent to no one, and the next probe on the GPU, in time, gives its
/// output's tier.
#[test]
fn d35_a_probe_not_finished_by_its_deadline_is_m0() {
    let mut run = Running::start(None);
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    let (release, released) = held();
    on_a_fake_gpu(&mut run, Duration::from_millis(100), move |_, _, _| {
        wait(&released)?;
        Ok(times(Tier::M2))
    });
    let (first, second) = (output(&run, 0), output(&run, 1));
    run.until("the deadline", |state| state.materials.settled(&first));
    assert!(run.state.materials.probing(), "the late probe still runs");
    assert!(!run.state.materials.settled(&second), "the second waits");
    assert_eq!(run.state.materials.probes(), 1, "its probe not started yet");
    release.send(()).expect("the first probe holds");
    release.send(()).expect("for the second");
    shell.pump(&mut run, "both probes", |_, state| {
        state.materials.probes() == 2 && !state.materials.probing()
    });
    shell.roundtrip(&mut run);
    assert_eq!(shell.tiers_of(&object), [0], "the late result is not sent");
    assert_eq!(run.state.materials.tier_of(Some(&first)), Tier::M0);
    assert_eq!(
        run.state.materials.tier_of(Some(&second)),
        Tier::M2,
        "in time"
    );
    run.stop();
}

/// An output no longer shown is M0, and its objects are told.
#[test]
fn d35_an_output_no_longer_shown_is_m0() {
    let mut run = Running::start(None);
    let mut shell = Local::connect(&mut run, None);
    let object = shell.tier(&mut run);
    on_a_fake_gpu(&mut run, LONG, |_, _, _| Ok(times(Tier::M2)));
    shell.pump(&mut run, "both probes", |_, state| {
        state.materials.probes() == 2 && !state.materials.probing()
    });
    shell.roundtrip(&mut run);
    assert_eq!(shell.tiers_of(&object), [0, 2]);
    let first = output(&run, 0);
    run.state.output_management.disabled.push(first);
    run.state.refresh_materials();
    shell.roundtrip(&mut run);
    assert_eq!(shell.tiers_of(&object), [0, 2, 0]);
    run.stop();
}
