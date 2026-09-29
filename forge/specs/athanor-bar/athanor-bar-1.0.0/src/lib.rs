//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, what logind offers, the time zone. The
//! binary draws it.

pub mod clock;
pub mod keyboard;
pub mod notices;
pub mod order;
pub mod popups;
pub mod power;
pub mod tiling;
pub mod tray;
