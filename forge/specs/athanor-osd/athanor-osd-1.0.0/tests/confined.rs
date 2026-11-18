//! The overlay's Landlock (doc_osd.md OD12) under the grants it computes: it writes beneath its
//! cache and dconf's runtime directory alone, and reads nothing beneath `/proc`, its own
//! environment included (maintainer decision, 2026-11-07).
//!
//! Landlock binds the thread that applies it, so the test runs itself again as a child with
//! directories of its own and confines that child's test thread.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use athanor_osd::confine;
use athanor_unit::dirs::Dirs;

const CHILD: &str = "ATHANOR_OSD_CONFINED_CHILD";

#[test]
fn the_confined_overlay_writes_only_its_directories_and_reads_no_proc() {
    let base = std::env::temp_dir().join(format!("athanor-osd-confined-{}", std::process::id()));
    fs::remove_dir_all(&base).ok();
    for dir in ["home/.cache", "runtime"] {
        fs::create_dir_all(base.join(dir)).expect("mkdir");
    }
    fs::set_permissions(base.join("runtime"), fs::Permissions::from_mode(0o700)).expect("chmod");
    let out = Command::new(std::env::current_exe().expect("exe"))
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, &base)
        .env("HOME", base.join("home"))
        .env("XDG_CACHE_HOME", base.join("home/.cache/athanor-osd"))
        .env("XDG_RUNTIME_DIR", base.join("runtime"))
        .output()
        .expect("child");
    let result = (
        out.status.success(),
        base.join("home/.cache/athanor-osd/written").is_file(),
        base.join("runtime/dconf/user").is_file(),
        base.join("outside").exists(),
    );
    // Teardown: a directory already gone is fine.
    fs::remove_dir_all(&base).ok();
    assert_eq!(
        result,
        (true, true, true, false),
        "child: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The confined half; it does nothing when the test above did not start it.
#[test]
fn child() {
    let Some(base) = std::env::var_os(CHILD).map(PathBuf::from) else {
        return;
    };
    let dirs = Dirs::from_vars("athanor-osd", |name| std::env::var_os(name)).expect("dirs");
    let grants = confine::overlay(&dirs, |name| std::env::var_os(name));
    assert_eq!(grants.write.len(), 2, "the cache and dconf's: {grants:?}");
    grants.apply().expect("confine");

    fs::write(dirs.cache.join("written"), b"x").expect("a write beneath the cache");
    fs::write(dirs.runtime.join("dconf/user"), b"x").expect("dconf's flag");
    let refused = |path: &Path, error: std::io::Error| {
        assert_eq!(
            error.kind(),
            ErrorKind::PermissionDenied,
            "{}",
            path.display()
        );
    };
    for path in [base.join("outside"), dirs.runtime.join("stray")] {
        let error = fs::write(&path, b"x").expect_err("a write outside the grants");
        refused(&path, error);
    }
    for path in ["/proc/self/environ", "/proc/self/cgroup", "/proc/1/environ"] {
        // Never print what was read: it is a process's environment.
        match fs::read(path) {
            Ok(_) => panic!("{path} was read: /proc is not granted"),
            Err(error) => refused(Path::new(path), error),
        }
    }
}
