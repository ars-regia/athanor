//! Confinement of the chooser (doc_update_trust.md binds user-side processes to restrict
//! themselves with Landlock at start, as the greeter does), with athanor-unit's ruleset. The
//! chooser writes the user layout document, and a newer one's backup beside it; GTK and Mesa
//! write the cache and the runtime directory; GPU rendering opens and drives the DRM nodes.
//! It connects to the compositor and the accessibility bus in the runtime directory, and to
//! the session bus.

use std::env;
use std::path::{Path, PathBuf};

use athanor_layout::user::write_target;
use athanor_unit::sandbox;

pub use athanor_unit::sandbox::ensure_single_threaded;

const DRM_DEVICE_DIR: &str = "/dev/dri";

/// Confines the process to what `writable` lists for `user_file`. A kernel below Landlock
/// ABI 9 is an error, never a best-effort no-op.
pub fn apply(user_file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Created before the ruleset, so that the grant has a directory to hold on to.
    let document_dir = write_target(user_file)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("the layout document has no directory")?;
    std::fs::create_dir_all(&document_dir)?;
    let write = writable(&document_dir);
    let connect: Vec<PathBuf> = runtime_dir().into_iter().chain(sandbox::session_bus()).collect();
    sandbox::restrict_writes(
        &write.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        &[Path::new(DRM_DEVICE_DIR)],
        &connect.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
    )
}

/// The document's directory, /tmp, the cache and the runtime directory.
fn writable(document_dir: &Path) -> Vec<PathBuf> {
    let mut paths = vec![document_dir.to_path_buf(), PathBuf::from("/tmp")];
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    paths.extend(cache);
    paths.extend(runtime_dir());
    paths.retain(|path| path.is_absolute());
    paths
}

fn runtime_dir() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(test: &str) -> PathBuf {
        let base = env::temp_dir().join(format!(
            "athanor-chooser-landlock-{}-{test}",
            std::process::id()
        ));
        std::fs::create_dir_all(base.join("dotfiles")).expect("mkdir");
        std::fs::create_dir_all(base.join("config/athanor")).expect("mkdir");
        std::fs::create_dir_all(base.join("elsewhere")).expect("mkdir");
        base
    }

    #[test]
    fn the_document_directory_is_the_link_target_s() {
        let base = probe("link");
        let link = base.join("config/athanor/layout.toml");
        std::os::unix::fs::symlink(base.join("dotfiles/layout.toml"), &link).expect("symlink");
        let granted = writable(write_target(&link).expect("target").parent().expect("dir"));
        assert_eq!(granted[0], base.join("dotfiles"));
        assert!(!granted.contains(&base.join("config/athanor")));
    }
}
