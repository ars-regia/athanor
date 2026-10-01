//! The command entry (LA2): `>` and a command line, run in the default terminal.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// The words of the command after `>`, quoted as a shell would; `None` when the query
/// has no `>`, nothing after it, or quoting the shell would refuse.
pub fn parse(query: &str) -> Option<Vec<String>> {
    let line = query.trim_start().strip_prefix('>')?.trim();
    // What the row shows is `text::line` of the line: refuse whatever that would change,
    // so the title is exactly what runs.
    if line.is_empty()
        || line.chars().count() > text::TITLE_CHARS
        || line.chars().any(|c| text::is_hidden(c) || is_zero_width(c))
    {
        return None;
    }
    let words = gio::glib::shell_parse_argv(line).ok()?;
    words.into_iter().map(|word| word.into_string().ok()).collect()
}

/// Invisible but not control characters: `text::line` keeps them, a command must not hold them.
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

pub fn hit(query: &str) -> Option<Hit> {
    let argv = parse(query)?;
    let line = query.trim_start().trim_start_matches('>').trim();
    Some(Hit {
        group: Group::Command,
        key: String::new(),
        title: line.to_owned(),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("utilities-terminal").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Command { argv },
    })
}
