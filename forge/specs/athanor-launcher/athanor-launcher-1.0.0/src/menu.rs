//! The actions of a result (LA5): what Tab offers, what Ctrl+Enter and Ctrl+C reach.

use athanor_search::item::Action;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The primary action.
    Open,
    /// Open a file with this application, by desktop id.
    OpenWith(String),
    ShowInFolder,
    Copy,
}

fn local(uri: &str) -> bool {
    uri.starts_with("file://")
}

/// The fixed choices of a result. "Open with" is listed by the UI after it has read the
/// file's content type (`offers_open_with`).
pub fn choices(action: &Action) -> Vec<Choice> {
    match action {
        Action::Open { uri } if local(uri) => vec![Choice::Open, Choice::ShowInFolder, Choice::Copy],
        Action::Open { .. } | Action::Launch { .. } | Action::Window { .. } | Action::Web { .. } => {
            vec![Choice::Open, Choice::Copy]
        }
        Action::Copy { .. } => vec![Choice::Copy],
        Action::Provider { .. } | Action::Command { .. } => vec![Choice::Open],
    }
}

/// What Enter and a click do (LA5): the primary action. A calculation's is to copy its result.
pub fn primary(action: &Action) -> Choice {
    match action {
        Action::Copy { .. } => Choice::Copy,
        _ => Choice::Open,
    }
}

pub fn offers_open_with(action: &Action) -> bool {
    matches!(action, Action::Open { uri } if local(uri))
}

/// What Copy puts on the clipboard: a calculation's result, a file's path, a link, or a
/// name or title. `title` is the row's title.
pub fn copy_text(action: &Action, title: &str) -> Option<String> {
    match action {
        Action::Copy { text } => Some(text.clone()),
        Action::Open { uri } if local(uri) => glib::filename_from_uri(uri).ok().map(|(path, _)| path.display().to_string()),
        Action::Open { uri } => Some(uri.clone()),
        Action::Web { url } => Some(url.clone()),
        Action::Launch { .. } | Action::Window { .. } => Some(title.to_owned()),
        Action::Provider { .. } | Action::Command { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_offers_every_action_and_a_url_fewer() {
        let file = Action::Open { uri: "file:///home/u/a%20b.txt".into() };
        assert_eq!(choices(&file), [Choice::Open, Choice::ShowInFolder, Choice::Copy]);
        assert!(offers_open_with(&file));
        let remote = Action::Open { uri: "sftp://host/a".into() };
        assert_eq!(choices(&remote), [Choice::Open, Choice::Copy]);
        assert!(!offers_open_with(&remote));
        assert_eq!(choices(&Action::Command { argv: vec!["top".into()] }), [Choice::Open]);
    }

    #[test]
    fn enter_on_a_calculation_copies_and_on_anything_else_opens() {
        assert_eq!(primary(&Action::Copy { text: "8".into() }), Choice::Copy);
        assert_eq!(primary(&Action::Open { uri: "file:///a".into() }), Choice::Open);
        assert_eq!(primary(&Action::Launch { desktop_id: "a.desktop".into() }), Choice::Open);
    }

    #[test]
    fn copy_takes_the_useful_text() {
        let file = Action::Open { uri: "file:///home/u/a%20b.txt".into() };
        assert_eq!(copy_text(&file, "a b.txt").as_deref(), Some("/home/u/a b.txt"));
        assert_eq!(copy_text(&Action::Copy { text: "8".into() }, "8").as_deref(), Some("8"));
        assert_eq!(copy_text(&Action::Launch { desktop_id: "firefox.desktop".into() }, "Firefox").as_deref(), Some("Firefox"));
        assert_eq!(copy_text(&Action::Web { url: "https://duckduckgo.com/?q=x".into() }, "x").as_deref(), Some("https://duckduckgo.com/?q=x"));
        assert_eq!(copy_text(&Action::Command { argv: vec!["top".into()] }, "top"), None);
    }
}
