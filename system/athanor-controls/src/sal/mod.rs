//! The pages drawn with Athanor's own toolkit (doc_own_stack.md, OS7): the same pages as the GTK
//! ones, which the control center's stack and the bar's popovers hold. A page adds its widgets
//! under a parent of the surface's [`athanor_sal::Ui`], applies the model's state to them in the
//! surface's turn, and takes the events of its own widgets. A surface draws its tree again when a
//! page says its shape changed, a network or a device coming or going.

pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod network;
pub mod widgets;

/// What a page's turn did to its widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// Nothing new to show.
    Unchanged,
    /// It set its widgets to the model's state: the surface lays out and draws again.
    Updated,
    /// Its shape changed, a device came or went: the surface builds its widgets again, with a
    /// new [`athanor_sal::Ui`] in which the page's `build` runs again.
    Rebuild,
}
