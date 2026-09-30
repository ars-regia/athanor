//! The power menu (doc_bar.md, BR3). Nothing happens on the first press: every action
//! asks, with Cancel focused, so a stray Enter backs out. The actions logind does not
//! offer are hidden; the menu asks again each time it opens. "Restart to update" stands
//! beside "Restart" while an update is downloaded, and is the update service's Apply.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::order::Module;
use athanor_bar::power::Action;
use athanor_bar::shield::Request;
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

/// What the confirmation will do: a logind action, or the update service's Apply (BR3:
/// "Restart to update" stands beside "Restart" and is the same request as SH11).
#[derive(Clone, Copy)]
enum Pending {
    Logind(Action),
    Update,
}

/// The confirmation page's widgets, which every row's handler holds weakly.
struct Page {
    pending: Rc<Cell<Option<Pending>>>,
    question: gtk4::Label,
    confirm: gtk4::Button,
    stack: gtk4::Stack,
    cancel: gtk4::Button,
}

impl Page {
    /// A press on `row` asks `ask` first, with `label` on the confirming button.
    fn asks(&self, row: &gtk4::Button, what: Pending, ask: String, label: String) {
        let (pending, question, confirm, stack, cancel) = (
            self.pending.clone(),
            self.question.downgrade(),
            self.confirm.downgrade(),
            self.stack.downgrade(),
            self.cancel.downgrade(),
        );
        row.connect_clicked(move |_| {
            let (Some(question), Some(confirm), Some(stack), Some(cancel)) = (
                question.upgrade(),
                confirm.upgrade(),
                stack.upgrade(),
                cancel.upgrade(),
            ) else {
                return;
            };
            pending.set(Some(what));
            question.set_text(&ask);
            confirm.set_label(&label);
            stack.set_visible_child_name("confirm");
            cancel.grab_focus();
        });
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

    // Every handler below lives on a widget inside the popover and holds the other widgets
    // weakly: a strong reference to an ancestor would be a cycle that keeps the whole menu
    // alive after a rebuild replaces it.
    let page = Page {
        pending: Rc::new(Cell::new(None)),
        question: question.clone(),
        confirm: confirm.clone(),
        stack: stack.clone(),
        cancel: cancel.clone(),
    };
    let mut asked = Vec::new();
    let mut update = None;
    for action in Action::ALL {
        let (label, ask) = texts(action);
        let row = row_button(&label, "bar-row");
        // Hidden until logind says it is offered.
        row.set_visible(action.can_method().is_none());
        page.asks(&row, Pending::Logind(action), ask, label);
        actions.append(&row);
        if action.can_method().is_some() {
            asked.push((action, row));
        }
        if action == Action::Reboot {
            let update_row = row_button(&tr("Restart to update"), "bar-row");
            update_row.set_visible(bar.trust().restart_to_update_offered());
            page.asks(
                &update_row,
                Pending::Update,
                tr("Restart and install the update now?"),
                tr("Restart to update"),
            );
            actions.append(&update_row);
            update = Some(update_row);
        }
    }
    let pending = page.pending;

    let back = {
        let (pending, stack) = (pending.clone(), stack.downgrade());
        move || {
            pending.set(None);
            if let Some(stack) = stack.upgrade() {
                stack.set_visible_child_name("actions");
            }
        }
    };
    let cancel_back = back.clone();
    cancel.connect_clicked(move |_| cancel_back());
    popup.popover.connect_closed(move |_| back());
    let popover = popup.popover.downgrade();
    let origin = popup.button.downgrade();
    let weak_bar = Rc::downgrade(bar);
    confirm.connect_clicked(move |_| {
        let Some(what) = pending.take() else { return };
        if let Some(popover) = popover.upgrade() {
            popover.popdown();
        }
        match what {
            Pending::Logind(action) => {
                glib::spawn_future_local(async move {
                    if let Err(err) = logind::run(action).await {
                        tracing::error!(error = %err, action = action.id(), "the power action failed");
                    }
                });
            }
            Pending::Update => {
                let (Some(bar), Some(origin)) = (weak_bar.upgrade(), origin.upgrade()) else {
                    return;
                };
                // The file may have changed while the question was open.
                if bar.trust().restart_to_update_offered() {
                    super::shield::request(&bar, Request::Apply, origin.upcast_ref());
                } else {
                    bar.trust().refused(bar.trust().withdrawn(Request::Apply));
                    bar.open_module_near(Module::Shield, Some(origin.upcast_ref()));
                }
            }
        }
    });

    let weak_bar = Rc::downgrade(bar);
    let ask_logind = move || {
        if let (Some(bar), Some(update)) = (weak_bar.upgrade(), &update) {
            update.set_visible(bar.trust().restart_to_update_offered());
        }
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
