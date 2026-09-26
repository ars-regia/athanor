//! The power menu (doc_bar.md, BR3). Nothing happens on the first press: every action
//! asks, with Cancel focused, so a stray Enter backs out. The actions logind does not
//! offer are hidden; the menu asks again each time it opens.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::power::Action;
use gtk4::glib;
use gtk4::prelude::*;

use super::logind;
use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct PowerUi {
    popup: Popup,
}

impl ModuleUi for PowerUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

/// The row's label and the confirmation's question.
fn texts(action: Action) -> (String, String) {
    match action {
        Action::Lock => (tr("Lock"), tr("Lock the screen?")),
        Action::LogOut => (tr("Log Out"), tr("Log out now?")),
        Action::Suspend => (tr("Suspend"), tr("Suspend now?")),
        Action::Reboot => (tr("Restart"), tr("Restart now?")),
        Action::PowerOff => (tr("Shut Down"), tr("Shut down now?")),
    }
}

fn row_button(label: &str, class: &str) -> gtk4::Button {
    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    let button = gtk4::Button::new();
    button.set_child(Some(&text));
    button.add_css_class(class);
    button
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let icon = gtk4::Image::from_icon_name("system-shutdown-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Power"));

    let actions = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let question = gtk4::Label::new(None);
    question.add_css_class("bar-popover-title");
    question.set_wrap(true);
    question.set_xalign(0.0);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    cancel.add_css_class("bar-row");
    let confirm = gtk4::Button::new();
    confirm.add_css_class("bar-confirm");
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&cancel);
    buttons.append(&confirm);
    let confirm_page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    confirm_page.append(&question);
    confirm_page.append(&buttons);
    let stack = gtk4::Stack::new();
    stack.add_named(&actions, Some("actions"));
    stack.add_named(&confirm_page, Some("confirm"));
    popup.popover.set_child(Some(&stack));

    let pending: Rc<Cell<Option<Action>>> = Rc::new(Cell::new(None));
    let mut asked = Vec::new();
    for action in Action::ALL {
        let (label, ask) = texts(action);
        let row = row_button(&label, "bar-row");
        // Hidden until logind says it is offered.
        row.set_visible(action.can_method().is_none());
        let (pending, question, confirm, stack, cancel) = (
            pending.clone(),
            question.clone(),
            confirm.clone(),
            stack.clone(),
            cancel.clone(),
        );
        row.connect_clicked(move |_| {
            pending.set(Some(action));
            question.set_text(&ask);
            confirm.set_label(&label);
            stack.set_visible_child_name("confirm");
            cancel.grab_focus();
        });
        actions.append(&row);
        if action.can_method().is_some() {
            asked.push((action, row));
        }
    }

    let back = {
        let (pending, stack) = (pending.clone(), stack.clone());
        move || {
            pending.set(None);
            stack.set_visible_child_name("actions");
        }
    };
    let cancel_back = back.clone();
    cancel.connect_clicked(move |_| cancel_back());
    popup.popover.connect_closed(move |_| back());
    let popover = popup.popover.clone();
    confirm.connect_clicked(move |_| {
        let Some(action) = pending.take() else { return };
        popover.popdown();
        glib::spawn_future_local(async move {
            if let Err(err) = logind::run(action).await {
                tracing::error!(error = %err, action = action.id(), "the power action failed");
            }
        });
    });

    let ask_logind = move || {
        for (action, row) in &asked {
            let (action, row) = (*action, row.clone());
            glib::spawn_future_local(async move {
                row.set_visible(logind::offered(action).await);
            });
        }
    };
    ask_logind();
    popup.popover.connect_show(move |_| ask_logind());
    Some(Box::new(PowerUi { popup }))
}
