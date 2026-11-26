//! The lock's confinement at start (doc_lock_and_prompts.md, LP15), as the greeter's: one
//! thread, then Landlock with writes only beneath `$XDG_RUNTIME_DIR`, no TCP and no abstract
//! Unix socket of the session, then no core file. Reads stay open, as for every Sal program
//! (`athanor_unit::sandbox::restrict_writes`): fonts, icons and the locale come from `/usr`. The
//! Wayland socket, the buses and `athanor-unlockd`'s socket are bound to paths and connected to,
//! which Landlock does not mediate. The crate is built without Sal's `gpu` feature, so no GPU
//! node is opened and none is granted.
//!
//! The confinement is in process because the lock's unit must keep the user manager's user
//! namespace: the directives that would give it a private `/tmp`, a read-only system and a
//! private network make one owned by the user, where every process of the same uid could attach
//! to the lock (D43, #1558). The unit keeps the lines that do not, among them
//! `SystemCallArchitectures=native`, `SystemCallFilter=` (seccomp, which also refuses the
//! metadata calls Landlock does not mediate) and `RestrictAddressFamilies=AF_UNIX`.
//!
//! The process stays dumpable here: Sulphur identifies a client when it connects by reading
//! `/proc/<pid>/exe`, which the kernel refuses to a reader of the same uid without
//! `CAP_SYS_PTRACE` once the process is not dumpable. The lock turns non-dumpable in Sal's
//! `Application::start`, after the connection and before the first surface, while no secret
//! exists yet (maintainer decision, 2026-11-21).

use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;

use athanor_unit::{dirs, sandbox};

/// Confines the process, or fails: a refused thread check applies nothing, a later failure may
/// leave some layers applied, and either way the caller exits rather than run unconfined
/// (LP15), and Sulphur keeps a session locked while no lock client runs (LP10).
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
    let runtime_dir =
        dirs::own_dir(runtime_dir).map_err(|error| dirs::not_granted(runtime_dir, error))?;
    sandbox::ensure_single_threaded()?;
    sandbox::restrict_writes(&[runtime_dir], &[])?;
    // In place of the unit's former `PrivateNetwork=yes` (D43): `RestrictAddressFamilies=AF_UNIX`,
    // with `SystemCallArchitectures=native`, refuses every other family, and these the TCP and
    // abstract sockets a private network hid.
    sandbox::deny_tcp()?;
    sandbox::deny_abstract_unix_sockets()?;
    // After Landlock, as the greeter's `forbid_core_dumps`: no core file by any path, the
    // ptrace-based ones that ignore the dumpable flag included (LP1, LP15).
    nix::sys::resource::setrlimit(nix::sys::resource::Resource::RLIMIT_CORE, 0, 0)?;
    Ok(())
}
