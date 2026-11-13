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
}

impl From<Modifiers> for KeyModifiers {
    fn from(held: Modifiers) -> Self {
        Self {
            ctrl: held.ctrl,
            shift: held.shift,
            alt: held.alt,
            logo: held.logo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locks_are_not_modifiers_of_a_key() {
        let held = Modifiers {
            ctrl: true,
            caps_lock: true,
            num_lock: true,
            ..Modifiers::default()
        };
        let want = KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        };
        assert_eq!(KeyModifiers::from(held), want);
    }
}
