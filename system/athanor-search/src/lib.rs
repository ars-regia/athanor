//! What the launcher and the library search, and in which order they show it
//! (doc_launcher.md LA2-LA4). Every source answers on the GLib main loop; nothing here
//! starts a thread or touches GTK.

pub mod item;
pub mod rank;
pub mod usage;
