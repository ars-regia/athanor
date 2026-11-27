//! The polkit authentication agent of doc_lock_and_prompts.md (LP11). This build is the
//! toolkit-free core of LP18 step 5 alone: it registers with nobody and draws nothing, so it says
//! so and exits with a failure rather than run as if it were the session's agent. The helper
//! client, the bus and the screen follow.

use std::process::ExitCode;

fn main() -> ExitCode {
    athanor_unit::panic::install();
    eprintln!(
        "<3>athanor-polkit-agent: this build is not an authentication agent yet (doc_lock_and_prompts.md, LP18 step 5); exiting"
    );
    ExitCode::FAILURE
}
