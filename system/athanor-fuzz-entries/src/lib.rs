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

/// Splits `data` at each NUL byte into at most `N` fields; a missing field is empty.
/// Lossy UTF-8: the text parsers take `&str`, and the interesting inputs are still text.
#[cfg(any(feature = "shelld", feature = "gtk"))]
fn fields<const N: usize>(data: &[u8]) -> [String; N] {
    let mut parts = data.splitn(N, |byte| *byte == 0);
    std::array::from_fn(|_| String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned())
}
