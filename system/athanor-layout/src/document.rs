//! The layout document (doc_shell.md, SH6 and SH8): TOML, versioned by `schema`, one
//! wildcard output table in version 1.
//!
//! ```toml
//! schema = 1
//! mandatory = ["panel"]      # policy layer only
//! [output."*"]
//! preset = "float"
//! panel = "top"
//! dock = "visible"
//! ```

use std::collections::BTreeSet;
use std::fmt;

use toml::{Table, Value};

use crate::preset::{DockKnob, PanelEdge, Preset};

/// The schema this build reads and writes.
pub const CURRENT_SCHEMA: i64 = 1;

/// Where a document comes from. Only the policy layer may mark keys mandatory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Vendor,
    Policy,
    User,
}

/// A key of the wildcard output table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Preset,
    Panel,
    Dock,
}

impl Key {
    pub const ALL: [Key; 3] = [Key::Preset, Key::Panel, Key::Dock];

    pub fn name(self) -> &'static str {
        match self {
            Key::Preset => "preset",
            Key::Panel => "panel",
            Key::Dock => "dock",
        }
    }

    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|key| key.name() == name)
    }
}

/// One layer's document. Every field is optional: a layer says only what it sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub preset: Option<Preset>,
    pub panel: Option<PanelEdge>,
    pub dock: Option<DockKnob>,
    pub mandatory: BTreeSet<Key>,
}

/// Why a document was rejected. Any of these rejects the whole document (SH8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    /// The file exists but could not be read.
    Unreadable(String),
    Malformed(String),
    NewerSchema(i64),
    UnknownKey(String),
    InvalidValue {
        key: String,
        value: String,
    },
    /// `preset = "bar"` with a `dock` value (SH7).
    DockWithBar,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Unreadable(err) => write!(f, "the file cannot be read: {err}"),
            DocumentError::Malformed(err) => write!(f, "the document is malformed: {err}"),
            DocumentError::NewerSchema(schema) => {
                write!(
                    f,
                    "schema {schema} is newer than this build reads ({CURRENT_SCHEMA})"
                )
            }
            DocumentError::UnknownKey(key) => write!(f, "unknown key {key}"),
            DocumentError::InvalidValue { key, value } => write!(f, "{key} cannot be {value}"),
            DocumentError::DockWithBar => write!(f, "the bar preset has no dock setting"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Parses one document of `layer`. The order of the checks decides what is reported:
/// syntax, then the schema (so a newer schema is named even when it brings keys this
/// build does not know), then the migration, then the keys and values.
pub fn parse(text: &str, layer: Layer) -> Result<Document, DocumentError> {
    let mut table: Table = text
        .parse()
        .map_err(|err: toml::de::Error| DocumentError::Malformed(err.message().to_string()))?;
    let schema = match table.remove("schema") {
        Some(Value::Integer(schema)) => schema,
        Some(other) => {
            return Err(DocumentError::Malformed(format!(
                "schema is a {}, not an integer",
                other.type_str()
            )))
        }
        None => return Err(DocumentError::Malformed("there is no schema key".into())),
    };
    if schema > CURRENT_SCHEMA {
        return Err(DocumentError::NewerSchema(schema));
    }
    migrate(schema, &mut table)?;
    validate(table, layer)
}

/// Brings a table of an older shipped schema to the current one, in memory; the file is
/// never rewritten here (SH8). Each schema that ships adds one arm that rewrites its
/// table to the next schema and falls through to it. Version 1 is the first.
fn migrate(schema: i64, _table: &mut Table) -> Result<(), DocumentError> {
    match schema {
        1 => Ok(()),
        other => Err(DocumentError::Malformed(format!(
            "schema {other} was never shipped"
        ))),
    }
}

fn validate(mut table: Table, layer: Layer) -> Result<Document, DocumentError> {
    let mut doc = Document::default();
    if let Some(value) = table.remove("mandatory") {
        if layer != Layer::Policy {
            return Err(DocumentError::UnknownKey("mandatory".into()));
        }
        doc.mandatory = mandatory_keys(value)?;
    }
    if let Some(value) = table.remove("output") {
        let Value::Table(mut outputs) = value else {
            return Err(DocumentError::Malformed("output is not a table".into()));
        };
        if let Some(wildcard) = outputs.remove("*") {
            let Value::Table(mut wildcard) = wildcard else {
                return Err(DocumentError::Malformed(
                    "output.\"*\" is not a table".into(),
                ));
            };
            doc.preset = take(&mut wildcard, "preset", Preset::from_id)?;
            doc.panel = take(&mut wildcard, "panel", PanelEdge::from_id)?;
            doc.dock = take(&mut wildcard, "dock", DockKnob::from_id)?;
            if let Some(key) = wildcard.keys().next() {
                return Err(DocumentError::UnknownKey(format!("output.\"*\".{key}")));
            }
        }
        if let Some(output) = outputs.keys().next() {
            return Err(DocumentError::UnknownKey(format!("output.\"{output}\"")));
        }
    }
    if let Some(key) = table.keys().next() {
        return Err(DocumentError::UnknownKey(key.clone()));
    }
    if doc.preset == Some(Preset::Bar) && doc.dock.is_some() {
        return Err(DocumentError::DockWithBar);
    }
    Ok(doc)
}

/// Removes `key` from `table` and reads it with `from_id`.
fn take<T>(
    table: &mut Table,
    key: &str,
    from_id: fn(&str) -> Option<T>,
) -> Result<Option<T>, DocumentError> {
    let invalid = |value: String| DocumentError::InvalidValue {
        key: key.into(),
        value,
    };
    match table.remove(key) {
        None => Ok(None),
        Some(Value::String(id)) => match from_id(&id) {
            Some(value) => Ok(Some(value)),
            None => Err(invalid(id)),
        },
        Some(other) => Err(invalid(other.to_string())),
    }
}

fn mandatory_keys(value: Value) -> Result<BTreeSet<Key>, DocumentError> {
    let invalid = |value: String| DocumentError::InvalidValue {
        key: "mandatory".into(),
        value,
    };
    let items = match value {
        Value::Array(items) => items,
        other => return Err(invalid(other.to_string())),
    };
    items
        .into_iter()
        .map(|item| match item.as_str().and_then(Key::from_name) {
            Some(key) => Ok(key),
            None => Err(invalid(item.to_string())),
        })
        .collect()
}

/// The preset a rejected document names, read as leniently as possible: "the nearest
/// preset it knows" (SH8). `None` when the text is not TOML or names no known preset.
pub fn nearest_preset(text: &str) -> Option<Preset> {
    let table: Table = text.parse().ok()?;
    let preset = table.get("output")?.get("*")?.get("preset")?.as_str()?;
    Preset::from_id(preset)
}

impl Document {
    /// The document as the user file holds it, at the current schema. A user document
    /// carries no `mandatory` list.
    pub fn to_user_toml(&self) -> String {
        let mut text = format!("schema = {CURRENT_SCHEMA}\n\n[output.\"*\"]\n");
        if let Some(preset) = self.preset {
            text.push_str(&format!("preset = \"{}\"\n", preset.id()));
        }
        if let Some(panel) = self.panel {
            text.push_str(&format!("panel = \"{}\"\n", panel.id()));
        }
        if let Some(dock) = self.dock {
            text.push_str(&format!("dock = \"{}\"\n", dock.id()));
        }
        text
    }

    /// `over` laid on this document: its keys win where it sets them; the mandatory
    /// lists are joined. Used for the files of one layer, read in lexical order.
    pub fn overlaid(&self, over: &Document) -> Document {
        Document {
            preset: over.preset.or(self.preset),
            panel: over.panel.or(self.panel),
            dock: over.dock.or(self.dock),
            mandatory: self.mandatory.union(&over.mandatory).copied().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Result<Document, DocumentError> {
        parse(text, Layer::User)
    }

    #[test]
    fn a_full_user_document_parses() {
        let doc = user("schema = 1\n[output.\"*\"]\npreset = \"float\"\npanel = \"bottom\"\ndock = \"auto-hide\"\n")
            .expect("valid");
        assert_eq!(doc.preset, Some(Preset::Float));
        assert_eq!(doc.panel, Some(PanelEdge::Bottom));
        assert_eq!(doc.dock, Some(DockKnob::AutoHide));
        assert!(doc.mandatory.is_empty());
    }

    #[test]
    fn schema_alone_is_a_valid_empty_document() {
        assert_eq!(user("schema = 1\n"), Ok(Document::default()));
    }

    #[test]
    fn a_missing_or_non_integer_schema_is_malformed() {
        assert!(matches!(
            user("[output.\"*\"]\npreset = \"bar\"\n"),
            Err(DocumentError::Malformed(_))
        ));
        assert!(matches!(
            user("schema = \"1\"\n"),
            Err(DocumentError::Malformed(_))
        ));
        assert!(matches!(
            user("schema = 1\n[output"),
            Err(DocumentError::Malformed(_))
        ));
    }

    #[test]
    fn a_newer_schema_is_reported_before_any_key_it_may_have_added() {
        assert_eq!(
            user("schema = 2\naccent = \"red\"\n"),
            Err(DocumentError::NewerSchema(2))
        );
    }

    #[test]
    fn a_schema_never_shipped_is_malformed() {
        assert!(matches!(
            user("schema = 0\n"),
            Err(DocumentError::Malformed(_))
        ));
    }

    #[test]
    fn an_unknown_key_rejects_the_document() {
        assert_eq!(
            user("schema = 1\naccent = \"red\"\n"),
            Err(DocumentError::UnknownKey("accent".into()))
        );
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\npreset = \"bar\"\nsize = 3\n"),
            Err(DocumentError::UnknownKey("output.\"*\".size".into()))
        );
    }

    #[test]
    fn version_one_accepts_only_the_wildcard_output() {
        assert_eq!(
            user("schema = 1\n[output.\"HDMI-A-1\"]\npreset = \"bar\"\n"),
            Err(DocumentError::UnknownKey("output.\"HDMI-A-1\"".into()))
        );
    }

    #[test]
    fn an_unknown_value_is_invalid() {
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\ndock = \"hidden\"\n"),
            Err(DocumentError::InvalidValue {
                key: "dock".into(),
                value: "hidden".into()
            })
        );
        assert!(matches!(
            user("schema = 1\n[output.\"*\"]\npanel = 1\n"),
            Err(DocumentError::InvalidValue { .. })
        ));
    }

    #[test]
    fn the_bar_rejects_a_dock_value() {
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\npreset = \"bar\"\ndock = \"none\"\n"),
            Err(DocumentError::DockWithBar)
        );
    }

    #[test]
    fn mandatory_belongs_to_the_policy_layer_only() {
        let text = "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n";
        let policy = parse(text, Layer::Policy).expect("valid policy");
        assert_eq!(policy.mandatory, BTreeSet::from([Key::Panel]));
        assert_eq!(
            user(text),
            Err(DocumentError::UnknownKey("mandatory".into()))
        );
        assert_eq!(
            parse(text, Layer::Vendor),
            Err(DocumentError::UnknownKey("mandatory".into()))
        );
        assert!(matches!(
            parse("schema = 1\nmandatory = [\"accent\"]\n", Layer::Policy),
            Err(DocumentError::InvalidValue { .. })
        ));
    }

    #[test]
    fn the_nearest_preset_is_read_from_a_rejected_document() {
        assert_eq!(
            nearest_preset("schema = 7\nfoo = 1\n[output.\"*\"]\npreset = \"bar\"\n"),
            Some(Preset::Bar)
        );
        assert_eq!(
            nearest_preset("schema = 1\n[output.\"*\"]\npreset = \"tiles\"\n"),
            None
        );
        assert_eq!(nearest_preset("not toml ["), None);
    }

    #[test]
    fn the_user_document_round_trips() {
        let doc = Document {
            preset: Some(Preset::Minimal),
            dock: Some(DockKnob::Visible),
            ..Document::default()
        };
        let text = doc.to_user_toml();
        assert!(text.starts_with("schema = 1\n"));
        assert_eq!(user(&text), Ok(doc));
        assert_eq!(
            user(&Document::default().to_user_toml()),
            Ok(Document::default())
        );
    }

    #[test]
    fn a_later_document_wins_per_key_and_mandatory_lists_are_joined() {
        let first = Document {
            preset: Some(Preset::Bar),
            panel: Some(PanelEdge::Top),
            mandatory: BTreeSet::from([Key::Preset]),
            ..Document::default()
        };
        let second = Document {
            panel: Some(PanelEdge::Bottom),
            mandatory: BTreeSet::from([Key::Panel]),
            ..Document::default()
        };
        let merged = first.overlaid(&second);
        assert_eq!(merged.preset, Some(Preset::Bar));
        assert_eq!(merged.panel, Some(PanelEdge::Bottom));
        assert_eq!(merged.mandatory, BTreeSet::from([Key::Preset, Key::Panel]));
    }
}
