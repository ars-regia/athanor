//! The battery module (doc_bar.md, BR3): the button and its popover. The page in the
//! popover is athanor-controls'. Without a present battery the module hides (SH1).

use std::cell::Cell;
use std::rc::Rc;

use athanor_controls::battery::{note, percent_text, Page};
use athanor_controls::bridge;
use athanor_services::battery;
use gtk4::accessible::Property;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct BatteryUi {
    popup: Popup,
    pending_open: Rc<Cell<bool>>,
    /// `None` without the runtime of the models; the module is then hidden.
    _page: Option<Page>,
}

impl ModuleUi for BatteryUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.popup.button.get_visible() {
            self.popup.open(bar);
        } else {
            self.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let icon = gtk4::Image::from_icon_name("battery-good-symbolic");
    let level = gtk4::Label::new(None);
    let face = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    face.append(&icon);
    face.append(&level);
    let popup = Popup::new(bar, &face, &tr("Battery"));
    popup.button.set_visible(false);
    let pending_open = Rc::new(Cell::new(false));
    let page = bridge::services().map(|services| {
        let page = Page::new(&services);
        popup.popover.set_child(Some(&page.widget()));
        let (popup, weak_bar, pending) = (popup.clone(), Rc::downgrade(bar), pending_open.clone());
        page.connect_shown(move |status| {
            let Some(bar) = weak_bar.upgrade() else {
                return;
            };
            let Some(battery) = status.battery else {
                popup.hide();
                bar.fit_groups();
                return;
            };
            let charge = percent_text(battery.percent);
            let note = note(&battery);
            icon.set_icon_name(Some(battery::icon(&battery)));
            level.set_text(&charge);
            let description = match &note {
                Some(note) => format!("{charge}, {note}"),
                None => charge.clone(),
            };
            popup.button.set_tooltip_text(Some(&description));
            popup
                .button
                .update_property(&[Property::Description(&description)]);
            popup.button.set_visible(true);
            if pending.take() {
                popup.open(&bar);
            }
            bar.fit_groups();
        });
        page
    });
    if page.is_none() {
        tracing::warn!("no runtime for the models; the battery module is hidden");
    }
    Some(Box::new(BatteryUi {
        popup,
        pending_open,
        _page: page,
    }))
}
