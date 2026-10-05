//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): find
//! its directories, count its failures and give up after five in ten minutes, log at journal priorities, tell
//! systemd it is ready, confine itself with Landlock, and make other processes' text safe
//! to show.

pub mod crash_loop;
pub mod dirs;
pub mod icon;
pub mod journal;
pub mod notify;
pub mod sandbox;
pub mod text;
