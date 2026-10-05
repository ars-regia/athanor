//! The notifications the daemon holds (doc_bar.md BR4). No D-Bus and no clock: the caller
//! passes the time in milliseconds since the daemon started.

use std::collections::VecDeque;

use crate::icon::Icon;
use crate::identity::Identity;
use crate::image::Image;
use crate::rules::Retention;

pub const CAPACITY: usize = 500;
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

/// What an inline reply (NC9) shows: the entry's placeholder, its button's text and icon
/// name. The reply's own text is never held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub placeholder: String,
    pub submit_text: String,
    pub submit_icon: String,
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
    /// Progress, 0 to 100 (NC9).
    pub value: Option<u8>,
    /// `Some` when the sender declared the `inline-reply` action.
    pub reply: Option<Reply>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub arrived_ms: u64,
    /// False for a restored notification: it lives in the list only, never as a popup.
    pub popup: bool,
    /// Unix seconds.
    pub time: i64,
    pub identity: Identity,
    /// The sender's unique bus name; empty once it is gone or the bus is another one.
    pub sender: String,
    pub read: bool,
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
}

impl Store {
    #[must_use]
    pub fn new() -> Store {
        Store::default()
    }

    /// A replaced notification keeps its id, arrives again (its popup restarts) and becomes
    /// the newest. An unknown `replaces_id` gets a new id, as the specification says. `popup`
    /// is the policy's word on whether it shows at all.
    #[allow(clippy::too_many_arguments)] // one arrival, as the policy and the bus describe it
    pub fn notify(
        &mut self,
        content: Content,
        replaces_id: u32,
        now_ms: u64,
        time: i64,
        identity: Identity,
        sender: String,
        popup: bool,
    ) -> Outcome {
        let replaced = self.replaceable(replaces_id, &identity, &sender).is_some()
            && self.remove(replaces_id).is_some();
        let id = if replaced {
            replaces_id
        } else {
            self.next_id()
        };
        let notification = Notification {
            id,
            arrived_ms: now_ms,
            popup,
            time,
            identity,
            sender,
            read: false,
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

    /// Replaces the held notifications with `restored` (oldest first, the newest CAPACITY
    /// kept); ids continue above the highest.
    pub fn restore(&mut self, restored: Vec<Notification>) {
        let skip = restored.len().saturating_sub(CAPACITY);
        self.held = restored.into_iter().skip(skip).collect();
        self.last_id = self.held.iter().map(|n| n.id).max().unwrap_or(0);
    }

    /// Drops what `retention` no longer keeps at unix time `now`; the ids dropped.
    pub fn prune(&mut self, now: i64, retention: Retention) -> Vec<u32> {
        match retention {
            Retention::UntilCleared => Vec::new(),
            Retention::Days(days) => {
                let cutoff = now.saturating_sub(i64::from(days) * 86_400);
                self.drain_where(|n| n.time < cutoff)
            }
        }
    }

    /// Marks read; the ids that changed.
    pub fn mark_read(&mut self, ids: &[u32]) -> Vec<u32> {
        let mut changed = Vec::new();
        for held in &mut self.held {
            if !held.read && ids.contains(&held.id) {
                held.read = true;
                changed.push(held.id);
            }
        }
        changed
    }

    pub fn clear_all(&mut self) -> Vec<u32> {
        self.drain_where(|_| true)
    }

    pub fn clear_group(&mut self, identity: &Identity) -> Vec<u32> {
        self.drain_where(|n| n.identity == *identity)
    }

    /// Forgets the unique name `name`; the ids that carried it.
    pub fn sender_gone(&mut self, name: &str) -> Vec<u32> {
        let mut changed = Vec::new();
        for held in &mut self.held {
            if !name.is_empty() && held.sender == name {
                held.sender.clear();
                changed.push(held.id);
            }
        }
        changed
    }

    fn drain_where(&mut self, mut gone: impl FnMut(&Notification) -> bool) -> Vec<u32> {
        let mut ids = Vec::new();
        self.held.retain(|n| {
            let drop = gone(n);
            if drop {
                ids.push(n.id);
            }
            !drop
        });
        ids
    }

    pub fn close(&mut self, id: u32) -> Option<Notification> {
        self.remove(id)
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.held.iter().find(|held| held.id == id)
    }

    /// Changes only the progress of a held notification (NC9): it keeps its place, its time,
    /// its read state and its popup.
    /// `sender` is the connection that sent the update: replies go to it from now on.
    pub fn set_value(&mut self, id: u32, value: Option<u8>, sender: String) -> Option<Notification> {
        let held = self.held.iter_mut().find(|held| held.id == id)?;
        held.content.value = value;
        held.sender = sender;
        Some(held.clone())
    }

    /// The held notification `replaces_id` names, if `identity` sent it: a replace is the
    /// sender's own (else one application could take over another's reply). The application
    /// is the cgroup's, so a new process of it may replace; an unidentified sender is told
    /// apart by its connection.
    #[must_use]
    pub fn replaceable(&self, replaces_id: u32, identity: &Identity, sender: &str) -> Option<&Notification> {
        if replaces_id == 0 {
            return None;
        }
        self.get(replaces_id).filter(|held| {
            held.identity == *identity && (*identity != Identity::Other || held.sender == sender)
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.held.iter()
    }

    fn remove(&mut self, id: u32) -> Option<Notification> {
        let at = self.held.iter().position(|held| held.id == id)?;
        self.held.remove(at)
    }

    /// An id no held notification has, for an arrival the policy refuses to keep: the
    /// application still gets an answer, and the next arrival continues above it.
    pub fn fresh_id(&mut self) -> u32 {
        self.next_id()
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
/// has ended and the notification lives in the list only. Whether do not disturb hides the
/// popup was decided when the notification arrived (`Notification::popup`). The bar owns the
/// pause under the pointer, not this count.
#[must_use]
pub fn popup_ms_left(notification: &Notification, now_ms: u64) -> u32 {
    let content = &notification.content;
    if !notification.popup {
        return 0;
    }
    if content.urgency == Urgency::Critical {
        return u32::MAX;
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
            value: None,
            reply: None,
        }
    }

    pub(crate) fn notification(id: u32) -> Notification {
        Notification {
            id,
            arrived_ms: 0,
            popup: true,
            time: 0,
            identity: Identity::Other,
            sender: String::new(),
            read: false,
            content: content("restored", Urgency::Normal, 5_000),
        }
    }

    #[test]
    fn ids_start_at_one_and_a_replace_keeps_the_id_and_moves_it_last() {
        let mut store = Store::new();
        let first = store
            .notify(
                content("a", Urgency::Normal, 5000),
                0,
                0,
                0,
                Identity::Other,
                String::new(),
                true,
            )
            .notification
            .id;
        let second = store
            .notify(
                content("b", Urgency::Normal, 5000),
                0,
                0,
                0,
                Identity::Other,
                String::new(),
                true,
            )
            .notification
            .id;
        assert_eq!((first, second), (1, 2));
        let again = store.notify(
            content("a2", Urgency::Normal, 5000),
            first,
            10,
            0,
            Identity::Other,
            String::new(),
            true,
        );
        assert!(again.replaced);
        assert_eq!(again.notification.id, first);
        assert_eq!(store.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 1]);
        let unknown = store.notify(
            content("c", Urgency::Normal, 5000),
            999,
            0,
            0,
            Identity::Other,
            String::new(),
            true,
        );
        assert!(!unknown.replaced);
        assert_eq!(unknown.notification.id, 3);
    }

    #[test]
    fn only_the_same_application_replaces_a_notification() {
        let (a, b) = (Identity::App("a".into()), Identity::App("b".into()));
        let mut store = Store::new();
        let put = |store: &mut Store, replaces, identity: &Identity, sender: &str| {
            store.notify(
                content("x", Urgency::Normal, 5000),
                replaces,
                0,
                0,
                identity.clone(),
                sender.into(),
                true,
            )
        };
        let mine = put(&mut store, 0, &a, ":1.1").notification.id;
        // The same application from a new connection replaces; another application does not.
        let again = put(&mut store, mine, &a, ":1.9");
        assert!(again.replaced && again.notification.id == mine);
        let stolen = put(&mut store, mine, &b, ":1.2");
        assert!(!stolen.replaced && stolen.notification.id != mine);
        assert_eq!(store.get(mine).map(|n| n.sender.as_str()), Some(":1.9"));
        // Unidentified senders are told apart by their connection.
        let other = put(&mut store, 0, &Identity::Other, ":1.3").notification.id;
        assert!(!put(&mut store, other, &Identity::Other, ":1.4").replaced);
        assert!(put(&mut store, other, &Identity::Other, ":1.3").replaced);
    }

    #[test]
    fn a_new_value_changes_the_notification_in_place() {
        let mut store = Store::new();
        let mut ids = Vec::new();
        for n in 0..3 {
            ids.push(
                store
                    .notify(
                        content("p", Urgency::Normal, 5000),
                        0,
                        7 + n,
                        100,
                        Identity::Other,
                        String::new(),
                        true,
                    )
                    .notification
                    .id,
            );
        }
        store.mark_read(&[ids[0]]);
        let updated = store.set_value(ids[0], Some(60), ":1.7".into()).expect("held");
        assert_eq!(updated.content.value, Some(60));
        assert!(updated.read && updated.popup);
        assert_eq!(updated.sender, ":1.7", "a new process of the application is who answers now");
        assert_eq!((updated.arrived_ms, updated.time), (7, 100));
        assert_eq!(store.iter().map(|n| n.id).collect::<Vec<_>>(), ids);
        assert!(store.set_value(999, Some(1), String::new()).is_none());
    }

    #[test]
    fn the_next_past_capacity_pushes_out_the_oldest() {
        let mut store = Store::new();
        for n in 0..CAPACITY {
            assert!(store
                .notify(
                    content(&n.to_string(), Urgency::Low, 1),
                    0,
                    0,
                    0,
                    Identity::Other,
                    String::new(),
                    true,
                )
                .evicted
                .is_empty());
        }
        let outcome = store.notify(
            content("new", Urgency::Low, 1),
            0,
            0,
            0,
            Identity::Other,
            String::new(),
            true,
        );
        assert_eq!(outcome.evicted, [1]);
        assert_eq!(store.iter().count(), CAPACITY);
    }

    #[test]
    fn ids_wrap_past_zero_and_skip_ids_still_held() {
        let mut store = Store::new();
        store.last_id = u32::MAX - 1;
        let held = store
            .notify(
                content("x", Urgency::Low, 1),
                0,
                0,
                0,
                Identity::Other,
                String::new(),
                true,
            )
            .notification
            .id;
        assert_eq!(held, u32::MAX);
        store.last_id = u32::MAX - 1;
        assert_eq!(
            store
                .notify(
                    content("y", Urgency::Low, 1),
                    0,
                    0,
                    0,
                    Identity::Other,
                    String::new(),
                    true,
                )
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
    fn a_restored_notification_never_pops_up() {
        for (urgency, timeout_ms) in [(Urgency::Critical, 0), (Urgency::Normal, 0)] {
            let mut restored = notification(1);
            restored.popup = false;
            restored.content = content("x", urgency, timeout_ms);
            assert_eq!(popup_ms_left(&restored, 0), 0, "{urgency:?}");
        }
    }

    #[test]
    fn popup_time_survives_a_bar_restart_only_within_the_timeout() {
        let at = |arrived_ms, urgency, timeout_ms| Notification {
            id: 1,
            arrived_ms,
            popup: true,
            time: 0,
            identity: Identity::Other,
            sender: String::new(),
            read: false,
            content: content("x", urgency, timeout_ms),
        };
        assert_eq!(
            popup_ms_left(&at(1000, Urgency::Normal, 5000), 3000),
            3000,
            "sent while no bar ran"
        );
        assert_eq!(
            popup_ms_left(&at(1000, Urgency::Normal, 5000), 9000),
            0,
            "old: list only"
        );
        assert_eq!(
            popup_ms_left(&at(0, Urgency::Normal, 0), 99_000),
            u32::MAX,
            "expire timeout 0"
        );
        assert_eq!(
            popup_ms_left(&at(0, Urgency::Critical, 0), 99_000),
            u32::MAX,
            "critical waits"
        );
        let mut hidden = at(0, Urgency::Normal, 0);
        hidden.popup = false;
        assert_eq!(
            popup_ms_left(&hidden, 0),
            0,
            "a popup the policy hid ends at once, sticky or not"
        );
    }
}
