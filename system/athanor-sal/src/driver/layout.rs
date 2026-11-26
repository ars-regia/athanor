//! The active keyboard layout's name, which a trusted surface shows where a password is typed
//! (doc_lock_and_prompts.md, LP14): the names come from the `wl_keyboard` keymap and the group
//! from its modifiers, as the compositor sends them. Switching the group is not Sal's. The
//! keymap also names the key a shortcut capture records, by its first level (doc_settings.md
//! SL5).

use xkbcommon::xkb;

/// The keymap, the names of its layouts, and the group the compositor last made effective.
#[derive(Default)]
pub(crate) struct Layouts {
    names: Vec<String>,
    group: u32,
    keymap: Option<xkb::Keymap>,
}

impl Layouts {
    /// Takes the names of a new keymap's layouts: `true` when the active layout changed.
    pub(crate) fn keymap(&mut self, text: &str) -> bool {
        let before = self.active();
        // xkbcommon 0.8 builds the keymap text and the layout names with
        // `str::from_utf8_unchecked`, so a byte that is not UTF-8 is refused here. libxkbcommon
        // writes the text, so it holds no NUL, on which `new_from_string` would panic.
        self.keymap = if text.contains('\0') || std::str::from_utf8(text.as_bytes()).is_err() {
            None
        } else {
            let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            xkb::Keymap::new_from_string(
                &context,
                text.to_owned(),
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        self.names = self
            .keymap
            .as_ref()
            .map(|keymap| {
                (0..keymap.num_layouts())
                    .map(|index| {
                        std::str::from_utf8(keymap.layout_get_name(index).as_bytes())
                            .map_or_else(|_| String::new(), str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.active() != before
    }

    /// Takes the effective group of a modifiers event: `true` when the active layout changed.
    pub(crate) fn group(&mut self, group: u32) -> bool {
        let before = self.active();
        self.group = group;
        self.active() != before
    }

    /// Forgets the keymap and the group when the seat loses its keyboard: `true` when a layout
    /// was named. A new keyboard starts at group 0 with a keymap of its own.
    pub(crate) fn clear(&mut self) -> bool {
        let before = self.active();
        *self = Self::default();
        before.is_some()
    }

    /// The keysym the key `raw_code`, a Wayland key code, gives at the first level of the
    /// effective layout, as with no modifier held: Shift+1 is `1` and not `exclam`. `keysym`,
    /// the one the key gave, before a keymap, or for a key with no single keysym there.
    pub(crate) fn base(&self, raw_code: u32, keysym: xkb::Keysym) -> xkb::Keysym {
        let Some(keymap) = &self.keymap else {
            return keysym;
        };
        // xkb numbers a key 8 above the Linux input code that wl_keyboard sends.
        let key = xkb::Keycode::new(raw_code.saturating_add(8));
        match keymap.key_get_syms_by_level(key, self.group, 0) {
            [one] => *one,
            _ => keysym,
        }
    }

    /// The effective group and its name; `None` before a keymap, or for a layout it names not.
    pub(crate) fn active(&self) -> Option<(u32, String)> {
        let name = self.names.get(usize::try_from(self.group).ok()?)?;
        (!name.is_empty()).then(|| (self.group, name.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keymap as a compositor sends it, with the names of its groups, and no include that
    /// would need xkeyboard-config.
    fn keymap(first: &str, second: &str) -> String {
        format!(
            r#"xkb_keymap {{
    xkb_keycodes "test" {{ minimum = 8; maximum = 255; <AC01> = 38; }};
    xkb_types "test" {{
        type "ONE_LEVEL" {{ modifiers = none; level_name[Level1] = "Any"; }};
    }};
    xkb_compat "test" {{ }};
    xkb_symbols "test" {{
        name[Group1] = "{first}";
        name[Group2] = "{second}";
        key <AC01> {{ [ a ], [ q ] }};
    }};
}};"#
        )
    }

    /// A keymap of two layouts whose keys give another symbol with Shift, as a compositor sends
    /// it.
    fn shifted() -> Layouts {
        let text = r#"xkb_keymap {
    xkb_keycodes "test" {
        minimum = 8; maximum = 255; <AE01> = 10; <TAB> = 23; <AC01> = 38; <AC10> = 47;
        <KP1> = 87;
    };
    xkb_types "test" {
        type "TWO_LEVEL" {
            modifiers = Shift; map[Shift] = Level2;
            level_name[Level1] = "Base"; level_name[Level2] = "Shift";
        };
    };
    xkb_compat "test" { };
    xkb_symbols "test" {
        name[Group1] = "English (US)";
        name[Group2] = "French";
        key <AE01> { type = "TWO_LEVEL", [ 1, exclam ], [ ampersand, 1 ] };
        key <TAB> { type = "TWO_LEVEL", [ Tab, ISO_Left_Tab ] };
        key <AC01> { type = "TWO_LEVEL", [ a, A ], [ q, Q ] };
        key <AC10> { type = "TWO_LEVEL", [ semicolon, colon ], [ m, M ] };
        key <KP1> { type = "TWO_LEVEL", [ KP_End, KP_1 ] };
    };
};"#;
        let mut layouts = Layouts::default();
        assert!(layouts.keymap(text));
        layouts
    }

    /// A shortcut records the key's first level, so that Shift+1 is `1` with Shift and not
    /// `exclam` (doc_settings.md SL5); the keys are the keymap's Wayland codes, 8 below xkb's.
    #[test]
    fn a_key_is_named_by_its_first_level_whatever_shift_made_of_it() {
        let layouts = shifted();
        let keys = [
            (2, xkb::Keysym::exclam, xkb::Keysym::_1),
            (15, xkb::Keysym::ISO_Left_Tab, xkb::Keysym::Tab),
            (39, xkb::Keysym::colon, xkb::Keysym::semicolon),
            (30, xkb::Keysym::A, xkb::Keysym::a),
        ];
        for (raw, pressed, base) in keys {
            assert_eq!(layouts.base(raw, pressed), base, "{pressed:?}");
            assert_eq!(layouts.base(raw, base), base, "{base:?} unshifted");
        }
    }

    #[test]
    fn the_first_level_is_the_effective_layouts() {
        let mut layouts = shifted();
        assert!(layouts.group(1));
        assert_eq!(layouts.base(30, xkb::Keysym::Q), xkb::Keysym::q);
        assert_eq!(layouts.base(2, xkb::Keysym::_1), xkb::Keysym::ampersand);
    }

    /// Tab is defined in the first layout only: in the second, xkb brings the group back into
    /// the key's range, so Shift+Tab is still `Tab` and not `ISO_Left_Tab`.
    #[test]
    fn a_key_missing_from_the_effective_layout_is_named_by_the_one_it_has() {
        let mut layouts = shifted();
        assert!(layouts.group(1));
        assert_eq!(
            layouts.base(15, xkb::Keysym::ISO_Left_Tab),
            xkb::Keysym::Tab
        );
    }

    /// The keypad's first level is its navigation symbol, so with NumLock on, KP_1 is recorded
    /// as `KP_End`: a shortcut on the keypad's 1 is the same with NumLock on or off.
    #[test]
    fn a_keypad_digit_is_recorded_as_its_navigation_key_with_numlock_on() {
        assert_eq!(shifted().base(79, xkb::Keysym::KP_1), xkb::Keysym::KP_End);
    }

    #[test]
    fn without_a_keymap_or_a_symbol_a_key_keeps_the_one_it_gave() {
        assert_eq!(
            Layouts::default().base(2, xkb::Keysym::exclam),
            xkb::Keysym::exclam
        );
        assert_eq!(shifted().base(80, xkb::Keysym::F12), xkb::Keysym::F12);
        let mut cleared = shifted();
        assert!(cleared.clear());
        assert_eq!(cleared.base(2, xkb::Keysym::exclam), xkb::Keysym::exclam);
    }

    fn us_it() -> Layouts {
        let mut layouts = Layouts::default();
        assert!(layouts.keymap(&keymap("English (US)", "Italian")));
        layouts
    }

    #[test]
    fn no_layout_before_a_keymap() {
        let mut layouts = Layouts::default();
        assert_eq!(layouts.active(), None);
        assert!(!layouts.group(1), "a group without a keymap names nothing");
        assert!(!layouts.keymap("not a keymap"));
        assert_eq!(layouts.active(), None);
    }

    #[test]
    fn a_keymap_names_its_layouts() {
        assert_eq!(us_it().active(), Some((0, "English (US)".to_owned())));
    }

    #[test]
    fn a_group_change_moves_the_active_layout() {
        let mut layouts = us_it();
        assert!(layouts.group(1));
        assert_eq!(layouts.active(), Some((1, "Italian".to_owned())));
    }

    #[test]
    fn modifiers_without_a_group_change_change_nothing() {
        let mut layouts = us_it();
        assert!(!layouts.group(0));
        assert!(!layouts.keymap(&keymap("English (US)", "German")), "the active one kept its name");
    }

    #[test]
    fn a_new_keymap_renaming_the_active_layout_is_a_change() {
        let mut layouts = us_it();
        assert!(layouts.group(1));
        assert!(layouts.keymap(&keymap("English (US)", "Italian (intl.)")));
        assert_eq!(layouts.active(), Some((1, "Italian (intl.)".to_owned())));
    }

    #[test]
    fn a_lost_keyboard_names_no_layout() {
        let mut layouts = us_it();
        assert!(layouts.group(1));
        assert!(layouts.clear());
        assert_eq!(layouts.active(), None);
        assert!(!layouts.clear(), "nothing was named the second time");
        assert!(layouts.keymap(&keymap("English (US)", "Italian")));
        assert_eq!(layouts.active(), Some((0, "English (US)".to_owned())), "a new keyboard starts at group 0");
    }
}
