//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): count
//! its failures and give up after five in ten minutes, log at journal priorities, tell
//! systemd it is ready, and confine itself with Landlock.

pub mod crash_loop;
pub mod journal;
pub mod notify;
pub mod sandbox;
