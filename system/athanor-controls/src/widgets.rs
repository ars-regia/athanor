//! The pieces the pages and the surfaces that hold them share.

use gtk4::prelude::*;

use crate::i18n::tr;

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

/// GTK 4.20 gives a check button no AT-SPI action: its accessible lists only the widget's own
/// parameterless actions. Without one, an assistive technology that acts through AT-SPI
/// (voice control, switch access, the rig) cannot choose a radio item. `radio.choose`
/// selects this item, as a click would, so its `toggled` handler runs.
pub fn expose_choose_action(button: &gtk4::CheckButton) {
    let choose = gtk4::gio::SimpleAction::new("choose", None);
    let weak = button.downgrade();
    choose.connect_activate(move |_, _| {
        if let Some(button) = weak.upgrade() {
            button.set_active(true);
        }
    });
    let group = gtk4::gio::SimpleActionGroup::new();
    group.add_action(&choose);
    button.insert_action_group("radio", Some(&group));
}

/// The page's root: its content, with the failure note under it. An action the person took
/// did not complete: the note shows until the page is hidden, and assistive technologies
/// announce it.
pub struct Failure {
    column: gtk4::Box,
    note: gtk4::Label,
}

impl Failure {
    pub fn new(content: &impl IsA<gtk4::Widget>) -> Failure {
        let note = gtk4::Label::new(Some(&tr("The action did not complete.")));
        note.add_css_class("bar-popover-note");
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.set_visible(false);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        column.append(content);
        column.append(&note);
        // A hidden page shows nothing, and would show a stale note when it is shown again.
        let hidden = note.downgrade();
        column.connect_unmap(move |_| {
            if let Some(note) = hidden.upgrade() {
                note.set_visible(false);
            }
        });
        Failure { column, note }
    }

    pub fn widget(&self) -> gtk4::Widget {
        self.column.clone().upcast()
    }

    /// Says that an action did not complete, if the page is on screen.
    pub fn show(&self) {
        if !self.column.is_mapped() {
            return;
        }
        self.note.set_visible(true);
        self.note.announce(
            &self.note.text(),
            gtk4::AccessibleAnnouncementPriority::Medium,
        );
    }
}
