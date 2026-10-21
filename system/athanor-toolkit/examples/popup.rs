//! A layer surface along the bottom edge of the first output with one button that has a tooltip
//! and a menu, as a dock's application button: the rig's check of the driver's popups (the dock
//! plan, task 4). The primary button, the secondary button and Shift+F10 open the menu above the
//! button. Prints the rectangles the check aims at, then a line when an item is activated, and
//! one when the menu refuses a popup of its own (Shift+F10 on an item).

use athanor_toolkit::driver::{
    self, Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell, Side,
    SurfaceId,
};
use athanor_toolkit::{Event, Kind, Rect, Theme, Ui};
use std::process::ExitCode;

/// The menu's widgets, laid out, and its first item.
fn menu() -> (Ui, usize) {
    let mut ui = Ui::new("Files", Theme::default(), false);
    let list = ui.add(Ui::ROOT, Kind::Menu);
    let first = ui.add(list, Kind::MenuItem("New Window".into()));
    ui.add(list, Kind::Separator);
    ui.add(list, Kind::MenuItem("Pin to Dock".into()));
    ui.add(list, Kind::MenuItem("Quit".into()));
    ui.layout();
    (ui, first)
}

fn print(name: &str, r: Rect) {
    println!("rect {name} {} {} {} {}", r.x, r.y, r.w, r.h);
}

#[derive(Default)]
struct Dock {
    shown: bool,
}

impl Application for Dock {
    fn output(&mut self, _: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        if std::mem::replace(&mut self.shown, true) {
            return None;
        }
        let mut ui = Ui::new("Dock", Theme::default(), false);
        let button = ui.add(Ui::ROOT, Kind::Button("Files".into()));
        ui.set_has_popup(button, true);
        ui.set_tooltip(button, Some("Files"));
        let (w, h) = ui.layout();
        print(
            "surface",
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        );
        print("button", ui.widget(button).rect);
        let (menu, first) = menu();
        print("item", menu.widget(first).rect);
        let options = LayerOptions {
            layer: Layer::Top,
            namespace: "athanor-toolkit-popup".into(),
            anchor: Anchor::BOTTOM,
            margin: [0; 4],
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::OnDemand,
            input_region: None,
        };
        Some((ui, options))
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        let (Event::Clicked(id) | Event::ContextMenu(id)) = event else {
            return;
        };
        let Some(ui) = shell.ui(surface) else {
            return;
        };
        match &ui.widget(id).kind {
            // A popup of the menu is refused, one popup is open at a time, and the menu stays.
            Kind::MenuItem(_) if matches!(event, Event::ContextMenu(_)) => {
                if let Err(error) = shell.open_popup(surface, id, menu().0, Side::Right) {
                    println!("refused {error}");
                }
            }
            Kind::MenuItem(label) => {
                println!("activated {label}");
                shell.close_popup();
            }
            Kind::Button(_) => {
                if let Err(error) = shell.open_popup(surface, id, menu().0, Side::Top) {
                    eprintln!("athanor-toolkit-popup: {error}");
                }
            }
            _ => {}
        }
    }
}

fn main() -> ExitCode {
    match driver::run(Dock::default()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-toolkit-popup: {error}");
            ExitCode::FAILURE
        }
    }
}
