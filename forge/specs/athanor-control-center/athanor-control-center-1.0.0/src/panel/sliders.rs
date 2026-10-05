//! A slider of the panel: a volume or the brightness. An optional button mutes, an optional
//! arrow opens the detail page.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::accessible::Property;
use gtk4::prelude::*;

use crate::i18n::tr_with;

/// A label and what the control does: the mute button's, or the arrow's.
pub type Action<'a, F> = Option<(&'a str, Box<F>)>;

pub struct Slider {
    pub row: gtk4::Box,
    scale: gtk4::Scale,
    mute: Option<gtk4::ToggleButton>,
    icon: gtk4::Image,
    /// A state the model reported is being shown: the scale's change is not the person's.
    syncing: Rc<Cell<bool>>,
}

impl Slider {
    /// `name` is what a screen reader says of the scale. `on_change` gets the percent the
    /// person chose; `on_mute` (with `mute_name`) gets the mute state they asked for.
    pub fn new(
        name: &str,
        icon: &str,
        on_change: impl Fn(u32) + 'static,
        mute: Action<'_, dyn Fn(bool)>,
        page: Action<'_, dyn Fn()>,
    ) -> Slider {
        let syncing = Rc::new(Cell::new(false));
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        scale.update_property(&[Property::Label(name)]);
        let guard = syncing.clone();
        scale.connect_value_changed(move |scale| {
            if !guard.get() {
                // Whole percents: the scale has no digits, so a drag sends each step once.
                on_change(scale.value().round() as u32);
            }
        });
        let image = gtk4::Image::from_icon_name(icon);
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.add_css_class("control-center-slider");
        let mute = match mute {
            Some((label, on_mute)) => {
                let button = gtk4::ToggleButton::new();
                button.set_child(Some(&image));
                button.update_property(&[Property::Label(label)]);
                button.connect_clicked(move |button| on_mute(button.is_active()));
                row.append(&button);
                Some(button)
            }
            None => {
                row.append(&image);
                None
            }
        };
        row.append(&scale);
        if let Some((label, open)) = page {
            let arrow = gtk4::Button::from_icon_name("go-next-symbolic");
            arrow.update_property(&[Property::Label(&tr_with(
                "Open the {name} page",
                "name",
                label,
            ))]);
            arrow.connect_clicked(move |_| open());
            row.append(&arrow);
        }
        Slider {
            row,
            scale,
            mute,
            icon: image,
            syncing,
        }
    }

    pub fn set(&self, percent: f64, muted: bool, icon: &str) {
        self.syncing.set(true);
        self.scale.set_value(percent);
        self.syncing.set(false);
        self.icon.set_icon_name(Some(icon));
        if let Some(button) = &self.mute {
            button.set_active(muted);
        }
    }
}
