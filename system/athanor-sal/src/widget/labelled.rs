//! [`Kind::LabelledButton`]: a flat button that shows an image beside a text, as a GTK button
//! holding a box of an image and a label, the bar's battery and clock.

use super::{Event, Kind, Ui, WidgetId};
use crate::icon::Image;

/// The size of a labelled button holding an image `image` wide, when there is one, then a
/// text `text` wide and `text_h` tall, `gap` apart: at least `button_min` on each side, with
/// `pad` added left and right, as GTK's min-width and padding.
pub(super) fn measure(
    image: Option<f32>,
    text: Option<(f32, f32)>,
    gap: f32,
    pad: f32,
    button_min: f32,
) -> (f32, f32) {
    let both = if image.is_some() && text.is_some() {
        gap
    } else {
        0.0
    };
    let w = image.unwrap_or(0.0) + both + text.map_or(0.0, |t| t.0);
    let h = image.unwrap_or(0.0).max(text.map_or(0.0, |t| t.1));
    (w.max(button_min) + 2.0 * pad, h.max(button_min))
}

impl Ui {
    /// Adds a flat button named `label` for assistive technologies, showing `image`, when
    /// there is one, in a square of `size` logical pixels, then `text`. See
    /// [`Kind::LabelledButton`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn labelled_button(
        &mut self,
        parent: WidgetId,
        label: &str,
        image: Option<Image>,
        size: f32,
        text: &str,
    ) -> WidgetId {
        let kind = Kind::LabelledButton {
            label: label.to_owned(),
            image,
            size,
            text: text.to_owned(),
        };
        self.add(parent, kind)
    }

    /// Replaces the text labelled button `id` shows; [`Ui::set_text`] replaces its name.
    /// Returns [`Event::Edited`] when the text changed; `None`, and nothing changes, for the
    /// same text or another kind. Call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_shown_text(&mut self, id: WidgetId, now: &str) -> Option<Event> {
        let Kind::LabelledButton { text, .. } = &mut self.widgets[id].kind else {
            return None;
        };
        if text == now {
            return None;
        }
        now.clone_into(text);
        Some(Event::Edited(id))
    }

    /// Replaces the image labelled button or icon button `id` shows, or takes a labelled
    /// button's away with `None`; `false`, and nothing changes, for another kind or for `None`
    /// on an icon button. Call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_image(&mut self, id: WidgetId, now: Option<Image>) -> bool {
        match (&mut self.widgets[id].kind, now) {
            (Kind::LabelledButton { image, .. }, now) => *image = now,
            (Kind::IconButton { image, .. }, Some(now)) => *image = now,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Kind, Theme, Ui};

    fn image() -> crate::Image {
        crate::icon::Image {
            pixmap: std::sync::Arc::new(vello_cpu::Pixmap::new(1, 1)),
        }
    }

    fn theme() -> Theme {
        Theme {
            padding: 8.0,
            gap: 4.0,
            button_min: 32.0,
            ..Theme::default()
        }
    }

    #[test]
    fn the_button_holds_its_image_and_its_text_inside_the_padding() {
        let mut ui = Ui::new("Bar", theme(), false);
        let empty = ui.labelled_button(Ui::ROOT, "Battery", Some(image()), 16.0, "");
        let both = ui.labelled_button(Ui::ROOT, "Battery", Some(image()), 16.0, "100 %");
        let text = ui.labelled_button(Ui::ROOT, "Clock", None, 16.0, "Fri 18 Sep 10:00");
        ui.layout();
        assert_eq!(
            ui.widget(empty).rect.w,
            32.0 + 16.0,
            "the minimum, then the padding"
        );
        assert_eq!(ui.widget(empty).rect.h, 32.0);
        let (b, t) = (ui.widget(both).rect, ui.widget(text).rect);
        assert!(b.w > 48.0);
        assert!(t.w > b.w, "no image, but a longer text");
        assert_eq!(b.h, 32.0);
        assert!(ui.is_flat(both));
    }

    #[test]
    fn its_name_and_its_shown_text_change_apart() {
        let mut ui = Ui::new("Bar", theme(), false);
        let button = ui.labelled_button(Ui::ROOT, "Clock", None, 16.0, "10:00");
        assert_eq!(ui.set_shown_text(button, "10:00"), None);
        assert_eq!(
            ui.set_shown_text(button, "10:01"),
            Some(Event::Edited(button))
        );
        assert_eq!(
            ui.set_text(button, "Friday 18 September 2026, 10:01"),
            Some(Event::Edited(button))
        );
        let Kind::LabelledButton { label, text, .. } = &ui.widget(button).kind else {
            panic!("a labelled button");
        };
        assert_eq!(
            (label.as_str(), text.as_str()),
            ("Friday 18 September 2026, 10:01", "10:01")
        );
        assert_eq!(ui.activate(button), Some(Event::Clicked(button)));
        assert_eq!(ui.set_shown_text(Ui::ROOT, "x"), None);
    }

    #[test]
    fn an_image_is_set_taken_away_or_refused() {
        let mut ui = Ui::new("Bar", theme(), false);
        let labelled = ui.labelled_button(Ui::ROOT, "Battery", None, 16.0, "50 %");
        let icon = ui.icon_button(Ui::ROOT, "Power", image(), 16.0, None);
        assert!(ui.set_image(labelled, Some(image())));
        assert!(matches!(
            ui.widget(labelled).kind,
            Kind::LabelledButton { image: Some(_), .. }
        ));
        assert!(ui.set_image(labelled, None));
        assert!(ui.set_image(icon, Some(image())));
        assert!(!ui.set_image(icon, None), "an icon button keeps an image");
        assert!(!ui.set_image(Ui::ROOT, Some(image())));
    }

    #[test]
    fn the_button_reads_as_a_button_named_by_its_label() {
        let mut ui = Ui::new("Bar", theme(), false);
        let button = ui.labelled_button(Ui::ROOT, "Battery", None, 16.0, "50 %");
        ui.set_description(button, Some("50 %, charging"));
        ui.set_has_popup(button, true);
        ui.layout();
        let node = crate::a11y::node(&ui, button, 1.0);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.label(), Some("Battery"));
        assert_eq!(node.description(), Some("50 %, charging"));
        let update = crate::a11y::tree_update(&ui, 1.0);
        let label = crate::a11y::row_label_id(button);
        assert_eq!(node.children(), [label], "the text it shows, as a label inside it");
        let shown = update.nodes.iter().find(|(id, _)| *id == label);
        assert_eq!(shown.map(|(_, n)| n.value()), Some(Some("50 %")));
        ui.set_shown_text(button, "");
        let node = crate::a11y::node(&ui, button, 1.0);
        assert!(node.children().is_empty(), "no label without a text");
    }
}
