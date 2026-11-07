//! The logic of `athanor-idle`, with no display and no bus (`doc_session_daemons.md`, SD4,
//! SD5, SD19). The Wayland and D-Bus sides, the schema and the migration come in later pull
//! requests and drive these types.

pub mod inhibitors;
pub mod screensaver;
pub mod sequence;
pub mod store;
pub mod timers;

pub use inhibitors::{Inhibitor, Inhibitors, Kind, RemoveError};
pub use sequence::{
    Action, Event, Hint, Phase, Sequence, Timer, FADE, LOCK_DELAY, LOCK_FAILED_NOTICE, LOCK_RETRY,
    LOCK_TIMEOUT,
};
pub use timers::{choose, Config, Hold, Timeouts};
