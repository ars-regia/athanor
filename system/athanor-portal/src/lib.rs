//! The decisions of Athanor's portal backend that the tests reach without a display
//! (doc_portal.md, PT2). No GTK and no GLib type: the backend reads the values and shows the
//! dialogs, this crate says what they mean and serves what needs no display.

pub mod remote_desktop;
pub mod settings;
