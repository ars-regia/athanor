//! The audio module (doc_bar.md, BR3): the button and its popover. The page in the popover,
//! with the media controls, is athanor-controls'. A lost sound server hides the module until
//! it returns.

use std::cell::Cell;
use std::rc::Rc;

use athanor_controls::audio::Page;
use athanor_controls::bridge;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct AudioUi {
    popup: Popup,
    /// `ATHANOR_BAR_OPEN=audio` came before the popover's content was complete.
    pending_open: Rc<Cell<bool>>,
    /// Whether the media controls are final, as the page last said.
    settled: Rc<Cell<bool>>,
    /// `None` without the runtime of the models; the module is then hidden.
    _page: Option<Page>,
}

impl ModuleUi for AudioUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.popup.button.get_visible() && self.settled.get() {
            self.popup.open(bar);
        } else {
            self.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let icon = gtk4::Image::from_icon_name("audio-volume-medium-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Sound"));
    popup.button.set_visible(false);
    let pending_open = Rc::new(Cell::new(false));
    let settled = Rc::new(Cell::new(true));
    let page = bridge::services().map(|services| {
        let page = Page::new(&services);
        popup.popover.set_child(Some(&page.widget()));
        let (popup, weak_bar) = (popup.clone(), Rc::downgrade(bar));
        let (pending, settled) = (pending_open.clone(), settled.clone());
        page.connect_shown(move |face| {
            let Some(bar) = weak_bar.upgrade() else {
                return;
            };
            let Some(face) = face else {
                popup.hide();
                bar.fit_groups();
                return;
            };
            icon.set_icon_name(Some(face.icon));
            settled.set(face.settled);
            popup.button.set_visible(true);
            if pending.get() && face.settled {
                pending.set(false);
                popup.open(&bar);
            }
            bar.fit_groups();
        });
        page
    });
    if page.is_none() {
        tracing::warn!("no runtime for the models; the audio module is hidden");
    }
    Some(Box::new(AudioUi {
        popup,
        pending_open,
        settled,
        _page: page,
    }))
}
