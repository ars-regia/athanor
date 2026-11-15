//! The clock and its calendar (doc_bar.md, BR3). Every tick reads the zone and the wall
//! clock again (`athanor_bar::clock::Clock`).

use std::rc::Rc;

use athanor_bar::clock::Clock;
use gtk4::accessible::Property;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct ClockUi {
    popup: Popup,
    label: gtk4::Label,
    clock: Rc<Clock>,
}

impl ModuleUi for ClockUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Tick {
            return;
        }
        let Some((short, long)) = self.clock.texts() else {
            return;
        };
        if self.label.text() != short {
            self.label.set_text(&short);
            self.popup.button.set_tooltip_text(Some(&long));
            self.popup.button.update_property(&[Property::Label(&long)]);
        }
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let label = gtk4::Label::new(None);
    label.add_css_class("bar-clock");
    let popup = Popup::new(bar, &label, &tr("Clock"));
    let calendar = gtk4::Calendar::new();
    popup.popover.set_child(Some(&calendar));
    let clock = Rc::new(Clock::new());
    let shown = clock.clone();
    // The calendar opens on today, even when it was left on another month.
    popup.popover.connect_show(move |_| {
        if let Ok(now) = shown.now() {
            calendar.select_day(&now);
        }
    });
    Some(Box::new(ClockUi {
        popup,
        label,
        clock,
    }))
}
