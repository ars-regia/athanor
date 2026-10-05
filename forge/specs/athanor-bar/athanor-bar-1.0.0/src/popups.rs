//! Which notification popups show (doc_bar.md BR4): at most three, the newest nearest the
//! panel, the others wait their turn. No clock and no GTK: the caller passes the
//! time that elapsed, and pauses a countdown by not calling `tick`.

use std::collections::HashMap;

use athanor_layout::preset::PanelEdge;
use gtk4_layer_shell::Edge;

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

/// The output new popups go to (BR4, "on the output of the active workspace"): the first
/// of `outputs` that the activated window is on, `activated` holding that window's outputs
/// in the order it entered them. The first output when no window is activated, or when the
/// activated one is on none of `outputs`. `None` with no output.
#[must_use]
pub fn target_output(activated: Option<&[String]>, outputs: &[String]) -> Option<usize> {
    if outputs.is_empty() {
        return None;
    }
    let focused = activated
        .into_iter()
        .flatten()
        .find_map(|name| outputs.iter().position(|output| output == name));
    Some(focused.unwrap_or(0))
}

/// Where the popups sit (NC5, `popup_corner`). `Bar` follows the panel's edge, at the end side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    #[default]
    Bar,
    TopStart,
    TopEnd,
    BottomStart,
    BottomEnd,
}

/// What the popups take from the daemon's `Settings` (NC12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub corner: Corner,
    /// Popups show only the application's name and icon.
    pub private: bool,
    /// While true the popups stay on the top layer, below fullscreen windows: the do not
    /// disturb trigger already silences them there.
    pub trigger_fullscreen: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            corner: Corner::Bar,
            private: false,
            trigger_fullscreen: true,
        }
    }
}

impl Look {
    /// From the daemon's `Settings` reply; a missing or unreadable key keeps its default.
    #[must_use]
    pub fn from_settings(pairs: &HashMap<String, String>) -> Look {
        let default = Look::default();
        let flag = |key: &str, fallback: bool| match pairs.get(key).map(String::as_str) {
            Some("true") => true,
            Some("false") => false,
            _ => fallback,
        };
        Look {
            corner: match pairs.get("popup_corner").map(String::as_str) {
                Some("top-start") => Corner::TopStart,
                Some("top-end") => Corner::TopEnd,
                Some("bottom-start") => Corner::BottomStart,
                Some("bottom-end") => Corner::BottomEnd,
                _ => default.corner,
            },
            private: flag("private_popups", default.private),
            trigger_fullscreen: flag("trigger_fullscreen", default.trigger_fullscreen),
        }
    }
}

/// The vertical and the horizontal edge the popups are anchored to. Left and right swap in a
/// right-to-left locale, as the bar's own end side does.
#[must_use]
pub fn corner_edges(corner: Corner, panel_edge: PanelEdge, rtl: bool) -> (Edge, Edge) {
    let (start, end) = if rtl {
        (Edge::Right, Edge::Left)
    } else {
        (Edge::Left, Edge::Right)
    };
    let panel = match panel_edge {
        PanelEdge::Top => Edge::Top,
        PanelEdge::Bottom => Edge::Bottom,
    };
    match corner {
        Corner::Bar => (panel, end),
        Corner::TopStart => (Edge::Top, start),
        Corner::TopEnd => (Edge::Top, end),
        Corner::BottomStart => (Edge::Bottom, start),
        Corner::BottomEnd => (Edge::Bottom, end),
    }
}

/// What a private popup shows (NC12): the application's name and nothing of the notification.
/// The first is the text on the card, the second its accessible name, which must not carry the
/// summary either.
#[must_use]
pub fn private_card(app_name: &str) -> (String, String) {
    (app_name.to_owned(), app_name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_output_follows_the_activated_window() {
        let outputs = ["DP-1".to_owned(), "HDMI-A-1".to_owned()];
        let on = |names: &[&str]| {
            names
                .iter()
                .map(|&name| name.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(target_output(Some(&on(&["HDMI-A-1"])), &outputs), Some(1));
        assert_eq!(
            target_output(Some(&on(&["HDMI-A-1", "DP-1"])), &outputs),
            Some(1),
            "the output the window entered first"
        );
        assert_eq!(
            target_output(Some(&on(&["gone", "HDMI-A-1"])), &outputs),
            Some(1),
            "an output that left is skipped"
        );
        assert_eq!(target_output(Some(&[]), &outputs), Some(0));
        assert_eq!(
            target_output(None, &outputs),
            Some(0),
            "no window activated"
        );
        assert_eq!(target_output(Some(&on(&["DP-1"])), &[]), None);
    }

    #[test]
    fn three_show_newest_first_and_the_rest_wait() {
        let mut popups = Popups::default();
        for id in 1..=5 {
            assert!(popups.show(id, WAITS, false));
        }
        assert_eq!(popups.visible(), [5, 4, 3]);
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
        assert_eq!(popups.visible(), [4, 3, 2]);
        assert_eq!(popups.tick(500), [1]);
        assert_eq!(popups.visible(), [4, 3, 2]);
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

    #[test]
    fn the_five_corners_anchor_two_edges_and_mirror_in_rtl() {
        use Corner::*;
        let edges = |corner, panel, rtl| corner_edges(corner, panel, rtl);
        assert_eq!(edges(Bar, PanelEdge::Top, false), (Edge::Top, Edge::Right));
        assert_eq!(edges(Bar, PanelEdge::Bottom, false), (Edge::Bottom, Edge::Right));
        assert_eq!(edges(Bar, PanelEdge::Top, true), (Edge::Top, Edge::Left));
        // The four named corners ignore the panel.
        for panel in [PanelEdge::Top, PanelEdge::Bottom] {
            assert_eq!(edges(TopStart, panel, false), (Edge::Top, Edge::Left));
            assert_eq!(edges(TopEnd, panel, false), (Edge::Top, Edge::Right));
            assert_eq!(edges(BottomStart, panel, false), (Edge::Bottom, Edge::Left));
            assert_eq!(edges(BottomEnd, panel, false), (Edge::Bottom, Edge::Right));
            assert_eq!(edges(TopStart, panel, true), (Edge::Top, Edge::Right));
            assert_eq!(edges(BottomEnd, panel, true), (Edge::Bottom, Edge::Left));
        }
    }

    #[test]
    fn a_private_card_carries_only_the_name() {
        let (shown, accessible) = private_card("Mail");
        assert_eq!((shown.as_str(), accessible.as_str()), ("Mail", "Mail"));
    }

    #[test]
    fn the_look_reads_the_settings_and_keeps_defaults_for_what_it_cannot_read() {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<HashMap<_, _>>()
        };
        assert_eq!(Look::from_settings(&HashMap::new()), Look::default());
        assert_eq!(
            Look::from_settings(&pairs(&[
                ("popup_corner", "bottom-start"),
                ("private_popups", "true"),
                ("trigger_fullscreen", "false"),
            ])),
            Look {
                corner: Corner::BottomStart,
                private: true,
                trigger_fullscreen: false
            }
        );
        assert_eq!(
            Look::from_settings(&pairs(&[("popup_corner", "middle"), ("private_popups", "yes")])),
            Look::default()
        );
    }
}
