//! The footer (CC4): the battery level, and the button to Settings.

use gtk4::accessible::Property;
use gtk4::prelude::*;
use athanor_services::battery::{self, Battery};

use crate::i18n::{tr, tr_with};

pub struct Footer {
    pub root: gtk4::Box,
    battery: gtk4::Box,
    icon: gtk4::Image,
    level: gtk4::Label,
}

impl Footer {
    pub fn new(open_settings: impl Fn() + 'static) -> Footer {
        let icon = gtk4::Image::new();
        let level = gtk4::Label::new(None);
        let battery = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        battery.set_hexpand(true);
        battery.set_halign(gtk4::Align::Start);
        battery.append(&icon);
        battery.append(&level);
        let settings = gtk4::Button::from_icon_name("preferences-system-symbolic");
        settings.set_tooltip_text(Some(&tr("Settings")));
        settings.update_property(&[Property::Label(&tr("Settings"))]);
        settings.connect_clicked(move |_| open_settings());
        let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        root.add_css_class("control-center-footer");
        root.append(&battery);
        root.append(&settings);
        // With no battery the button still sits at the end.
        settings.set_halign(gtk4::Align::End);
        Footer {
            root,
            battery,
            icon,
            level,
        }
    }

    pub fn set(&self, battery: Option<&Battery>) {
        self.battery.set_visible(battery.is_some());
        if let Some(battery) = battery {
            let text = tr_with("{percent}%", "percent", &format!("{:.0}", battery.percent));
            self.icon.set_icon_name(Some(battery::icon(battery)));
            self.level.set_text(&text);
            self.battery
                .update_property(&[Property::Label(&tr_with("Battery {level}", "level", &text))]);
        }
    }
}
