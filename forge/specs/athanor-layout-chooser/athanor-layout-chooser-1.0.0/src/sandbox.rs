//! Confinement of the chooser (doc_update_trust.md binds user-side processes to restrict
//! themselves with Landlock at start, as the greeter does). The chooser writes the user
//! layout document, and a newer one's backup beside it; GTK and Mesa write the cache and
//! the runtime directory; GPU rendering opens the DRM nodes read-write.

use landlock::{
    AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, ABI,
};
use std::env;
use std::path::{Path, PathBuf};

use athanor_layout::user::write_target;

const DRM_DEVICE_DIR: &str = "/dev/dri";

/// Fails unless the calling process has exactly one thread: Landlock confines the
/// calling thread and the threads it creates afterwards, not those that already exist.
pub fn ensure_single_threaded() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!(
            "{threads} threads exist; Landlock would leave all but one unconfined"
        )
        .into());
    }
    Ok(())
}

/// Confines writes to what `grants` lists for `user_file`. A kernel without Landlock is
/// an error, never a best-effort no-op.
pub fn apply(user_file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Created before the ruleset, so that the grant has a directory to hold on to.
    let document_dir = write_target(user_file)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("the layout document has no directory")?;
    std::fs::create_dir_all(&document_dir)?;
    restrict_writes_to(&grants(&document_dir))
}

/// The document's directory, the cache, /tmp and the runtime directory with the write
/// set; the DRM nodes with `WriteFile` alone.
fn grants(document_dir: &Path) -> Vec<(PathBuf, BitFlags<AccessFs>)> {
    let write_access = AccessFs::from_write(ABI::V1);
    let mut paths = vec![document_dir.to_path_buf(), PathBuf::from("/tmp")];
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    paths.extend(cache);
    paths.extend(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from));
    paths.retain(|path| path.is_absolute());
    let mut grants: Vec<_> = paths.into_iter().map(|path| (path, write_access)).collect();
    grants.push((PathBuf::from(DRM_DEVICE_DIR), AccessFs::WriteFile.into()));
    grants
}

/// Restricts the calling thread, and the threads it creates, to the write accesses
/// granted beneath each path; a path that does not exist is skipped.
fn restrict_writes_to(
    grants: &[(PathBuf, BitFlags<AccessFs>)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_write(ABI::V1))?
        .create()?;
    for (path, access) in grants {
        if path.exists() {
            ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, *access))?;
        }
    }
    ruleset.restrict_self()?;
    Ok(())
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
        let granted: Vec<_> = grants(write_target(&link).expect("target").parent().expect("dir"))
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(granted[0], base.join("dotfiles"));
        assert!(!granted.contains(&base.join("config/athanor")));
    }

    #[test]
    fn writes_outside_the_grants_are_refused() {
        let base = probe("enforced");
        let (allowed, denied) = (base.join("dotfiles"), base.join("elsewhere"));
        std::thread::spawn(move || {
            restrict_writes_to(&[(allowed.clone(), AccessFs::from_write(ABI::V1))])
                .expect("Landlock must be enforced");
            let err = std::fs::write(denied.join("probe"), b"x").expect_err("outside the grants");
            assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
            std::fs::write(allowed.join("layout.toml"), b"x").expect("inside the grants");
        })
        .join()
        .expect("sandbox test thread");
    }
}
