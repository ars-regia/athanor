//! One layer surface on each output, along its bottom edge, with a button that names the output:
//! the rig's check of the driver's layer surfaces as outputs come, go and change scale (the dock
//! plan, task 3). Prints a line when a button is pressed.
//!
//! With `--extra <output>`, once that output is announced, two more layer surfaces open on it by
//! its name, `athanor-sal-overlay` on the background layer and `athanor-sal-closed` on the top
//! one. The first moves to the overlay layer after its first frame; before the move, a
//! `set_layer` with widgets too wide to lay out, a margin of 40 at the bottom and an input
//! region of (1, 2) 3 by 4 is refused, printing `refused <namespace>`. The second closes in the
//! turn its first frame is drawn, before the driver reports it, printing
//! `closed athanor-sal-closed before its drawn`; a surface reported drawn once closed ends the
//! client. Prints `closed <namespace>` for each surface closed without the application asking.
//!
//! With `--motion`, one surface on the first output instead, which moves on its frame callbacks
//! (Sal's requirement S3, doc_osd.md OD10; `toolkit_motion_check.py`): once its first frame is
//! drawn it enters, from transparent and 12 px down, in 200 ms; then leaves, back to transparent
//! and 12 px down, in 133 ms; then enters again with reduced motion, a fade of 100 ms in place.
//! Prints `entered in <ms>`, `left in <ms>` and `entered reduced in <ms>` as each motion
//! settles, timed from when it was asked for. One binary with the layer check's, so that the
//! rig's release build links no more examples at once than its memory holds.

use athanor_sal::driver::{
    self, Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    SurfaceId,
};
use athanor_sal::{Curve, Event, Kind, Motion, Pose, Rect, Theme, Ui};
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// The namespace of the bars.
const BAR: &str = "athanor-sal-layer";
/// The namespace of the surface that moves to the overlay layer.
const OVERLAY: &str = "athanor-sal-overlay";
/// The namespace of the surface the application closes.
const CLOSED: &str = "athanor-sal-closed";
/// The namespace of the surface of `--motion`.
const MOTION: &str = "athanor-sal-motion";
/// A width no buffer holds: `wl_shm` sizes are 16-bit.
const TOO_WIDE: f32 = 100_000.0;

struct Bars {
    /// The output of `--extra`, until its surfaces open.
    extra: Option<String>,
    /// The surfaces of `--extra`, with their namespace.
    extras: Vec<(SurfaceId, &'static str)>,
    /// Whether the driver reported the first frame of the surface the application closes.
    closed_drawn: bool,
}

fn button(name: &str) -> Ui {
    let mut ui = Ui::new(name, Theme::default(), false);
    ui.add(Ui::ROOT, Kind::Button(name.to_owned()));
    ui
}

fn options(layer: Layer, namespace: &str) -> LayerOptions {
    let mut options = LayerOptions::default();
    options.layer = layer;
    options.namespace = namespace.into();
    options.anchor = Anchor::BOTTOM;
    options.margin = [0, 0, 8, 0];
    options.keyboard = KeyboardInteractivity::OnDemand;
    options
}

impl Application for Bars {
    fn output(&mut self, output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        let name = output.name.clone().unwrap_or_else(|| output.id.to_string());
        Some((button(&name), options(Layer::Top, BAR)))
    }

    fn output_changed(&mut self, shell: &mut Shell, _: SurfaceId, output: &OutputInfo) {
        if output.name.is_none() || output.name != self.extra {
            return;
        }
        let name = self.extra.take();
        for (namespace, layer) in [(OVERLAY, Layer::Background), (CLOSED, Layer::Top)] {
            match shell.open_layer(
                button(namespace),
                options(layer, namespace),
                name.as_deref(),
            ) {
                Ok(id) => self.extras.push((id, namespace)),
                Err(error) => shell.fail(error),
            }
        }
    }

    fn turn(&mut self, shell: &mut Shell) {
        // The surface's first frame was drawn in this turn's events; the driver reports it after
        // the turn, and must not once it is closed.
        let closing = self.extras.iter().find(|(id, n)| *n == CLOSED && shell.drawn(*id));
        let Some(&(id, _)) = closing else {
            return;
        };
        shell.close(id);
        if !self.closed_drawn {
            println!("closed {CLOSED} before its drawn");
        }
    }

    fn drawn(&mut self, shell: &mut Shell, surface: SurfaceId) {
        if shell.ui(surface).is_none() {
            return shell.fail(format!("{surface:?} reported drawn once closed").into());
        }
        let Some(&(_, namespace)) = self.extras.iter().find(|(id, _)| *id == surface) else {
            return;
        };
        if namespace == CLOSED {
            self.closed_drawn = true;
            return;
        }
        // Refused: the margin and the region it asks for are never sent, and so never applied.
        let mut refused = options(Layer::Overlay, namespace);
        refused.margin = [0, 0, 40, 0];
        refused.input_region = Some(vec![Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        }]);
        let width = shell
            .ui_mut(surface)
            .map(|ui| std::mem::replace(&mut ui.min_width, TOO_WIDE));
        let outcome = shell.set_layer(surface, &refused);
        if let (Some(ui), Some(width)) = (shell.ui_mut(surface), width) {
            ui.min_width = width;
        }
        if outcome.is_ok() {
            return shell.fail("set_layer took widgets too wide to lay out".into());
        }
        println!("refused {namespace}");
        if let Err(error) = shell.set_layer(surface, &options(Layer::Overlay, namespace)) {
            shell.fail(error);
        }
    }

    fn closed(&mut self, _: &mut Shell, surface: SurfaceId) {
        let extra = self.extras.iter().find(|(id, _)| *id == surface);
        println!("closed {}", extra.map_or(BAR, |(_, namespace)| namespace));
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if let (Event::Clicked(_), Some(ui)) = (event, shell.ui(surface)) {
            println!("pressed on {}", ui.title);
        }
    }
}

/// Hidden: where the surface enters from and leaves to.
const HIDDEN: Pose = Pose {
    opacity: 0.0,
    offset: (0.0, 12.0),
};

/// The motions of the run, in order: what each prints, where it starts and ends, how long it
/// lasts, and whether it runs with reduced motion.
const STEPS: [(&str, Pose, Pose, u64, bool); 3] = [
    ("entered", HIDDEN, Pose::REST, 200, false),
    ("left", Pose::REST, HIDDEN, 133, false),
    ("entered reduced", HIDDEN, Pose::REST, 200, true),
];

#[derive(Default)]
struct Moves {
    /// Whether the surface was asked for.
    opened: bool,
    /// The step under way, and when it was asked for.
    step: Option<(usize, Instant)>,
}

impl Moves {
    fn begin(&mut self, shell: &mut Shell, surface: SurfaceId, step: usize) {
        let Some(&(_, from, to, millis, reduced)) = STEPS.get(step) else {
            return;
        };
        shell.set_reduced_motion(reduced);
        let motion = Motion {
            from,
            to,
            duration: Duration::from_millis(millis),
            curve: Curve([0.2, 0.0, 0.0, 1.0]),
            reduced: Duration::from_millis(100),
        };
        self.step = Some((step, Instant::now()));
        if let Err(error) = shell.animate(surface, motion) {
            shell.fail(error);
        }
    }
}

impl Application for Moves {
    fn output(&mut self, _: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        if std::mem::replace(&mut self.opened, true) {
            return None;
        }
        Some((button(MOTION), options(Layer::Top, MOTION)))
    }

    fn event(&mut self, _: &mut Shell, _: SurfaceId, _: Event) {}

    fn drawn(&mut self, shell: &mut Shell, surface: SurfaceId) {
        if self.step.is_none() {
            self.begin(shell, surface, 0);
        }
    }

    fn settled(&mut self, shell: &mut Shell, surface: SurfaceId) {
        let Some((step, asked)) = self.step else {
            return;
        };
        if shell.pose(surface) != Some(STEPS[step].2) {
            return shell.fail(format!("{} settled away from its end pose", STEPS[step].0).into());
        }
        println!("{} in {}", STEPS[step].0, asked.elapsed().as_millis());
        self.begin(shell, surface, step + 1);
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let extra = match (args.next().as_deref(), args.next(), args.next()) {
        (None, _, _) => None,
        (Some("--extra"), Some(output), None) => Some(output),
        (Some("--motion"), None, _) => return exit(driver::run(Moves::default())),
        _ => {
            eprintln!("usage: athanor-sal-layer [--extra <output> | --motion]");
            return ExitCode::from(2);
        }
    };
    let bars = Bars {
        extra,
        extras: Vec::new(),
        closed_drawn: false,
    };
    exit(driver::run(bars))
}

fn exit(run: Result<(), athanor_sal::Error>) -> ExitCode {
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-layer: {error}");
            ExitCode::FAILURE
        }
    }
}
