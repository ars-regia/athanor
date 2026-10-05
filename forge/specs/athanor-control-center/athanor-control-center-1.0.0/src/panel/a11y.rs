//! The accessible name of a control (ST7). The name goes to assistive technologies as the
//! label property and to sighted people as the tooltip, from one call, so the one the tests
//! can read back (GTK has no getter for a property's value) is the one that was set.

use gtk4::accessible::Property;
use gtk4::prelude::*;

pub fn name(widget: &(impl IsA<gtk4::Widget> + IsA<gtk4::Accessible>), text: &str) {
    widget.update_property(&[Property::Label(text)]);
    widget.set_tooltip_text(Some(text));
}

/// Whether `widget` is named `expected`: it has the label property and the name set is that.
#[cfg(test)]
pub fn is_named(widget: &(impl IsA<gtk4::Widget> + IsA<gtk4::Accessible>), expected: &str) -> bool {
    gtk4::test_accessible_has_property(widget, gtk4::AccessibleProperty::Label)
        && widget.tooltip_text().as_deref() == Some(expected)
}
