//! An icon toggle: a raised button that shows only an icon, named for assistive technologies,
//! that turns on and off by itself, as the control center's mute buttons beside its volume
//! sliders. AT-SPI reads it as GTK's ToggleButton: a toggle button, pressed while on. VL10 keeps
//! the accent off it: on, it takes a stronger share of the text colour than its hover.

use crate::icon::Image;
use crate::widget::{Kind, Ui, WidgetId};

/// An icon toggle: see [`Ui::icon_toggle`].
pub struct IconToggle {
    /// Its name for assistive technologies.
    pub label: String,
    /// What it shows; the caller tints it, as an icon button's.
    pub image: Image,
    /// The side of the square the image is drawn in, in logical pixels.
    pub size: f32,
    /// Whether it is on.
    pub active: bool,
}

/// How much of the text colour an icon toggle's fill takes while it is on.
pub(crate) const ON: f32 = 0.18;

impl Ui {
    /// Adds an icon toggle named `label`, showing `image` in a square of `size` logical pixels,
    /// on when `active`. Activating it turns it on or off and raises [`crate::Event::Toggled`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn icon_toggle(
        &mut self,
        parent: WidgetId,
        label: &str,
        image: Image,
        size: f32,
        active: bool,
    ) -> WidgetId {
        let toggle = IconToggle {
            label: label.to_owned(),
            image,
            size,
            active,
        };
        self.add(parent, Kind::IconToggle(toggle))
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use crate::widget::{Event, Kind, Ui};

    #[test]
    fn an_icon_toggle_turns_on_and_off_and_takes_a_new_name() -> Result<(), crate::Error> {
        let mut ui = Ui::new("t", Theme::default(), false);
        let image = crate::icon::Image::from_rgba8(1, 1, vec![0, 0, 0, 255])?;
        let mute = ui.icon_toggle(Ui::ROOT, "Mute output", image, 16.0, false);
        assert!(ui.widget(mute).is_focusable());
        assert_eq!(ui.activate(mute), Some(Event::Toggled(mute)));
        assert!(ui.is_active(mute));
        assert_eq!(ui.set_active(mute, true), None);
        assert_eq!(ui.set_active(mute, false), Some(Event::Toggled(mute)));
        assert_eq!(
            ui.set_text(mute, "Mute microphone"),
            Some(Event::Edited(mute))
        );
        assert!(
            matches!(&ui.widget(mute).kind, Kind::IconToggle(t) if t.label == "Mute microphone")
        );
        ui.layout();
        let r = ui.widget(mute).rect;
        assert!(r.w >= ui.theme.button_min && r.h >= ui.theme.button_min);
        Ok(())
    }
}
