//! Entry points of the fuzz targets (`fuzz/fuzz_targets`). Each takes the raw bytes a fuzzer
//! mutates and must return for every input: a panic is a finding, and `panic = "abort"` makes
//! it a lost daemon. `tests/replay.rs` runs the committed corpus through the same functions
//! on stable, so a crash a fuzzer once found stays a regression test.

/// What an entry point returns: `Err` is a failure of the harness (a scratch directory, a
/// fixed type string), never a finding. A panic is the finding.
pub type Setup = std::io::Result<()>;

/// For the fuzz targets: a harness failure ends the run loudly instead of passing as coverage.
pub fn must(result: Setup) {
    if let Err(error) = result {
        eprintln!("fuzz harness failed: {error}");
        std::process::abort();
    }
}

#[cfg(feature = "gtk")]
pub mod bar;
#[cfg(feature = "gtk")]
pub mod compositor;
#[cfg(feature = "layout")]
pub mod layout;
#[cfg(feature = "shelld")]
pub mod shelld;
#[cfg(feature = "update")]
pub mod update;

// athanor-update is a binary crate, so the modules under test are included by path, at the
// crate root because tools.rs names `crate::sigobj`: the fuzzed code is the shipped source.
#[cfg(feature = "update")]
#[path = "../../../forge/specs/athanor-update/athanor-update-1.0.0/src/sigobj.rs"]
#[allow(dead_code)]
mod sigobj;
#[cfg(feature = "update")]
#[path = "../../../forge/specs/athanor-update/athanor-update-1.0.0/src/tools.rs"]
#[allow(dead_code)]
mod tools;

/// A private scratch directory named after `name`, created once per process under the
/// temporary directory and returned on every later call. The name carries the process id and
/// the clock in nanoseconds, so a reused pid never meets a stale directory; `create` (not
/// `create_dir_all`) with mode 0700 fails on a path another user placed in the shared
/// temporary directory, so the writes into it never follow someone else's symlink. Nothing
/// removes it (`panic = "abort"` skips destructors); it lives under the temporary directory.
#[cfg(any(feature = "update", feature = "gtk"))]
fn scratch_dir(name: &'static str) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    use std::sync::Mutex;
    static DIRS: Mutex<Vec<(&'static str, std::path::PathBuf)>> = Mutex::new(Vec::new());
    let mut dirs = DIRS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, dir)) = dirs.iter().find(|(known, _)| *known == name) {
        return Ok(dir.clone());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let dir = std::env::temp_dir().join(format!("athanor-fuzz-{name}-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    dirs.push((name, dir.clone()));
    Ok(dir)
}

/// Splits `data` at each NUL byte into at most `N` fields; a missing field is empty.
/// Lossy UTF-8: the text parsers take `&str`, and the interesting inputs are still text.
#[cfg(any(feature = "shelld", feature = "gtk"))]
fn fields<const N: usize>(data: &[u8]) -> [String; N] {
    let mut parts = data.splitn(N, |byte| *byte == 0);
    std::array::from_fn(|_| String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned())
}
