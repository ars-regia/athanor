//! The clock and its calendar (doc_bar.md, BR3). Every tick reads the zone and the wall
//! clock again: nothing is counted, so a resume or a clock that jumps shows at once.

use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use athanor_bar::clock;
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

/// The zone identifier last read, and the zone it gave.
type Zone = Rc<RefCell<Option<(Option<String>, glib::TimeZone)>>>;

struct ClockUi {
    popup: Popup,
    label: gtk4::Label,
    zone: Zone,
    /// The last tick could not read or format the time: logged once, not every second.
    failed: Cell<bool>,
}

fn current_zone(zone: &Zone) -> glib::TimeZone {
    let link = fs::read_link("/etc/localtime").ok();
    let id = clock::zone(
        env::var("TZ").ok().as_deref(),
        link.as_deref().map(Path::new),
    );
    let mut last = zone.borrow_mut();
    match &*last {
        Some((last_id, tz)) if *last_id == id => tz.clone(),
        _ => {
            let (tz, unknown) = clock::time_zone(id.as_deref());
            if unknown {
                // Logged when the zone changes, not every second.
                tracing::warn!(
                    zone = id.as_deref().unwrap_or(""),
                    "unknown time zone; the clock shows local time"
                );
            }
            *last = Some((id, tz.clone()));
            tz
        }
    }
}

impl ModuleUi for ClockUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Tick {
            return;
        }
        let texts = glib::DateTime::now(&current_zone(&self.zone)).and_then(|now| {
            // TRANSLATORS: a g_date_time_format() pattern for the clock on the bar.
            let short = now.format(&tr("%a %-d %b %H:%M"))?;
            // TRANSLATORS: a g_date_time_format() pattern, read aloud by screen readers.
            let long = now.format(&tr("%A %-d %B %Y, %H:%M"))?;
            Ok((short, long))
        });
        let (short, long) = match texts {
            Ok(texts) => {
                self.failed.set(false);
                texts
            }
            Err(err) => {
                if !self.failed.replace(true) {
                    tracing::error!(error = %err, "cannot read or format the time; the clock is not updated");
                }
                return;
            }
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
    let zone: Zone = Rc::new(RefCell::new(None));
    let shown_zone = zone.clone();
    // The calendar opens on today, even when it was left on another month.
    popup.popover.connect_show(move |_| {
        if let Ok(now) = glib::DateTime::now(&current_zone(&shown_zone)) {
            calendar.select_day(&now);
        }
    });
    Some(Box::new(ClockUi {
        popup,
        label,
        zone,
        failed: Cell::new(false),
    }))
}
