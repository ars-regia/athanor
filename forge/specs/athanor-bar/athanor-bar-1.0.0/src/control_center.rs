//! `os.athanor.ControlCenter1`, as the bar reads it (doc_control_center.md, CC9): the property
//! `Open`, which hides the notification popups while the panel is shown. Everything the peer
//! sends is type-checked here; a wrong type is `None`, never a panic.

use glib::Variant;

use crate::props::{self, Change};

pub const NAME: &str = "os.athanor.ControlCenter1";
pub const PATH: &str = "/os/athanor/ControlCenter1";

/// `Open` in the `GetAll` reply of the interface.
pub fn open_in_get_all(reply: &Variant) -> Option<bool> {
    props::value(&props::get_all(reply)?, "Open")
}

/// `Open` in a `PropertiesChanged` signal's parameters; `None` for another interface, for a
/// change that does not carry it, and for a wrong type.
pub fn open_in_change(params: &Variant) -> Option<bool> {
    match props::change(
        PATH,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        params,
    )? {
        Change::Properties {
            interface, changed, ..
        } if interface == NAME => props::value(&changed, "Open"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;
    use std::collections::HashMap;

    fn changed(interface: &str, open: Variant) -> Variant {
        let props = HashMap::from([("Open".to_owned(), open)]);
        (interface, props, Vec::<String>::new()).to_variant()
    }

    #[test]
    fn open_is_read_from_a_change_of_its_interface() {
        assert_eq!(
            open_in_change(&changed(NAME, true.to_variant())),
            Some(true)
        );
        assert_eq!(
            open_in_change(&changed(NAME, false.to_variant())),
            Some(false)
        );
    }

    #[test]
    fn another_interface_a_wrong_type_or_no_open_is_none() {
        assert_eq!(
            open_in_change(&changed("org.example.Other", true.to_variant())),
            None
        );
        assert_eq!(open_in_change(&changed(NAME, "yes".to_variant())), None);
        assert_eq!(open_in_change(&("x", 1u32).to_variant()), None);
        let silent = (
            NAME,
            HashMap::<String, Variant>::new(),
            vec!["Open".to_owned()],
        )
            .to_variant();
        assert_eq!(open_in_change(&silent), None);
    }

    #[test]
    fn open_is_read_from_get_all() {
        let reply = (HashMap::from([("Open".to_owned(), true.to_variant())]),).to_variant();
        assert_eq!(open_in_get_all(&reply), Some(true));
        assert_eq!(
            open_in_get_all(&(HashMap::<String, Variant>::new(),).to_variant()),
            None
        );
        assert_eq!(open_in_get_all(&"x".to_variant()), None);
    }
}
