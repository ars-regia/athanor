//! athanor-shelld (doc_bar.md BR1): the session's notifications server and StatusNotifier
//! watcher, and the private interface the bar reads them through. Every module but the
//! D-Bus layer is plain Rust, tested without a bus. The bar links this crate for `wire`.

pub mod dnd;
pub mod hints;
pub mod icon;
pub mod image;
pub mod notifications;
pub mod sender;
pub mod server;
pub mod store;
pub mod watcher;
pub mod wire;

use std::error::Error;
use std::path::Path;

/// Landlock (BR1): reads beneath `/usr` and `/etc`, every access beneath `state_dir`, held as
/// `athanor_unit::dirs::owned_dir` checked it (#1052), nothing else. No `/proc`: a caller's unit comes from the bus and systemd ([`sender`]). The caller
/// has proved the process single-threaded (`athanor_unit::sandbox::ensure_single_threaded`).
pub fn confine(state_dir: &Path) -> Result<(), Box<dyn Error>> {
    athanor_unit::sandbox::restrict(
        &[Path::new("/usr"), Path::new("/etc")],
        &[athanor_unit::dirs::owned_dir(state_dir)?],
    )
}
