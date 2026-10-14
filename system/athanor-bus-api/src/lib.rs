//! The shared polkit client of the Athanor daemons.
//!
//! [`polkit`] holds the subject types, the authority proxy and the caller-authorization
//! helpers used by every daemon that gates a D-Bus method behind an action id.
//!
//! This is a library crate: it has no `main.rs` and produces no binary or daemon. It is
//! consumed as a path dependency by the daemons that need it.

pub mod polkit;
