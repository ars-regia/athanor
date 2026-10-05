//! The panel's header (NC11): the do-not-disturb switch with its state in words, its menu,
//! and "Clear all". The menus are drawers inside the panel and not popovers: a popover is a
//! surface of its own, and the panel closes when its own surface loses the keyboard (CC9).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use athanor_services::notifications::{Dnd, NotificationsCommand};
use gtk4::accessible::{Property, Relation, State};
use gtk4::prelude::*;
use gtk4::glib;

use super::text::{dnd_words, next_at};
use super::Panel;
use crate::i18n::tr;

/// The Settings entry "Edit schedule" and every "Notification settings" open.
pub const SETTINGS: &str = "com.system76.CosmicSettings.desktop";
const ONE_HOUR: i64 = 3600;
const MORNING: i32 = 8;

/// A toggle that shows `content` right under itself, for a menu that stays inside the surface.
pub fn drawer(name: &str) -> (gtk4::ToggleButton, gtk4::Revealer, gtk4::Box) {
    let toggle = gtk4::ToggleButton::new();
    toggle.set_icon_name("view-more-symbolic");
    toggle.add_css_class("flat");
    toggle.set_tooltip_text(Some(name));
    toggle.update_property(&[Property::Label(name)]);
    toggle.update_state(&[State::Expanded(Some(false))]);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let revealer = gtk4::Revealer::new();
    revealer.set_child(Some(&content));
    toggle
        .bind_property("active", &revealer, "reveal-child")
        .sync_create()
        .build();
    toggle.connect_toggled(|toggle| {
        toggle.update_state(&[State::Expanded(Some(toggle.is_active()))]);
    });
    (toggle, revealer, content)
}

/// A menu entry: a flat button with its label at the start.
pub fn item(label: &str, on_click: impl Fn() + 'static) -> gtk4::Button {
    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    let button = gtk4::Button::new();
    button.set_child(Some(&text));
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

pub struct Header {
    pub root: gtk4::Box,
    switch: gtk4::Switch,
    words: gtk4::Label,
    clear: gtk4::Button,
    /// The switch is being set from the model, not by the user.
    updating: Cell<bool>,
}

impl Header {
    pub fn new(panel: &Weak<Panel>) -> Rc<Header> {
        let title = gtk4::Label::new(Some(&tr("Notifications")));
        title.add_css_class("title-4");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let clear = gtk4::Button::with_label(&tr("Clear all"));
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        top.append(&title);
        top.append(&clear);

        let name = gtk4::Label::new(Some(&tr("Do not disturb")));
        name.set_xalign(0.0);
        let words = gtk4::Label::new(None);
        words.set_xalign(0.0);
        words.add_css_class("nc-dim");
        let text = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        text.set_hexpand(true);
        text.append(&name);
        text.append(&words);
        let switch = gtk4::Switch::new();
        switch.set_valign(gtk4::Align::Center);
        switch.update_relation(&[
            Relation::LabelledBy(&[name.upcast_ref()]),
            Relation::DescribedBy(&[words.upcast_ref()]),
        ]);
        let (toggle, revealer, menu) = drawer(&tr("Do not disturb options"));
        let dnd = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        dnd.append(&text);
        dnd.append(&switch);
        dnd.append(&toggle);

        let ask = |panel: &Weak<Panel>, until: fn() -> Option<i64>, toggle: &gtk4::ToggleButton| {
            let (panel, toggle) = (panel.clone(), toggle.downgrade());
            move || {
                if let (Some(panel), Some(until)) = (panel.upgrade(), until()) {
                    panel.send(NotificationsCommand::SetDnd { on: true, until: Some(until) });
                }
                if let Some(toggle) = toggle.upgrade() {
                    toggle.set_active(false);
                }
            }
        };
        menu.append(&item(&tr("For one hour"), ask(panel, in_one_hour, &toggle)));
        menu.append(&item(&tr("Until 08:00"), ask(panel, until_morning, &toggle)));
        let settings = panel.clone();
        menu.append(&item(&tr("Edit schedule"), move || {
            if let Some(panel) = settings.upgrade() {
                panel.open_app(SETTINGS);
            }
        }));

        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        root.append(&top);
        root.append(&dnd);
        root.append(&revealer);

        let header = Rc::new(Header {
            root,
            switch,
            words,
            clear,
            updating: Cell::new(false),
        });
        let (weak_header, weak_panel) = (Rc::downgrade(&header), panel.clone());
        header.switch.connect_state_set(move |_, on| {
            let (Some(header), Some(panel)) = (weak_header.upgrade(), weak_panel.upgrade()) else {
                return glib::Propagation::Proceed;
            };
            if header.updating.get() {
                return glib::Propagation::Proceed;
            }
            // The switch takes its state from the model, once the daemon accepted the change,
            // so a refused call leaves it as the daemon has it.
            panel.send(NotificationsCommand::SetDnd { on, until: None });
            glib::Propagation::Stop
        });
        let weak_panel = panel.clone();
        header.clear.connect_clicked(move |_| {
            if let Some(panel) = weak_panel.upgrade() {
                panel.send(NotificationsCommand::ClearAll);
            }
        });
        header
    }

    pub fn update(&self, dnd: &Dnd, available: bool, any: bool) {
        self.updating.set(true);
        self.switch.set_active(dnd.on);
        self.switch.set_state(dnd.on);
        self.updating.set(false);
        self.switch.set_sensitive(available);
        self.clear.set_sensitive(available && any);
        let until = (dnd.until > 0)
            .then(|| glib::DateTime::from_unix_local(dnd.until).ok())
            .flatten()
            // ponytail: 24-hour clock; COSMIC's clock setting is not read here yet.
            .and_then(|time| time.format("%H:%M").ok())
            .map(|time| time.to_string());
        self.words.set_text(&dnd_words(dnd, until));
    }
}

fn in_one_hour() -> Option<i64> {
    Some(glib::real_time() / 1_000_000 + ONE_HOUR)
}

fn until_morning() -> Option<i64> {
    let now = glib::DateTime::now_local().ok()?;
    next_at(&now, MORNING).map(|time| time.to_unix())
}
