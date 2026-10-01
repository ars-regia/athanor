//! What the launcher and the library search, and in which order they show it
//! (doc_launcher.md LA2-LA4). Every source answers on the GLib main loop; nothing here
//! starts a thread or touches GTK.

pub mod apps;
pub mod calc;
pub mod command;
pub mod files;
pub mod item;
pub mod providers;
pub mod rank;
pub mod usage;
pub mod web;
pub mod windows;
