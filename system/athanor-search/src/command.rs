//! The command entry (LA2): `>` and a command line, run in the default terminal.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// The words of the command after `>`, quoted as a shell would; `None` when the query
/// has no `>`, nothing after it, or quoting the shell would refuse.
pub fn parse(query: &str) -> Option<Vec<String>> {
    let line = query.trim_start().strip_prefix('>')?.trim();
    if line.is_empty() {
        return None;
    }
    let words = gio::glib::shell_parse_argv(line).ok()?;
    words.into_iter().map(|word| word.into_string().ok()).collect()
}

pub fn hit(query: &str) -> Option<Hit> {
    let argv = parse(query)?;
    let line = query.trim_start().trim_start_matches('>').trim();
    Some(Hit {
        group: Group::Command,
        key: String::new(),
        title: text::line(line, text::TITLE_CHARS),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("utilities-terminal").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Command { argv },
    })
}
