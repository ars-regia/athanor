//! The detail pages of the shell's system services (doc_control_center.md, CC3): network,
//! Bluetooth, audio with the media controls, and battery. The bar shows each in a popover;
//! the control center shows the same pages. A page draws the state of a model of
//! athanor-services and sends the person's actions back to it; it knows nothing of the
//! surface that holds it, which hands it a [`Host`] where it needs one. Everything here runs
//! on the GTK main thread.

pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod bridge;
pub mod i18n;
mod media;
pub mod network;
pub mod widgets;

mod host;
mod services;

pub use host::Host;
pub use services::Services;
