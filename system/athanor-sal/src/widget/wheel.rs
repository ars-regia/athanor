//! The wheel over a widget where no scroll or text area takes the turn: what GTK's scroll
//! controller with discrete steps gives a status notifier item of the bar's tray. The middle
//! button is [`Event::MiddleClick`], in the widget module.

use super::{Event, Ui};

/// One step of the wheel, in `value120`'s 120ths.
const STEP: i32 = 120;

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
    /// the point, in logical pixels, where no scroll took it. The turns add up on each axis
    /// over the focusable widget under the point, as GTK's scroll controller with discrete
    /// steps does: [`Event::Wheel`] gives the widget the whole steps they make and keeps the
    /// rest for the next turn, until the pointer leaves the widget or the surface. `None` for
    /// no turn, for no widget there, and while the turns make no whole step.
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dx: i32, dy: i32) -> Option<Event> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let Some(widget) = self.hit(x, y) else {
            self.wheel = None;
            return None;
        };
        let mut rest = match self.wheel {
            Some((over, rest)) if over == widget => rest,
            _ => [0, 0],
        };
        rest[0] = rest[0].saturating_add(dx);
        rest[1] = rest[1].saturating_add(dy);
        self.wheel = Some((widget, rest.map(|r| r % STEP)));
        let [dx, dy] = rest.map(|r| r / STEP);
        (dx != 0 || dy != 0).then_some(Event::Wheel { widget, dx, dy })
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
            dy: -1,
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

    #[test]
    fn a_high_resolution_wheel_adds_its_turns_up_to_whole_steps_on_each_axis() {
        let mut ui = Ui::new("Tray", Theme::default(), false);
        let item = ui.add(Ui::ROOT, Kind::Button("Item".into()));
        let other = ui.add(Ui::ROOT, Kind::Button("Other".into()));
        ui.layout();
        let at = |ui: &Ui, id| (ui.widget(id).rect.x + 1.0, ui.widget(id).rect.y + 1.0);
        let (x, y) = at(&ui, item);
        let step = |dx, dy| Some(Event::Wheel { widget: item, dx, dy });
        for _ in 0..3 {
            assert_eq!(ui.pointer_wheel(x, y, 0, 30), None, "a quarter of a notch");
        }
        assert_eq!(ui.pointer_wheel(x, y, 0, 30), step(0, 1), "one notch, one step");
        assert_eq!(ui.pointer_wheel(x, y, 90, 0), None);
        assert_eq!(ui.pointer_wheel(x, y, 270, -30), step(3, 0), "each axis keeps its own");
        assert_eq!(ui.pointer_wheel(x, y, 0, -90), step(0, -1));
        ui.pointer_wheel(x, y, 0, 90);
        let (ox, oy) = at(&ui, other);
        ui.pointer_motion(ox, oy);
        ui.pointer_motion(x, y);
        assert_eq!(ui.pointer_wheel(x, y, 0, 30), None, "the rest went with the pointer");
        ui.pointer_wheel(x, y, 0, 60);
        ui.pointer_leave();
        assert_eq!(ui.pointer_wheel(x, y, 0, 30), None, "and with the surface");
    }
}
