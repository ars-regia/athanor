//! The modifiers held with a key, which an application reads in [`super::Application::key`].

use smithay_client_toolkit::seat::keyboard::Modifiers;

/// The modifiers held with a key: what tells Ctrl+Enter from Enter, say. New ones may be added:
/// start from [`KeyModifiers::default`], which holds none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyModifiers {
    /// Control.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt.
    pub alt: bool,
    /// The logo key, Super.
    pub logo: bool,
    /// Caps Lock is on, as the keyboard's xkb state has it: a lock, not a modifier held with
    /// the key, which a password prompt warns of.
    pub caps_lock: bool,
}

impl From<Modifiers> for KeyModifiers {
    fn from(held: Modifiers) -> Self {
        Self {
            ctrl: held.ctrl,
            shift: held.shift,
            alt: held.alt,
            logo: held.logo,
            caps_lock: held.caps_lock,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_is_reported_and_num_lock_is_not() {
        let held = Modifiers {
            ctrl: true,
            caps_lock: true,
            num_lock: true,
            ..Modifiers::default()
        };
        let want = KeyModifiers {
            ctrl: true,
            caps_lock: true,
            ..KeyModifiers::default()
        };
        assert_eq!(KeyModifiers::from(held), want);
    }

    #[test]
    fn caps_lock_off_is_reported_off() {
        let held = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(!KeyModifiers::from(held).caps_lock);
    }
}
