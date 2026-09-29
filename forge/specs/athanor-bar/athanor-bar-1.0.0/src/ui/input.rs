//! The input source (doc_bar.md, BR3): the keyboard layouts the compositor has, one of them
//! active. Hidden with fewer than two.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::keyboard;
use gtk4::accessible::Property;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

struct InputUi {
    popup: Popup,
    list: gtk4::Box,
    /// The layouts and group the list was built from, so a refresh that changes nothing
    /// does not rebuild it under the user's pointer.
    shown: std::cell::RefCell<(Vec<String>, u32)>,
    /// The list is being rebuilt: its toggles are not the user's.
    rebuilding: Rc<Cell<bool>>,
}

impl ModuleUi for InputUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Keyboard {
            return;
        }
        let Some(client) = bar.client() else { return };
        let (layouts, group) = (client.keyboard_layouts(), client.keyboard_group());
        self.popup.button.set_visible(keyboard::shown(&layouts));
        if *self.shown.borrow() == (layouts.clone(), group) {
            return;
        }
        let name = tr_with(
            "Input source: {name}",
            "name",
            keyboard::active(&layouts, group).unwrap_or(""),
        );
        self.popup.button.set_tooltip_text(Some(&name));
        self.popup.button.update_property(&[Property::Label(&name)]);

        self.rebuilding.set(true);
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let mut first: Option<gtk4::CheckButton> = None;
        for (index, layout) in layouts.iter().enumerate() {
            let check = gtk4::CheckButton::with_label(layout);
            check.set_group(first.as_ref());
            check.set_active(u32::try_from(index).is_ok_and(|index| index == group));
            let (weak, rebuilding) = (Rc::downgrade(bar), self.rebuilding.clone());
            check.connect_toggled(move |check| {
                if rebuilding.get() || !check.is_active() {
                    return;
                }
                let (Some(bar), Ok(index)) = (weak.upgrade(), u32::try_from(index)) else {
                    return;
                };
                if let Some(Err(err)) = bar.client().map(|client| client.set_keyboard_group(index))
                {
                    tracing::error!(error = %err, "cannot switch the keyboard layout");
                }
            });
            first.get_or_insert(check.clone());
            self.list.append(&check);
        }
        self.rebuilding.set(false);
        self.shown.replace((layouts, group));
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    bar.client()?;
    let icon = gtk4::Image::from_icon_name("input-keyboard-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Input source"));
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    popup.popover.set_child(Some(&list));
    popup.button.set_visible(false);
    Some(Box::new(InputUi {
        popup,
        list,
        shown: std::cell::RefCell::new((Vec::new(), u32::MAX)),
        rebuilding: Rc::new(Cell::new(false)),
    }))
}
