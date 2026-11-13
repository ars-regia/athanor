//! Athanor's own widget toolkit (ADR-0121, doc_own_stack.md OS7 to OS9): a Wayland driver on
//! smithay-client-toolkit, widgets laid out in logical pixels and mirrored in right-to-left, text
//! on Parley, drawing on vello_cpu (or vello_gpu, with the `gpu` feature) and accessibility on AccessKit.

#![warn(missing_docs)]

// The chooser's tree is shared by the example and the tests, which name the crate.
extern crate self as athanor_sal;

pub(crate) mod a11y;
mod align;
pub mod driver;
pub(crate) mod focus;
#[cfg(feature = "gpu")]
pub(crate) mod gpu;
mod icon;
mod motion;
mod popup;
mod range;
pub(crate) mod render;
pub(crate) mod text;
mod theme;
mod widget;
mod wrap;

pub use align::Align;
pub use driver::{run_with, Application, LayerOptions, Shell, Side, SurfaceId};
pub use icon::{IconTheme, Image, FALLBACK_ICON, MAX_IMAGE_SIDE};
pub use motion::{Curve, Motion, Pose};
pub use render::Error;
pub use text::TextStyle;
pub use theme::{Rgba, Theme};
pub use wrap::Wrap;
pub use widget::{
    Event, Key, Kind, Level, PointerButton, Rect, RootRole, Slider, Ui, Widget, WidgetId, WindowButton,
};

#[cfg(test)]
#[path = "../examples/chooser/tree.rs"]
mod chooser;
