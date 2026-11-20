//! The lock's confinement at start (doc_lock_and_prompts.md, LP15), as the greeter's: one
//! thread, then Landlock with writes only beneath `$XDG_RUNTIME_DIR`, then non-dumpable. Reads
//! stay open, as for every Sal program (`athanor_unit::sandbox::restrict_writes`): fonts, icons
//! and the locale come from `/usr`. The Wayland socket, the buses and `athanor-unlockd`'s socket
//! are connected to, which Landlock does not mediate. The crate is built without Sal's `gpu`
//! feature, so no GPU node is opened and none is granted.

use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;

use athanor_unit::{dirs, sandbox};

/// Confines the process, or fails and applies nothing: the caller then exits rather than run
/// unconfined (LP15), and Sulphur keeps a session locked while no lock client runs (LP10).
///
/// The thread count is read here, right before the ruleset, because Landlock confines the
/// calling thread and those it starts afterwards, never one that already runs: a second thread,
/// the logind thread started too early say, fails the call. `runtime_dir` is
/// `$XDG_RUNTIME_DIR`, which must be an absolute directory of this user; Landlock grants it by
/// the descriptor `dirs` opened without following a link. A kernel that cannot enforce the
/// ruleset is an error, not a best effort.
pub fn confine(runtime_dir: Option<&OsStr>) -> Result<(), Box<dyn Error>> {
    let runtime_dir = runtime_dir
        .map(Path::new)
        .filter(|dir| dir.is_absolute())
        .ok_or("XDG_RUNTIME_DIR is unset or not absolute")?;
    let runtime_dir = dirs::own_dir(runtime_dir)
        .map_err(|error| format!("{} is not granted: {error}", runtime_dir.display()))?;
    sandbox::ensure_single_threaded()?;
    sandbox::restrict_writes(&[runtime_dir], &[])?;
    // After Landlock, as the greeter: no core dump and no ptrace by the user (LP1, LP15).
    nix::sys::prctl::set_dumpable(false)?;
    Ok(())
}
