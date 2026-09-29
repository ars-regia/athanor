//! Which notification popups show (doc_bar.md BR4): at most three, the newest nearest the
//! panel, the others counted as "+N waiting". No clock and no GTK: the caller passes the
//! time that elapsed, and pauses a countdown by not calling `tick`.

use crate::notices::WAITS;

pub const VISIBLE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Live {
    id: u32,
    /// `None` while the popup waits for the user.
    left_ms: Option<u32>,
    critical: bool,
}

/// The popups that have not ended, oldest first.
#[derive(Debug, Default)]
pub struct Popups {
    live: Vec<Live>,
}

impl Popups {
    /// Shows `id`'s popup with `ms_left` to go (`WAITS`: until the user closes it). A popup
    /// already there starts again as the newest. False when `ms_left` is 0: it shows in the
    /// list only.
    pub fn show(&mut self, id: u32, ms_left: u32, critical: bool) -> bool {
        self.remove(id);
        if ms_left == 0 {
            return false;
        }
        self.live.push(Live {
            id,
            left_ms: (ms_left != WAITS).then_some(ms_left),
            critical,
        });
        true
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.live.len();
        self.live.retain(|live| live.id != id);
        self.live.len() != before
    }

    pub fn clear(&mut self) {
        self.live.clear();
    }

    /// Counts every popup down, the waiting ones too (ruling 5), and returns the ids whose
    /// time ran out, oldest first.
    pub fn tick(&mut self, elapsed_ms: u32) -> Vec<u32> {
        let mut ended = Vec::new();
        self.live.retain_mut(|live| match live.left_ms.as_mut() {
            Some(left) => {
                *left = left.saturating_sub(elapsed_ms);
                if *left == 0 {
                    ended.push(live.id);
                }
                *left != 0
            }
            None => true,
        });
        ended
    }

    /// Do not disturb: every popup but a critical one ends (BR4).
    pub fn end_non_critical(&mut self) -> Vec<u32> {
        let ended = self
            .live
            .iter()
            .filter(|live| !live.critical)
            .map(|live| live.id)
            .collect();
        self.live.retain(|live| live.critical);
        ended
    }

    /// The popups on screen, newest first.
    #[must_use]
    pub fn visible(&self) -> Vec<u32> {
        self.live
            .iter()
            .rev()
            .take(VISIBLE)
            .map(|live| live.id)
            .collect()
    }

    #[must_use]
    pub fn waiting(&self) -> usize {
        self.live.len().saturating_sub(VISIBLE)
    }

    /// Some popup still has a countdown: the caller keeps its timer.
    #[must_use]
    pub fn counting(&self) -> bool {
        self.live.iter().any(|live| live.left_ms.is_some())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_show_newest_first_and_the_rest_wait() {
        let mut popups = Popups::default();
        for id in 1..=5 {
            assert!(popups.show(id, WAITS, false));
        }
        assert_eq!(popups.visible(), [5, 4, 3]);
        assert_eq!(popups.waiting(), 2);
        assert!(popups.remove(5));
        assert_eq!(popups.visible(), [4, 3, 2], "a waiting one takes the place");
    }

    #[test]
    fn zero_shows_nothing() {
        let mut popups = Popups::default();
        assert!(!popups.show(1, 0, false));
        assert!(popups.is_empty());
    }

    #[test]
    fn a_waiting_popup_never_ends_by_itself() {
        let mut popups = Popups::default();
        popups.show(1, WAITS, true);
        assert!(!popups.counting());
        assert!(popups.tick(u32::MAX).is_empty());
        assert_eq!(popups.visible(), [1]);
    }

    #[test]
    fn a_countdown_ends_exactly_once() {
        let mut popups = Popups::default();
        popups.show(1, 1000, false);
        popups.show(2, 3000, false);
        assert!(popups.counting());
        assert!(popups.tick(999).is_empty());
        assert_eq!(popups.tick(1), [1]);
        assert_eq!(popups.tick(5000), [2]);
        assert!(popups.tick(5000).is_empty());
        assert!(!popups.counting());
    }

    #[test]
    fn a_replacement_restarts_as_the_newest() {
        let mut popups = Popups::default();
        popups.show(1, 1000, false);
        popups.show(2, WAITS, false);
        popups.tick(900);
        popups.show(1, 1000, false);
        assert_eq!(popups.visible(), [1, 2]);
        assert!(popups.tick(900).is_empty(), "the time started again");
    }

    #[test]
    fn waiting_popups_count_down_too() {
        let mut popups = Popups::default();
        popups.show(1, 500, false);
        for id in 2..=4 {
            popups.show(id, WAITS, false);
        }
        assert_eq!(popups.waiting(), 1);
        assert_eq!(popups.tick(500), [1]);
        assert_eq!(popups.waiting(), 0);
    }

    #[test]
    fn do_not_disturb_keeps_only_critical_popups() {
        let mut popups = Popups::default();
        popups.show(1, WAITS, false);
        popups.show(2, WAITS, true);
        popups.show(3, 4000, false);
        assert_eq!(popups.end_non_critical(), [1, 3]);
        assert_eq!(popups.visible(), [2]);
    }
}
