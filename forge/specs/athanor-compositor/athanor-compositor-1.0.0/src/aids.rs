//! The keyboard aids of doc_accessibility.md AX11: sticky, slow and bounce keys, in the
//! compositor's input path before the bindings, the keyboard monitor and the clients (OS4,
//! decision 30). A key of the hardware keyboard or of a virtual one enters `State::aided`; what
//! the aids let through goes on to `State::typed`, so a binding, Orca and a client see the same
//! keys. Bounce keys drop a press within their delay of the same key's release; slow keys hold a
//! press back until the key has been held for their delay; sticky keys latch a modifier pressed
//! and released alone, lock it when it is pressed again, and let it go at the third press. The
//! protected keys of decision 30 pass slow and bounce keys unfiltered. The settings come from
//! `athanor_a11y_v1` (#864); the compositor keeps them for the session only, and every aid is
//! off until a client sets it.

use crate::bindings;
use crate::identity::Privilege;
use crate::input::keymap_text;
use crate::state::{granted, State};
use smithay::backend::input::{InputTime, KeyState};
use smithay::input::keyboard::{xkb, KeyboardHandle, KeyboardSource, Keycode, Keysym};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::RegistrationToken;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::virtual_keyboard::VirtualKeyboardDevice;
use std::collections::HashMap;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

/// The longest delay of slow and bounce keys, in milliseconds (AX11): a longer one is the
/// protocol's `invalid_value`.
pub const MAX_DELAY: u32 = 2000;
/// Shift pressed this many times in a row offers sticky keys (AX11).
const SHIFT_PRESSES: u8 = 5;
/// Shift held this long offers slow keys (AX11).
const SHIFT_HOLD: Duration = Duration::from_secs(8);

/// The modifiers of `athanor_a11y_v1`'s `modifiers` event, as its bitfield numbers them.
pub const SHIFT: u32 = 1;
pub const CONTROL: u32 = 2;
pub const ALT: u32 = 4;
pub const SUPER: u32 = 8;
pub const ALTGR: u32 = 16;

/// An aid, as `athanor_a11y_v1`'s `aid` enum names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aid {
    Sticky,
    Slow,
    Bounce,
}

/// What the aids tell the clients of `athanor_a11y_v1`, one event each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The modifiers sticky keys latched and locked.
    Modifiers { latched: u32, locked: u32 },
    /// A key the aid rejected, never which key.
    Rejected(Aid),
    /// A Shift gesture that offers the aid.
    Gesture(Aid),
    /// An aid the compositor turned off itself: sticky keys, by two-key-off.
    AidOff(Aid),
}

/// A delay of slow or bounce keys above `MAX_DELAY`, in milliseconds.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDelay(pub u32);

/// Where a key came from: a keyboard read under the keymap the seat has when it arrives, or a
/// virtual keyboard, read under its own (`State::with_keymap_of`).
#[derive(Clone, Debug)]
pub enum Origin {
    Seat(KeyboardSource),
    Virtual(VirtualKeyboardDevice),
}

impl Origin {
    fn source(&self) -> KeyboardSource {
        match self {
            Self::Seat(source) => *source,
            Self::Virtual(device) => device.source(),
        }
    }
}

/// A press the aids hold back, and its release with it. With no timer the press is dropped,
/// and the entry is what swallows its release.
#[derive(Debug)]
struct Withheld {
    origin: Origin,
    /// The modifier the key is, 0 for none.
    modifier: u32,
    /// Slow keys' timer, which lets the press through; none for a dropped press.
    slow: Option<RegistrationToken>,
    /// Whether bounce keys dropped it: its release restarts their delay, as a delivered
    /// press's does, and that of a press never delivered otherwise does not.
    bounced: bool,
}

/// A modifier key sticky keys saw pressed and not yet released.
#[derive(Debug)]
struct Held {
    key: (KeyboardSource, Keycode),
    modifier: u32,
    /// Whether another key went down while it was held: then its release latches nothing.
    chorded: bool,
}

/// The aids' settings and what they hold.
#[derive(Debug, Default)]
pub struct Aids {
    /// Sticky keys on, and whether two keys pressed together turn them off.
    sticky: Option<bool>,
    slow: Option<Duration>,
    bounce: Option<Duration>,
    gestures: bool,
    /// The modifiers sticky keys latched and locked, as `Notice::Modifiers` numbers them.
    latched: u32,
    locked: u32,
    /// The modifiers the aids set last, latched and locked, as `Notice::Modifiers` numbers
    /// them: they take back only their own.
    set: (u32, u32),
    held: Vec<Held>,
    withheld: HashMap<(KeyboardSource, Keycode), Withheld>,
    /// The client the keyboard last went to.
    focus: Option<ClientId>,
    /// When each key was last released, for bounce keys.
    released: HashMap<(KeyboardSource, Keycode), Instant>,
    shift_presses: u8,
    shift_hold: Option<RegistrationToken>,
    /// Whether the last notice found the session locked or a trusted prompt focused.
    quiet: bool,
    /// The aids turned off while quiet, told when the quiet ends.
    deferred: Vec<Aid>,
    /// Every notice sent, for the fixture.
    #[cfg(test)]
    pub sent: Vec<Notice>,
}

impl Aids {
    fn any(&self) -> bool {
        self.sticky.is_some() || self.slow.is_some() || self.bounce.is_some() || self.gestures
    }
}

/// The modifier a key of first-level syms `syms` is, as `Notice::Modifiers` numbers it.
fn modifier(syms: &[Keysym]) -> u32 {
    let is = |names: &[Keysym]| syms.iter().any(|sym| names.contains(sym));
    [
        (SHIFT, &[Keysym::Shift_L, Keysym::Shift_R][..]),
        (CONTROL, &[Keysym::Control_L, Keysym::Control_R]),
        (
            ALT,
            &[Keysym::Alt_L, Keysym::Alt_R, Keysym::Meta_L, Keysym::Meta_R],
        ),
        (SUPER, &[Keysym::Super_L, Keysym::Super_R]),
        (ALTGR, &[Keysym::ISO_Level3_Shift]),
    ]
    .into_iter()
    .filter(|(_, names)| is(names))
    .fold(0, |bits, (bit, _)| bits | bit)
}

/// What each modifier sets under a keymap with no key for it, by the bit's position in
/// `Notice::Modifiers`: Shift, Control, Mod1, Mod4 and Mod5, as xkeyboard-config's keymaps map them.
const USUAL_MASKS: [u32; 5] = [0x01, 0x04, 0x08, 0x40, 0x80];

/// The xkb mask each modifier sets under the keymap `text`, by the bit's position in
/// `Notice::Modifiers`: read from the keys, since a virtual keyboard's keymap may put a modifier
/// where it likes. The keymap is compiled again here, so `State::modifier_masks` asks only while
/// a modifier is latched or locked.
fn keymap_masks(text: String) -> [u32; 5] {
    let mut masks = [0; 5];
    let context = xkb::Context::new(xkb::CONTEXT_NO_DEFAULT_INCLUDES);
    let Some(keymap) = xkb::Keymap::new_from_string(
        &context,
        text,
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ) else {
        tracing::error!("the seat's keymap did not compile again: the usual modifier masks");
        return USUAL_MASKS;
    };
    let mut state = xkb::State::new(&keymap);
    for layout in 0..keymap.num_layouts() {
        state.update_mask(0, 0, 0, 0, 0, layout);
        for code in keymap.min_keycode().raw()..=keymap.max_keycode().raw() {
            let code = Keycode::new(code);
            let bits = modifier(keymap.key_get_syms_by_level(code, layout, 0));
            if bits == 0 {
                continue;
            }
            state.update_key(code, xkb::KeyDirection::Down);
            let set = state.serialize_mods(xkb::STATE_MODS_DEPRESSED);
            state.update_key(code, xkb::KeyDirection::Up);
            for bit in (0..5).filter(|bit| bits & (1 << bit) != 0) {
                masks[bit] |= set;
            }
        }
    }
    std::array::from_fn(|bit| match masks[bit] {
        0 => USUAL_MASKS[bit],
        mask => mask,
    })
}

/// The xkb mask of the modifiers `bits`, under `masks`.
fn xkb_mask(masks: &[u32; 5], bits: u32) -> u32 {
    (0..5)
        .filter(|bit| bits & (1 << bit) != 0)
        .fold(0, |mask, bit| mask | masks[bit])
}

/// `bits` as the bindings' mask names them; AltGr is no modifier of a binding.
fn binding_mask(bits: u32) -> u8 {
    [
        (SHIFT, bindings::SHIFT),
        (CONTROL, bindings::CTRL),
        (ALT, bindings::ALT),
        (SUPER, bindings::SUPER),
    ]
    .into_iter()
    .filter(|(bit, _)| bits & bit != 0)
    .fold(0, |mask, (_, binding)| mask | binding)
}

fn delay(on: bool, ms: u32) -> Result<Option<Duration>, InvalidDelay> {
    if ms > MAX_DELAY {
        return Err(InvalidDelay(ms));
    }
    Ok(on.then(|| Duration::from_millis(ms.into())))
}

// The setters are `athanor_a11y_v1`'s requests, which #864 serves; until then only the
// fixture calls them.
#[cfg_attr(not(test), allow(dead_code))]
impl State {
    /// Sticky keys on or off; off lets go of every modifier they latched or locked.
    pub fn set_sticky_keys(&mut self, on: bool, two_key_off: bool) {
        if on {
            self.aids.sticky = Some(two_key_off);
        } else {
            self.sticky_released();
        }
    }

    /// Slow keys on or off, with their delay in milliseconds.
    pub fn set_slow_keys(&mut self, on: bool, ms: u32) -> Result<(), InvalidDelay> {
        self.aids.slow = delay(on, ms)?;
        Ok(())
    }

    /// Bounce keys on or off, with their delay in milliseconds.
    pub fn set_bounce_keys(&mut self, on: bool, ms: u32) -> Result<(), InvalidDelay> {
        self.aids.bounce = delay(on, ms)?;
        if !on {
            self.aids.released.clear();
        }
        Ok(())
    }

    /// The Shift gestures reported or not.
    pub fn set_aid_gestures(&mut self, on: bool) {
        self.aids.gestures = on;
        if !on {
            self.aids.shift_presses = 0;
            self.stop_shift_hold();
        }
    }

    /// The modifiers sticky keys latched and locked, for a client that binds
    /// `athanor_a11y_v1`; none while the session is locked or a trusted prompt has the focus,
    /// when they are sent once that ends.
    pub fn aid_modifiers(&self) -> Option<Notice> {
        (!self.aids_quiet()).then_some(Notice::Modifiers {
            latched: self.aids.latched,
            locked: self.aids.locked,
        })
    }

    /// A key from `origin`, through bounce, slow and sticky keys, on to `typed`. A key of a
    /// virtual keyboard arrives with that keyboard's keymap loaded.
    pub(crate) fn aided(
        &mut self,
        code: Keycode,
        key_state: KeyState,
        time: InputTime,
        origin: Origin,
    ) {
        let source = origin.source();
        // A key still held back comes here for its release, though the last aid went off.
        if !self.aids.any() && self.aids.withheld.is_empty() {
            return self.typed(code, key_state, time, source, false);
        }
        if source == KeyboardSource::Physical {
            self.restore_seat_keymap();
        }
        let syms = self.raw_syms(code);
        let pressed = key_state == KeyState::Pressed;
        // Only the hardware keyboard's: a client's keys must not offer the user an aid.
        if source == KeyboardSource::Physical {
            self.shift_gestures(&syms, pressed);
        }
        let key = (source, code);
        let now = Instant::now();
        if !pressed {
            let withheld = self.aids.withheld.remove(&key);
            // A press the aids rejected for another reason was never typed, and bounced nothing.
            if self.aids.bounce.is_some() && withheld.as_ref().is_none_or(|held| held.bounced) {
                // Every release restarts the delay, a bounced press's too.
                let max = Duration::from_millis(MAX_DELAY.into());
                self.aids
                    .released
                    .retain(|_, at| now.duration_since(*at) < max);
                self.aids.released.insert(key, now);
            }
            if let Some(withheld) = withheld {
                if let Some(timer) = withheld.slow {
                    self.loop_handle.remove(timer);
                    self.aid_notice(Notice::Rejected(Aid::Slow));
                }
                return;
            }
            return self.sticky_typed(code, key_state, time, source);
        }
        // A press of a key already held back, which only a virtual keyboard sends, changes
        // nothing: the timer running is the one its release cancels.
        if self.aids.withheld.contains_key(&key) {
            return;
        }
        let modifier = modifier(&syms);
        let pending = self.protected(&syms, source, false);
        if pending || self.protected(&syms, source, true) {
            // The modifiers slow keys hold back with it are let through first, so that the key
            // reads as the chord it is. One bounce keys rejected goes too only when the chord
            // needs it, since a protected key passes them; one dropped stays dropped.
            let held: Vec<_> = self
                .aids
                .withheld
                .iter()
                .filter(|((from, _), held)| {
                    *from == source
                        && held.modifier != 0
                        && (held.slow.is_some() || (!pending && held.bounced))
                })
                .map(|(key, _)| *key)
                .collect();
            for key in held {
                if let Some(Withheld {
                    slow: Some(timer), ..
                }) = self.aids.withheld.remove(&key)
                {
                    self.loop_handle.remove(timer);
                }
                self.sticky_typed(key.1, KeyState::Pressed, time, source);
            }
            return self.sticky_typed(code, key_state, time, source);
        }
        if let Some(delay) = self.aids.bounce {
            if self
                .aids
                .released
                .get(&key)
                .is_some_and(|at| now.duration_since(*at) < delay)
            {
                let withheld = Withheld {
                    origin,
                    modifier,
                    slow: None,
                    bounced: true,
                };
                self.aids.withheld.insert(key, withheld);
                return self.aid_notice(Notice::Rejected(Aid::Bounce));
            }
        }
        if let Some(delay) = self.aids.slow.filter(|delay| !delay.is_zero()) {
            let timer = Timer::from_duration(delay);
            match self.loop_handle.insert_source(timer, move |_, _, state| {
                state.slow_accepted(key, time);
                TimeoutAction::Drop
            }) {
                Ok(timer) => {
                    let withheld = Withheld {
                        origin,
                        modifier,
                        slow: Some(timer),
                        bounced: false,
                    };
                    self.aids.withheld.insert(key, withheld);
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "slow keys have no timer: the key passes at once");
                }
            }
        }
        self.sticky_typed(code, key_state, time, source);
    }

    /// The press of `key`, held for slow keys' delay: it goes through, under the keymap of
    /// the keyboard it came from.
    fn slow_accepted(&mut self, key: (KeyboardSource, Keycode), time: InputTime) {
        let Some(withheld) = self.aids.withheld.remove(&key) else {
            return;
        };
        let (source, code) = key;
        match withheld.origin {
            Origin::Seat(_) => {
                if source == KeyboardSource::Physical {
                    self.restore_seat_keymap();
                }
                self.sticky_typed(code, KeyState::Pressed, time, source);
                self.send_keyboard_group();
            }
            Origin::Virtual(device) => {
                // While locked a virtual keyboard's press is dropped (`drops_key`).
                if !self.drops_virtual_input() {
                    self.with_keymap_of(&device, |state| {
                        state.sticky_typed(code, KeyState::Pressed, time, source)
                    });
                }
            }
        }
    }

    /// Sticky keys around `typed`: a modifier pressed and released alone is latched, then
    /// locked, then let go; a key pressed with the modifiers latched lets them go once it is
    /// read with them. With two-key-off, a key of the hardware keyboard pressed while one of its
    /// modifiers is held turns sticky keys off, unless it is another modifier or a protected key,
    /// so that Ctrl+Alt+Tab does not (the maintainer's decision of 2026-10-30, AX11); a client's
    /// chord on a virtual keyboard turns off nothing of the seat's.
    fn sticky_typed(
        &mut self,
        code: Keycode,
        key_state: KeyState,
        time: InputTime,
        source: KeyboardSource,
    ) {
        let Some(two_key_off) = self.aids.sticky else {
            return self.typed(code, key_state, time, source, false);
        };
        let syms = self.raw_syms(code);
        let modifier = modifier(&syms);
        let key = (source, code);
        if key_state == KeyState::Pressed {
            let chord = source == KeyboardSource::Physical
                && self.aids.held.iter().any(|held| held.key.0 == source);
            if two_key_off && chord && modifier == 0 && !self.protected(&syms, source, false) {
                self.sticky_released();
                self.aid_notice(Notice::AidOff(Aid::Sticky));
                return self.typed(code, key_state, time, source, false);
            }
            for held in self.aids.held.iter_mut().filter(|h| h.key.0 == source) {
                held.chorded = true;
            }
            self.typed(code, key_state, time, source, false);
            if modifier != 0 {
                let chorded = false;
                self.aids.held.push(Held {
                    key,
                    modifier,
                    chorded,
                });
            } else if self.aids.latched != 0 {
                self.aids.latched = 0;
                self.sticky_apply();
            }
            return;
        }
        let tapped = self
            .aids
            .held
            .iter()
            .position(|held| held.key == key)
            .map(|at| self.aids.held.swap_remove(at))
            .filter(|held| !held.chorded)
            .map(|held| held.modifier);
        // Super tapped latches; it does not show the launcher too (D4).
        if tapped.is_some() {
            self.super_tap = None;
        }
        self.typed(code, key_state, time, source, false);
        if let Some(bits) = tapped {
            let aids = &mut self.aids;
            if aids.locked & bits != 0 {
                aids.locked &= !bits;
            } else if aids.latched & bits != 0 {
                aids.latched &= !bits;
                aids.locked |= bits;
            } else {
                aids.latched |= bits;
            }
            self.sticky_apply();
        }
    }

    /// Sticky keys off: every modifier they latched or locked let go.
    fn sticky_released(&mut self) {
        self.aids.sticky = None;
        self.aids.held.clear();
        if self.aids.latched | self.aids.locked != 0 {
            self.aids.latched = 0;
            self.aids.locked = 0;
            self.sticky_apply();
        }
    }

    /// The latched and locked modifiers given to the keyboard, and so to the focused client,
    /// and told.
    fn sticky_apply(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let masks = self.modifier_masks();
        let current = keyboard.modifier_state().serialized;
        let (set_latched, set_locked) = self.aids.set;
        let latched = (current.latched & !xkb_mask(&masks, set_latched))
            | xkb_mask(&masks, self.aids.latched);
        let locked = (current.locked & !xkb_mask(&masks, set_locked))
            | xkb_mask(&masks, self.aids.locked);
        self.aids.set = (self.aids.latched, self.aids.locked);
        let mask = [current.depressed, latched, locked, current.layout_effective];
        self.set_modifiers(&keyboard, mask);
        self.aid_notice(Notice::Modifiers {
            latched: self.aids.latched,
            locked: self.aids.locked,
        });
    }

    /// Shift pressed five times in a row offers sticky keys, and held for eight seconds slow
    /// keys, while the gestures are on; any other key starts the count again.
    fn shift_gestures(&mut self, syms: &[Keysym], pressed: bool) {
        let shift = syms
            .iter()
            .any(|sym| [Keysym::Shift_L, Keysym::Shift_R].contains(sym));
        if !pressed {
            if shift {
                self.stop_shift_hold();
            }
            return;
        }
        self.stop_shift_hold();
        if !shift || !self.aids.gestures {
            self.aids.shift_presses = 0;
            return;
        }
        self.aids.shift_presses += 1;
        if self.aids.shift_presses == SHIFT_PRESSES {
            self.aids.shift_presses = 0;
            self.aid_notice(Notice::Gesture(Aid::Sticky));
        }
        let timer = Timer::from_duration(SHIFT_HOLD);
        match self.loop_handle.insert_source(timer, |_, _, state| {
            state.aids.shift_hold = None;
            state.aid_notice(Notice::Gesture(Aid::Slow));
            TimeoutAction::Drop
        }) {
            Ok(timer) => self.aids.shift_hold = Some(timer),
            Err(error) => tracing::warn!(%error, "Shift held cannot be timed: no gesture"),
        }
    }

    fn stop_shift_hold(&mut self) {
        if let Some(timer) = self.aids.shift_hold.take() {
            self.loop_handle.remove(timer);
        }
    }

    /// Whether a key of `source` of first-level syms `syms`, pressed now, is protected
    /// (decision 30), with the modifiers held, latched or locked and those slow keys hold back
    /// on the same keyboard, and if `rejected` those bounce keys rejected; a modifier dropped
    /// counts for nothing.
    fn protected(&self, syms: &[Keysym], source: KeyboardSource, rejected: bool) -> bool {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let held_back = self
            .aids
            .withheld
            .iter()
            .filter(|((from, _), withheld)| {
                *from == source && (withheld.slow.is_some() || (rejected && withheld.bounced))
            })
            .fold(0, |bits, (_, withheld)| bits | withheld.modifier);
        let mods = bindings::mask(&keyboard.modifier_state()) | binding_mask(held_back);
        bindings::protected(mods, syms)
    }

    /// The first-level syms of `code` in the keymap's active layout, then in its others.
    fn raw_syms(&mut self, code: Keycode) -> Vec<Keysym> {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Vec::new();
        };
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap_or_else(PoisonError::into_inner);
            let active = xkb.active_layout();
            let others = xkb.layouts().filter(|layout| *layout != active);
            std::iter::once(active)
                .chain(others)
                .flat_map(|layout| xkb.raw_syms_for_key_in_layout(code, layout).to_vec())
                .collect()
        })
    }

    /// A pointer button or a tablet's tip pressed: it breaks a run of Shift presses, as any
    /// other key does (the maintainer's decision of 2026-10-30, AX11).
    pub(crate) fn aids_pointer_pressed(&mut self) {
        self.aids.shift_presses = 0;
    }

    /// `keyboard`'s modifiers as `[depressed, latched, locked, group]`, without those sticky
    /// keys set: what is saved across a virtual keyboard's keys and keymap, since sticky keys
    /// may change theirs meanwhile.
    pub(crate) fn seat_modifiers(&mut self, keyboard: &KeyboardHandle<Self>) -> [u32; 4] {
        let masks = self.modifier_masks();
        let mods = keyboard.modifier_state().serialized;
        let (latched, locked) = self.aids.set;
        [
            mods.depressed,
            mods.latched & !xkb_mask(&masks, latched),
            mods.locked & !xkb_mask(&masks, locked),
            mods.layout_effective,
        ]
    }

    /// `mask`, `[depressed, latched, locked, group]`, with the modifiers sticky keys set.
    pub(crate) fn with_sticky(
        &mut self,
        [depressed, latched, locked, group]: [u32; 4],
    ) -> [u32; 4] {
        let masks = self.modifier_masks();
        let (latches, locks) = self.aids.set;
        [
            depressed,
            latched | xkb_mask(&masks, latches),
            locked | xkb_mask(&masks, locks),
            group,
        ]
    }

    /// The xkb mask of each modifier under the keymap the seat has now, the seat's own or a
    /// virtual keyboard's, whose client reads the modifiers by it. None is needed while sticky
    /// keys hold no modifier.
    fn modifier_masks(&mut self) -> [u32; 5] {
        let (latched, locked) = self.aids.set;
        let keyboard = self.seat.get_keyboard();
        match keyboard {
            Some(keyboard) if latched | locked | self.aids.latched | self.aids.locked != 0 => {
                keymap_masks(keyboard.with_xkb_state(self, |context| keymap_text(context.xkb())))
            }
            _ => [0; 5],
        }
    }

    /// Drops the presses slow keys hold back, and so their releases: rejecting a key is safe,
    /// delivering it to whatever has the keyboard when its delay ends is not, since a password's
    /// last characters would reach the window under the lock. Called when the lock changes and
    /// from `aids_focus_changed`, inside Smithay's lock on the keyboard, so it must not read
    /// the keyboard or tell anything.
    pub(crate) fn aids_drop_pending(&mut self) {
        let timers: Vec<_> = self
            .aids
            .withheld
            .values_mut()
            .filter_map(|withheld| withheld.slow.take())
            .collect();
        for timer in timers {
            self.loop_handle.remove(timer);
        }
    }

    /// The session paused: the keys held are released on the other terminal, if at all, so
    /// what the aids keep of them goes, or it would swallow the key's next press or turn sticky
    /// keys off at the next key. Latched and locked modifiers stay, as Caps Lock does.
    pub(crate) fn aids_paused(&mut self) {
        self.aids_drop_pending();
        self.aids.withheld.clear();
        self.aids.held.clear();
        self.aids.shift_presses = 0;
        self.stop_shift_hold();
    }

    /// The keyboard went to `client`: a press held back for another client is dropped.
    pub(crate) fn aids_focus_changed(&mut self, client: Option<ClientId>) {
        if self.aids.focus != client {
            self.aids.focus = client;
            self.aids_drop_pending();
        }
    }

    /// Whether the session is locked or a trusted prompt has the keyboard: then no modifiers,
    /// rejected key or gesture is told (AX11, acceptance item 5).
    fn aids_quiet(&self) -> bool {
        self.lock.is_some()
            || self
                .seat
                .get_keyboard()
                .and_then(|keyboard| keyboard.current_focus())
                .and_then(|surface| surface.client())
                .is_some_and(|client| granted(&client, Privilege::TrustedPrompt))
    }

    /// Tells `notice`, unless the aids are quiet: an aid turned off is told once the quiet
    /// ends, the modifiers are told again then, and the rest is never told.
    fn aid_notice(&mut self, notice: Notice) {
        self.aids_quiet_changed();
        match notice {
            Notice::AidOff(aid) if self.aids.quiet => self.aids.deferred.push(aid),
            _ if self.aids.quiet => {}
            notice => self.send_aid_notice(notice),
        }
    }

    /// After a lock, an unlock or a change of the keyboard focus: the end of the quiet tells
    /// the modifiers and the aids turned off meanwhile.
    pub(crate) fn aids_quiet_changed(&mut self) {
        let quiet = self.aids_quiet();
        let ended = self.aids.quiet && !quiet;
        self.aids.quiet = quiet;
        if ended {
            self.send_aid_notice(Notice::Modifiers {
                latched: self.aids.latched,
                locked: self.aids.locked,
            });
            for aid in std::mem::take(&mut self.aids.deferred) {
                self.send_aid_notice(Notice::AidOff(aid));
            }
        }
    }

    /// Sends `notice` to the clients of `athanor_a11y_v1`, which #864 serves; until then it is
    /// logged.
    fn send_aid_notice(&mut self, notice: Notice) {
        tracing::debug!(?notice, "keyboard aid");
        #[cfg(test)]
        self.aids.sent.push(notice);
    }
}
