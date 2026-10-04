//! The notification history on disk (doc_bar.md BR4): `notifications.json`, kept across
//! restarts and reboots. Never written: transient notifications, pixel images, icon files.
//! Serde reads what it knows and defaults the rest, so another version of the file loads.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::icon::{self, Icon};
use crate::identity::Identity;
use crate::rules::write_atomic;
use crate::store::{Content, Notification, Store, Urgency, Visual, CAPACITY};

const VERSION: u32 = 1;
/// CAPACITY entries of bounded fields fit well within this; more is not our file.
const READ_LIMIT: u64 = 16 * 1024 * 1024;
const MIN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    bus_id: String,
    #[serde(default)]
    notifications: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    time: i64,
    #[serde(default = "other")]
    identity: Identity,
    #[serde(default)]
    sender: String,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    actions: Vec<(String, String)>,
    #[serde(default = "normal")]
    urgency: u8,
    #[serde(default)]
    resident: bool,
    #[serde(default)]
    desktop_entry: Option<String>,
    #[serde(default)]
    icon_name: String,
}

fn other() -> Identity {
    Identity::Other
}

fn normal() -> u8 {
    Urgency::Normal as u8
}

impl Entry {
    fn of(n: &Notification) -> Entry {
        let c = &n.content;
        Entry {
            id: n.id,
            time: n.time,
            identity: n.identity.clone(),
            sender: n.sender.clone(),
            read: n.read,
            app_name: c.app_name.clone(),
            summary: c.summary.clone(),
            body: c.body.clone(),
            actions: c.actions.clone(),
            urgency: c.urgency as u8,
            resident: c.resident,
            desktop_entry: c.desktop_entry.clone(),
            icon_name: match &c.visual {
                Visual::Icon(Icon::Name(name)) => name.clone(),
                _ => String::new(),
            },
        }
    }

    fn restore(self) -> Notification {
        let visual = match icon::parse(&self.icon_name) {
            Some(Icon::Name(name)) => Visual::Icon(Icon::Name(name)),
            _ => Visual::None,
        };
        Notification {
            id: self.id,
            arrived_ms: 0,
            time: self.time,
            identity: self.identity,
            sender: self.sender,
            read: self.read,
            content: Content {
                app_name: self.app_name,
                summary: self.summary,
                body: self.body,
                actions: self.actions,
                urgency: Urgency::from_hint(Some(self.urgency)),
                transient: false,
                resident: self.resident,
                desktop_entry: self.desktop_entry,
                visual,
                // No popup replays: a 1 ms timeout has ended by the time anyone asks.
                timeout_ms: 1,
            },
        }
    }
}

fn parse(path: &Path) -> io::Result<File> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(READ_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > READ_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::from)
}

/// The saved history, oldest first. A missing file is an empty history; one that does not
/// parse is renamed `<name>.corrupt` and is an empty history too.
#[must_use]
pub fn load(path: &Path, bus_id: &str) -> Vec<Notification> {
    let file = match parse(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            let mut corrupt = path.as_os_str().to_owned();
            corrupt.push(".corrupt");
            match fs::rename(path, &corrupt) {
                Ok(()) => {
                    warn!(path = %path.display(), %err, "notification history unreadable, kept as .corrupt")
                }
                Err(rename) => {
                    warn!(path = %path.display(), %err, %rename, "notification history unreadable and cannot be set aside")
                }
            }
            return Vec::new();
        }
    };
    // Unique names restart with a new bus: `:1.42` would name another program.
    let same_bus = file.bus_id == bus_id;
    let mut seen = HashSet::new();
    let mut restored: Vec<Notification> = file
        .notifications
        .into_iter()
        .filter(|entry| entry.id != 0 && seen.insert(entry.id))
        .map(|entry| {
            let mut n = entry.restore();
            if !same_bus {
                n.sender.clear();
            }
            n
        })
        .collect();
    restored.drain(..restored.len().saturating_sub(CAPACITY));
    restored
}

/// Writes the history without its transient entries. The parent directory must exist.
pub fn save(path: &Path, store: &Store, bus_id: &str) -> io::Result<()> {
    let file = File {
        version: VERSION,
        bus_id: bus_id.to_owned(),
        notifications: store
            .iter()
            .filter(|n| !n.content.transient)
            .map(Entry::of)
            .collect(),
    };
    write_atomic(path, &serde_json::to_vec(&file)?)
}

/// Paces the writes: one every two seconds, and one failure warning until one succeeds.
#[derive(Debug, Default)]
pub struct Coalescer {
    last_write: Option<Instant>,
    failing: bool,
}

impl Coalescer {
    #[must_use]
    pub fn new() -> Coalescer {
        Coalescer::default()
    }

    /// A change at `now`: `Some(delay)` to wait before writing, `None` when a write is due.
    pub fn changed(&mut self, now: Instant) -> Option<Duration> {
        let due = self.last_write? + MIN_INTERVAL;
        due.checked_duration_since(now).filter(|d| !d.is_zero())
    }

    /// A write attempt ended at `now`; true when a warning must be logged.
    pub fn written(&mut self, now: Instant, ok: bool) -> bool {
        self.last_write = Some(now);
        let warn = !ok && !self.failing;
        self.failing = !ok;
        warn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Retention;
    use crate::store::tests::{content, notification};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "athanor-shelld-history-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn held(store: &mut Store, summary: &str, transient: bool, time: i64) -> u32 {
        let mut content = content(summary, Urgency::Normal, 5_000);
        content.transient = transient;
        store
            .notify(
                content,
                0,
                0,
                time,
                Identity::App("org.example.Chat".into()),
                ":1.7".into(),
            )
            .notification
            .id
    }

    #[test]
    fn a_saved_history_reads_back_without_transients_or_images() {
        let dir = temp("roundtrip");
        let path = dir.join("notifications.json");
        let mut store = Store::new(false);
        held(&mut store, "kept", false, 100);
        held(&mut store, "transient", true, 101);
        let mut pixels = content("picture", Urgency::Normal, 5_000);
        pixels.visual = Visual::Pixels(crate::image::tests::one_pixel());
        store.notify(pixels, 0, 0, 102, Identity::Other, ":1.8".into());
        save(&path, &store, "bus-a").expect("save");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let back = load(&path, "bus-a");
        assert_eq!(
            back.iter()
                .map(|n| n.content.summary.as_str())
                .collect::<Vec<_>>(),
            ["kept", "picture"]
        );
        assert_eq!(back[1].content.visual, Visual::None);
        assert_eq!(back[0].sender, ":1.7", "same bus: the sender is kept");
        assert!(
            load(&path, "bus-b").iter().all(|n| n.sender.is_empty()),
            "another bus: senders emptied"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn ids_continue_above_the_highest_restored() {
        let mut store = Store::new(false);
        store.restore(vec![notification(41), notification(7)]);
        let next = held(&mut store, "new", false, 0);
        assert_eq!(next, 42);
    }

    #[test]
    fn retention_by_age_and_by_count() {
        let mut store = Store::new(false);
        let day = 86_400;
        held(&mut store, "old", false, 0);
        held(&mut store, "recent", false, 7 * day);
        assert_eq!(store.prune(8 * day, Retention::Days(7)).len(), 1);
        assert_eq!(store.prune(800 * day, Retention::UntilCleared).len(), 0);
        for n in 0..CAPACITY + 3 {
            held(&mut store, &format!("n{n}"), false, 8 * day);
        }
        assert_eq!(store.iter().count(), CAPACITY);
    }

    #[test]
    fn a_corrupt_file_is_renamed_and_the_history_starts_empty() {
        let dir = temp("corrupt");
        let path = dir.join("notifications.json");
        std::fs::write(&path, "{not json").expect("write");
        assert!(load(&path, "bus").is_empty());
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("notifications.json.corrupt")).expect("kept"),
            "{not json"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn another_version_reads_with_defaults() {
        let dir = temp("version");
        let path = dir.join("notifications.json");
        std::fs::write(&path, r#"{"version":2,"bus_id":"b","future":1,"notifications":[{"id":3,"time":5,"identity":"Other","summary":"s","later":true}]}"#).expect("write");
        let back = load(&path, "b");
        assert_eq!(back.len(), 1);
        assert_eq!(
            (back[0].id, back[0].content.summary.as_str(), back[0].read),
            (3, "s", false)
        );
        assert!(!dir.join("notifications.json.corrupt").exists());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_failed_write_keeps_the_store_and_warns_once() {
        let dir = temp("readonly");
        let mut store = Store::new(false);
        held(&mut store, "kept", false, 0);
        let path = dir.join("missing-dir/notifications.json");
        assert!(save(&path, &store, "bus").is_err());
        assert_eq!(store.iter().count(), 1);
        let mut coalescer = Coalescer::new();
        let now = Instant::now();
        assert!(coalescer.written(now, false), "the first failure warns");
        assert!(!coalescer.written(now, false), "the second does not");
        assert!(!coalescer.written(now, true));
        assert!(
            coalescer.written(now, false),
            "after a success, a new failure warns again"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn writes_are_coalesced_to_one_every_two_seconds() {
        let mut coalescer = Coalescer::new();
        let start = Instant::now();
        assert_eq!(
            coalescer.changed(start),
            None,
            "the first change writes at once"
        );
        coalescer.written(start, true);
        assert_eq!(
            coalescer.changed(start + Duration::from_millis(500)),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            coalescer.changed(start + Duration::from_millis(900)),
            Some(Duration::from_millis(1100))
        );
        assert_eq!(coalescer.changed(start + Duration::from_secs(3)), None);
    }
}
