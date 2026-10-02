//! What `athanor-update` keeps on disk (docs/architecture/doc_update_trust.md, UT1, UT6, UT7).
//!
//! `/run/athanor-update/`   `lock`, `state.json` (0644, written to a temporary name and renamed)
//! `/var/lib/athanor-update/`
//!   `held`            the digest the user went back from; nothing releases it
//!   `refused`         the digest the policy last refused; cleared by a download that passes
//!   `newest-booted`   the newest build time this machine has booted, seconds since the epoch
//!   `last-success`    when the registry last answered a check, seconds since the epoch
//!   `migrated`        stamp of `athanor-update migrate`
//!   `signatures/<hex>/`  the signature object of a digest, as `skopeo copy … dir:` wrote it
use athanor_trust_state::State;
use std::fs::File;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Store {
    pub run: PathBuf,
    pub var: PathBuf,
}

/// Holds the lock of UT1 until dropped.
pub struct Lock(#[allow(dead_code)] File);

impl Store {
    #[must_use]
    pub fn system() -> Self {
        Self { run: "/run/athanor-update".into(), var: "/var/lib/athanor-update".into() }
    }

    fn lock_file(&self) -> std::io::Result<File> {
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(self.run.join("lock"))
    }

    /// Waits for the lock: the check may have to wait for a request, never the reverse.
    ///
    /// # Errors
    /// The run directory is missing or the lock cannot be taken.
    pub fn lock(&self) -> std::io::Result<Lock> {
        let file = self.lock_file()?;
        file.lock()?;
        Ok(Lock(file))
    }

    /// Takes the lock or reports that a check or another request holds it.
    ///
    /// # Errors
    /// `WouldBlock` when the lock is held; otherwise as [`Store::lock`].
    pub fn try_lock(&self) -> std::io::Result<Lock> {
        let file = self.lock_file()?;
        file.try_lock().map_err(|err| match err {
            std::fs::TryLockError::WouldBlock => std::io::ErrorKind::WouldBlock.into(),
            std::fs::TryLockError::Error(err) => err,
        })?;
        Ok(Lock(file))
    }

    fn read_line(&self, name: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.var.join(name)).ok()?;
        Some(text.trim().to_owned()).filter(|line| !line.is_empty())
    }

    pub(crate) fn replace(dir: &Path, name: &str, mode: u32, contents: &[u8]) -> std::io::Result<()> {
        let temporary = dir.join(format!(".{name}.{}", std::process::id()));
        // A leftover of a killed run would make create_new fail for ever. create_new is
        // O_EXCL, which never follows a link planted at the temporary name.
        match std::fs::remove_file(&temporary) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&temporary, dir.join(name))
    }

    #[must_use]
    pub fn held(&self) -> Option<String> {
        self.read_line("held")
    }

    /// # Errors
    /// The file cannot be written.
    pub fn set_held(&self, digest: &str) -> std::io::Result<()> {
        Self::replace(&self.var, "held", 0o644, format!("{digest}\n").as_bytes())
    }

    #[must_use]
    pub fn refused(&self) -> Option<String> {
        self.read_line("refused")
    }

    /// # Errors
    /// The file cannot be written or removed.
    pub fn set_refused(&self, digest: Option<&str>) -> std::io::Result<()> {
        match digest {
            Some(digest) => Self::replace(&self.var, "refused", 0o644, format!("{digest}\n").as_bytes()),
            None => match std::fs::remove_file(self.var.join("refused")) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
                _ => Ok(()),
            },
        }
    }

    #[must_use]
    pub fn newest_booted(&self) -> i64 {
        self.read_line("newest-booted").and_then(|line| line.parse().ok()).unwrap_or(0)
    }

    /// Records `build_time` when it is newer than the record, and returns the record.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn record_booted(&self, build_time: i64) -> std::io::Result<i64> {
        let newest = self.newest_booted();
        if build_time <= newest {
            return Ok(newest);
        }
        Self::replace(&self.var, "newest-booted", 0o644, format!("{build_time}\n").as_bytes())?;
        Ok(build_time)
    }

    #[must_use]
    pub fn last_success(&self) -> Option<i64> {
        self.read_line("last-success").and_then(|line| line.parse().ok())
    }

    /// # Errors
    /// The file cannot be written.
    pub fn set_last_success(&self, now: i64) -> std::io::Result<()> {
        Self::replace(&self.var, "last-success", 0o644, format!("{now}\n").as_bytes())
    }

    #[must_use]
    pub fn migrated(&self) -> bool {
        self.var.join("migrated").exists()
    }

    /// # Errors
    /// The file cannot be written.
    pub fn set_migrated(&self) -> std::io::Result<()> {
        Self::replace(&self.var, "migrated", 0o644, b"")
    }

    /// The directory of the signature object of `digest`; `None` for anything but `sha256:<64 hex>`.
    #[must_use]
    pub fn signature_dir(&self, digest: &str) -> Option<PathBuf> {
        let hex = digest.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then(|| self.var.join("signatures").join(hex))
    }

    /// Publishes the state: a temporary name in the same directory, then a rename.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn publish(&self, state: &State) -> std::io::Result<()> {
        let json = serde_json::to_vec(state).map_err(std::io::Error::other)?;
        Self::replace(&self.run, "state.json", 0o644, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn scratch(test: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("athanor-update-store-{}-{test}", std::process::id()));
        let store = Store { run: dir.join("run"), var: dir.join("var") };
        std::fs::create_dir_all(&store.run).expect("mkdir");
        std::fs::create_dir_all(store.var.join("signatures")).expect("mkdir");
        store
    }

    #[test]
    fn the_newest_booted_build_time_only_grows() {
        let store = scratch("newest");
        assert_eq!(store.record_booted(2000).expect("write"), 2000);
        assert_eq!(store.record_booted(1000).expect("write"), 2000, "booting an older image after going back");
        assert_eq!(store.newest_booted(), 2000);
    }

    #[test]
    fn held_and_refused_survive_and_only_refused_clears() {
        let store = scratch("held");
        assert_eq!((store.held(), store.refused()), (None, None));
        store.set_held("sha256:aa").expect("write");
        store.set_refused(Some("sha256:bb")).expect("write");
        assert_eq!((store.held().as_deref(), store.refused().as_deref()), (Some("sha256:aa"), Some("sha256:bb")));
        store.set_refused(None).expect("clear");
        store.set_refused(None).expect("clearing twice is fine");
        assert_eq!(store.refused(), None);
    }

    #[test]
    fn a_second_taker_of_the_lock_is_told_it_is_busy() {
        let store = scratch("lock");
        let held = store.lock().expect("lock");
        assert_eq!(store.try_lock().err().map(|err| err.kind()), Some(std::io::ErrorKind::WouldBlock));
        drop(held);
        assert!(store.try_lock().is_ok());
    }

    #[test]
    fn a_signature_directory_exists_only_for_a_sha256_digest() {
        let store = scratch("sigdir");
        assert!(store.signature_dir(&format!("sha256:{}", "a".repeat(64))).is_some());
        for bad in ["sha256:../../etc", "sha512:aa", "", &format!("sha256:{}", "A".repeat(64))] {
            assert_eq!(store.signature_dir(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_state_is_replaced_by_rename_and_is_world_readable() {
        use std::os::unix::fs::PermissionsExt as _;
        let store = scratch("publish");
        std::os::unix::fs::symlink("/etc/passwd", store.run.join(format!(".state.json.{}", std::process::id()))).expect("plant a link at the temporary name");
        let state: State = serde_json::from_str(include_str!("../tests/state-verified.json")).expect("fixture");
        store.publish(&state).expect("publish");
        let meta = std::fs::symlink_metadata(store.run.join("state.json")).expect("metadata");
        assert!(meta.is_file());
        assert_eq!(meta.permissions().mode() & 0o777, 0o644);
        assert_eq!(athanor_trust_state::read_owned_by(&store.run.join("state.json"), std::os::unix::fs::MetadataExt::uid(&meta)), Ok(state));
    }
}
