//! The web entry (LA2): always last, opens the default browser on the engine below with
//! the whole query percent-encoded. The query leaves the machine only when it is chosen.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// A search engine that needs no account and keeps no search history by default.
pub const ENGINE: &str = "https://duckduckgo.com/?q=";

pub fn hit(query: &str) -> Option<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    // Everything outside RFC 3986's unreserved set is escaped, UTF-8 included.
    let escaped = gio::glib::uri_escape_string(query, None::<&str>, false);
    Some(Hit {
        group: Group::Web,
        key: String::new(),
        title: text::line(query, text::TITLE_CHARS),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("web-browser").into()),
        tier: Tier::Scattered,
        score: 0,
        learned: false,
        action: Action::Web { url: format!("{ENGINE}{escaped}") },
    })
}
