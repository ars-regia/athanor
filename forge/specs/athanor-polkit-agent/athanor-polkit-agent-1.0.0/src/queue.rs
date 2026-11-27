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

/// The most requests held at once. polkitd sends one per action a program asks for, and a
/// user answers them one at a time, so more than a few is a program asking in a loop; the bound
/// keeps that within the unit's `MemoryMax=` (the agent's own choice, beyond the plan).
pub const MAX_REQUESTS: usize = 16;
/// The most bytes of the cookie, the action id, the message, the icon name and of each key and
/// value of the details. polkit's own texts are a line or two; a dialog shows no more.
pub const MAX_TEXT_BYTES: usize = 4096;
/// The most entries of the details, and the most identities offered.
pub const MAX_DETAILS: usize = 32;
pub const MAX_IDENTITIES: usize = 32;

/// Why `push` refused a request; the caller answers polkitd's `BeginAuthentication` with an
/// error for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A request already holds the cookie, so that a cookie names one request.
    HeldCookie,
    /// MAX_REQUESTS are held.
    Full,
    /// A text, the details or the identities are over their bound.
    TooLarge,
}

/// A request that left the queue, and whether it had an attempt, a dialog and a helper
/// connection: for `Attempted` the caller ends that attempt (plan, section 3.1), even when a
/// lock hides its dialog.
#[derive(Debug)]
pub enum Removed {
    Queued(Request),
    Attempted(Request),
}

#[derive(Debug)]
pub struct Queue {
    requests: VecDeque<Request>,
    locked: bool,
    /// Whether the front request has had an attempt: it has been current since it reached the
    /// front. A lock hides its dialog and keeps the attempt.
    attempted: bool,
}

impl Queue {
    /// An empty queue. The caller starts it locked until logind's `LockedHint` has been read: a
    /// hint that cannot be read counts as locked, which only delays a dialog (plan, section 3.3).
    #[must_use]
    pub fn new(locked: bool) -> Self {
        Self {
            requests: VecDeque::new(),
            locked,
            attempted: false,
        }
    }

    /// Adds a request behind the others, or refuses it: a cookie already held, a full queue,
    /// or a request over its bounds.
    pub fn push(&mut self, request: Request) -> Result<(), Refused> {
        if self.position(request.cookie.as_str()).is_some() {
            return Err(Refused::HeldCookie);
        }
        if self.requests.len() >= MAX_REQUESTS {
            return Err(Refused::Full);
        }
        if too_large(&request) {
            return Err(Refused::TooLarge);
        }
        self.requests.push_back(request);
        self.attempted |= !self.locked;
        Ok(())
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

    /// logind's `LockedHint`, and the request whose dialog it hides or shows, if any. Locking
    /// does not drop the current request or end its attempt: it waits at the front, and is
    /// current again after unlock.
    pub fn set_locked(&mut self, locked: bool) -> Option<&Request> {
        if self.locked == locked {
            return None;
        }
        self.locked = locked;
        self.attempted |= !locked && !self.requests.is_empty();
        self.requests.front()
    }

    /// Takes the request of `cookie` out of the queue: on `CancelAuthentication`, or when its
    /// attempt has ended. `None` when no request holds that cookie.
    pub fn remove(&mut self, cookie: &str) -> Option<Removed> {
        let at = self.position(cookie)?;
        let request = self.requests.remove(at)?;
        if at != 0 {
            return Some(Removed::Queued(request));
        }
        let attempted = self.attempted;
        self.attempted = !self.locked && !self.requests.is_empty();
        Some(if attempted {
            Removed::Attempted(request)
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

fn too_large(request: &Request) -> bool {
    let texts = [
        request.cookie.as_str(),
        &request.action_id,
        &request.message,
        &request.icon_name,
    ];
    texts.iter().any(|t| t.len() > MAX_TEXT_BYTES)
        || request.details.len() > MAX_DETAILS
        || request
            .details
            .iter()
            .any(|(k, v)| k.len() > MAX_TEXT_BYTES || v.len() > MAX_TEXT_BYTES)
        || request.identities.len() > MAX_IDENTITIES
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
            assert_eq!(queue.push(request(cookie)), Ok(()));
        }
        queue
    }

    #[test]
    fn requests_are_shown_one_at_a_time_in_arrival_order() {
        let mut queue = unlocked(&["a", "b", "c"]);
        for cookie in ["a", "b", "c"] {
            assert_eq!(current(&queue), Some(cookie));
            assert!(
                matches!(queue.remove(cookie), Some(Removed::Attempted(r)) if r.cookie.as_str() == cookie)
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
        assert!(
            matches!(queue.remove("a"), Some(Removed::Attempted(r)) if r.cookie.as_str() == "a")
        );
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
        assert_eq!(queue.push(request("a")), Err(Refused::HeldCookie));
        assert!(queue.remove("a").is_some());
        assert!(queue.remove("a").is_none());
    }

    #[test]
    fn requests_wait_while_locked_and_appear_in_order_after_unlock() {
        let mut queue = Queue::new(true);
        assert_eq!(queue.push(request("a")), Ok(()));
        assert_eq!(queue.push(request("b")), Ok(()));
        assert_eq!(current(&queue), None);
        queue.set_locked(false);
        assert_eq!(current(&queue), Some("a"));
    }

    #[test]
    fn locking_while_a_dialog_is_shown_hides_it_and_keeps_its_place() {
        let mut queue = unlocked(&["a", "b"]);
        assert_eq!(current(&queue), Some("a"));
        let hidden = queue.set_locked(true).map(|r| r.cookie.as_str().to_owned());
        assert_eq!(hidden.as_deref(), Some("a"));
        assert_eq!(current(&queue), None);
        assert!(queue.set_locked(true).is_none());
        let shown = queue
            .set_locked(false)
            .map(|r| r.cookie.as_str().to_owned());
        assert_eq!(shown.as_deref(), Some("a"));
        assert_eq!(current(&queue), Some("a"));
    }

    #[test]
    fn a_cancel_of_a_request_never_shown_has_no_attempt_to_end() {
        let mut queue = Queue::new(true);
        assert_eq!(queue.push(request("a")), Ok(()));
        assert_eq!(queue.push(request("b")), Ok(()));
        assert!(matches!(queue.remove("a"), Some(Removed::Queued(r)) if r.cookie.as_str() == "a"));
        assert!(queue.set_locked(false).is_some());
        assert_eq!(current(&queue), Some("b"));
    }

    #[test]
    fn a_cancel_of_a_shown_request_hidden_by_a_lock_ends_its_attempt() {
        // The dialog of "a" was shown, then the session locked, then polkitd cancelled "a".
        let mut queue = unlocked(&["a", "b"]);
        assert!(queue.set_locked(true).is_some());
        assert!(
            matches!(queue.remove("a"), Some(Removed::Attempted(r)) if r.cookie.as_str() == "a")
        );
        // "b" reached the front while locked: it has had no attempt yet.
        assert!(matches!(queue.remove("b"), Some(Removed::Queued(_))));
    }

    #[test]
    fn the_queue_holds_max_requests_and_refuses_one_more() {
        let mut queue = Queue::new(false);
        for n in 0..MAX_REQUESTS {
            assert_eq!(queue.push(request(&n.to_string())), Ok(()));
        }
        assert_eq!(queue.push(request("one-more")), Err(Refused::Full));
    }

    #[test]
    fn a_request_at_its_bounds_is_held_and_one_past_them_refused() {
        let at_bound = || {
            let mut r = request("a");
            r.message = "m".repeat(MAX_TEXT_BYTES);
            r.details = (0..MAX_DETAILS)
                .map(|n| (n.to_string(), "v".repeat(MAX_TEXT_BYTES)))
                .collect();
            r.identities = vec![1000; MAX_IDENTITIES];
            r
        };
        assert_eq!(Queue::new(false).push(at_bound()), Ok(()));
        let past: [fn(&mut Request); 5] = [
            |r| r.message.push('m'),
            |r| r.action_id = "a".repeat(MAX_TEXT_BYTES + 1),
            |r| {
                r.details.insert("one-more".to_owned(), String::new());
            },
            |r| {
                r.details
                    .insert("0".to_owned(), "v".repeat(MAX_TEXT_BYTES + 1));
            },
            |r| r.identities.push(1000),
        ];
        for grow in past {
            let mut r = at_bound();
            grow(&mut r);
            assert_eq!(Queue::new(false).push(r), Err(Refused::TooLarge));
        }
    }

    #[test]
    fn the_cookie_is_not_shown_by_debug() {
        let shown = format!("{:?}", request("secret-cookie"));
        assert!(!shown.contains("secret-cookie"), "{shown}");
    }
}
