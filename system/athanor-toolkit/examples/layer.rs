//! One layer surface on each output, along its bottom edge, with a button that names the output:
//! the rig's check of the driver's layer surfaces as outputs come, go and change scale (the dock
//! plan, task 3). Prints a line when a button is pressed.

use athanor_toolkit::driver::{
    self, Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    SurfaceId,
};
use athanor_toolkit::{Event, Kind, Theme, Ui};
use std::process::ExitCode;

struct Bars;

impl Application for Bars {
    fn output(&mut self, output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        let name = output.name.clone().unwrap_or_else(|| output.id.to_string());
        let mut ui = Ui::new(&name, Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Button(name));
        let options = LayerOptions {
            layer: Layer::Top,
            namespace: "athanor-toolkit-layer".into(),
            anchor: Anchor::BOTTOM,
            margin: [0, 0, 8, 0],
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::OnDemand,
            input_region: None,
        };
        Some((ui, options))
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if let (Event::Clicked(_), Some(ui)) = (event, shell.ui(surface)) {
            println!("pressed on {}", ui.title);
        }
    }
}

fn main() -> ExitCode {
    match driver::run(Bars) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-toolkit-layer: {error}");
            ExitCode::FAILURE
        }
    }
}
