//! The accessible name of a widget (ST7). A control is named by [`name`]: the label property
//! for assistive technologies and the tooltip for sighted people, from one call, so the name
//! the tests read back (GTK has no getter for a property's value) is the one that was set. A
//! container or a picture is labelled by [`label`] only: GTK4 inherits tooltips, so one on a
//! container would show over every unnamed area inside it.

use gtk4::accessible::Property;
use gtk4::prelude::*;

pub fn label(widget: &impl IsA<gtk4::Accessible>, text: &str) {
    widget.update_property(&[Property::Label(text)]);
}

pub fn name(widget: &(impl IsA<gtk4::Widget> + IsA<gtk4::Accessible>), text: &str) {
    label(widget, text);
    widget.set_tooltip_text(Some(text));
}

/// Whether `widget` has a label, and no tooltip of its own.
#[cfg(test)]
pub fn is_labelled(widget: &(impl IsA<gtk4::Widget> + IsA<gtk4::Accessible>)) -> bool {
    gtk4::test_accessible_has_property(widget, gtk4::AccessibleProperty::Label)
        && widget.tooltip_text().is_none()
}

/// Whether `widget` is named `expected`: it has the label property and the name set is that.
#[cfg(test)]
pub fn is_named(widget: &(impl IsA<gtk4::Widget> + IsA<gtk4::Accessible>), expected: &str) -> bool {
    gtk4::test_accessible_has_property(widget, gtk4::AccessibleProperty::Label)
        && widget.tooltip_text().as_deref() == Some(expected)
}
