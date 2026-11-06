//! The inhibitor table (SD5): cookies, owners leaving the bus, suspend-only inhibitors.

use crate::timers::Hold;
use std::collections::BTreeMap;

/// Bounds (proposed, revision 3): inhibitors per owner, inhibitors in the table, and bytes of
/// an application id or a reason.
pub const MAX_PER_OWNER: usize = 16;
pub const MAX_ENTRIES: usize = 256;
pub const MAX_TEXT: usize = 256;

/// `s` cut to at most `max` bytes, at a character boundary.
pub fn truncate(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// What an inhibitor removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `org.freedesktop.ScreenSaver.Inhibit`: neither timer exists.
    Screen,
    /// `os.athanor.Idle1.InhibitSuspend`: the suspend timer only.
    SuspendOnly,
}

/// One inhibitor, with what the journal line of SD5 names: the application id and the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inhibitor {
    pub kind: Kind,
    /// The unique bus name of the caller; the inhibitor ends when it leaves the bus.
    pub owner: String,
    /// The application id of the caller's unit; empty outside an application unit.
    pub app_id: String,
    pub reason: String,
}

/// Why `remove` changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveError {
    /// The cookie is not live.
    UnknownCookie,
    /// The caller is not the peer that created the inhibitor.
    NotOwner,
}

#[derive(Debug, Default)]
pub struct Inhibitors {
    table: BTreeMap<u32, Inhibitor>,
    last: u32,
}

impl Inhibitors {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an inhibitor and returns its cookie: non-zero, unique among the live ones.
    /// Returns `None` only when all 2^32 - 1 cookies are in use.
    pub fn add(&mut self, inhibitor: Inhibitor) -> Option<u32> {
        let mut cookie = self.last;
        for _ in 0..u32::MAX {
            cookie = cookie.checked_add(1).unwrap_or(1);
            if !self.table.contains_key(&cookie) {
                self.last = cookie;
                self.table.insert(cookie, inhibitor);
                return Some(cookie);
            }
        }
        None
    }

    /// `UnInhibit` / `Uninhibit`, accepted only from the peer that created the inhibitor;
    /// any other caller is refused and nothing changes.
    pub fn remove(&mut self, cookie: u32, caller: &str) -> Result<Inhibitor, RemoveError> {
        match self.table.get(&cookie) {
            None => Err(RemoveError::UnknownCookie),
            Some(i) if i.owner != caller => Err(RemoveError::NotOwner),
            Some(_) => self.table.remove(&cookie).ok_or(RemoveError::UnknownCookie),
        }
    }

    /// An owner left the bus: its inhibitors end, and are returned for the journal.
    pub fn remove_owner(&mut self, owner: &str) -> Vec<Inhibitor> {
        let cookies: Vec<u32> = self
            .table
            .iter()
            .filter(|(_, i)| i.owner == owner)
            .map(|(c, _)| *c)
            .collect();
        cookies
            .iter()
            .filter_map(|c| self.table.remove(c))
            .collect()
    }

    pub fn hold(&self) -> Hold {
        if self.table.values().any(|i| i.kind == Kind::Screen) {
            Hold::All
        } else if self.table.is_empty() {
            Hold::None
        } else {
            Hold::SuspendOnly
        }
    }

    /// The live inhibitors in cookie order, for `inhibitors.json` (SN7).
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Inhibitor)> {
        self.table.iter().map(|(c, i)| (*c, i))
    }

    /// Puts back an inhibitor under its old cookie; new cookies continue above the highest
    /// restored one (SN7). A cookie of 0 or one already live is refused.
    pub fn restore(&mut self, cookie: u32, inhibitor: Inhibitor) -> bool {
        if cookie == 0 || self.table.contains_key(&cookie) {
            return false;
        }
        self.table.insert(cookie, inhibitor);
        self.last = self.last.max(cookie);
        true
    }

    /// How many inhibitors this owner holds.
    pub fn count_owner(&self, owner: &str) -> usize {
        self.table.values().filter(|i| i.owner == owner).count()
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inh(kind: Kind, owner: &str) -> Inhibitor {
        Inhibitor {
            kind,
            owner: owner.into(),
            app_id: "org.example.App".into(),
            reason: "playing".into(),
        }
    }

    #[test]
    fn truncate_cuts_at_a_character_boundary() {
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("aé", 2), "a");
        assert_eq!(truncate("ab", 9), "ab");
    }

    #[test]
    fn empty_table_holds_nothing() {
        assert_eq!(Inhibitors::new().hold(), Hold::None);
    }

    #[test]
    fn cookies_are_nonzero_unique_and_not_reused_at_once() {
        let mut t = Inhibitors::new();
        let a = t.add(inh(Kind::Screen, ":1.1"));
        let b = t.add(inh(Kind::Screen, ":1.1"));
        assert_eq!((a, b), (Some(1), Some(2)));
        let _ = t.remove(1, ":1.1");
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(3));
    }

    #[test]
    fn the_wrap_skips_a_live_cookie() {
        let mut t = Inhibitors::new();
        let first = t.add(inh(Kind::Screen, ":1.1")).unwrap();
        assert_eq!(first, 1);
        t.last = u32::MAX;
        // The next candidate after the wrap is 1, which is live: 2 is taken.
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(2));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn cookie_wraps_past_zero_and_live_cookies() {
        let mut t = Inhibitors::new();
        t.last = u32::MAX - 1;
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(u32::MAX));
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(1));
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(2));
    }

    #[test]
    fn unknown_cookie_removes_nothing() {
        let mut t = Inhibitors::new();
        t.add(inh(Kind::Screen, ":1.1"));
        assert_eq!(
            t.remove(99, ":1.1").unwrap_err(),
            RemoveError::UnknownCookie
        );
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn only_the_creating_peer_can_remove() {
        let mut t = Inhibitors::new();
        let c = t.add(inh(Kind::Screen, ":1.1")).unwrap();
        assert_eq!(t.remove(c, ":1.2").unwrap_err(), RemoveError::NotOwner);
        assert_eq!(t.len(), 1);
        assert_eq!(t.hold(), Hold::All);
        assert!(t.remove(c, ":1.1").is_ok());
        assert!(t.is_empty());
    }

    #[test]
    fn screen_inhibitor_holds_all_until_removed() {
        let mut t = Inhibitors::new();
        let c = t.add(inh(Kind::Screen, ":1.1")).unwrap();
        assert_eq!(t.hold(), Hold::All);
        assert_eq!(t.remove(c, ":1.1").unwrap().app_id, "org.example.App");
        assert_eq!(t.hold(), Hold::None);
    }

    #[test]
    fn suspend_only_keeps_the_blank_and_screen_wins() {
        let mut t = Inhibitors::new();
        t.add(inh(Kind::SuspendOnly, ":1.2"));
        assert_eq!(t.hold(), Hold::SuspendOnly);
        t.add(inh(Kind::Screen, ":1.3"));
        assert_eq!(t.hold(), Hold::All);
        t.remove_owner(":1.3");
        assert_eq!(t.hold(), Hold::SuspendOnly);
    }

    #[test]
    fn owner_leaving_drops_only_its_inhibitors() {
        let mut t = Inhibitors::new();
        t.add(inh(Kind::Screen, ":1.1"));
        t.add(inh(Kind::SuspendOnly, ":1.1"));
        t.add(inh(Kind::Screen, ":1.2"));
        assert_eq!(t.remove_owner(":1.1").len(), 2);
        assert_eq!(t.len(), 1);
        assert!(t.remove_owner(":1.1").is_empty());
    }
}
