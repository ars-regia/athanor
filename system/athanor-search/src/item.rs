//! One result, whatever its source.

use crate::rank::Tier;

/// The groups of the board, in the order they are shown (LA3). `Command` is alone when
/// present: a query that starts with `>` shows nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Command,
    Apps,
    Windows,
    Calc,
    Settings,
    Files,
    Providers,
    Web,
}

impl Group {
    /// Rows shown at most; a provider's own cap is `PROVIDER_ROWS`.
    pub const ROWS: usize = 5;
    pub const PROVIDER_ROWS: usize = 3;

    /// Groups whose best row may become the top hit (LA3).
    pub fn competes_for_top(self) -> bool {
        matches!(self, Group::Apps | Group::Windows | Group::Settings | Group::Files)
    }
}

/// What Enter does (LA5).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// An application or a settings page, by desktop id, started through BR2.
    Launch { desktop_id: String },
    /// The window at this index of the snapshot the query was run against.
    Window { index: usize },
    Copy { text: String },
    /// A file, by URI, opened with its default application through BR2.
    Open { uri: String },
    Provider { bus_name: String, object_path: String, result_id: String, terms: Vec<String> },
    /// A command line, run in the default terminal through BR2.
    Command { argv: Vec<String> },
    Web { url: String },
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub group: Group,
    /// The usage key: `app:<desktop id>`, `window:<app id>`, a file's URI; empty for rows
    /// that are never recorded (calculator, providers, command, web).
    pub key: String,
    /// Plain text, already cleaned by `athanor_unit::text`.
    pub title: String,
    pub subtitle: String,
    pub icon: Option<gio::Icon>,
    pub tier: Tier,
    pub score: i64,
    /// The user completed this query with this item before.
    pub learned: bool,
    pub action: Action,
}
