//! The command entry (LA2): `>` and a command line, run in the default terminal.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// Why the line after `>` is not run. The query stays in command mode all the same: it never
/// reaches the web entry or any other source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing after `>`.
    Empty,
    /// Longer than a row shows (`text::TITLE_CHARS`).
    TooLong,
    /// Hidden, control or zero-width characters: the row would not show what runs.
    Hidden,
    /// Quoting the shell would refuse, such as an unclosed quote.
    Quoting,
}

/// The line after `>`; `None` when the query does not start with it, which is the one test
/// of command mode: a query that starts with `>` is a command, run or refused. White space and
/// invisible format characters before it do not count, so a pasted U+FEFF or U+200E cannot
/// send a command to the web entry.
fn line(query: &str) -> Option<&str> {
    Some(query.trim_start_matches(|c: char| c.is_whitespace() || is_format(c)).strip_prefix('>')?.trim())
}

/// Unicode general category Cf (format characters), as of Unicode 16.
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

fn argv(line: &str) -> Result<Vec<String>, Refusal> {
    if line.is_empty() {
        return Err(Refusal::Empty);
    }
    if line.chars().count() > text::TITLE_CHARS {
        return Err(Refusal::TooLong);
    }
    // What the row shows is `text::line` of the line: refuse whatever that would change,
    // so the title is exactly what runs.
    if line.chars().any(|c| text::is_hidden(c) || is_zero_width(c)) {
        return Err(Refusal::Hidden);
    }
    let words = gio::glib::shell_parse_argv(line).map_err(|_| Refusal::Quoting)?;
    words.into_iter().map(|word| word.into_string().map_err(|_| Refusal::Quoting)).collect()
}

/// The words of the command after `>`, quoted as a shell would; `None` when the query
/// is not a command or the command is refused.
pub fn parse(query: &str) -> Option<Vec<String>> {
    argv(line(query)?).ok()
}

/// Invisible but not control characters: `text::line` keeps them, a command must not hold them.
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

/// `None` when the query is not a command; else the row that runs it, or why it is refused.
pub fn hit(query: &str) -> Option<Result<Hit, Refusal>> {
    let line = line(query)?;
    Some(argv(line).map(|argv| Hit {
        group: Group::Command,
        key: String::new(),
        title: line.to_owned(),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("utilities-terminal").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Command { argv },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(query: &str) -> Option<Refusal> {
        hit(query).and_then(Result::err)
    }

    #[test]
    fn a_refused_command_says_why() {
        assert_eq!(refusal(">\"unclosed"), Some(Refusal::Quoting));
        assert_eq!(refusal(">"), Some(Refusal::Empty));
        assert_eq!(refusal(" >  "), Some(Refusal::Empty));
        assert_eq!(refusal(">\u{200B}"), Some(Refusal::Hidden));
        assert_eq!(refusal("> ls\u{200B}"), Some(Refusal::Hidden));
        assert_eq!(refusal(&format!(">{}", "a".repeat(text::TITLE_CHARS + 1))), Some(Refusal::TooLong));
        assert!(hit("ls").is_none() && hit("x>").is_none(), "no prefix, no command");
    }

    #[test]
    fn format_characters_before_the_prefix_do_not_hide_it() {
        for query in ["\u{FEFF}>ls", "\u{200E}>ls", " \u{200B}\u{2060} >ls", "\u{E0001}>ls"] {
            assert_eq!(parse(query), Some(vec!["ls".to_owned()]), "{query:?}");
        }
        assert_eq!(refusal("\u{FEFF}>\"unclosed"), Some(Refusal::Quoting));
        assert!(hit("\u{FEFF}ls").is_none(), "no prefix, no command");
    }
}
