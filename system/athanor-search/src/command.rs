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
/// of command mode: a query that starts with `>` is a command, run or refused.
fn line(query: &str) -> Option<&str> {
    Some(query.trim_start().strip_prefix('>')?.trim())
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
}
