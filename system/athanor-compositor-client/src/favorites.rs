//! The one-time import of COSMIC's favourites (doc_bar.md, BR7). This crate is the only
//! one that knows COSMIC's paths; the favourites file itself belongs to `athanor-layout`.

use std::path::Path;

use crate::cosmic_config;

const APP_LIST: &str = "com.system76.CosmicAppList";

/// The desktop ids of the user's own COSMIC favourites, in COSMIC's order; `None` when
/// the user has no readable list. COSMIC's system default under `/usr/share/cosmic` is a
/// vendor choice like our own list, not the user's: importing it would shadow the vendor
/// favourites of the bar (doc_bar.md, BR7).
pub fn cosmic_favorites() -> Option<Vec<String>> {
    cosmic_favorites_in(&cosmic_config::user_dir()?)
}

fn cosmic_favorites_in(user_cosmic_dir: &Path) -> Option<Vec<String>> {
    let text = cosmic_config::key(&[user_cosmic_dir.to_path_buf()], APP_LIST, "favorites")?;
    let favorites = parse(&text);
    if favorites.is_none() {
        tracing::warn!("COSMIC's favourites list does not parse; it is not imported");
    }
    favorites
}

/// A RON list of strings: `[ "firefox", "com.system76.CosmicFiles", ]`. Anything else in
/// the file refuses the whole list, since a partial import cannot be told from a full
/// one. An entry that cannot be a desktop id is skipped, as is a repeated one.
fn parse(text: &str) -> Option<Vec<String>> {
    let body = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut ids: Vec<String> = Vec::new();
    let mut chars = body.chars();
    let mut expect_value = true;
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ',' if !expect_value => expect_value = true,
            '"' if expect_value => {
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => value.push(match chars.next()? {
                            c @ ('"' | '\\') => c,
                            _ => return None,
                        }),
                        c => value.push(c),
                    }
                }
                let id = format!("{value}.desktop");
                if is_desktop_id(&value) && !ids.contains(&id) {
                    ids.push(id);
                }
                expect_value = false;
            }
            _ => return None,
        }
    }
    Some(ids)
}

/// The characters of a desktop file name (Desktop Entry Specification, "Desktop File ID"),
/// with no path separator.
fn is_desktop_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosmic_default_list_parses_in_order() {
        let text = "[\n    \"com.system76.CosmicFiles\",\n    \"firefox\",\n    \"com.system76.CosmicTerm\",\n]\n";
        assert_eq!(
            parse(text).expect("parses"),
            [
                "com.system76.CosmicFiles.desktop",
                "firefox.desktop",
                "com.system76.CosmicTerm.desktop"
            ]
        );
    }

    #[test]
    fn an_empty_list_and_no_trailing_comma_are_lists() {
        assert_eq!(parse("[]"), Some(Vec::new()));
        assert_eq!(parse("[\"a\"]"), Some(vec!["a.desktop".to_owned()]));
    }

    #[test]
    fn bad_entries_are_skipped_and_duplicates_dropped() {
        let text = r#"["../../etc/passwd", "", "a b", "ok", "ok", "we\"ird"]"#;
        assert_eq!(parse(text), Some(vec!["ok.desktop".to_owned()]));
    }

    #[test]
    fn only_the_users_own_list_is_read() {
        let dir = std::env::temp_dir().join(format!("athanor-favs-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&dir);
        assert_eq!(cosmic_favorites_in(&dir), None);
        let list = crate::cosmic_config::component(&dir, APP_LIST);
        std::fs::create_dir_all(&list).expect("mkdir");
        std::fs::write(list.join("favorites"), "[\"firefox\"]").expect("write");
        assert_eq!(
            cosmic_favorites_in(&dir),
            Some(vec!["firefox.desktop".to_owned()])
        );
    }

    #[test]
    fn anything_but_a_list_of_strings_is_refused() {
        for text in [
            "",
            "firefox",
            "[firefox]",
            "[\"a\" \"b\"]",
            "[\"a\",,\"b\"]",
            "[\"unterminated]",
            "[\"a\\n\"]",
            "(\"a\")",
            "[\"a\"] trailing",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }
}
