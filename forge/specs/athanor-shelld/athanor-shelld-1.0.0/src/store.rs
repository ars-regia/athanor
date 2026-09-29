//! The notifications the daemon holds (doc_bar.md BR4). No D-Bus and no clock: the caller
//! passes the time in milliseconds since the daemon started.

use std::collections::VecDeque;

use crate::icon::Icon;
use crate::image::Image;

pub const CAPACITY: usize = 100;
pub const DEFAULT_TIMEOUT_MS: u32 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low = 0,
    Normal = 1,
    Critical = 2,
}

impl Urgency {
    /// The byte of the `urgency` hint; absent or unknown means normal.
    #[must_use]
    pub fn from_hint(byte: Option<u8>) -> Urgency {
        match byte {
            Some(0) => Urgency::Low,
            Some(2) => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// Why a notification closed, numbered as the specification numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visual {
    None,
    Pixels(Image),
    Icon(Icon),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// (key, label). The key goes back to the application exactly as it sent it.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    pub visual: Visual,
    /// How long the popup shows; 0 until the user closes it.
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub arrived_ms: u64,
    pub content: Content,
}

#[derive(Debug)]
pub struct Outcome {
    pub notification: Notification,
    /// True when `replaces_id` named a notification still held.
    pub replaced: bool,
    /// Ids pushed out by the capacity, oldest first.
    pub evicted: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Store {
    held: VecDeque<Notification>,
    last_id: u32,
    dnd: bool,
}

impl Store {
    #[must_use]
    pub fn new(dnd: bool) -> Store {
        Store {
            dnd,
            ..Store::default()
        }
    }

    /// A replaced notification keeps its id, arrives again (its popup restarts) and becomes
    /// the newest. An unknown `replaces_id` gets a new id, as the specification says.
    pub fn notify(&mut self, content: Content, replaces_id: u32, now_ms: u64) -> Outcome {
        let replaced = replaces_id != 0 && self.remove(replaces_id).is_some();
        let id = if replaced {
            replaces_id
        } else {
            self.next_id()
        };
        let notification = Notification {
            id,
            arrived_ms: now_ms,
            content,
        };
        self.held.push_back(notification.clone());
        let excess = self.held.len().saturating_sub(CAPACITY);
        let evicted = self.held.drain(..excess).map(|old| old.id).collect();
        Outcome {
            notification,
            replaced,
            evicted,
        }
    }

    pub fn close(&mut self, id: u32) -> Option<Notification> {
        self.remove(id)
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.held.iter().find(|held| held.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.held.iter()
    }

    #[must_use]
    pub fn dnd(&self) -> bool {
        self.dnd
    }

    pub fn set_dnd(&mut self, on: bool) {
        self.dnd = on;
    }

    fn remove(&mut self, id: u32) -> Option<Notification> {
        let at = self.held.iter().position(|held| held.id == id)?;
        self.held.remove(at)
    }

    /// The id after the last one given, skipping 0 and ids still held. At most CAPACITY ids
    /// are held, so the loop ends within CAPACITY + 2 turns.
    fn next_id(&mut self) -> u32 {
        loop {
            self.last_id = self.last_id.wrapping_add(1);
            if self.last_id != 0 && self.get(self.last_id).is_none() {
                return self.last_id;
            }
        }
    }
}

/// The popup's time: 0 (until closed) for a critical notification and for an expire timeout
/// of 0; 5 s when the application asks for none (-1); its own time otherwise.
#[must_use]
pub fn timeout_ms(expire_timeout: i32, urgency: Urgency) -> u32 {
    match (urgency, expire_timeout) {
        (Urgency::Critical, _) | (_, 0) => 0,
        (_, requested) if requested < 0 => DEFAULT_TIMEOUT_MS,
        (_, requested) => requested.unsigned_abs(),
    }
}

/// What remains of the popup at `now_ms`: `u32::MAX` while it waits for the user, 0 once it
/// has ended and the notification lives in the list only. Do not disturb ends every popup
/// but a critical one's (BR4). The bar owns the pause under the pointer, not this count.
#[must_use]
pub fn popup_ms_left(notification: &Notification, now_ms: u64, dnd: bool) -> u32 {
    let content = &notification.content;
    if content.urgency == Urgency::Critical {
        return u32::MAX;
    }
    if dnd {
        return 0;
    }
    if content.timeout_ms == 0 {
        return u32::MAX;
    }
    let end = notification.arrived_ms + u64::from(content.timeout_ms);
    u32::try_from(end.saturating_sub(now_ms)).unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn content(summary: &str, urgency: Urgency, timeout_ms: u32) -> Content {
        Content {
            app_name: "test".into(),
            summary: summary.into(),
            body: String::new(),
            actions: Vec::new(),
            urgency,
            transient: false,
            resident: false,
            desktop_entry: None,
            visual: Visual::None,
            timeout_ms,
        }
    }

    #[test]
    fn ids_start_at_one_and_a_replace_keeps_the_id_and_moves_it_last() {
        let mut store = Store::new(false);
        let first = store
            .notify(content("a", Urgency::Normal, 5000), 0, 0)
            .notification
            .id;
        let second = store
            .notify(content("b", Urgency::Normal, 5000), 0, 0)
            .notification
            .id;
        assert_eq!((first, second), (1, 2));
        let again = store.notify(content("a2", Urgency::Normal, 5000), first, 10);
        assert!(again.replaced);
        assert_eq!(again.notification.id, first);
        assert_eq!(store.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 1]);
        let unknown = store.notify(content("c", Urgency::Normal, 5000), 999, 0);
        assert!(!unknown.replaced);
        assert_eq!(unknown.notification.id, 3);
    }

    #[test]
    fn the_hundred_and_first_pushes_out_the_oldest() {
        let mut store = Store::new(false);
        for n in 0..CAPACITY {
            assert!(store
                .notify(content(&n.to_string(), Urgency::Low, 1), 0, 0)
                .evicted
                .is_empty());
        }
        let outcome = store.notify(content("new", Urgency::Low, 1), 0, 0);
        assert_eq!(outcome.evicted, [1]);
        assert_eq!(store.iter().count(), CAPACITY);
    }

    #[test]
    fn ids_wrap_past_zero_and_skip_ids_still_held() {
        let mut store = Store::new(false);
        store.last_id = u32::MAX - 1;
        let held = store
            .notify(content("x", Urgency::Low, 1), 0, 0)
            .notification
            .id;
        assert_eq!(held, u32::MAX);
        store.last_id = u32::MAX - 1;
        assert_eq!(
            store
                .notify(content("y", Urgency::Low, 1), 0, 0)
                .notification
                .id,
            1,
            "skips MAX (held) and 0"
        );
    }

    #[test]
    fn timeouts_follow_the_specification_and_critical_waits() {
        assert_eq!(timeout_ms(-1, Urgency::Normal), DEFAULT_TIMEOUT_MS);
        assert_eq!(timeout_ms(0, Urgency::Normal), 0);
        assert_eq!(timeout_ms(1200, Urgency::Low), 1200);
        assert_eq!(timeout_ms(1200, Urgency::Critical), 0);
    }

    #[test]
    fn popup_time_survives_a_bar_restart_only_within_the_timeout() {
        let at = |arrived_ms, urgency, timeout_ms| Notification {
            id: 1,
            arrived_ms,
            content: content("x", urgency, timeout_ms),
        };
        assert_eq!(
            popup_ms_left(&at(1000, Urgency::Normal, 5000), 3000, false),
            3000,
            "sent while no bar ran"
        );
        assert_eq!(
            popup_ms_left(&at(1000, Urgency::Normal, 5000), 9000, false),
            0,
            "old: list only"
        );
        assert_eq!(
            popup_ms_left(&at(0, Urgency::Normal, 0), 99_000, false),
            u32::MAX,
            "expire timeout 0"
        );
        assert_eq!(
            popup_ms_left(&at(0, Urgency::Critical, 0), 99_000, true),
            u32::MAX,
            "critical under DND"
        );
        assert_eq!(
            popup_ms_left(&at(1000, Urgency::Normal, 5000), 1000, true),
            0,
            "DND ends the popup"
        );
        assert_eq!(
            popup_ms_left(&at(0, Urgency::Normal, 0), 0, true),
            0,
            "DND ends a sticky popup too"
        );
    }
}
