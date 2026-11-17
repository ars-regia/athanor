//! Confinement of the chooser (doc_update_trust.md binds user-side processes to restrict
//! themselves with Landlock at start, as the greeter does). The chooser writes the user
//! layout document, and a newer one's backup beside it; the toolkit and Mesa write the cache
//! and the runtime directory; GPU rendering opens the DRM nodes, and on the -nvidia images the
//! NVIDIA driver's nodes, read-write. Every grant is a descriptor `athanor_unit::dirs` opened
//! without following a link and checked (#1059), so a link put at a granted path widens nothing.

use std::env;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use athanor_layout::user::write_target;
use athanor_unit::dirs;
pub use athanor_unit::sandbox::ensure_single_threaded;

const DRM_DEVICE_DIR: &str = "/dev/dri";

/// Confines writes to the directories of `writable_dirs` and to the device nodes, each by the
/// descriptor its check opened. A grant that fails its check (a link, another user's directory)
/// stops the chooser, and so does a kernel without Landlock: never a best-effort no-op.
pub fn apply(user_file: &Path) -> Result<(), Box<dyn Error>> {
    let write = opened(&writable_dirs(&document_dir(user_file)?), dirs::owned_dir)?;
    let mut nodes = vec![PathBuf::from(DRM_DEVICE_DIR)];
    nodes.extend(nvidia_device_nodes(Path::new("/dev")));
    let write_file = opened(&nodes, dirs::owned_node)?;
    athanor_unit::sandbox::restrict_writes(&write, &write_file)
}

/// The directory the document is written in, created before the ruleset so that the grant has
/// a directory to hold on to. The document's link, or a link to its directory (a dotfiles
/// tree), is resolved here once; the caller then opens the result without following a link. A
/// later swap of a link points the chooser's writes at a directory the ruleset does not hold,
/// where they are refused.
fn document_dir(user_file: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let dir = write_target(user_file)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("the layout document has no directory")?;
    std::fs::create_dir_all(&dir)?;
    Ok(std::fs::canonicalize(dir)?)
}

/// The directories granted the write set: the document's, /tmp, the cache and the runtime
/// directory.
fn writable_dirs(document_dir: &Path) -> Vec<PathBuf> {
    let mut paths = vec![document_dir.to_path_buf(), PathBuf::from("/tmp")];
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    paths.extend(cache);
    paths.extend(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from));
    paths.retain(|path| path.is_absolute());
    paths
}

/// Each of `paths` opened and checked by `open` (`dirs::owned_dir`, `dirs::owned_node`). A
/// missing one is skipped, as Landlock has nothing to hold on to; one that fails its check is
/// an error naming the path, unlike `dirs::existing`, which leaves it out with a warning: the
/// chooser confines itself before logging exists, so a warning would be lost.
fn opened<T>(paths: &[PathBuf], open: fn(&Path) -> io::Result<T>) -> io::Result<Vec<T>> {
    paths
        .iter()
        .filter_map(|path| match open(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => Some(Err(io::Error::new(
                error.kind(),
                format!("{} is not granted: {error}", path.display()),
            ))),
            Ok(fd) => Some(Ok(fd)),
        })
        .collect()
}

/// The NVIDIA driver's nodes in `dev`, as the greeter grants them (`verify.py gpu-grants`
/// keeps the two copies equal): the control and modeset nodes and one numbered node per
/// GPU. The proprietary EGL stack opens them read-write next to the DRM nodes; denied, it
/// finds no GPU and GTK renders in software (#535). Each node is granted by itself rather
/// than `/dev` as a whole, and the nvidia-uvm nodes stay out: they serve CUDA only.
fn nvidia_device_nodes(dev: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Vec::new();
    };
    let mut nodes: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| {
            name.to_str().is_some_and(|name| match name {
                "nvidiactl" | "nvidia-modeset" => true,
                _ => name.strip_prefix("nvidia").is_some_and(|gpu| {
                    !gpu.is_empty() && gpu.bytes().all(|byte| byte.is_ascii_digit())
                }),
            })
        })
        .map(|name| dev.join(name))
        .collect();
    nodes.sort();
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_unit::sandbox::restrict_writes;

    fn probe(test: &str) -> PathBuf {
        let base = env::temp_dir().join(format!(
            "athanor-chooser-landlock-{}-{test}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&base).ok();
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
        let granted = writable_dirs(write_target(&link).expect("target").parent().expect("dir"));
        assert_eq!(granted[0], base.join("dotfiles"));
        assert!(!granted.contains(&base.join("config/athanor")));
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_linked_document_directory_is_granted_where_it_leads() {
        let base = probe("linked-dir");
        // A dotfiles tree links the whole directory, and no document exists yet.
        std::fs::remove_dir(base.join("config/athanor")).expect("rmdir");
        std::os::unix::fs::symlink(base.join("dotfiles"), base.join("config/athanor"))
            .expect("symlink");
        let dir = document_dir(&base.join("config/athanor/layout.toml")).expect("resolved");
        assert_eq!(
            dir,
            std::fs::canonicalize(base.join("dotfiles")).expect("real")
        );
        assert_eq!(opened(&[dir], dirs::owned_dir).expect("checked").len(), 1);
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_link_at_a_granted_directory_stops_the_chooser_and_a_missing_one_is_skipped() {
        let base = probe("refused");
        let link = base.join("cache");
        std::os::unix::fs::symlink(base.join("elsewhere"), &link).expect("symlink");
        let error = opened(&[link], dirs::owned_dir).expect_err("a link to a directory");
        // O_DIRECTORY with O_NOFOLLOW makes the kernel answer ENOTDIR for a link: an error,
        // not the NotFound that is skipped.
        assert_eq!(error.kind(), io::ErrorKind::NotADirectory, "{error}");
        assert!(
            error.to_string().contains("cache is not granted"),
            "{error}"
        );
        let found = opened(
            &[base.join("missing"), base.join("dotfiles")],
            dirs::owned_dir,
        )
        .expect("a missing directory is skipped");
        assert_eq!(found.len(), 1);
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_link_swapped_in_after_the_check_does_not_move_the_grant() {
        let base = probe("swapped");
        let link = base.join("config/athanor/layout.toml");
        std::os::unix::fs::symlink(base.join("dotfiles/layout.toml"), &link).expect("symlink");
        let document_dir = write_target(&link)
            .expect("target")
            .parent()
            .expect("dir")
            .to_path_buf();
        let write = opened(&[document_dir.clone()], dirs::owned_dir).expect("checked");
        // After the check, the dotfiles directory moves and a link to another one of the
        // user's takes its path: the link the document names now leads there.
        std::fs::rename(&document_dir, base.join("moved")).expect("rename");
        std::os::unix::fs::symlink(base.join("elsewhere"), &document_dir).expect("symlink");
        let thread_base = base.clone();
        std::thread::spawn(move || {
            restrict_writes(&write, &[]).expect("Landlock must be enforced");
            std::fs::write(thread_base.join("moved/layout.toml"), b"x")
                .expect("the checked directory");
            let error = std::fs::write(&link, b"x").expect_err("through the swapped link");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        })
        .join()
        .expect("sandbox test thread");
        assert!(!base.join("elsewhere/layout.toml").exists());
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn writes_outside_the_grants_are_refused() {
        let base = probe("enforced");
        let (allowed, denied) = (base.join("dotfiles"), base.join("elsewhere"));
        let write = opened(&[allowed.clone()], dirs::owned_dir).expect("checked");
        std::thread::spawn(move || {
            restrict_writes(&write, &[]).expect("Landlock must be enforced");
            let err = std::fs::write(denied.join("probe"), b"x").expect_err("outside the grants");
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
            std::fs::write(allowed.join("layout.toml"), b"x").expect("inside the grants");
        })
        .join()
        .expect("sandbox test thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }
}
