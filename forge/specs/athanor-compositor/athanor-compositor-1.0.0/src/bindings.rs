//! The compositor's default key bindings (doc_own_stack.md OS13): one compiled-in table, the
//! rule that matches a key against it, as cosmic-comp matches, and the rule of what acts while
//! the session is locked (doc_lock_and_prompts.md section 1, L14). A bound key that acts is kept
//! from every client, the input method's grab included, press and release (`State::kept_keys`);
//! every other key, decision 28's six included, is forwarded. The compositor reads no
//! configuration for its defaults: a user's own bindings are a later change (OS13).

use crate::session_calls::{
    Call, Command, Job, Request, CONTROL_CENTER, LOG_OUT, MIC_MUTE, MUTE, NEXT, PLAY_PAUSE,
    POWER_OFF, PREVIOUS,
};
use crate::state::State;
use smithay::desktop::Window;
use smithay::input::keyboard::{Keysym, KeysymHandle, ModifiersState};
use std::sync::atomic::Ordering;

/// The modifiers of a trigger, as a mask of these bits; Caps Lock and Num Lock are not read.
const SUPER: u8 = 1;
const CTRL: u8 = 2;
const ALT: u8 = 4;
const SHIFT: u8 = 8;

/// A key with the exact set of Super, Ctrl, Alt and Shift held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Trigger {
    mods: u8,
    /// The key's sym at its first shift level, or its Latin sym under a non-Latin layout.
    sym: Keysym,
}

/// What a bound key does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Action {
    /// Super+Escape: the keyboard leaves an exclusive surface and the session is locked.
    Lock,
    /// Ctrl+Alt+F1 to F12: to that virtual terminal.
    Vt(i32),
    /// Super+Alt+Escape: the compositor ends, and the session with it (LP10).
    Terminate,
    /// Super+Alt+T, the reveal key of OS4: kept from every client, locked or not, with no effect
    /// until the border and its overlay (gap 2); while locked it acts on nothing.
    Reveal,
    /// Super+Q and Alt+F4: the window with the keyboard focus is sent `close` (D15).
    CloseFocused,
    /// Super+Space: the seat's next layout group, the compositor's own (D14).
    NextGroup,
    /// A method of the session bus, which the session-bus worker calls (D5, D7).
    Call(&'static Call),
    /// A command, which the user manager runs as a transient unit at the worker's request (D6).
    Run(&'static Command),
    /// The touchpad key: the touchpads stop or start sending events, for the session (D16).
    TouchpadToggle,
}

impl Action {
    /// Whether the action is taken while the session is locked: doc_lock_and_prompts.md
    /// section 1's list.
    pub(crate) fn while_locked(&self) -> bool {
        match self {
            Self::Vt(_) | Self::Terminate | Self::NextGroup => true,
            Self::Call(call) => call.while_locked,
            Self::Run(command) => command.while_locked,
            Self::Lock | Self::Reveal | Self::CloseFocused | Self::TouchpadToggle => false,
        }
    }

    /// Whether the key is kept from the lock surface while the session is locked: the actions
    /// taken then, and the reveal key, which no client is ever sent (D11). Any other bound key
    /// goes to the lock surface as any key does.
    fn kept_while_locked(&self) -> bool {
        self.while_locked() || *self == Self::Reveal
    }

    /// Whether the action stays the compositor's under a client's keyboard-shortcuts inhibitor:
    /// the user's way out and the trusted path (OS4). No inhibitor is served yet; the protocol's
    /// change reads this set.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn never_inhibited(&self) -> bool {
        matches!(
            self,
            Self::Lock | Self::Vt(_) | Self::Terminate | Self::Reveal
        )
    }
}

/// The default bindings. The terminal switches are not rows: they are read from the keymap's
/// modified sym (`terminal_switch`).
pub(crate) const DEFAULTS: &[(Trigger, Action)] = &[
    (on(SUPER, Keysym::Escape), Action::Lock),
    (on(SUPER | ALT, Keysym::Escape), Action::Terminate),
    (on(SUPER | ALT, Keysym::t), Action::Reveal),
    (on(SUPER, Keysym::q), Action::CloseFocused),
    (on(ALT, Keysym::F4), Action::CloseFocused),
    (on(SUPER, Keysym::space), Action::NextGroup),
    (on(SUPER, Keysym::c), Action::Call(&CONTROL_CENTER)),
    (on(0, Keysym::XF86_AudioMute), Action::Run(&MUTE)),
    (on(0, Keysym::XF86_AudioMicMute), Action::Run(&MIC_MUTE)),
    (on(0, Keysym::XF86_AudioPlay), Action::Run(&PLAY_PAUSE)),
    (on(0, Keysym::XF86_AudioNext), Action::Run(&NEXT)),
    (on(0, Keysym::XF86_AudioPrev), Action::Run(&PREVIOUS)),
    (on(SUPER | SHIFT, Keysym::Escape), Action::Run(&LOG_OUT)),
    (on(0, Keysym::XF86_PowerOff), Action::Run(&POWER_OFF)),
    (on(0, Keysym::XF86_TouchpadToggle), Action::TouchpadToggle),
    (
        on(SUPER | CTRL, Keysym::XF86_TouchpadToggle),
        Action::TouchpadToggle,
    ),
];

const fn on(mods: u8, sym: Keysym) -> Trigger {
    Trigger { mods, sym }
}

/// The modifiers of `modifiers` a trigger names.
fn mask(modifiers: &ModifiersState) -> u8 {
    [
        (modifiers.logo, SUPER),
        (modifiers.ctrl, CTRL),
        (modifiers.alt, ALT),
        (modifiers.shift, SHIFT),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |mask, (_, bit)| mask | bit)
}

/// Whether the press of `handle` with `modifiers` arms Super alone (D4): Super_L or Super_R,
/// with no modifier but Super held.
pub(crate) fn arms_super(modifiers: &ModifiersState, handle: &KeysymHandle<'_>) -> bool {
    mask(modifiers) & !SUPER == 0
        && handle
            .raw_syms()
            .iter()
            .any(|sym| [Keysym::Super_L, Keysym::Super_R].contains(sym))
}

/// The action of the key `handle` pressed with `modifiers`, if the compositor keeps it: none
/// while `locked` but those `Action::kept_while_locked` names.
pub(crate) fn find(
    modifiers: &ModifiersState,
    handle: &KeysymHandle<'_>,
    locked: bool,
) -> Option<Action> {
    matching(
        mask(modifiers),
        handle.modified_sym(),
        &handle.raw_syms(),
        handle.raw_latin_sym_or_raw_current_sym(),
        locked,
    )
}

/// `find` on the key's syms: `modified` under the keymap's state, `raw` at its first shift
/// level, `latin` its Latin sym or its first raw one. A trigger matches a raw sym or the Latin
/// one with exactly its modifiers, as cosmic-comp matches, so that a shifted key is read on its
/// unshifted sym and a binding works under a Cyrillic layout.
fn matching(
    mods: u8,
    modified: Keysym,
    raw: &[Keysym],
    latin: Option<Keysym>,
    locked: bool,
) -> Option<Action> {
    let action = terminal_switch(mods, modified).or_else(|| {
        DEFAULTS
            .iter()
            .find(|(trigger, _)| {
                trigger.mods == mods && (raw.contains(&trigger.sym) || latin == Some(trigger.sym))
            })
            .map(|(_, action)| *action)
    })?;
    (!locked || action.kept_while_locked()).then_some(action)
}

/// Ctrl+Alt+F1 to F12, which the keymap turns into XF86Switch_VT_1 to 12; a switch the keymap
/// sends without Ctrl and Alt held switches nothing.
fn terminal_switch(mods: u8, modified: Keysym) -> Option<Action> {
    if mods & (CTRL | ALT) != CTRL | ALT {
        return None;
    }
    let vt = modified.raw().checked_sub(Keysym::XF86_Switch_VT_1.raw())?;
    (vt < 12).then(|| Action::Vt(vt as i32 + 1))
}

impl State {
    /// Takes `action`, of a bound key the compositor kept; while the session is locked, only an
    /// action `Action::while_locked` names.
    pub(crate) fn fire(&mut self, action: Action) {
        if self.lock.is_some() && !action.while_locked() {
            return;
        }
        #[cfg(test)]
        self.fired.push(action);
        match action {
            Action::Lock => self.request_lock(),
            Action::Vt(vt) => self.switch_vt(vt),
            Action::Terminate => {
                tracing::info!("Super+Alt+Escape: the compositor ends");
                self.loop_signal.stop();
            }
            Action::Reveal => {}
            Action::CloseFocused => self.close_focused(),
            Action::NextGroup => self.next_keyboard_group(),
            Action::Call(call) => self.send_job(Job::Call(call)),
            Action::Run(command) => self.send_job(Job::Run(command)),
            Action::TouchpadToggle => self.toggle_touchpads(),
        }
    }

    /// Hands `job` to the session-bus worker, stamped with the lock epoch; with no worker,
    /// headless, it is only logged.
    fn send_job(&self, job: Job) {
        let Some(calls) = &self.session_calls else {
            tracing::debug!(?job, "no session-bus worker; the key's job is not done");
            return;
        };
        calls.send(Request {
            job,
            epoch: self.lock_epoch.load(Ordering::SeqCst),
        });
    }

    /// Super+Q and Alt+F4 (D15): the window with the keyboard focus is sent `close`, as
    /// athanor_toplevel_actions_v1's close sends it; a layer surface or a popup with the focus,
    /// or no focus, closes nothing.
    fn close_focused(&mut self) {
        let focus = self
            .seat
            .get_keyboard()
            .and_then(|keyboard| keyboard.current_focus());
        let window = focus.and_then(|surface| self.window_for(&surface));
        if let Some(toplevel) = window.as_ref().and_then(Window::toplevel) {
            toplevel.send_close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `matching` for a key whose raw and modified sym is `sym`, under a Latin layout.
    fn plain(mods: u8, sym: Keysym, locked: bool) -> Option<Action> {
        matching(mods, sym, &[sym], Some(sym), locked)
    }

    #[test]
    fn every_trigger_is_unique() {
        for (at, (trigger, _)) in DEFAULTS.iter().enumerate() {
            assert!(
                !DEFAULTS[..at].iter().any(|(other, _)| other == trigger),
                "{trigger:?} is bound twice"
            );
        }
    }

    #[test]
    fn the_lock_list_is_doc_lock_section_1() {
        let all = [
            Action::Lock,
            Action::Vt(1),
            Action::Terminate,
            Action::Reveal,
            Action::CloseFocused,
            Action::NextGroup,
            Action::Call(&CONTROL_CENTER),
            Action::Run(&MUTE),
            Action::Run(&LOG_OUT),
            Action::TouchpadToggle,
        ];
        let acting: Vec<Action> = all.into_iter().filter(Action::while_locked).collect();
        assert_eq!(
            acting,
            [
                Action::Vt(1),
                Action::Terminate,
                Action::NextGroup,
                Action::Run(&MUTE)
            ]
        );
        let kept: Vec<Action> = all.into_iter().filter(Action::kept_while_locked).collect();
        assert_eq!(
            kept,
            [
                Action::Vt(1),
                Action::Terminate,
                Action::Reveal,
                Action::NextGroup,
                Action::Run(&MUTE)
            ],
            "the reveal key is kept from the lock surface, and acts on nothing"
        );
    }

    #[test]
    fn the_never_inhibited_set() {
        for action in [
            Action::Lock,
            Action::Vt(3),
            Action::Terminate,
            Action::Reveal,
        ] {
            assert!(action.never_inhibited(), "{action:?}");
        }
        assert!(!Action::CloseFocused.never_inhibited());
        assert!(!Action::NextGroup.never_inhibited());
        assert!(!Action::Call(&CONTROL_CENTER).never_inhibited());
        assert!(!Action::Run(&LOG_OUT).never_inhibited());
        assert!(!Action::TouchpadToggle.never_inhibited());
    }

    #[test]
    fn the_compositor_keeps_its_escape_and_the_terminal_switches() {
        assert_eq!(plain(SUPER, Keysym::Escape, false), Some(Action::Lock));
        assert_eq!(
            plain(SUPER, Keysym::Escape, true),
            None,
            "locked, it is the lock surface's"
        );
        assert_eq!(plain(0, Keysym::Escape, false), None);
        assert_eq!(
            plain(SUPER | ALT, Keysym::Escape, false),
            Some(Action::Terminate)
        );
        assert_eq!(
            plain(SUPER | ALT, Keysym::Escape, true),
            Some(Action::Terminate),
            "the only exit from a lock that cannot start (LP10)"
        );
        assert_eq!(
            plain(SUPER | SHIFT, Keysym::Escape, false),
            Some(Action::Run(&LOG_OUT))
        );
        for (mods, what) in [
            (SUPER | CTRL, "Super+Ctrl+Escape"),
            (CTRL | ALT, "Ctrl+Alt+Escape"),
        ] {
            assert_eq!(
                plain(mods, Keysym::Escape, false),
                None,
                "{what} is the client's"
            );
        }
        assert_eq!(
            plain(CTRL | ALT, Keysym::XF86_Switch_VT_1, false),
            Some(Action::Vt(1))
        );
        assert_eq!(
            plain(CTRL | ALT, Keysym::XF86_Switch_VT_12, true),
            Some(Action::Vt(12))
        );
        assert_eq!(
            plain(0, Keysym::XF86_Switch_VT_1, false),
            None,
            "a switch the keymap sends without Ctrl+Alt switches nothing"
        );
        assert_eq!(
            plain(
                CTRL | ALT,
                Keysym::from(Keysym::XF86_Switch_VT_12.raw() + 1),
                false
            ),
            None
        );
        assert_eq!(plain(CTRL | ALT, Keysym::a, false), None);
    }

    #[test]
    fn super_q_and_alt_f4_close_and_ctrl_alt_f4_switches() {
        assert_eq!(plain(SUPER, Keysym::q, false), Some(Action::CloseFocused));
        assert_eq!(plain(ALT, Keysym::F4, false), Some(Action::CloseFocused));
        assert_eq!(
            plain(ALT, Keysym::F4, true),
            None,
            "locked, the lock surface's"
        );
        assert_eq!(
            matching(
                CTRL | ALT,
                Keysym::XF86_Switch_VT_4,
                &[Keysym::F4],
                Some(Keysym::F4),
                false
            ),
            Some(Action::Vt(4))
        );
    }

    #[test]
    fn keys_match_without_shift_and_on_a_latin_sym() {
        // Caps Lock on: the modified sym is T, the raw one t, and Caps Lock is not a modifier of
        // a trigger.
        let caps = ModifiersState {
            logo: true,
            alt: true,
            caps_lock: true,
            ..ModifiersState::default()
        };
        assert_eq!(mask(&caps), SUPER | ALT);
        assert_eq!(
            matching(SUPER | ALT, Keysym::T, &[Keysym::t], Some(Keysym::t), false),
            Some(Action::Reveal)
        );
        // Under a Russian layout the key of T is Cyrillic_ie; its Latin sym is t.
        assert_eq!(
            matching(
                SUPER | ALT,
                Keysym::Cyrillic_ie,
                &[Keysym::Cyrillic_ie],
                Some(Keysym::t),
                false
            ),
            Some(Action::Reveal)
        );
        assert_eq!(
            plain(SUPER | ALT | SHIFT, Keysym::t, false),
            None,
            "the set of modifiers is exact"
        );
        assert_eq!(plain(SUPER, Keysym::t, false), None);
    }
}
