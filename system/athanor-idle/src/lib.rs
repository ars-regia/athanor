//! `athanor-idle` (`doc_session_daemons.md`, SD3, SD4, SD5, SD19): the logic with no display
//! and no bus, tested without either, and the Wayland and D-Bus sides the daemon drives it
//! with.

pub mod battery;
pub mod bus;
pub mod confine;
pub mod follow;
pub mod inhibitors;
pub mod screensaver;
pub mod sequence;
pub mod store;
pub mod timers;
pub mod wayland;

pub use inhibitors::{Inhibitor, Inhibitors, Kind, RemoveError};
pub use sequence::{
    Action, Event, Hint, Phase, Sequence, Timer, FADE, LOCK_DELAY, LOCK_FAILED_NOTICE, LOCK_RETRY,
    LOCK_TIMEOUT,
};
pub use timers::{choose, Config, Hold, Timeouts};
