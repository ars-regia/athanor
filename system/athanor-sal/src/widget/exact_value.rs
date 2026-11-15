//! A slider's value set by the application as it is, off the slider's steps: what GTK's
//! `gtk_range_set_value` does, which snaps only what the person moves. A volume read from the
//! sound server, 39.9993896484375 % for one, stays that number for assistive technologies, as
//! it does in a GTK scale, where [`Ui::set_value`] would move it to the step nearest it.

use super::{Event, Kind, Ui, WidgetId};

impl Ui {
    /// Sets the slider `id` to `value`, inside its range but not snapped to its step, as
    /// `gtk_range_set_value` does: for a value the application reads from elsewhere, such as a
    /// volume, that the person did not choose. The keys and the pointer still move the slider to
    /// its steps. `None`, and nothing changes, for a widget that is not a slider, for NaN and for
    /// a value that lands where the slider already is.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_exact_value(&mut self, id: WidgetId, value: f64) -> Option<Event> {
        let Kind::Slider(slider) = &mut self.widgets[id].kind else {
            return None;
        };
        if value.is_nan() {
            return None;
        }
        let value = value.clamp(slider.min, slider.max);
        let moved = value != slider.value;
        slider.value = value;
        moved.then_some(Event::Changed(id))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Kind, Theme, Ui};

    /// The fixtures' output volume, 26214 of PipeWire's 65536, as a percentage.
    const VOLUME: f64 = 39.9993896484375;

    fn volume() -> (Ui, crate::WidgetId) {
        let mut ui = Ui::new("Sound", Theme::default(), false);
        let id = ui.slider(Ui::ROOT, "Output volume", 0.0, 100.0, 1.0, 0.0);
        assert!(id.is_some());
        let id = id.unwrap_or(Ui::ROOT);
        (ui, id)
    }

    #[test]
    fn the_exact_value_stays_off_the_steps_where_set_value_snaps() {
        let (mut ui, id) = volume();
        assert_eq!(ui.set_value(id, VOLUME), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(40.0), "set_value snaps to the step");
        assert_eq!(ui.set_exact_value(id, VOLUME), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(VOLUME));
        ui.layout();
        let node = crate::a11y::node(&ui, id, 1.0);
        assert_eq!(node.numeric_value(), Some(VOLUME));
    }

    #[test]
    fn the_exact_value_stays_in_the_range_and_refuses_nan() {
        let (mut ui, id) = volume();
        assert_eq!(ui.set_exact_value(id, 140.0), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(100.0));
        assert_eq!(ui.set_exact_value(id, -3.0), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(0.0));
        assert_eq!(ui.set_exact_value(id, 0.0), None, "where it already is");
        assert_eq!(ui.set_exact_value(id, f64::NAN), None);
        assert_eq!(ui.value(id), Some(0.0));
        let label = ui.add(Ui::ROOT, Kind::Label("Volume".into()));
        assert_eq!(ui.set_exact_value(label, VOLUME), None);
    }

    #[test]
    fn the_keys_move_an_exact_value_to_the_steps() {
        let (mut ui, id) = volume();
        assert_eq!(ui.set_exact_value(id, VOLUME), Some(Event::Changed(id)));
        assert_eq!(ui.step_value(id, 1), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(41.0));
        assert_eq!(ui.set_exact_value(id, VOLUME), Some(Event::Changed(id)));
        assert_eq!(ui.step_value(id, -1), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(39.0));
    }
}
