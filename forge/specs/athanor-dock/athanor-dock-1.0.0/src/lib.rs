//! The logic of athanor-dock, with no GTK type (doc_bar.md, BR7): which outputs get a
//! dock, on which edge, and when an auto-hiding dock shows; the start every dock makes
//! and its translations. The binaries draw it.

pub mod autohide;
pub mod i18n;
pub mod placement;
pub mod start;
