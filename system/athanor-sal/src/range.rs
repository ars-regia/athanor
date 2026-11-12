//! A slider's range, changed after it was added: a seek bar takes the length of each track.

use crate::widget::{Kind, Slider, Ui, WidgetId};

impl Ui {
    /// Gives slider `id` the range `min` to `max` in steps of `step`, as [`Ui::slider`] takes
    /// them, and moves its value to the step nearest to where it was, inside the new range.
    /// Whether it changed: `false`, and nothing changes, for a widget that is not a slider, for
    /// the range it has, and for numbers [`Ui::slider`] refuses. Raises no event, as a range
    /// set by the application is not the person's; call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_range(&mut self, id: WidgetId, min: f64, max: f64, step: f64) -> bool {
        let Kind::Slider(slider) = &mut self.widgets[id].kind else {
            return false;
        };
        if (slider.min, slider.max, slider.step) == (min, max, step) {
            return false;
        }
        let Some(new) = Slider::new(&slider.name, min, max, step, slider.value) else {
            return false;
        };
        *slider = new;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use crate::widget::{Event, Ui};

    #[test]
    fn a_new_range_keeps_the_value_inside_it_and_raises_nothing() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let Some(seek) = ui.slider(Ui::ROOT, "Seek", 0.0, 1.0, 1.0, 0.0) else {
            unreachable!("a valid range is taken");
        };
        assert!(ui.set_range(seek, 0.0, 240.0, 1.0), "a track 240 s long");
        assert_eq!(ui.value(seek), Some(0.0));
        assert_eq!(ui.set_value(seek, 200.0), Some(Event::Changed(seek)));
        assert!(!ui.set_range(seek, 0.0, 240.0, 1.0), "the same range");
        assert!(ui.set_range(seek, 0.0, 90.0, 1.0), "a shorter track");
        assert_eq!(ui.value(seek), Some(90.0), "held to the new end");
        assert!(ui.set_range(seek, 0.0, 90.0, 0.5));
        assert_eq!(ui.set_value(seek, 10.25), Some(Event::Changed(seek)));
        assert_eq!(ui.value(seek), Some(10.5), "on the new step");
    }

    #[test]
    fn a_refused_range_or_another_widget_changes_nothing() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let Some(volume) = ui.slider(Ui::ROOT, "Volume", 0.0, 100.0, 1.0, 40.0) else {
            unreachable!("a valid range is taken");
        };
        for (min, max, step) in [
            (10.0, 10.0, 1.0),
            (0.0, 1.0, 0.0),
            (0.0, f64::NAN, 1.0),
            (0.0, f64::INFINITY, 1.0),
            (-1e308, 1e308, 1.0),
            (0.0, 1e12, 1e-3),
        ] {
            assert!(!ui.set_range(volume, min, max, step), "{min} {max} {step}");
        }
        assert_eq!(ui.value(volume), Some(40.0));
        let label = ui.add(Ui::ROOT, crate::Kind::Label("Volume".into()));
        assert!(!ui.set_range(label, 0.0, 1.0, 1.0));
    }
}
