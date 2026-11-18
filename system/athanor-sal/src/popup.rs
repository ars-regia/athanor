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
    /// Records `popup`, opened from `anchor` of `parent`, and marks it a popup, which raises no
    /// [`Event::PressedOutside`](crate::Event::PressedOutside): a menu expands its button and
    /// puts the focus on its first item, as GTK's popover does when it shows, unless the
    /// application gave it to another, as GTK's default widget.
    pub fn opened(
        parent_surface: S,
        anchor: WidgetId,
        surface: S,
        parent: &mut Ui,
        popup: &mut Ui,
        menu: bool,
    ) -> Self {
        popup.popup = true;
        if menu {
            parent.set_expanded(anchor, true);
            if popup.focus().is_none() {
                popup.set_focus(focus::step(popup, false));
            }
        }
        Self {
            surface,
            parent: parent_surface,
            anchor,
            menu,
        }
    }

    /// The anchor in `parent`, while it is the widget the popup opened from: `None` once the
    /// application removed it, whatever took its slot since.
    pub fn anchor_in(&self, parent: &Ui) -> Option<WidgetId> {
        parent.contains(self.anchor).then_some(self.anchor)
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

    /// The popup closed: a menu returns the focus to its button, which collapses, unless the
    /// button was removed.
    pub fn closed(&self, parent: &mut Ui) {
        if let Some(anchor) = self.anchor_in(parent).filter(|_| self.menu) {
            parent.popup_closed(anchor);
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

/// The surface with the keyboard once the `popups` close, in their order, given the one that
/// had it: a popup that had it hands it to the nearest of its parent and the parent's ancestors
/// (`parent_of`, the surface a dialog was opened on) that `is_open`, else to none, as a dialog
/// closed with its drop-down hands it to its window. The compositor gives the keys back to the
/// parent when the popup's grab ends, which a Smithay compositor sees only at the next key: until
/// then the parent stays the focused window, and the menu's button, which takes the focus back,
/// keeps the screen reader's locus (AX7).
pub(crate) fn keyboard_after_closed<S: Copy + PartialEq>(
    popups: &[OpenPopup<S>],
    keyboard: Option<S>,
    is_open: impl Fn(S) -> bool,
    parent_of: impl Fn(S) -> Option<S>,
) -> Option<S> {
    popups.iter().fold(keyboard, |keyboard, popup| {
        if keyboard == Some(popup.surface) {
            std::iter::successors(Some(popup.parent), |&surface| parent_of(surface))
                .find(|&surface| is_open(surface))
        } else {
            keyboard
        }
    })
}

/// The surface with the keyboard after the compositor's `leave` of `left`, a surface still open,
/// or none for one already closed: none when `left` had it, else the one that had it. A popup's
/// `leave` that comes after its keyboard was handed back ([`keyboard_after_closed`]) changes
/// nothing.
pub(crate) fn keyboard_after_leave<S: PartialEq>(
    keyboard: Option<S>,
    left: Option<S>,
) -> Option<S> {
    if left.is_some() && left == keyboard {
        None
    } else {
        keyboard
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
    /// The serial of the key pressed last, which its repeats are handled with.
    pressed_key: Option<u32>,
    /// The serial of the last press of a button or a key, kept once it ends.
    last: Option<u32>,
}

impl GrabSerial {
    /// A button was pressed with `serial`.
    pub fn pressed(&mut self, serial: u32) {
        self.press = Some(serial);
        self.last = Some(serial);
    }

    /// A button was released, or the pointer left: no press is held.
    pub fn released(&mut self) {
        self.press = None;
    }

    /// The key pressed with `serial` is being handled; `None` once it has been.
    pub fn key(&mut self, serial: Option<u32>) {
        self.key = serial;
        self.pressed_key = serial.or(self.pressed_key);
        self.last = serial.or(self.last);
    }

    /// The key pressed last repeats: it is handled again with its press's serial, until
    /// `key(None)`.
    pub fn repeat(&mut self) {
        self.key = self.pressed_key;
    }

    /// The serial to grab with, while an input is under way.
    pub fn serial(&self) -> Option<u32> {
        self.key.or(self.press)
    }

    /// The serial of the last press of a button or a key, after it ended too: what an
    /// xdg-activation token names for an action the input started, as GTK names its seat's last
    /// serial. A key's repeats are no new press.
    pub fn last(&self) -> Option<u32> {
        self.last
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

    /// The widget whose tooltip shows, or waits to.
    pub fn target(&self) -> Option<&T> {
        self.target.as_ref()
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
    use crate::widget::{Event, Key, Kind, PointerButton, Ui};
    use std::time::{Duration, Instant};

    /// Every popup the driver opens goes through [`OpenPopup::opened`], which marks it: a press
    /// on the bare part of a menu or a tooltip then raises no press outside, which the
    /// application would take for one outside its panel.
    #[test]
    fn an_opened_popup_raises_no_press_outside() {
        for menu in [true, false] {
            let mut parent = Ui::new("t", Theme::default(), false);
            let anchor = parent.add(Ui::ROOT, Kind::Button("Player".into()));
            let mut popup = Ui::new("t", Theme::default(), false);
            popup.min_width = 200.0;
            popup.min_height = 200.0;
            let list = popup.add(Ui::ROOT, Kind::Menu);
            popup.add(list, Kind::Button("Spotify".into()));
            popup.layout();
            let press = |popup: &mut Ui| {
                let event = popup.pointer_press(199.0, 199.0, PointerButton::Primary);
                popup.pointer_release(199.0, 199.0, PointerButton::Primary);
                event
            };
            assert_eq!(press(&mut popup), Some(Event::PressedOutside), "not yet opened");
            let _open = OpenPopup::opened(0, anchor, 1, &mut parent, &mut popup, menu);
            assert_eq!(press(&mut popup), None, "menu {menu}");
        }
    }

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
            anchor: WidgetId(5, 0),
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

        // A focus the application gave stays: a confirmation's Cancel, not the item after it.
        let mut asked = Ui::new("Files", Theme::default(), false);
        let row = asked.add(Ui::ROOT, Kind::Row);
        let cancel = asked.add(row, Kind::Button("Cancel".into()));
        asked.add(row, Kind::Button("Quit".into()));
        asked.set_focus(Some(cancel));
        let open = OpenPopup::opened(1, app, 2, &mut parent, &mut asked, true);
        assert_eq!(asked.focus(), Some(cancel));
        open.closed(&mut parent);

        // A tooltip changes neither the focus nor the button's state.
        parent.set_focus(Some(other));
        let tip = OpenPopup::opened(1, app, 3, &mut parent, &mut menu, false);
        assert!(!parent.is_expanded(app));
        tip.closed(&mut parent);
        assert_eq!(parent.focus(), Some(other));
    }

    #[test]
    fn a_menu_whose_button_was_removed_gives_the_focus_to_no_widget_in_its_slot() {
        let mut parent = Ui::new("Dock", Theme::default(), false);
        let apps = parent.add(Ui::ROOT, Kind::Row);
        let app = parent.add(apps, Kind::Button("Files".into()));
        parent.set_has_popup(app, true);
        let mut menu = Ui::new("Files", Theme::default(), false);
        let list = menu.add(Ui::ROOT, Kind::Menu);
        menu.add(list, Kind::MenuItem("New Window".into()));
        let open = OpenPopup::opened(1, app, 2, &mut parent, &mut menu, true);
        assert_eq!(open.anchor_in(&parent), Some(app));
        // The application rebuilds the row while the menu is open.
        parent.remove_children(apps);
        let next = parent.add(apps, Kind::Button("Terminal".into()));
        assert_eq!(next.index(), app.index(), "the slot is reused");
        parent.set_focus(None);
        open.closed(&mut parent);
        assert_eq!(open.anchor_in(&parent), None);
        assert_eq!(
            (parent.focus(), parent.is_expanded(next)),
            (None, false),
            "the new button is left as it is"
        );
    }

    #[test]
    fn the_open_menu_is_the_focused_window_while_the_application_has_the_keys() {
        let menu = OpenPopup {
            surface: 2,
            parent: 1,
            anchor: WidgetId(5, 0),
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
    fn a_closed_popup_hands_the_keyboard_it_had_back_to_its_parent() {
        let menu = OpenPopup {
            surface: 2,
            parent: 1,
            anchor: WidgetId(5, 0),
            menu: true,
        };
        let after = |keyboard| keyboard_after_closed(&[menu], keyboard, |_| true, |_| None);
        assert_eq!(after(Some(2)), Some(1), "the popup's");
        assert_eq!(
            focused_window(None, after(Some(2))),
            Some(1),
            "the parent is the focused window"
        );
        assert_eq!(
            keyboard_after_closed(&[menu], Some(2), |id| id != 1, |_| None),
            None,
            "the parent closed with it"
        );
        assert_eq!(
            after(Some(1)),
            Some(1),
            "the compositor kept the keyboard on the parent"
        );
        assert_eq!(after(Some(3)), Some(3), "another surface");
        assert_eq!(after(None), None, "no keyboard");
    }

    #[test]
    fn popups_closed_together_hand_the_keyboard_to_the_nearest_open_ancestor() {
        let popup = |surface, parent| OpenPopup {
            surface,
            parent,
            anchor: WidgetId(5, 0),
            menu: true,
        };
        // Window 1, its dialog 2, the dialog's drop-down 3; menus 4 and 5 of the window.
        let parent_of = |id| (id == 2).then_some(1);
        let open = |id| id != 3 && id != 4 && id != 5;
        assert_eq!(
            keyboard_after_closed(&[popup(3, 2)], Some(3), |id| id == 1, parent_of),
            Some(1),
            "a drop-down closed with its dialog hands it to the dialog's window"
        );
        assert_eq!(
            keyboard_after_closed(&[popup(3, 2)], Some(3), open, parent_of),
            Some(2),
            "a drop-down closed alone hands it to its open dialog"
        );
        assert_eq!(
            keyboard_after_closed(&[popup(4, 1), popup(5, 1)], Some(4), open, parent_of),
            Some(1),
            "menu 4, replaced by menu 5, hands it to the window"
        );
        assert_eq!(
            keyboard_after_closed(&[popup(4, 1), popup(5, 1)], Some(5), open, parent_of),
            Some(1),
            "menu 5, closed after menu 4, hands it to the window"
        );
        assert_eq!(
            keyboard_after_closed(&[popup(3, 2)], Some(3), |_| false, parent_of),
            None,
            "no ancestor open"
        );
    }

    #[test]
    fn a_leave_takes_the_keyboard_only_from_the_surface_that_had_it() {
        assert_eq!(
            keyboard_after_leave(Some(1), Some(1)),
            None,
            "the focused one"
        );
        assert_eq!(
            keyboard_after_leave(Some(1), None),
            Some(1),
            "a closed popup's, after the hand-back"
        );
        assert_eq!(
            keyboard_after_leave(Some(1), Some(2)),
            Some(1),
            "another one"
        );
        assert_eq!(keyboard_after_leave::<u32>(None, None), None, "none had it");
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

    #[test]
    fn an_activation_token_names_the_last_press_after_it_ended() {
        let mut grab = GrabSerial::default();
        assert_eq!(grab.last(), None, "no input yet");
        // A click: the action runs at the release, once the press grabs no more.
        grab.pressed(7);
        grab.released();
        assert_eq!(grab.last(), Some(7));
        // A key handled, then its repeats: the press is the last input.
        grab.key(Some(9));
        grab.key(None);
        grab.repeat();
        grab.key(None);
        assert_eq!(grab.last(), Some(9));
        grab.pressed(11);
        assert_eq!(grab.last(), Some(11), "the newer press");
    }

    #[test]
    fn a_fields_menu_opens_on_its_first_item_that_applies_and_the_arrows_skip_the_others() {
        let mut parent = Ui::new("Text", Theme::default(), false);
        let name = parent.field(Ui::ROOT, "Name", "abc");
        parent.layout();
        // No selection: Paste and Select All apply.
        let (mut menu, items) = parent.field_menu(name, true).expect("a field has a menu");
        menu.layout();
        let item = |n: usize| Some(items[n].0);
        let open = OpenPopup::opened(1, name, 2, &mut parent, &mut menu, true);
        assert!(
            !parent.is_expanded(name),
            "a field reads as neither collapsed nor expanded"
        );
        assert_eq!(menu.focus(), item(2), "Paste");
        menu.key(Key::Down);
        assert_eq!(menu.focus(), item(4), "Select All, past Delete");
        menu.key(Key::Down);
        assert_eq!(menu.focus(), item(2), "round, past Cut and Copy");
        menu.key(Key::Up);
        assert_eq!(menu.focus(), item(4));
        assert_eq!(menu.key(Key::Enter), Some(Event::Clicked(items[4].0)));
        // Escape closes it, and the focus returns to the field.
        assert!(open.dismissed_by(Dismiss::Escape));
        parent.set_focus(None);
        open.closed(&mut parent);
        assert_eq!(parent.focus(), Some(name));

        // Nothing applies: no item takes the focus, and Escape still closes the menu.
        let mut empty = Ui::new("Text", Theme::default(), false);
        let name = empty.field(Ui::ROOT, "Name", "");
        let (mut menu, _) = empty.field_menu(name, false).expect("a field has a menu");
        let open = OpenPopup::opened(1, name, 2, &mut empty, &mut menu, true);
        assert_eq!(menu.focus(), None);
        assert_eq!(menu.key(Key::Enter), None);
        assert!(open.dismissed_by(Dismiss::Escape));
    }

    #[test]
    fn a_held_key_repeats_with_the_serial_of_its_press() {
        let mut grab = GrabSerial::default();
        grab.repeat();
        assert_eq!(grab.serial(), None, "no key was pressed");
        grab.key(Some(4));
        grab.key(None);
        assert_eq!(grab.serial(), None);
        // A repeat comes with no serial of its own: the press's names it while it is handled.
        grab.repeat();
        assert_eq!(grab.serial(), Some(4));
        grab.key(None);
        assert_eq!(grab.serial(), None);
    }
}
