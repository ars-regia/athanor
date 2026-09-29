//! athanor-shelld (doc_bar.md BR1): the session's notifications server and StatusNotifier
//! watcher, and the private interface the bar reads them through. Every module but the
//! D-Bus layer is plain Rust, tested without a bus. The bar links this crate for `wire`.

pub mod dnd;
pub mod hints;
pub mod icon;
pub mod image;
pub mod notifications;
pub mod sender;
pub mod server;
pub mod store;
pub mod text;
pub mod watcher;
pub mod wire;
