//! The active keyboard layout's name, which a trusted surface shows where a password is typed
//! (doc_lock_and_prompts.md, LP14): the names come from the `wl_keyboard` keymap and the group
//! from its modifiers, as the compositor sends them. Switching the group is not Sal's.

use xkbcommon::xkb;

/// The names of the keymap's layouts, and the group the compositor last made effective.
#[derive(Debug, Default)]
pub(crate) struct Layouts {
    names: Vec<String>,
    group: u32,
}

impl Layouts {
    /// Takes the names of a new keymap's layouts: `true` when the active layout changed.
    pub(crate) fn keymap(&mut self, text: &str) -> bool {
        let before = self.active();
        // xkbcommon 0.8 builds the keymap text and the layout names with
        // `str::from_utf8_unchecked`, so a byte that is not UTF-8 is refused here. libxkbcommon
        // writes the text, so it holds no NUL, on which `new_from_string` would panic.
        self.names = if text.contains('\0') || std::str::from_utf8(text.as_bytes()).is_err() {
            Vec::new()
        } else {
            let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            xkb::Keymap::new_from_string(
                &context,
                text.to_owned(),
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
            .map(|keymap| {
                (0..keymap.num_layouts())
                    .map(|index| {
                        std::str::from_utf8(keymap.layout_get_name(index).as_bytes())
                            .map_or_else(|_| String::new(), str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default()
        };
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
