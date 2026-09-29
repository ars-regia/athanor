//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, favourites against windows, what logind offers, the time zone. The
//! binary draws it.

pub mod clock;
pub mod keyboard;
pub mod order;
pub mod power;
pub mod running;
pub mod tiling;
