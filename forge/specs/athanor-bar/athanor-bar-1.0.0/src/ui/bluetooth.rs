//! The Bluetooth module (doc_bar.md, BR3): the button and its popover. The page in the
//! popover is athanor-controls'.

use std::cell::Cell;
use std::rc::Rc;

use athanor_controls::bluetooth::Page;
use athanor_controls::bridge;
use athanor_services::bluetooth;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct BluetoothUi {
    popup: Popup,
    /// `open` came before the module could show (the captures of BR9).
    pending_open: Rc<Cell<bool>>,
    /// `None` without the runtime of the models; the module is then hidden.
    _page: Option<Page>,
}

impl ModuleUi for BluetoothUi {
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
    let icon = gtk4::Image::from_icon_name("bluetooth-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Bluetooth"));
    popup.button.set_visible(false);
    let pending_open = Rc::new(Cell::new(false));
    let page = bridge::services().map(|services| {
        // The bar owns the one agent of the session (doc_control_center.md).
        services.register_bluetooth_agent();
        let page = Page::new(&services, popup.host(bar));
        popup.popover.set_child(Some(&page.widget()));
        let (popup, weak_bar, pending) = (popup.clone(), Rc::downgrade(bar), pending_open.clone());
        page.connect_shown(move |state| {
            let Some(bar) = weak_bar.upgrade() else {
                return;
            };
            match state {
                None => popup.hide(),
                Some(state) => {
                    icon.set_icon_name(Some(bluetooth::module_icon(state)));
                    popup.button.set_visible(true);
                    if pending.take() {
                        popup.open(&bar);
                    }
                }
            }
            bar.fit_groups();
        });
        page
    });
    if page.is_none() {
        tracing::warn!("no runtime for the models; the Bluetooth module is hidden");
    }
    Some(Box::new(BluetoothUi {
        popup,
        pending_open,
        _page: page,
    }))
}
