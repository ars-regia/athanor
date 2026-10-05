//! One tile of the panel: a toggle that says what it is and what state it is in, and, for a
//! service with a detail page, an arrow that opens the page (CC1).

use gtk4::accessible::Property;
use gtk4::prelude::*;

use crate::i18n::tr_with;

pub struct Tile {
    pub root: gtk4::Box,
    pub button: gtk4::ToggleButton,
    icon: gtk4::Image,
    subtitle: gtk4::Label,
    title: String,
}

impl Tile {
    /// `on_press` runs when the person presses the tile, with the state the press asks for.
    /// `page` is the arrow's action: `Some` for a tile with a detail page.
    pub fn new(
        title: &str,
        icon: &str,
        on_press: impl Fn(bool) + 'static,
        page: Option<Box<dyn Fn()>>,
    ) -> Tile {
        let image = gtk4::Image::from_icon_name(icon);
        let name = gtk4::Label::new(Some(title));
        name.set_xalign(0.0);
        name.add_css_class("heading");
        let subtitle = gtk4::Label::new(None);
        subtitle.set_xalign(0.0);
        subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        subtitle.add_css_class("caption");
        let texts = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        texts.set_hexpand(true);
        texts.append(&name);
        texts.append(&subtitle);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.append(&image);
        content.append(&texts);
        let button = gtk4::ToggleButton::new();
        button.set_child(Some(&content));
        button.set_hexpand(true);
        button.update_property(&[Property::Label(title)]);
        // Not `toggled`: a state the models report sets the button without a press.
        button.connect_clicked(move |button| on_press(button.is_active()));
        let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        root.add_css_class("control-center-tile");
        root.append(&button);
        if let Some(open) = page {
            let arrow = gtk4::Button::from_icon_name("go-next-symbolic");
            arrow.update_property(&[Property::Label(&tr_with(
                "Open the {name} page",
                "name",
                title,
            ))]);
            arrow.connect_clicked(move |_| open());
            root.append(&arrow);
        }
        Tile {
            root,
            button,
            icon: image,
            subtitle,
            title: title.to_owned(),
        }
    }

    /// Shows the state without pressing the tile.
    pub fn set(&self, active: bool, subtitle: &str, icon: &str) {
        self.button.set_active(active);
        self.subtitle.set_text(subtitle);
        self.icon.set_icon_name(Some(icon));
        self.button.update_property(&[
            Property::Label(&self.title),
            Property::Description(subtitle),
        ]);
    }
}
