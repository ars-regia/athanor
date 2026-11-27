//! The agent's requests (doc_lock_and_prompts.md, LP11): one per `BeginAuthentication`, held in
//! arrival order and keyed by polkitd's cookie. At most one is current, the one whose dialog is
//! shown; while the session is locked none is, and the requests wait in order (LP11, "While
//! locked"; "Requests in a row"). No I/O: the caller owns the bus, the helper and the screen.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use zeroize::Zeroizing;

/// polkitd's cookie for one request. Within the user it is the request's capability: whoever
/// holds it can answer for it on the helper's socket, so it is zeroized when the request goes,
/// and its `Debug` does not show it.
pub struct Cookie(Zeroizing<String>);

impl Cookie {
    #[must_use]
    pub fn new(cookie: String) -> Self {
        Self(Zeroizing::new(cookie))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cookie(..)")
    }
}

/// One `BeginAuthentication`, as polkitd sent it.
#[derive(Debug)]
pub struct Request {
    pub cookie: Cookie,
    pub action_id: String,
    pub message: String,
    pub icon_name: String,
    pub details: HashMap<String, String>,
    /// The uids of the `unix-user` identities polkit offers.
    pub identities: Vec<u32>,
}

/// A request that left the queue, and whether its dialog was shown: the caller then ends its
/// attempt, closes the dialog and drops its helper connection.
#[derive(Debug)]
pub enum Removed {
    Queued(Request),
    Current(Request),
}

#[derive(Debug)]
pub struct Queue {
    requests: VecDeque<Request>,
    locked: bool,
}

impl Queue {
    /// An empty queue. The caller starts it locked until logind's `LockedHint` has been read: a
    /// hint that cannot be read counts as locked, which only delays a dialog (plan, section 3.3).
    #[must_use]
    pub fn new(locked: bool) -> Self {
        Self {
            requests: VecDeque::new(),
            locked,
        }
    }

    /// Adds a request behind the others, and says whether it did: a cookie already held is
    /// refused, so that a cookie names one request.
    #[must_use]
    pub fn push(&mut self, request: Request) -> bool {
        if self.position(request.cookie.as_str()).is_some() {
            return false;
        }
        self.requests.push_back(request);
        true
    }

    /// The request whose dialog is shown: the oldest, unless the session is locked.
    #[must_use]
    pub fn current(&self) -> Option<&Request> {
        if self.locked {
            None
        } else {
            self.requests.front()
        }
    }

    /// logind's `LockedHint`. Locking does not drop the current request: it waits at the front,
    /// and is current again after unlock.
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Takes the request of `cookie` out of the queue: on `CancelAuthentication`, or when its
    /// attempt has ended. `None` when no request holds that cookie.
    pub fn remove(&mut self, cookie: &str) -> Option<Removed> {
        let at = self.position(cookie)?;
        let shown = at == 0 && !self.locked;
        let request = self.requests.remove(at)?;
        Some(if shown {
            Removed::Current(request)
        } else {
            Removed::Queued(request)
        })
    }

    fn position(&self, cookie: &str) -> Option<usize> {
        self.requests
            .iter()
            .position(|r| r.cookie.as_str() == cookie)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(cookie: &str) -> Request {
        Request {
            cookie: Cookie::new(cookie.to_owned()),
            action_id: "org.example.action".to_owned(),
            message: String::new(),
            icon_name: String::new(),
            details: HashMap::new(),
            identities: vec![1000],
        }
    }

    fn current(queue: &Queue) -> Option<&str> {
        queue.current().map(|r| r.cookie.as_str())
    }

    fn unlocked(cookies: &[&str]) -> Queue {
        let mut queue = Queue::new(false);
        for cookie in cookies {
            assert!(queue.push(request(cookie)));
        }
        queue
    }

    #[test]
    fn requests_are_shown_one_at_a_time_in_arrival_order() {
        let mut queue = unlocked(&["a", "b", "c"]);
        for cookie in ["a", "b", "c"] {
            assert_eq!(current(&queue), Some(cookie));
            assert!(
                matches!(queue.remove(cookie), Some(Removed::Current(r)) if r.cookie.as_str() == cookie)
            );
        }
        assert_eq!(current(&queue), None);
    }

    #[test]
    fn a_cancel_of_a_queued_request_removes_it_and_leaves_the_dialog() {
        let mut queue = unlocked(&["a", "b", "c"]);
        assert!(matches!(queue.remove("b"), Some(Removed::Queued(r)) if r.cookie.as_str() == "b"));
        assert_eq!(current(&queue), Some("a"));
        assert!(queue.remove("a").is_some());
        assert_eq!(current(&queue), Some("c"));
    }

    #[test]
    fn a_cancel_of_the_current_request_ends_it_and_shows_the_next() {
        let mut queue = unlocked(&["a", "b"]);
        assert!(matches!(queue.remove("a"), Some(Removed::Current(r)) if r.cookie.as_str() == "a"));
        assert_eq!(current(&queue), Some("b"));
    }

    #[test]
    fn an_unknown_cookie_removes_nothing() {
        let mut queue = unlocked(&["a"]);
        assert!(queue.remove("z").is_none());
        assert_eq!(current(&queue), Some("a"));
    }

    #[test]
    fn a_second_request_with_a_held_cookie_is_refused() {
        let mut queue = unlocked(&["a"]);
        assert!(!queue.push(request("a")));
        assert!(queue.remove("a").is_some());
        assert!(queue.remove("a").is_none());
    }

    #[test]
    fn requests_wait_while_locked_and_appear_in_order_after_unlock() {
        let mut queue = Queue::new(true);
        assert!(queue.push(request("a")));
        assert!(queue.push(request("b")));
        assert_eq!(current(&queue), None);
        queue.set_locked(false);
        assert_eq!(current(&queue), Some("a"));
    }

    #[test]
    fn locking_while_a_dialog_is_shown_hides_it_and_keeps_its_place() {
        let mut queue = unlocked(&["a", "b"]);
        assert_eq!(current(&queue), Some("a"));
        queue.set_locked(true);
        assert_eq!(current(&queue), None);
        queue.set_locked(false);
        assert_eq!(current(&queue), Some("a"));
    }

    #[test]
    fn a_cancel_while_locked_is_of_a_queued_request() {
        // The dialog of "a" was shown, then the session locked: nothing is shown to close.
        let mut queue = unlocked(&["a", "b"]);
        queue.set_locked(true);
        assert!(matches!(queue.remove("a"), Some(Removed::Queued(r)) if r.cookie.as_str() == "a"));
        queue.set_locked(false);
        assert_eq!(current(&queue), Some("b"));
    }

    #[test]
    fn the_cookie_is_not_shown_by_debug() {
        let shown = format!("{:?}", request("secret-cookie"));
        assert!(!shown.contains("secret-cookie"), "{shown}");
    }
}
