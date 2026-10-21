//! What the driver keeps about popups, without a compositor: which one is open, what dismisses
//! it, and when a tooltip shows, with GTK 4's delays (gtk/gtktooltip.c).

use crate::focus;
use crate::widget::{Ui, WidgetId};
use std::time::{Duration, Instant};

/// How long the pointer rests on a widget before its tooltip shows: GTK's `HOVER_TIMEOUT`.
pub(crate) const HOVER: Duration = Duration::from_millis(500);
/// The same wait while browsing, shortly after another tooltip hid: GTK's `BROWSE_TIMEOUT`.
pub(crate) const BROWSE: Duration = Duration::from_millis(60);
/// How long after a tooltip hides the next one browses: GTK's `BROWSE_DISABLE_TIMEOUT`.
pub(crate) const BROWSE_DISABLE: Duration = Duration::from_millis(500);
/// How far a tooltip keeps from its widget's bounds, as GTK's anchor rectangle without a cursor
/// theme to size it.
pub(crate) const TOOLTIP_PADDING: f32 = 4.0;

/// The popup open on the application's surfaces: at most one, as GTK's autohide popovers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OpenPopup<S> {
    /// The popup's surface.
    pub surface: S,
    /// The surface it was opened from.
    pub parent: S,
    /// The widget of the parent it is placed against.
    pub anchor: WidgetId,
    /// A menu, which grabs the input and takes the focus; otherwise a tooltip, which does neither.
    pub menu: bool,
}

/// What may close the open popup.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Dismiss<S> {
    /// The Escape key.
    Escape,
    /// A press of a pointer button on one of the application's surfaces.
    Press(S),
}

impl<S: Copy + PartialEq> OpenPopup<S> {
    /// Records `popup`, opened from `anchor` of `parent`: a menu expands its button and puts the
    /// focus on its first item, as GTK's popover does when it shows.
    pub fn opened(
        parent_surface: S,
        anchor: WidgetId,
        surface: S,
        parent: &mut Ui,
        popup: &mut Ui,
        menu: bool,
    ) -> Self {
        if menu {
            parent.set_expanded(anchor, true);
            popup.set_focus(focus::step(popup, false));
        }
        Self {
            surface,
            parent: parent_surface,
            anchor,
            menu,
        }
    }

    /// Whether `input` closes the popup: Escape or a press outside closes a menu; a tooltip
    /// closes on its own terms (see [`Tooltip`]).
    pub fn dismissed_by(&self, input: Dismiss<S>) -> bool {
        match input {
            _ if !self.menu => false,
            Dismiss::Escape => true,
            Dismiss::Press(on) => on != self.surface,
        }
    }

    /// The popup closed: a menu returns the focus to its button, which collapses.
    pub fn closed(&self, parent: &mut Ui) {
        if self.menu {
            parent.popup_closed(self.anchor);
        }
    }
}

/// The surface whose accessibility adapter is the focused window, given the open popup and the
/// surface with the keyboard: an open menu takes the application's keys, whichever surface the
/// compositor sends them to, so it is the focused window while the application has them.
pub(crate) fn focused_window<S: Copy>(
    open: Option<OpenPopup<S>>,
    keyboard: Option<S>,
) -> Option<S> {
    match open {
        Some(popup) if popup.menu => keyboard.map(|_| popup.surface),
        _ => keyboard,
    }
}

/// The serial a menu's xdg_popup.grab names: the press of the button held, until its release or
/// the pointer's leave, or the key being handled, as GTK grabs with its implicit grab's serial.
/// A menu opened with no input under way, as by an assistive technology's action, grabs nothing:
/// mutter and KWin refuse a stale serial and dismiss the popup at once.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GrabSerial {
    press: Option<u32>,
    key: Option<u32>,
}

impl GrabSerial {
    /// A button was pressed with `serial`.
    pub fn pressed(&mut self, serial: u32) {
        self.press = Some(serial);
    }

    /// A button was released, or the pointer left: no press is held.
    pub fn released(&mut self) {
        self.press = None;
    }

    /// The key pressed with `serial` is being handled; `None` once it has been.
    pub fn key(&mut self, serial: Option<u32>) {
        self.key = serial;
    }

    /// The serial to grab with, while an input is under way.
    pub fn serial(&self) -> Option<u32> {
        self.key.or(self.press)
    }
}

/// When the tooltip of the widget under the pointer, or of the focused one, shows. `T` names
/// the widget; each wait carries a generation, and a timer of an older one is stale.
#[derive(Debug)]
pub(crate) struct Tooltip<T> {
    target: Option<T>,
    shown: bool,
    hidden_at: Option<Instant>,
    generation: u64,
}

impl<T> Default for Tooltip<T> {
    fn default() -> Self {
        Self {
            target: None,
            shown: false,
            hidden_at: None,
            generation: 0,
        }
    }
}

impl<T: Copy + PartialEq> Tooltip<T> {
    /// The widget with a tooltip under the pointer, or newly focused, is now `target`. Returns
    /// whether the shown tooltip hides, and how long to wait for the new one with the generation
    /// of that wait.
    pub fn point(&mut self, target: Option<T>, now: Instant) -> (bool, Option<(Duration, u64)>) {
        if target == self.target {
            return (false, None);
        }
        let hide = self.hide(now);
        self.target = target;
        let browsing = self
            .hidden_at
            .is_some_and(|at| now.saturating_duration_since(at) < BROWSE_DISABLE);
        let wait = if browsing { BROWSE } else { HOVER };
        (hide, target.map(|_| (wait, self.generation)))
    }

    /// A press, a key or a scroll: the shown tooltip hides, and the pending one does not show
    /// until the target changes. Returns whether one was shown.
    pub fn hide(&mut self, now: Instant) -> bool {
        self.generation += 1;
        let shown = std::mem::take(&mut self.shown);
        if shown {
            self.hidden_at = Some(now);
        }
        shown
    }

    /// The wait of `generation` ran out: the target whose tooltip shows now, unless it is stale.
    pub fn due(&mut self, generation: u64) -> Option<T> {
        let target = self.target.filter(|_| generation == self.generation)?;
        self.shown = true;
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crate::widget::{Kind, Ui};
    use std::time::{Duration, Instant};

    #[test]
    fn a_tooltip_waits_gtks_hover_delay_then_browses_at_its_short_one() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut tip = Tooltip::default();
        // The pointer comes onto a widget with a tooltip: 500 ms.
        let (hide, wait) = tip.point(Some(1), at(0));
        let Some((delay, first)) = wait else {
            panic!("no tooltip pending")
        };
        assert_eq!((hide, delay), (false, Duration::from_millis(500)));
        assert_eq!(tip.point(Some(1), at(100)), (false, None), "same widget");
        assert_eq!(tip.due(first), Some(1));
        // On to the next widget within 500 ms of the hide: browse mode, 60 ms.
        let (hide, wait) = tip.point(Some(2), at(700));
        assert!(hide, "the shown tooltip hides");
        assert_eq!(wait.map(|w| w.0), Some(Duration::from_millis(60)));
        assert_eq!(tip.due(first), None, "a timer of an older target is stale");
        let Some((_, second)) = wait else {
            panic!("no tooltip pending")
        };
        assert_eq!(tip.due(second), Some(2));
        // Off every widget, then onto one after the browse window: 500 ms again.
        assert_eq!(tip.point(None, at(800)), (true, None));
        let (_, wait) = tip.point(Some(3), at(1400));
        assert_eq!(wait.map(|w| w.0), Some(Duration::from_millis(500)));
    }

    #[test]
    fn a_press_or_a_key_hides_the_tooltip_until_the_target_changes() {
        let now = Instant::now();
        let mut tip = Tooltip::default();
        let Some((_, generation)) = tip.point(Some(1), now).1 else {
            panic!("no tooltip pending")
        };
        assert!(!tip.hide(now), "nothing was shown");
        assert_eq!(
            tip.due(generation),
            None,
            "the pending tooltip is cancelled"
        );
        assert_eq!(
            tip.point(Some(1), now),
            (false, None),
            "the same widget waits"
        );
        let Some((_, generation)) = tip.point(Some(2), now).1 else {
            panic!("no tooltip pending")
        };
        assert_eq!(tip.due(generation), Some(2));
        assert!(tip.hide(now));
    }

    #[test]
    fn escape_and_a_press_outside_dismiss_a_menu_but_not_a_tooltip() {
        let menu = OpenPopup {
            surface: 2,
            parent: 1,
            anchor: 5,
            menu: true,
        };
        assert!(menu.dismissed_by(Dismiss::Escape));
        assert!(
            menu.dismissed_by(Dismiss::Press(1)),
            "a press on the parent"
        );
        assert!(
            menu.dismissed_by(Dismiss::Press(3)),
            "a press on another surface"
        );
        assert!(
            !menu.dismissed_by(Dismiss::Press(2)),
            "a press inside the menu"
        );
        let tooltip = OpenPopup {
            menu: false,
            ..menu
        };
        assert!(!tooltip.dismissed_by(Dismiss::Escape));
        assert!(!tooltip.dismissed_by(Dismiss::Press(1)));
    }

    #[test]
    fn escape_closes_the_menu_and_the_focus_returns_to_its_button() {
        let mut parent = Ui::new("Dock", Theme::default(), false);
        let other = parent.add(Ui::ROOT, Kind::Button("Terminal".into()));
        let app = parent.add(Ui::ROOT, Kind::Button("Files".into()));
        parent.set_has_popup(app, true);
        parent.set_focus(Some(other));
        let mut menu = Ui::new("Files", Theme::default(), false);
        let list = menu.add(Ui::ROOT, Kind::Menu);
        let first = menu.add(list, Kind::MenuItem("New Window".into()));
        menu.add(list, Kind::MenuItem("Quit".into()));

        let open = OpenPopup::opened(1, app, 2, &mut parent, &mut menu, true);
        assert!(parent.is_expanded(app));
        assert_eq!(
            menu.focus(),
            Some(first),
            "the first item takes the focus, as GTK's"
        );
        assert!(open.dismissed_by(Dismiss::Escape));
        open.closed(&mut parent);
        assert_eq!(
            (parent.focus(), parent.is_expanded(app)),
            (Some(app), false)
        );

        // A tooltip changes neither the focus nor the button's state.
        parent.set_focus(Some(other));
        let tip = OpenPopup::opened(1, app, 3, &mut parent, &mut menu, false);
        assert!(!parent.is_expanded(app));
        tip.closed(&mut parent);
        assert_eq!(parent.focus(), Some(other));
    }

    #[test]
    fn the_open_menu_is_the_focused_window_while_the_application_has_the_keys() {
        let menu = OpenPopup {
            surface: 2,
            parent: 1,
            anchor: 5,
            menu: true,
        };
        let tooltip = OpenPopup {
            menu: false,
            ..menu
        };
        assert_eq!(focused_window(None, Some(1)), Some(1));
        assert_eq!(
            focused_window(Some(menu), Some(1)),
            Some(2),
            "the compositor kept the keyboard on the parent"
        );
        assert_eq!(focused_window(Some(menu), Some(2)), Some(2));
        assert_eq!(
            focused_window(Some(menu), Some(3)),
            Some(2),
            "another surface"
        );
        assert_eq!(focused_window(Some(menu), None), None, "no keyboard");
        assert_eq!(
            focused_window(Some(tooltip), Some(1)),
            Some(1),
            "a tooltip takes no keys"
        );
        // Once the menu closes, the window focus returns to the surface with the keyboard.
        assert_eq!(focused_window::<u32>(None, Some(1)), Some(1));
    }

    #[test]
    fn a_menu_grabs_only_with_the_input_under_way() {
        let mut grab = GrabSerial::default();
        assert_eq!(grab.serial(), None);
        // A plain button pressed and released: a menu opened at the release grabs with the
        // press, which grabs no more once the button is up.
        grab.pressed(7);
        assert_eq!(grab.serial(), Some(7));
        grab.released();
        assert_eq!(
            grab.serial(),
            None,
            "a menu an assistive technology's Click opens grabs nothing"
        );
        // A key grabs while it is handled, before a press held under it.
        grab.pressed(8);
        grab.key(Some(9));
        assert_eq!(grab.serial(), Some(9));
        grab.key(None);
        assert_eq!(grab.serial(), Some(8), "the button is still down");
        // The pointer left with the button down: the release goes elsewhere.
        grab.released();
        assert_eq!(grab.serial(), None);
    }
}
