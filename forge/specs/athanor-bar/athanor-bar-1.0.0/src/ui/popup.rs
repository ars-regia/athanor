//! A bar button with a popover (doc_bar.md, BR6). One popover is open for the whole bar,
//! it opens towards the inside of the screen, and the button tells assistive technologies
//! that it has a popup and whether it is expanded.

use std::rc::Rc;

use athanor_layout::preset::PanelEdge;
use gtk4::accessible::{Property, State};
use gtk4::prelude::*;

use super::Bar;

pub struct Popup {
    pub button: gtk4::Button,
    pub popover: gtk4::Popover,
}

impl Popup {
    pub fn new(bar: &Rc<Bar>, child: &impl IsA<gtk4::Widget>, name: &str) -> Popup {
        let button = gtk4::Button::new();
        button.set_child(Some(child));
        button.add_css_class("bar-button");
        button.set_tooltip_text(Some(name));
        button.update_property(&[Property::Label(name)]);
        let popover = attach(bar, &button);
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
        Popup { button, popover }
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

/// A popover for `button`: parented to it, opening towards the inside of the screen, and
/// keeping the button's `Expanded` state. The running applications' context menu uses it
/// too, with its own triggers.
pub fn attach(bar: &Rc<Bar>, button: &gtk4::Button) -> gtk4::Popover {
    button.update_property(&[Property::HasPopup(true)]);
    button.update_state(&[State::Expanded(Some(false))]);
    let popover = gtk4::Popover::new();
    popover.add_css_class("athanor-bar-popover");
    popover.set_parent(button);
    popover.set_position(match bar.layout().panel() {
        PanelEdge::Top => gtk4::PositionType::Bottom,
        PanelEdge::Bottom => gtk4::PositionType::Top,
    });
    let expanded = |button: &gtk4::Button, open: bool| {
        button.update_state(&[State::Expanded(Some(open))]);
    };
    let weak_button = button.downgrade();
    popover.connect_show(move |_| {
        if let Some(button) = weak_button.upgrade() {
            expanded(&button, true);
        }
    });
    let weak_button = button.downgrade();
    popover.connect_closed(move |_| {
        if let Some(button) = weak_button.upgrade() {
            expanded(&button, false);
        }
    });
    // The popover is parented by hand, so it is unparented by hand.
    let child = popover.clone();
    button.connect_destroy(move |_| child.unparent());
    popover
}
