//! Popovers for buttons (doc_bar.md, BR6): parented to the button, opening where the caller
//! says (towards the inside of the screen), and keeping the button's `HasPopup` and
//! `Expanded` state. Every popover of the bar and of the dock is attached here; what its
//! showing or closing means to the program is the caller's to connect.

use gtk4::accessible::{Property, State};
use gtk4::prelude::*;

/// A new popover for `button`, attached by [`attach_popover`].
pub fn attach(button: &gtk4::Button, position: gtk4::PositionType) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    attach_popover(button, &popover, position);
    popover
}

/// Attaches `popover`, a `gtk4::Popover` or a subclass, to `button`.
pub fn attach_popover(
    button: &gtk4::Button,
    popover: &impl IsA<gtk4::Popover>,
    position: gtk4::PositionType,
) {
    let popover = popover.upcast_ref::<gtk4::Popover>();
    button.update_property(&[Property::HasPopup(true)]);
    button.update_state(&[State::Expanded(Some(false))]);
    // One class for the bar's and the dock's popovers: they look the same.
    popover.add_css_class("athanor-bar-popover");
    popover.set_parent(button);
    popover.set_position(position);
    crate::timing::watch(popover, "popover");
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
}
