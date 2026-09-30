//! A bar button with a popover (doc_bar.md, BR6). One popover is open for the whole bar,
//! it opens towards the inside of the screen, and the button tells assistive technologies
//! that it has a popup and whether it is expanded.

use std::rc::Rc;

use athanor_layout::preset::PanelEdge;
use gtk4::accessible::Property;
use gtk4::prelude::*;

use super::Bar;
use crate::i18n::tr;

pub struct Popup {
    pub button: gtk4::Button,
    pub popover: gtk4::Popover,
    /// Under the content set by [`Popup::set_content`]: an action did not complete.
    note: gtk4::Label,
}

impl Popup {
    pub fn new(bar: &Rc<Bar>, child: &impl IsA<gtk4::Widget>, name: &str) -> Popup {
        let button = gtk4::Button::new();
        button.set_child(Some(child));
        button.add_css_class("bar-button");
        button.set_tooltip_text(Some(name));
        button.update_property(&[Property::Label(name)]);
        let popover = attach(bar, &button);
        let note = gtk4::Label::new(Some(&tr("The action did not complete.")));
        note.add_css_class("bar-popover-note");
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.set_visible(false);
        let hidden = note.downgrade();
        popover.connect_closed(move |_| {
            if let Some(note) = hidden.upgrade() {
                note.set_visible(false);
            }
        });
        let weak_bar = Rc::downgrade(bar);
        let toggled = popover.clone();
        button.connect_clicked(move |_| {
            if toggled.is_visible() {
                toggled.popdown();
            } else if let Some(bar) = weak_bar.upgrade() {
                bar.popover_opened(&toggled);
                toggled.popup();
            }
        });
        Popup {
            button,
            popover,
            note,
        }
    }

    /// The popover's content, with the failure note under it.
    pub fn set_content(&self, content: &impl IsA<gtk4::Widget>) {
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        column.append(content);
        column.append(&self.note);
        self.popover.set_child(Some(&column));
    }

    /// An action the person took in the open popover did not complete: the note shows until
    /// the popover closes, and assistive technologies announce it. A closed popover shows
    /// nothing, and would show a stale note at its next opening.
    pub fn failed(&self) {
        if !self.popover.is_visible() {
            return;
        }
        self.note.set_visible(true);
        self.note.announce(
            &self.note.text(),
            gtk4::AccessibleAnnouncementPriority::Medium,
        );
    }

    pub fn open(&self, bar: &Rc<Bar>) {
        bar.popover_opened(&self.popover);
        self.popover.popup();
    }
}

/// A labelled switch, as a row of popover content: the label and the switch, with the
/// switch's `LabelledBy` relation set to it.
pub fn switch_row(text: &str) -> (gtk4::Box, gtk4::Switch) {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let switch = gtk4::Switch::new();
    switch.set_valign(gtk4::Align::Center);
    switch.update_relation(&[gtk4::accessible::Relation::LabelledBy(
        &[label.upcast_ref()],
    )]);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.append(&label);
    row.append(&switch);
    (row, switch)
}

/// A popover for `button`, attached by [`attach_popover`].
pub fn attach(bar: &Rc<Bar>, button: &gtk4::Button) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    attach_popover(bar, button, &popover);
    popover
}

/// `athanor_apps::menu::attach_popover` towards the inside of the screen, for a popover of
/// the bar (`Popup`'s, or the tray's `PopoverMenu`), which also tells the bar when it shows
/// and closes so the notification popups hide under it (BR6, "Stacking"). The rows' menus
/// reach the bar through `Host::menu_opened` and `Host::hold` instead.
pub fn attach_popover(bar: &Rc<Bar>, button: &gtk4::Button, popover: &impl IsA<gtk4::Popover>) {
    athanor_apps::menu::attach_popover(button, popover, towards_inside(bar));
    let popover = popover.upcast_ref::<gtk4::Popover>();
    let weak_bar = Rc::downgrade(bar);
    popover.connect_show(move |_| {
        if let Some(bar) = weak_bar.upgrade() {
            bar.popovers_changed_later();
        }
    });
    let weak_bar = Rc::downgrade(bar);
    popover.connect_closed(move |_| {
        if let Some(bar) = weak_bar.upgrade() {
            bar.popovers_changed_later();
        }
    });
}

/// Where the bar's popovers and menus open: towards the inside of the screen.
pub fn towards_inside(bar: &Bar) -> gtk4::PositionType {
    match bar.layout().panel() {
        PanelEdge::Top => gtk4::PositionType::Bottom,
        PanelEdge::Bottom => gtk4::PositionType::Top,
    }
}
