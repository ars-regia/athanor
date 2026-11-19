//! The session lock of doc_lock_and_prompts.md (LP2). This build is the crate, the unit and the
//! package of LP18 step 2 alone: it locks nothing, so it says so and exits with a failure rather
//! than run as if it guarded the session. The state machine, logind and the screen follow.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!(
        "athanor-lock: this build does not lock the session yet (doc_lock_and_prompts.md, LP18 step 2); exiting"
    );
    ExitCode::FAILURE
}
