//! The detail pages of the shell's system services (doc_control_center.md, CC3): network,
//! Bluetooth, audio with the media controls, and battery. The bar shows each in a popover;
//! the control center shows the same pages. A page draws the state of a model of
//! athanor-services and sends the person's actions back to it; it knows nothing of the
//! surface that holds it, which hands it a [`Host`] where it needs one. Everything here runs
//! on the thread of GLib's main context: GTK's main thread with the `gtk` feature, the
//! toolkit's loop with `sal`, whose pages live in [`sal`].

#[cfg(feature = "gtk")]
pub mod audio;
#[cfg(feature = "gtk")]
pub mod battery;
#[cfg(feature = "gtk")]
pub mod bluetooth;
pub mod bridge;
pub mod i18n;
mod labels;
mod media;
#[cfg(feature = "gtk")]
pub mod network;
#[cfg(feature = "sal")]
pub mod sal;
#[cfg(feature = "gtk")]
pub mod widgets;

#[cfg(feature = "gtk")]
mod host;

pub use athanor_services::Services;
#[cfg(feature = "gtk")]
pub use host::Host;
