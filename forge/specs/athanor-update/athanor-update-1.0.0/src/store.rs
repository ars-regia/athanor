//! What `athanor-update` keeps on disk (docs/architecture/doc_update_trust.md, UT1, UT6, UT7).
//!
//! `/run/athanor-update/`   `lock`, `state.json` (0644, written to a temporary name and renamed)
//! `/var/lib/athanor-update/`
//!   `held`            the digest the user went back from, or a queued rollback left;
//!                     nothing releases it, and `migrate` never writes it
//!   `refused`         the digest the policy last refused; cleared by a download that passes
//!   `newest-booted`   the newest build time this machine has booted, seconds since the epoch
//!   `last-success`    when the registry last answered a check, seconds since the epoch
//!   `lineage.json`    the lineage of security updates of UT13 (`crate::lineage`): the anchor,
//!                     and the open lineage's first notice, boots left, postpone, last boot id
//!   `automatic-security-updates`  `on` or `off`, the opt-out of UT13; absent is `on`
//!   `migrated`        stamp of `athanor-update migrate`
//!   `channel-absent`  `migrate` found no manifest for the channel; cleared when it finds one
//!   `move-record`     `<digest> <failed boots> [<deployment>]`: deployments of that digest
//!                     `migrate` staged that did not boot, and the one it staged last, until
//!                     a later run sees what became of it; removed with the stamp
//!   `signatures/<hex>/`  the signature object of a digest, as `skopeo copy … dir:` wrote it
use crate::lineage::Lineage;
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

/// What `migrate` knows of its moves to one digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRecord {
    pub digest: String,
    /// Deployments of the digest that were deployed and did not boot for good.
    pub failed_boots: u32,
    /// The deployment the last switch staged (`Deployed::deployment`), until a later run sees
    /// what became of it. `None` as well when bootc did not name it: nothing is counted then.
    pub staged: Option<String>,
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

    /// The first value of a state file: `None` only when the file is absent or empty.
    ///
    /// # Errors
    /// The file exists and cannot be read, or is not UTF-8. An unreadable answer is never
    /// "no value": for `held` that would release the digest the user went back from.
    fn read_line(&self, name: &str) -> std::io::Result<Option<String>> {
        match std::fs::read_to_string(self.var.join(name)) {
            Ok(text) => Ok(Some(text.trim().to_owned()).filter(|line| !line.is_empty())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// As [`Store::read_line`], parsed; a value that is no number is `InvalidData`.
    fn read_number(&self, name: &str) -> std::io::Result<Option<i64>> {
        self.read_line(name)?.map(|line| line.parse().map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name}: {err}")))).transpose()
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

    /// # Errors
    /// The file exists and cannot be read.
    pub fn held(&self) -> std::io::Result<Option<String>> {
        self.read_line("held")
    }

    /// # Errors
    /// The file cannot be written.
    pub fn set_held(&self, digest: &str) -> std::io::Result<()> {
        Self::replace(&self.var, "held", 0o644, format!("{digest}\n").as_bytes())
    }

    /// # Errors
    /// The file exists and cannot be read.
    pub fn refused(&self) -> std::io::Result<Option<String>> {
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

    /// # Errors
    /// The file exists and cannot be read or parsed: reading it as 0 would let an older build pass.
    pub fn newest_booted(&self) -> std::io::Result<i64> {
        Ok(self.read_number("newest-booted")?.unwrap_or(0))
    }

    /// Records `build_time` when it is newer than the record, and returns the record.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn record_booted(&self, build_time: i64) -> std::io::Result<i64> {
        let newest = self.newest_booted()?;
        if build_time <= newest {
            return Ok(newest);
        }
        Self::replace(&self.var, "newest-booted", 0o644, format!("{build_time}\n").as_bytes())?;
        Ok(build_time)
    }

    /// # Errors
    /// The file exists and cannot be read or parsed.
    pub fn last_success(&self) -> std::io::Result<Option<i64>> {
        self.read_number("last-success")
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
    /// A file cannot be written or removed.
    pub fn set_migrated(&self) -> std::io::Result<()> {
        Self::replace(&self.var, "migrated", 0o644, b"")?;
        // The signed reference booted: a later return from it is no failed boot to count.
        match std::fs::remove_file(self.var.join("move-record")) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        self.set_channel_absent(false)
    }

    #[must_use]
    pub fn channel_absent(&self) -> bool {
        self.var.join("channel-absent").exists()
    }

    /// # Errors
    /// The file cannot be written or removed.
    pub fn set_channel_absent(&self, absent: bool) -> std::io::Result<()> {
        if absent {
            return Self::replace(&self.var, "channel-absent", 0o644, b"");
        }
        match std::fs::remove_file(self.var.join("channel-absent")) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    /// What `migrate` recorded of its moves to `digest`; an empty record for any other digest.
    ///
    /// # Errors
    /// The file exists and cannot be read.
    pub fn move_record(&self, digest: &str) -> std::io::Result<MoveRecord> {
        let line = self.read_line("move-record")?.unwrap_or_default();
        let mut fields = line.split(' ');
        let mut record = MoveRecord { digest: digest.to_owned(), failed_boots: 0, staged: None };
        if fields.next() == Some(digest) {
            record.failed_boots = fields.next().and_then(|count| count.parse().ok()).unwrap_or(0);
            record.staged = fields.next().map(str::to_owned);
        }
        Ok(record)
    }

    /// # Errors
    /// The file cannot be written.
    pub fn set_move_record(&self, record: &MoveRecord) -> std::io::Result<()> {
        let staged = record.staged.as_deref().map(|staged| format!(" {staged}")).unwrap_or_default();
        Self::replace(&self.var, "move-record", 0o644, format!("{} {}{staged}\n", record.digest, record.failed_boots).as_bytes())
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

    /// The lineage of UT13; a closed one with no anchor when the file is absent.
    ///
    /// # Errors
    /// The file exists and cannot be read, or is not a lineage the service could have
    /// written (`InvalidData`). It is left as it is: a reset would grant a new budget.
    pub fn lineage(&self) -> std::io::Result<Lineage> {
        let text = match std::fs::read(self.var.join("lineage.json")) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Lineage::default()),
            Err(err) => return Err(err),
        };
        let invalid = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("lineage.json: {what}"));
        let lineage: Lineage = serde_json::from_slice(&text).map_err(|err| invalid(err.to_string()))?;
        match lineage.published() {
            Some(postpone) if !postpone.consistent() => Err(invalid("boots or first notice out of range".into())),
            _ => Ok(lineage),
        }
    }

    /// # Errors
    /// The file cannot be written.
    #[cfg_attr(not(test), expect(dead_code, reason = "the security-class check and Postpone() of UT13 call it; until then only the tests do"))]
    pub fn set_lineage(&self, lineage: &Lineage) -> std::io::Result<()> {
        let json = serde_json::to_vec(lineage).map_err(std::io::Error::other)?;
        Self::replace(&self.var, "lineage.json", 0o644, &json)
    }

    /// The opt-out of UT13, inverted: `true` unless the file says `off`.
    ///
    /// # Errors
    /// The file exists and cannot be read, or says neither `on` nor `off` (`InvalidData`),
    /// an empty file included: an unreadable answer is never the default.
    pub fn automatic_security_updates(&self) -> std::io::Result<bool> {
        match std::fs::read_to_string(self.var.join("automatic-security-updates")) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(err) => Err(err),
            Ok(text) => match text.trim() {
                "on" => Ok(true),
                "off" => Ok(false),
                other => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("automatic-security-updates: {other:?} is neither on nor off"))),
            },
        }
    }

    /// # Errors
    /// The file cannot be written.
    #[cfg_attr(not(test), expect(dead_code, reason = "the security-class check and Postpone() of UT13 call it; until then only the tests do"))]
    pub fn set_automatic_security_updates(&self, enabled: bool) -> std::io::Result<()> {
        Self::replace(&self.var, "automatic-security-updates", 0o644, if enabled { "on\n" } else { "off\n" }.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::Lineage;

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
        assert_eq!(store.newest_booted().expect("read"), 2000);
    }

    #[test]
    fn held_and_refused_survive_and_only_refused_clears() {
        let store = scratch("held");
        assert_eq!((store.held().expect("read"), store.refused().expect("read")), (None, None));
        store.set_held("sha256:aa").expect("write");
        store.set_refused(Some("sha256:bb")).expect("write");
        assert_eq!((store.held().expect("read").as_deref(), store.refused().expect("read").as_deref()), (Some("sha256:aa"), Some("sha256:bb")));
        store.set_refused(None).expect("clear");
        store.set_refused(None).expect("clearing twice is fine");
        assert_eq!(store.refused().expect("read"), None);
    }

    #[test]
    fn a_state_file_that_cannot_be_read_is_an_error_and_only_an_absent_one_is_none() {
        let store = scratch("unreadable");
        for name in ["held", "refused", "newest-booted", "last-success", "move-record"] {
            assert_eq!(store.read_line(name).expect("absent"), None, "{name}");
            // A directory in place of the file fails with EISDIR, whoever runs the test.
            std::fs::create_dir(store.var.join(name)).expect("mkdir");
            assert!(store.read_line(name).is_err(), "{name}");
        }
        assert!(store.held().is_err() && store.refused().is_err());
        assert!(store.newest_booted().is_err() && store.last_success().is_err());
        assert!(store.move_record("sha256:aa").is_err());
        assert!(store.record_booted(1).is_err(), "an unreadable record is not read as 0");
    }

    #[test]
    fn a_number_file_that_is_not_a_number_is_an_error() {
        let store = scratch("garbled");
        std::fs::write(store.var.join("newest-booted"), "soon\n").expect("write");
        assert_eq!(store.newest_booted().expect_err("garbled").kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_lineage_survives_and_starts_closed() {
        let store = scratch("lineage");
        assert_eq!(store.lineage().expect("absent"), Lineage::default());
        let mut lineage = Lineage::default();
        lineage.applied("sha256:aa");
        lineage.notice(1_790_000_000, "boot-0");
        store.set_lineage(&lineage).expect("write");
        assert_eq!(store.lineage().expect("read"), lineage);
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(store.var.join("lineage.json")).expect("metadata").permissions());
        assert_eq!(mode & 0o777, 0o644);
    }

    #[test]
    fn a_malformed_lineage_file_is_a_storage_error() {
        let store = scratch("lineage-malformed");
        let good = r#"{"anchor":null,"open":{"first_notice_at":1790000000,"boots_remaining":7,"postponed":false,"last_boot_id":"b"}}"#;
        std::fs::write(store.var.join("lineage.json"), good).expect("write");
        assert!(store.lineage().is_ok());
        for bad in ["", "{", "{}", "null", &good.replace("7", "8"), &good.replace("1790000000", "-1"), &good.replace(r#""b"}"#, r#""b","extra":1}"#)] {
            std::fs::write(store.var.join("lineage.json"), bad).expect("write");
            assert_eq!(store.lineage().map_err(|err| err.kind()), Err(std::io::ErrorKind::InvalidData), "{bad:?}");
            assert_eq!(std::fs::read_to_string(store.var.join("lineage.json")).expect("read"), bad, "the file is left as it is");
        }
        std::fs::remove_file(store.var.join("lineage.json")).expect("remove");
        std::fs::create_dir(store.var.join("lineage.json")).expect("mkdir");
        assert!(store.lineage().is_err(), "an unreadable file is not an absent one");
    }

    #[test]
    fn a_malformed_opt_out_file_is_a_storage_error() {
        let store = scratch("opt-out");
        assert!(store.automatic_security_updates().expect("absent is on"));
        store.set_automatic_security_updates(false).expect("write");
        assert_eq!(std::fs::read_to_string(store.var.join("automatic-security-updates")).expect("read"), "off\n");
        assert!(!store.automatic_security_updates().expect("read"));
        store.set_automatic_security_updates(true).expect("write");
        assert!(store.automatic_security_updates().expect("read"));
        for bad in ["maybe\n", "", "\n", "On\n", "on off\n"] {
            std::fs::write(store.var.join("automatic-security-updates"), bad).expect("write");
            assert_eq!(store.automatic_security_updates().map_err(|err| err.kind()), Err(std::io::ErrorKind::InvalidData), "{bad:?}");
        }
        std::fs::remove_file(store.var.join("automatic-security-updates")).expect("remove");
        std::fs::create_dir(store.var.join("automatic-security-updates")).expect("mkdir");
        assert!(store.automatic_security_updates().is_err(), "an unreadable file is not an absent one");
    }

    #[test]
    fn a_second_taker_of_the_lock_is_told_it_is_busy() {
        let _spawning = crate::goback::tests::SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let store = scratch("lock");
        let held = store.lock().expect("lock");
        assert_eq!(store.try_lock().err().map(|err| err.kind()), Some(std::io::ErrorKind::WouldBlock));
        drop(held);
        assert!(store.try_lock().is_ok());
    }

    #[test]
    fn the_move_record_survives_and_belongs_to_one_digest() {
        let store = scratch("move-record");
        let empty = MoveRecord { digest: "sha256:aa".into(), failed_boots: 0, staged: None };
        assert_eq!(store.move_record("sha256:aa").expect("read"), empty);
        let record = MoveRecord { failed_boots: 1, staged: Some("b87d.1".into()), ..empty.clone() };
        store.set_move_record(&record).expect("write");
        assert_eq!(store.move_record("sha256:aa").expect("read"), record);
        assert_eq!(store.move_record("sha256:a").expect("read").failed_boots, 0, "a prefix is another digest");
        store.set_move_record(&MoveRecord { staged: None, ..record }).expect("write");
        assert_eq!(store.move_record("sha256:aa").expect("read").staged, None);
        assert_eq!(store.move_record("sha256:bb").expect("read"), MoveRecord { digest: "sha256:bb".into(), ..empty });
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
