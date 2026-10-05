//! athanor-shelld (doc_bar.md BR1): the session's notifications server and StatusNotifier
//! watcher, and the private interface the bar reads them through. Every module but the
//! D-Bus layer is plain Rust, tested without a bus. The bar links this crate for `wire`.

pub mod battery;
pub mod clock;
pub mod dnd;
pub mod hints;
pub mod history;
pub mod i18n;
pub mod icon;
pub mod identity;
pub mod image;
pub mod notifications;
pub mod policy;
pub mod rules;
pub mod sender;
pub mod server;
pub mod sound;
pub mod store;
pub mod watcher;
pub mod wire;
