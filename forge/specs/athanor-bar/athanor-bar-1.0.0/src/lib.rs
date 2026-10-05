//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, what logind offers, the time zone, and the state of NetworkManager,
//! BlueZ and UPower, and the trust shield. The binary draws it.

pub mod clock;
pub mod control_center;
pub mod dbusmenu;
pub mod fullscreen;
pub mod keyboard;
pub mod notices;
pub mod order;
pub mod popups;
pub mod power;
pub mod props;
pub mod shield;
pub mod tiling;
pub mod tray;

/// Until the other models move (control-center plan, Task 7), the modules that name
/// `athanor_bar::battery` keep their path.
pub use athanor_services::battery;
