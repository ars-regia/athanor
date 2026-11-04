//! Layout names from an XKB keymap. cosmic-comp sends the keymap as text on the
//! `wl_keyboard`; the keyboard-layout protocol reports only the index of the active group.

/// The keymaps cosmic-comp sends are tens of kilobytes; anything beyond this is refused.
pub(crate) const MAX_KEYMAP: u32 = 1 << 20;

/// More groups than a keymap holds: a higher index is not a group, and allocates nothing.
const MAX_GROUPS: usize = 32;

/// The `name[GroupN]` values of the `xkb_symbols` section, one entry per group up to the
/// last one named, empty for a group with no name, so that an entry's place is its group.
/// libxkbcommon writes `name[Group1]="English (US)";`; a bare index (`name[1]`) is accepted
/// too. Lines that do not parse are skipped, and a group named twice keeps its first name.
pub fn layout_names(keymap: &str) -> Vec<String> {
    let symbols = keymap.find("xkb_symbols").map_or("", |at| &keymap[at..]);
    let mut names: Vec<Option<String>> = Vec::new();
    for (group, name) in symbols.lines().filter_map(group_name) {
        let Some(place) = group.checked_sub(1).filter(|place| *place < MAX_GROUPS) else {
            continue;
        };
        if names.len() <= place {
            names.resize(place + 1, None);
        }
        names[place].get_or_insert(name);
    }
    names.into_iter().map(Option::unwrap_or_default).collect()
}

fn group_name(line: &str) -> Option<(usize, String)> {
    let rest = line.trim().strip_prefix("name[")?;
    let (group, rest) = rest.split_once(']')?;
    let group = group.trim();
    // `get` rather than indexing: a name with a multibyte character must not abort.
    let digits = match (group.get(..5), group.get(5..)) {
        (Some(prefix), Some(digits)) if prefix.eq_ignore_ascii_case("group") => digits,
        _ => group,
    };
    let index = digits.parse::<usize>().ok()?;
    let value = rest
        .trim_start()
        .strip_prefix('=')?
        .trim_start()
        .strip_prefix('"')?;
    let (name, _) = value.split_once('"')?;
    Some((index, name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYMAP: &str = r#"xkb_keymap {
xkb_keycodes "evdev+aliases(qwerty)" {
	minimum = 8;
	indicator 1 = "Caps Lock";
};
xkb_types "complete" {
	type "ONE_LEVEL" {
		modifiers= none;
		level_name[1]= "Any";
	};
};
xkb_symbols "pc+us+it:2+inet(evdev)" {
	name[Group2]="Italian";
	name[Group1]="English (US)";
	key <AE01> { [ 1, exclam ] };
};
};
"#;

    #[test]
    fn names_come_in_group_order_from_the_symbols_section() {
        assert_eq!(layout_names(KEYMAP), ["English (US)", "Italian"]);
    }

    #[test]
    fn a_bare_index_and_spacing_are_accepted() {
        let keymap =
            "xkb_symbols \"x\" {\n  name[ 1 ] = \"German\";\n  name[group2]=\"French\";\n};";
        assert_eq!(layout_names(keymap), ["German", "French"]);
    }

    #[test]
    fn malformed_lines_are_skipped_and_the_first_name_of_a_group_wins() {
        let keymap = "xkb_symbols \"x\" {\n name[Group1]=\"A\";\n name[Group1]=\"B\";\n name[GroupX]=\"C\";\n name[Group3]=\"unterminated;\n name[Group4] \"no equals\";\n name[Grouü1]=\"D\";\n};";
        assert_eq!(layout_names(keymap), ["A"]);
    }

    #[test]
    fn a_group_with_no_name_keeps_its_place() {
        let keymap = "xkb_symbols \"x\" {\n name[Group2]=\"Italian\";\n};";
        assert_eq!(layout_names(keymap), ["", "Italian"]);
    }

    #[test]
    fn an_index_that_is_no_group_is_skipped() {
        let keymap = "xkb_symbols \"x\" {\n name[Group0]=\"A\";\n name[Group33]=\"B\";\n name[Group4294967295]=\"C\";\n name[Group1]=\"D\";\n};";
        assert_eq!(layout_names(keymap), ["D"]);
    }

    #[test]
    fn a_keymap_without_symbols_has_no_layouts() {
        assert!(layout_names("").is_empty());
        assert!(layout_names("xkb_keycodes { name[Group1]=\"Not here\"; };").is_empty());
    }
}
