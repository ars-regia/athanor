//! Athanor's own widget toolkit (ADR-0121, doc_own_stack.md OS7 to OS9): a Wayland driver on
//! smithay-client-toolkit, widgets laid out in logical pixels and mirrored in right-to-left, text
//! on Parley, drawing on vello_cpu (or vello_gpu, with the `gpu` feature) and accessibility on AccessKit.

#![warn(missing_docs)]

// The chooser's tree is shared by the example and the tests, which name the crate.
extern crate self as athanor_toolkit;

pub mod a11y;
pub mod driver;
pub mod focus;
#[cfg(feature = "gpu")]
pub mod gpu;
mod popup;
pub mod render;
pub mod text;
pub mod theme;
pub mod widget;

pub use theme::{Rgba, Theme};
pub use widget::{
    Event, Key, Kind, PointerButton, Rect, Slider, Ui, Widget, WidgetId, WindowButton,
};

#[cfg(test)]
#[path = "../examples/chooser/tree.rs"]
mod chooser;
