//! Atomic replacement of a small text file: a temporary in the same directory, fsync,
//! rename, fsync of the directory.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Replaces `path` with `text` in one step: a reader sees the old file or the new one,
/// never half of either, and the new name survives a crash once this returns. The
/// temporary file is unique to this call and lives in the same directory, so concurrent
/// writers never share one and the rename never crosses a filesystem.
///
/// ponytail: a leftover `.<name>.<pid>.<n>.athanor-tmp` from a writer that died is never
/// reused, because `create_new` and the counter make every name unique, and it is not swept.
/// Upgrade path: remove matching temporaries older than a minute at start, if they are ever
/// seen piling up.
pub fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let (temporary, mut file) = loop {
        let temporary = path.with_file_name(format!(
            ".{}.{}.{}.athanor-tmp",
            name.to_string_lossy(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            // A dead writer's leftover under a reused pid: not ours to touch, take the next name.
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    };
    let written = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        // The rename did not happen, and the open above succeeded: the temporary is ours
        // alone to remove.
        let _ignored = fs::remove_file(&temporary);
    }
    written?;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::testing::scratch;

    #[test]
    fn an_atomic_write_leaves_only_the_file() {
        let dir = scratch("atomic");
        let file = dir.join("entries");
        write_atomically(&file, "[\"Panel\"]").expect("write");
        write_atomically(&file, "[\"Panel\",\"Dock\"]").expect("rewrite");
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "[\"Panel\",\"Dock\"]"
        );
        let names: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names, ["entries"]);
    }

    #[test]
    fn a_leftover_temporary_from_a_dead_writer_does_not_block_the_next_write() {
        let dir = scratch("leftover-tmp");
        let path = dir.join("key");
        std::fs::write(dir.join(".key.athanor-tmp"), "stale").expect("write");
        write_atomically(&path, "new").expect("writes");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
    }

    #[test]
    fn a_dead_writers_temporary_under_this_pid_is_skipped_and_left_alone() {
        let dir = scratch("reused-pid-tmp");
        let path = dir.join("key");
        let pid = std::process::id();
        let planted: Vec<PathBuf> = (0..1024)
            .map(|n| dir.join(format!(".key.{pid}.{n}.athanor-tmp")))
            .collect();
        for temporary in &planted {
            std::fs::write(temporary, "stale").expect("plant");
        }
        write_atomically(&path, "new").expect("writes");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
        for temporary in &planted {
            assert_eq!(
                std::fs::read_to_string(temporary).expect("still there"),
                "stale",
                "{temporary:?} was touched"
            );
        }
    }

    #[test]
    fn concurrent_writers_never_leave_a_torn_file_or_a_temporary() {
        let dir = scratch("concurrent");
        let path = dir.join("key");
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    write_atomically(&path, &format!("writer {n}\n").repeat(512))
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("joins").expect("writes");
        }
        let text = std::fs::read_to_string(&path).expect("read");
        let first = text.lines().next().expect("a line");
        assert!(text.lines().all(|line| line == first), "torn file");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from("key")]);
    }
}
