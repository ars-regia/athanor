//! The logic of `athanor-sessiond` (`doc_session_daemons.md` SD1, SD14, SD16, SD19): which
//! battery notice UPower's warning level asks for and its text, and the reset of COSMIC's
//! appearance values; `bus` drives the notices on the bus. The headless daemon has no GTK and
//! no Wayland connection.

pub mod appearance;
pub mod battery;
pub mod bus;
