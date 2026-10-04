//! COSMIC's configuration on disk, read without libcosmic. cosmic-config resolves every
//! key on its own, the user's file first, then the system directories; so do we.

use std::env;
use std::fs;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/cosmic`, then `<dir>/cosmic` for every directory in `XDG_DATA_DIRS`.
pub(crate) fn dirs() -> Vec<PathBuf> {
    user_dir().into_iter().chain(system_dirs()).collect()
}

/// `<dir>/cosmic` for every directory in `XDG_DATA_DIRS`: the defaults the packages ship.
pub(crate) fn system_dirs() -> Vec<PathBuf> {
    let data = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty());
    let data = data.as_deref().unwrap_or("/usr/local/share:/usr/share");
    data.split(':')
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("cosmic"))
        .collect()
}

/// `$XDG_CONFIG_HOME/cosmic`, the only directory whose keys change while a session runs.
pub(crate) fn user_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("cosmic"))
}

/// The directory of a component's keys under a `cosmic` directory.
pub(crate) fn component(dir: &std::path::Path, component: &str) -> PathBuf {
    dir.join(component).join("v1")
}

/// The first readable copy of a key, highest directory first.
pub(crate) fn key(dirs: &[PathBuf], component_name: &str, name: &str) -> Option<String> {
    dirs.iter()
        .find_map(|dir| fs::read_to_string(component(dir, component_name).join(name)).ok())
}
