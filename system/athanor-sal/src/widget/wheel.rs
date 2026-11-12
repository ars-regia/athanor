//! The wheel and the middle button on a widget that is not a scroll or a field: what GTK's
//! scroll controller with discrete steps, and a click gesture on the middle button, give a
//! status notifier item of the bar's tray.

use super::{Event, Ui};

/// The wheel's turn on one axis, in 120ths of a step, from the values of a `wl_pointer` axis
/// event: `value120` from a compositor that sends it, or else the deprecated `discrete` count
/// of steps. A touchpad or another continuous source sends neither, and gives none.
// ponytail: continuous scrolling raises no Event::Wheel; accumulate `absolute` into steps, as
// GTK's discrete flag does, once a widget needs the touchpad.
pub fn steps(value120: i32, discrete: i32) -> i32 {
    if value120 != 0 {
        value120
    } else {
        discrete.saturating_mul(120)
    }
}

impl Ui {
    /// The wheel turned by `dx` and `dy` 120ths of a step, right and down when positive, at
    /// the point, in logical pixels, where no scroll took it: [`Event::Wheel`] for the
    /// focusable widget under the point, if any; `None` for no turn.
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dx: i32, dy: i32) -> Option<Event> {
        if dx == 0 && dy == 0 {
            return None;
        }
        self.hit(x, y).map(|widget| Event::Wheel { widget, dx, dy })
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Kind, PointerButton, Theme, Ui};

    #[test]
    fn the_wheel_steps_come_from_value120_or_the_discrete_count() {
        assert_eq!(super::steps(120, 1), 120);
        assert_eq!(super::steps(-60, -1), -60, "a high resolution wheel's half step");
        assert_eq!(super::steps(0, -2), -240);
        assert_eq!(super::steps(0, 0), 0, "a touchpad");
    }

    #[test]
    fn the_wheel_and_the_middle_button_reach_the_widget_under_the_pointer() {
        let mut ui = Ui::new("Tray", Theme::default(), false);
        let button = ui.add(Ui::ROOT, Kind::Button("Item".into()));
        let field = ui.field(Ui::ROOT, "Name", "");
        ui.layout();
        let b = ui.widget(button).rect;
        let (x, y) = (b.x + 1.0, b.y + 1.0);
        let wheel = ui.pointer_wheel(x, y, 0, -120);
        let up = Event::Wheel {
            widget: button,
            dx: 0,
            dy: -120,
        };
        assert_eq!(wheel, Some(up));
        assert_eq!(ui.pointer_wheel(x, y, 0, 0), None);
        assert_eq!(ui.pointer_wheel(-50.0, -50.0, 0, 120), None, "nothing under it");
        let middle = ui.pointer_press(x, y, PointerButton::Middle);
        assert_eq!(middle, Some(Event::MiddleClick(button)));
        assert_eq!(ui.focus(), None, "the middle button does not focus a button");
        let f = ui.widget(field).rect;
        let pasted = ui.pointer_press(f.x + 1.0, f.y + 1.0, PointerButton::Middle);
        assert_eq!(pasted, None, "a field pastes the primary selection instead");
        ui.set_sensitive(button, false);
        assert_eq!(ui.pointer_press(x, y, PointerButton::Middle), None);
        assert_eq!(ui.pointer_wheel(x, y, 120, 0), None);
    }
}
