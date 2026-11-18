//! A slider's range, changed after it was added: a seek bar takes the length of each track.

use crate::widget::{Kind, Slider, Ui, WidgetId};

impl Ui {
    /// Gives slider `id` the range `min` to `max` in steps of `step`, as [`Ui::slider`] takes
    /// them, and moves its value to the step nearest to where it was, inside the new range: a
    /// value half a step from two steps goes to the upper one, and a value nearer to `max` than
    /// to the last step goes to `max`. Raises no event, as a range set by the application is not
    /// the person's; call [`Ui::layout`] afterwards.
    ///
    /// - `Some(true)`: the range changed.
    /// - `Some(false)`: the slider already has this range; nothing changes.
    /// - `None`: refused, and nothing changes, neither the value nor the range: `id` is not a
    ///   slider, or [`Ui::slider`] would refuse these numbers (not finite, `min >= max`,
    ///   `step <= 0`, or too many steps).
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_range(&mut self, id: WidgetId, min: f64, max: f64, step: f64) -> Option<bool> {
        let Kind::Slider(slider) = &mut self.widgets[id].kind else {
            return None;
        };
        if (slider.min, slider.max, slider.step) == (min, max, step) {
            return Some(false);
        }
        *slider = Slider::new(&slider.name, min, max, step, slider.value)?;
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use crate::widget::{Event, Ui, WidgetId};

    /// The value, minimum, maximum and step that assistive technologies read on `id`.
    fn read(ui: &Ui, id: WidgetId) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
        let node = crate::a11y::node(ui, id, 1.0);
        (
            node.numeric_value(),
            node.min_numeric_value(),
            node.max_numeric_value(),
            node.numeric_value_step(),
        )
    }

    fn slider(ui: &mut Ui, min: f64, max: f64, step: f64, value: f64) -> WidgetId {
        let Some(id) = ui.slider(Ui::ROOT, "Seek", min, max, step, value) else {
            unreachable!("a valid range is taken");
        };
        id
    }

    #[test]
    fn a_new_range_keeps_the_value_inside_it_and_raises_nothing() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let seek = slider(&mut ui, 0.0, 1.0, 1.0, 0.0);
        assert_eq!(
            ui.set_range(seek, 0.0, 240.0, 1.0),
            Some(true),
            "a track 240 s long"
        );
        assert_eq!(ui.value(seek), Some(0.0));
        assert_eq!(ui.set_value(seek, 200.0), Some(Event::Changed(seek)));
        assert_eq!(
            ui.set_range(seek, 0.0, 240.0, 1.0),
            Some(false),
            "the same range"
        );
        assert_eq!(
            ui.set_range(seek, 0.0, 90.0, 1.0),
            Some(true),
            "a shorter track"
        );
        assert_eq!(ui.value(seek), Some(90.0), "held to the new end");
        assert_eq!(ui.set_range(seek, 0.0, 90.0, 0.5), Some(true));
        assert_eq!(ui.set_value(seek, 10.25), Some(Event::Changed(seek)));
        assert_eq!(ui.value(seek), Some(10.5), "on the new step");
    }

    #[test]
    fn a_new_step_snaps_the_value_to_the_nearest_step_of_it() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let seek = slider(&mut ui, 0.0, 100.0, 1.0, 7.0);
        assert_eq!(ui.set_range(seek, 0.0, 100.0, 5.0), Some(true));
        assert_eq!(ui.value(seek), Some(5.0), "7 is nearer to 5 than to 10");

        let half = slider(&mut ui, 0.0, 10.0, 1.0, 3.0);
        assert_eq!(ui.set_range(half, 0.0, 10.0, 2.0), Some(true));
        assert_eq!(ui.value(half), Some(4.0), "half a step goes up");
    }

    #[test]
    fn a_minimum_raised_above_the_value_carries_the_value_up() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let seek = slider(&mut ui, 0.0, 100.0, 1.0, 40.0);
        assert_eq!(ui.set_range(seek, 50.0, 100.0, 1.0), Some(true));
        assert_eq!(ui.value(seek), Some(50.0));
    }

    #[test]
    fn a_span_the_step_does_not_divide_keeps_its_maximum_as_a_value() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let seek = slider(&mut ui, 0.0, 10.0, 0.1, 9.9);
        assert_eq!(ui.set_range(seek, 0.0, 10.0, 3.0), Some(true));
        assert_eq!(ui.value(seek), Some(10.0), "9.9 is nearer to 10 than to 9");
        assert_eq!(ui.set_value(seek, 8.0), Some(Event::Changed(seek)));
        assert_eq!(ui.value(seek), Some(9.0), "the steps still count from 0");
    }

    #[test]
    fn a_refused_range_or_another_widget_changes_nothing() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let volume = slider(&mut ui, 0.0, 100.0, 1.0, 40.0);
        let before = read(&ui, volume);
        assert_eq!(before, (Some(40.0), Some(0.0), Some(100.0), Some(1.0)));
        for (min, max, step) in [
            (10.0, 10.0, 1.0),
            (20.0, 10.0, 1.0),
            (0.0, 1.0, 0.0),
            (0.0, 1.0, -1.0),
            (0.0, f64::NAN, 1.0),
            (0.0, f64::INFINITY, 1.0),
            (-1e308, 1e308, 1.0),
            (0.0, 1e12, 1e-3),
        ] {
            assert_eq!(
                ui.set_range(volume, min, max, step),
                None,
                "{min} {max} {step}"
            );
            assert_eq!(read(&ui, volume), before, "{min} {max} {step}");
        }
        let label = ui.add(Ui::ROOT, crate::Kind::Label("Volume".into()));
        assert_eq!(ui.set_range(label, 0.0, 1.0, 1.0), None, "not a slider");
    }

    #[test]
    fn assistive_technologies_read_the_new_range() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let seek = slider(&mut ui, 0.0, 1.0, 1.0, 0.0);
        assert_eq!(ui.set_range(seek, 0.0, 240.0, 2.0), Some(true));
        assert_eq!(ui.set_value(seek, 121.0), Some(Event::Changed(seek)));
        ui.layout();
        assert_eq!(
            read(&ui, seek),
            (Some(122.0), Some(0.0), Some(240.0), Some(2.0))
        );
    }
}
