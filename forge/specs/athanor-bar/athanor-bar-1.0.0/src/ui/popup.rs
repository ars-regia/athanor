//! A bar button with a popover (doc_bar.md, BR6). One popover is open for the whole bar,
//! it opens towards the inside of the screen, and the button tells assistive technologies
//! that it has a popup and whether it is expanded.

use std::rc::Rc;

pub use athanor_controls::widgets::{expose_choose_action, switch_row};
use athanor_controls::Host;
use athanor_layout::preset::PanelEdge;
use gtk4::accessible::Property;
use gtk4::prelude::*;

use super::Bar;

#[derive(Clone)]
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

    /// What a page in this popover asks of it: to open, to close, and whether its button is
    /// on screen.
    pub fn host(&self, bar: &Rc<Bar>) -> Host {
        let (popup, weak_bar) = (self.clone(), Rc::downgrade(bar));
        let popover = self.popover.clone();
        let button = self.button.clone();
        Host::new(
            move || {
                if !popup.popover.is_visible() {
                    if let Some(bar) = weak_bar.upgrade() {
                        popup.open(&bar);
                    }
                }
            },
            move || popover.popdown(),
            move || button.is_mapped(),
        )
    }

    /// The module has nothing to show: its popover closes and its button leaves.
    pub fn hide(&self) {
        self.popover.popdown();
        self.button.set_visible(false);
    }

    pub fn open(&self, bar: &Rc<Bar>) {
        bar.popover_opened(&self.popover);
        self.popover.popup();
    }
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
