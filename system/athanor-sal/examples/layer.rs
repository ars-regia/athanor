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

use athanor_sal::driver::{
    self, Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    SurfaceId,
};
use athanor_sal::{Event, Kind, Rect, Theme, Ui};
use std::process::ExitCode;

/// The namespace of the bars.
const BAR: &str = "athanor-sal-layer";
/// The namespace of the surface that moves to the overlay layer.
const OVERLAY: &str = "athanor-sal-overlay";
/// The namespace of the surface the application closes.
const CLOSED: &str = "athanor-sal-closed";
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

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let extra = match (args.next().as_deref(), args.next(), args.next()) {
        (None, _, _) => None,
        (Some("--extra"), Some(output), None) => Some(output),
        _ => {
            eprintln!("usage: athanor-sal-layer [--extra <output>]");
            return ExitCode::from(2);
        }
    };
    let bars = Bars {
        extra,
        extras: Vec::new(),
        closed_drawn: false,
    };
    match driver::run(bars) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-layer: {error}");
            ExitCode::FAILURE
        }
    }
}
