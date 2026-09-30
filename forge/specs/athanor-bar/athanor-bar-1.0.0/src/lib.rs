//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, what logind offers, the time zone, and the state of NetworkManager,
//! BlueZ, the sound server and UPower. The binary draws it.

pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod clock;
pub mod dbusmenu;
pub mod keyboard;
pub mod network;
pub mod notices;
pub mod order;
pub mod popups;
pub mod power;
pub mod props;
pub mod tiling;
pub mod tray;
