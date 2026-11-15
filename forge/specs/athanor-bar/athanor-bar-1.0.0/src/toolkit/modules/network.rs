//! The bar's network (doc_bar.md, BR3). Not drawn yet: the GTK bar's
//! `src/ui/network.rs`, removed with the GTK bar, is the reference to port, and its goldens the
//! look to match or to record a difference against.

use super::{Module, Start};

/// `None` until the module is ported: the bar shows no button for it.
pub fn new(_start: &Start) -> Option<Box<dyn Module>> {
    None
}
