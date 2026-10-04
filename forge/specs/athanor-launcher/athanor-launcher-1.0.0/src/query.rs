//! LA5: shown again within 30 seconds, the launcher keeps the last query, selected.

use std::time::{Duration, Instant};

pub const KEEP: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct Memory {
    last: Option<(String, Instant)>,
}

impl Memory {
    pub fn hidden(&mut self, text: &str, at: Instant) {
        self.last = (!text.trim().is_empty()).then(|| (text.to_owned(), at));
    }

    /// The query to show again, once.
    pub fn shown(&mut self, at: Instant) -> Option<String> {
        let (text, hidden) = self.last.take()?;
        (at.saturating_duration_since(hidden) <= KEEP).then_some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_query_comes_back_within_thirty_seconds_once() {
        let at = Instant::now();
        let mut memory = Memory::default();
        memory.hidden("firefox", at);
        assert_eq!(memory.shown(at + Duration::from_secs(29)).as_deref(), Some("firefox"));
        assert_eq!(memory.shown(at + Duration::from_secs(29)), None, "taken");
        memory.hidden("firefox", at);
        assert_eq!(memory.shown(at + KEEP + Duration::from_millis(1)), None, "too late");
        memory.hidden("   ", at);
        assert_eq!(memory.shown(at), None, "nothing to keep");
    }
}
