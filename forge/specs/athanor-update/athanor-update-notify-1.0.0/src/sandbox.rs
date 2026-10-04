//! The notifier's state directory. Its confinement is athanor-unit's (`restrict`): reads
//! are handled too, because this process needs almost nothing: `/usr` (its libraries and
//! the translations), the state directory of athanor-update, one directory of its own to
//! write, and the session and system buses to connect to.
use std::path::PathBuf;

pub use athanor_unit::sandbox::{ensure_single_threaded, restrict, session_bus, system_bus};

/// `$XDG_STATE_HOME/athanor-update-notify`, created if missing.
pub fn state_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let dir = base.filter(|dir| dir.is_absolute()).ok_or("neither XDG_STATE_HOME nor HOME is an absolute path")?.join("athanor-update-notify");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
