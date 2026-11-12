//! Decision 35 and VL13: each output's material tier, athanor-material-v1 (`material.rs`). The
//! grant, software rendering, and on a render node the probe; the thresholds, the deadline and
//! the live check are `material.rs`'s unit tests.

use super::*;
use athanor_material::athanor_material_manager_v1::AthanorMaterialManagerV1;
use athanor_material::athanor_material_v1::AthanorMaterialV1;

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

/// On a GPU an output is M0 until its probe has run, which finishes by its deadline, off the
/// event loop: `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.
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
    assert_eq!(run.state.materials.probes(), 2, "a probe for each output");
    let id = object.id();
    shell.pump(&mut run, "the probe's tier", |client, state| {
        !state.materials.probing() && client.tiers.iter().filter(|(of, _)| *of == id).count() >= 1
    });
    shell.frames(&mut run, 3);
    let tiers = shell.tiers_of(&object);
    eprintln!("tiers {tiers:?} after {:?}", started.elapsed());
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "by the deadline"
    );
    assert!(tiers.iter().all(|tier| *tier <= 2));
    run.stop();
}
