//! A shortcut capture (doc_settings.md SL5): a raised button that, activated, records the next
//! key combination, as Settings' shortcuts page assigns one (SE15). While it records, every key
//! goes to it before any other widget or the application sees it, the function keys and Ctrl
//! with any letter included: the modifiers Shift, Ctrl, Alt and Super, left or right, add up
//! until another key ends the combination, or until one of them is released with no other key
//! in between, which records it alone. Bare Escape, or the focus leaving it, cancels and keeps
//! the combination it had. It reads as a button, named by its label, and announces its prompt
//! when it starts recording, which it shows in place of the label until it stops.

use crate::driver::KeyModifiers;
use crate::widget::{Event, Kind, Priority, Ui, WidgetId};
use smithay_client_toolkit::seat::keyboard::Keysym;
use xkbcommon::xkb;

/// A key combination: the modifiers held and the key, by its xkb keysym name, as the
/// compositor's shortcuts name it; no key for a modifier recorded alone, as Super.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    /// The modifiers held.
    pub modifiers: KeyModifiers,
    /// The xkb name of the key, by its first level in the effective layout, and in lower case
    /// for a letter: Shift+A is `a` with Shift, Shift+1 is `1` and Shift+Tab is `Tab`. `None`
    /// for a modifier alone.
    pub key: Option<String>,
}

/// A shortcut capture: see [`Ui::shortcut_capture`].
pub struct ShortcutCapture {
    /// Its name for assistive technologies, drawn while it does not record.
    pub label: String,
    /// What it announces when it starts recording, drawn while it records.
    pub prompt: String,
    /// The combination it holds, if any: the last one recorded.
    pub value: Option<Shortcut>,
    /// While it records, the modifiers pressed since it started.
    pub(crate) recording: Option<KeyModifiers>,
}

/// The modifier a key is, of the four a [`KeyModifiers`] holds; any other key, AltGr
/// (`ISO_Level3_Shift`) and the locks included, is an ordinary key of a combination.
fn modifier(keysym: Keysym) -> Option<KeyModifiers> {
    let mut held = KeyModifiers::default();
    match keysym {
        Keysym::Shift_L | Keysym::Shift_R => held.shift = true,
        Keysym::Control_L | Keysym::Control_R => held.ctrl = true,
        Keysym::Alt_L | Keysym::Alt_R => held.alt = true,
        Keysym::Super_L | Keysym::Super_R => held.logo = true,
        _ => return None,
    }
    Some(held)
}

fn union(a: KeyModifiers, b: KeyModifiers) -> KeyModifiers {
    let mut held = a;
    held.ctrl |= b.ctrl;
    held.shift |= b.shift;
    held.alt |= b.alt;
    held.logo |= b.logo;
    held
}

/// The xkb name of a key, an upper-case letter's lower-case one, or `None` for a keysym
/// without a name.
fn key_name(keysym: Keysym) -> Option<String> {
    let lower = keysym
        .key_char()
        .filter(|c| c.is_uppercase())
        .and_then(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => Some(Keysym::from_char(l)),
                _ => None,
            }
        })
        .unwrap_or(keysym);
    let name = xkb::keysym_get_name(lower);
    (lower != Keysym::NoSymbol && !name.is_empty()).then_some(name)
}

impl Ui {
    /// Adds a shortcut capture named `label`, holding `value`. Activating it, by Enter, Space,
    /// a click or an assistive technology, starts recording and announces `prompt`, which it
    /// shows until it stops; the first whole combination ends it, sets the value and raises
    /// [`Event::Recorded`]. See [`Shortcut`] for what it records.
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn shortcut_capture(
        &mut self,
        parent: WidgetId,
        label: &str,
        prompt: &str,
        value: Option<Shortcut>,
    ) -> WidgetId {
        let capture = ShortcutCapture {
            label: label.to_owned(),
            prompt: prompt.to_owned(),
            value,
            recording: None,
        };
        self.add(parent, Kind::Shortcut(capture))
    }

    /// The combination a shortcut capture holds, if `id` is one and holds one.
    pub fn shortcut(&self, id: WidgetId) -> Option<&Shortcut> {
        match &self.widgets.get(id)?.kind {
            Kind::Shortcut(capture) => capture.value.as_ref(),
            _ => None,
        }
    }

    /// Whether `id` is a shortcut capture that records.
    pub fn is_recording(&self, id: WidgetId) -> bool {
        matches!(self.widgets.get(id).map(|w| &w.kind), Some(Kind::Shortcut(c)) if c.recording.is_some())
    }

    /// Starts `id`, a shortcut capture, recording, and announces its prompt; one that records
    /// already goes on.
    pub(crate) fn start_recording(&mut self, id: WidgetId) {
        let Some(Kind::Shortcut(capture)) = self.widgets.get_mut(id).map(|w| &mut w.kind) else {
            return;
        };
        if capture.recording.is_some() {
            return;
        }
        capture.recording = Some(KeyModifiers::default());
        let prompt = capture.prompt.clone();
        self.announce(&prompt, Priority::Assertive);
    }

    /// Stops the focused shortcut capture recording, keeping the combination it had. Whether
    /// one recorded.
    pub(crate) fn stop_recording(&mut self) -> bool {
        let Some(Kind::Shortcut(capture)) = self
            .focus
            .and_then(|id| self.widgets.get_mut(id))
            .map(|w| &mut w.kind)
        else {
            return false;
        };
        capture.recording.take().is_some()
    }

    /// The keyboard left the surface: the capture that records stops, and a key held since it
    /// ended a combination no longer repeats into it. Whether anything changed.
    pub(crate) fn keyboard_left(&mut self) -> bool {
        self.held_back = None;
        self.stop_recording()
    }

    /// A key pressed, with the modifiers the compositor says are held, while a shortcut
    /// capture may record: `None` when none does, and the key goes on to the application and
    /// the widgets; otherwise the capture took it, with the event it raised, if any.
    pub(crate) fn record_press(
        &mut self,
        raw: u32,
        keysym: Keysym,
        modifiers: KeyModifiers,
    ) -> Option<Option<Event>> {
        let id = self.focus.filter(|&id| self.is_recording(id))?;
        let Some(Kind::Shortcut(capture)) = self.widgets.get_mut(id).map(|w| &mut w.kind) else {
            return None;
        };
        let held = capture.recording.unwrap_or_default();
        if let Some(pressed) = modifier(keysym) {
            capture.recording = Some(union(held, pressed));
            return Some(None);
        }
        let modifiers = union(held, modifiers);
        if keysym == Keysym::Escape && modifiers == KeyModifiers::default() {
            capture.recording = None;
            return Some(None);
        }
        let Some(key) = key_name(keysym) else {
            return Some(None);
        };
        capture.value = Some(Shortcut {
            modifiers,
            key: Some(key),
        });
        capture.recording = None;
        self.held_back = Some(raw);
        Some(Some(Event::Recorded(id)))
    }

    /// A key released: a modifier pressed while the capture records, with no other key since,
    /// is recorded alone. As [`Ui::record_press`], `None` when no capture records.
    pub(crate) fn record_release(&mut self, raw: u32, keysym: Keysym) -> Option<Option<Event>> {
        if self.held_back == Some(raw) {
            self.held_back = None;
        }
        let id = self.focus.filter(|&id| self.is_recording(id))?;
        let Some(Kind::Shortcut(capture)) = self.widgets.get_mut(id).map(|w| &mut w.kind) else {
            return None;
        };
        let held = capture.recording.unwrap_or_default();
        let Some(released) = modifier(keysym).filter(|&m| union(held, m) == held) else {
            return Some(None);
        };
        capture.value = Some(Shortcut {
            modifiers: union(held, released),
            key: None,
        });
        capture.recording = None;
        Some(Some(Event::Recorded(id)))
    }

    /// Whether a key held down is not to repeat: while a capture records, and the key that
    /// ended its combination, until it is released.
    pub(crate) fn record_repeat(&self, raw: u32) -> bool {
        self.held_back == Some(raw) || self.focus.is_some_and(|id| self.is_recording(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crate::widget::Key;

    fn held(f: impl FnOnce(&mut KeyModifiers)) -> KeyModifiers {
        let mut m = KeyModifiers::default();
        f(&mut m);
        m
    }

    fn ctrl() -> KeyModifiers {
        held(|m| m.ctrl = true)
    }

    fn combination(modifiers: KeyModifiers, key: Option<&str>) -> Option<Shortcut> {
        Some(Shortcut {
            modifiers,
            key: key.map(str::to_owned),
        })
    }

    /// A window with a button, then a capture holding Super+T, then another button, the
    /// capture focused and recording.
    fn recording() -> (Ui, WidgetId) {
        let mut ui = Ui::new("Shortcuts", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Button("Before".into()));
        let old = combination(held(|m| m.logo = true), Some("t"));
        let capture = ui.shortcut_capture(Ui::ROOT, "Terminal", "Press the new shortcut", old);
        ui.add(Ui::ROOT, Kind::Button("After".into()));
        ui.layout();
        assert_eq!(ui.activate(capture), None, "activating starts recording");
        assert!(ui.is_recording(capture));
        (ui, capture)
    }

    #[test]
    fn ctrl_with_a_letter_and_the_function_keys_are_recorded_and_taken() {
        let (mut ui, capture) = recording();
        let none = KeyModifiers::default();
        assert_eq!(ui.record_press(37, Keysym::Control_L, none), Some(None));
        let taken = ui.record_press(24, Keysym::q, ctrl());
        assert_eq!(
            taken,
            Some(Some(Event::Recorded(capture))),
            "Ctrl+Q is the capture's"
        );
        assert_eq!(
            ui.shortcut(capture),
            combination(ctrl(), Some("q")).as_ref()
        );
        assert!(!ui.is_recording(capture));

        ui.activate(capture);
        let taken = ui.record_press(71, Keysym::F5, none);
        assert_eq!(
            taken,
            Some(Some(Event::Recorded(capture))),
            "F5 is the capture's"
        );
        assert_eq!(ui.shortcut(capture), combination(none, Some("F5")).as_ref());
    }

    #[test]
    fn bare_escape_cancels_and_keeps_the_previous_combination() {
        let (mut ui, capture) = recording();
        let none = KeyModifiers::default();
        let before = ui.shortcut(capture).cloned();
        assert_eq!(ui.record_press(9, Keysym::Escape, none), Some(None));
        assert!(!ui.is_recording(capture));
        assert_eq!(ui.shortcut(capture).cloned(), before);
        // Not recording, Escape goes on to the widgets, and the surface's escape.
        assert_eq!(ui.record_press(9, Keysym::Escape, none), None);
        assert_eq!(ui.key(Key::Escape), Some(Event::Escape));
    }

    #[test]
    fn escape_with_a_modifier_is_recorded() {
        let (mut ui, capture) = recording();
        let alt = held(|m| m.alt = true);
        ui.record_press(64, Keysym::Alt_L, KeyModifiers::default());
        let taken = ui.record_press(9, Keysym::Escape, alt);
        assert_eq!(taken, Some(Some(Event::Recorded(capture))));
        assert_eq!(
            ui.shortcut(capture),
            combination(alt, Some("Escape")).as_ref()
        );
    }

    #[test]
    fn it_announces_its_prompt_and_shows_it_while_it_records() {
        let mut ui = Ui::new("Shortcuts", Theme::default(), false);
        let capture = ui.shortcut_capture(Ui::ROOT, "Terminal", "Press the new shortcut", None);
        ui.layout();
        // The width of the text it lays out: the prompt is wider than the label.
        let width = |ui: &mut Ui| {
            ui.layout();
            ui.widget(capture).text.as_ref().map_or(0.0, |l| l.width())
        };
        assert_eq!(ui.announcement(), None);
        let label = width(&mut ui);
        ui.activate(capture);
        let prompt = ("Press the new shortcut", Priority::Assertive);
        assert_eq!(ui.announcement(), Some(prompt));
        assert!(width(&mut ui) > label, "it shows its prompt");
        ui.record_press(9, Keysym::Escape, KeyModifiers::default());
        assert_eq!(width(&mut ui), label, "and its label again");
    }

    #[test]
    fn once_it_stops_the_keys_go_to_the_widgets_again() {
        let (mut ui, capture) = recording();
        let none = KeyModifiers::default();
        let taken = ui.record_press(23, Keysym::Tab, none);
        assert_eq!(taken, Some(Some(Event::Recorded(capture))));
        assert_eq!(
            ui.shortcut(capture),
            combination(none, Some("Tab")).as_ref()
        );
        assert_eq!(ui.focus(), Some(capture), "the recorded Tab moved no focus");
        // Held, the Tab that ended the combination does not repeat into the widgets.
        assert!(ui.record_repeat(23));
        assert_eq!(ui.record_release(23, Keysym::Tab), None);
        assert!(!ui.record_repeat(23));
        assert_eq!(ui.record_press(23, Keysym::Tab, none), None);
        ui.key(Key::Tab);
        assert_ne!(ui.focus(), Some(capture), "Tab moves the focus again");
    }

    #[test]
    fn a_modifier_released_alone_is_recorded_alone() {
        let (mut ui, capture) = recording();
        let none = KeyModifiers::default();
        assert_eq!(ui.record_press(133, Keysym::Super_L, none), Some(None));
        let taken = ui.record_release(133, Keysym::Super_L);
        assert_eq!(taken, Some(Some(Event::Recorded(capture))));
        let logo = held(|m| m.logo = true);
        assert_eq!(ui.shortcut(capture), combination(logo, None).as_ref());
        // A modifier held before it started, released while it records, records nothing.
        ui.activate(capture);
        assert_eq!(ui.record_release(50, Keysym::Shift_L), Some(None));
        assert!(ui.is_recording(capture));
        // Another key in between makes a combination, which the release does not end again.
        ui.record_press(105, Keysym::Control_R, none);
        ui.record_press(71, Keysym::F5, ctrl());
        assert_eq!(
            ui.shortcut(capture),
            combination(ctrl(), Some("F5")).as_ref()
        );
        assert_eq!(ui.record_release(105, Keysym::Control_R), None);
    }

    #[test]
    fn shift_with_a_letter_records_the_letter_with_shift() {
        let (mut ui, capture) = recording();
        let shift = held(|m| m.shift = true);
        ui.record_press(50, Keysym::Shift_L, KeyModifiers::default());
        ui.record_press(38, Keysym::A, shift);
        assert_eq!(ui.shortcut(capture), combination(shift, Some("a")).as_ref());
    }

    #[test]
    fn altgr_backspace_and_the_keypad_keep_their_names() {
        let names = [
            (Keysym::ISO_Level3_Shift, "ISO_Level3_Shift"),
            (Keysym::BackSpace, "BackSpace"),
            (Keysym::Return, "Return"),
            (Keysym::Tab, "Tab"),
            (Keysym::KP_1, "KP_1"),
            (Keysym::Insert, "Insert"),
            (Keysym::F12, "F12"),
        ];
        let none = KeyModifiers::default();
        for (keysym, name) in names {
            let (mut ui, capture) = recording();
            let taken = ui.record_press(1, keysym, none);
            assert_eq!(taken, Some(Some(Event::Recorded(capture))), "{name}");
            assert_eq!(ui.shortcut(capture), combination(none, Some(name)).as_ref());
        }
    }

    #[test]
    fn losing_the_focus_cancels_and_keeps_the_previous_combination() {
        let (mut ui, capture) = recording();
        let before = ui.shortcut(capture).cloned();
        ui.record_press(37, Keysym::Control_L, KeyModifiers::default());
        let after = ui
            .ids()
            .find(|&id| matches!(&ui.widget(id).kind, Kind::Button(t) if t == "After"));
        ui.set_focus(after);
        assert!(!ui.is_recording(capture));
        assert_eq!(ui.shortcut(capture).cloned(), before);
        // The window losing the keyboard cancels it too.
        ui.activate(capture);
        assert!(ui.keyboard_left());
        assert!(!ui.is_recording(capture));
        assert_eq!(ui.shortcut(capture).cloned(), before);
    }

    /// The seat losing its keyboard, or the window losing the keyboard, stops the recording
    /// and lets the key that ended a combination repeat again.
    #[test]
    fn losing_the_keyboard_frees_the_key_held_back() {
        let (mut ui, capture) = recording();
        let taken = ui.record_press(28, Keysym::F5, KeyModifiers::default());
        assert_eq!(taken, Some(Some(Event::Recorded(capture))));
        assert!(
            ui.record_repeat(28),
            "the key that ended the combination does not repeat"
        );
        assert!(!ui.keyboard_left(), "nothing recorded any more");
        assert!(
            !ui.record_repeat(28),
            "nor is it held back after the keyboard is lost"
        );
        ui.activate(capture);
        assert!(ui.keyboard_left());
        assert!(!ui.is_recording(capture));
    }

    #[test]
    fn it_reads_as_a_button_named_by_its_label() {
        let (ui, capture) = recording();
        assert!(ui.widget(capture).is_focusable());
        let node = crate::a11y::node(&ui, capture, 1.0);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.label(), Some("Terminal"));
    }
}
