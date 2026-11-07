//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): find
//! its directories, count its failures and give up after five in ten minutes, log at journal priorities, tell
//! systemd it is ready, confine itself with Landlock, make other processes' text safe
//! to show, and, with the `bus-caller` feature, tell which unit a D-Bus caller runs in.

#[cfg(feature = "bus-caller")]
pub mod bus_caller;
pub mod crash_loop;
pub mod dirs;
pub mod journal;
pub mod notify;
pub mod sandbox;
pub mod text;
