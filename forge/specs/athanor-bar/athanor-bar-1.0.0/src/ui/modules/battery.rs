//! The bar's battery (doc_bar.md, BR3), as the GTK bar's `ui/battery.rs` was: a button with the
//! battery's icon and its charge, the time left in its description, and a popover with
//! athanor-controls' battery page. Without a present battery the module is hidden (SH1), as
//! without the runtime of the models.

use athanor_bar::i18n::tr;
use athanor_controls::bridge;
use athanor_controls::sal::battery::{note, percent_text, Page};
use athanor_controls::sal::Turn;
use athanor_sal::{Event, Ui, WidgetId};
use athanor_services::battery::{self, Battery};

use super::{controls, Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct BatteryModule {
    page: Page,
    /// What the buttons show, as last drawn; `None` while hidden.
    drawn: Option<Face>,
}

/// A button's icon, its charge and its description, the charge with the time left.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Face {
    icon: &'static str,
    charge: String,
    description: String,
}

impl Face {
    fn of(battery: &Battery) -> Face {
        let charge = percent_text(battery.percent);
        let description = match note(battery) {
            Some(note) => format!("{charge}, {note}"),
            None => charge.clone(),
        };
        Face {
            icon: battery::icon(battery),
            charge,
            description,
        }
    }
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let Some(services) = bridge::services() else {
        tracing::warn!("no runtime for the models; the battery module is hidden");
        return None;
    };
    // The page first: its follower of the model is woken before the bar's.
    let page = Page::new(&services);
    bridge::follow(services.battery.0.clone(), controls::waker(start.wake));
    Some(Box::new(BatteryModule { page, drawn: None }))
}

fn face(page: &Page) -> Option<Face> {
    page.state().battery.as_ref().map(Face::of)
}

impl Module for BatteryModule {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        self.drawn = face(&self.page);
        let face = self.drawn.as_ref()?;
        let image = cx.icons.image(face.icon, ICON, cx.scale);
        let button =
            ui.labelled_button(group, &tr("Battery"), image, f32::from(ICON), &face.charge);
        ui.set_tooltip(button, Some(&face.description));
        ui.set_description(button, Some(&face.description));
        Some(button)
    }

    fn stale(&self, _cx: &Cx) -> bool {
        face(&self.page) != self.drawn
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let (mut ui, frame) =
            look::popover(&tr("Battery"), controls::theme(cx.theme), cx.rtl, cx.top);
        self.page
            .build(&mut ui, frame, &controls::look(cx.icons, cx.scale, cx.rtl, cx.secondary));
        // The brightness keys change the backlight without a signal: read it again.
        self.page.opened(&mut ui);
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        self.page.event(ui, &event);
        Reply::Stay
    }

    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        if face(&self.page).is_none() {
            // The battery left: the popover closes with its button.
            return Reply::Close;
        }
        let size = ui.size();
        match self.page.turn(ui) {
            // The power modes or the brightness showed or hid: the popover keeps its size once
            // open, so it is built again at the new one.
            Turn::Updated if ui.layout() != size => Reply::Reopen,
            Turn::Rebuild => Reply::Reopen,
            Turn::Updated | Turn::Unchanged => Reply::Stay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_services::battery::Charge;

    #[test]
    fn the_description_adds_the_time_left_to_the_charge() {
        let mut battery = Battery {
            percent: 72.0,
            charge: Charge::Discharging,
            seconds: Some(3 * 3600 + 25 * 60),
        };
        let face = Face::of(&battery);
        assert_eq!(face.charge, "72 %");
        assert_eq!(face.description, "72 %, 3 h 25 min left");
        battery.seconds = None;
        assert_eq!(Face::of(&battery).description, "72 %");
    }
}
