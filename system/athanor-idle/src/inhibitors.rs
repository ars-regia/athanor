//! The inhibitor table (SD5): cookies, owners leaving the bus, suspend-only inhibitors.

use crate::timers::Hold;
use std::collections::BTreeMap;

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

    /// `UnInhibit` / `Uninhibit`: `None` for a cookie that is not live.
    pub fn remove(&mut self, cookie: u32) -> Option<Inhibitor> {
        self.table.remove(&cookie)
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
    fn empty_table_holds_nothing() {
        assert_eq!(Inhibitors::new().hold(), Hold::None);
    }

    #[test]
    fn cookies_are_nonzero_unique_and_not_reused_at_once() {
        let mut t = Inhibitors::new();
        let a = t.add(inh(Kind::Screen, ":1.1"));
        let b = t.add(inh(Kind::Screen, ":1.1"));
        assert_eq!((a, b), (Some(1), Some(2)));
        t.remove(1);
        assert_eq!(t.add(inh(Kind::Screen, ":1.1")), Some(3));
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
        assert_eq!(t.remove(99), None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn screen_inhibitor_holds_all_until_removed() {
        let mut t = Inhibitors::new();
        let c = t.add(inh(Kind::Screen, ":1.1")).unwrap();
        assert_eq!(t.hold(), Hold::All);
        assert_eq!(t.remove(c).unwrap().app_id, "org.example.App");
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
