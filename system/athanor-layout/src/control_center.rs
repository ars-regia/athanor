//! `control-center.toml` (doc_control_center.md, CC6): which tiles of the toggles grid the
//! control center shows, and in what order. Same layers as the layout document, its own
//! `schema = 1`; an error of any kind rejects the whole file and the layer below applies
//! (doc_shell.md, SH8).
//!
//! ```toml
//! schema = 1
//! [[tile]]
//! id = "dark-mode"
//! size = 2        # optional: 1 (default) or 2 cells
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use toml::{Table, Value};

use crate::loader::{self, Paths};
use crate::preset::Preset;

/// The schema of this file, its own and not the layout document's.
const SCHEMA: i64 = 1;

pub const FILE_NAME: &str = "control-center.toml";

/// A tile of the toggles grid. The set is closed; the identifiers are permanent.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TileId {
    DarkMode,
    NightLight,
    DoNotDisturb,
    KeepAwake,
    PowerProfile,
    ClipboardHistory,
    Screenshot,
    ScreenRecording,
    ColorPicker,
    RotationLock,
    OnScreenKeyboard,
    /// A third-party tile (CC12), written `ext:<descriptor id>`. Whether the descriptor is
    /// installed is the panel's question at render time; the file is not rejected for it.
    /// CC12 does not constrain a descriptor id, so it is reverse-DNS-like: 1 to 255
    /// characters of `[A-Za-z0-9._-]`.
    External(String),
}

impl TileId {
    pub const ALL: [TileId; 11] = [
        TileId::DarkMode,
        TileId::NightLight,
        TileId::DoNotDisturb,
        TileId::KeepAwake,
        TileId::PowerProfile,
        TileId::ClipboardHistory,
        TileId::Screenshot,
        TileId::ScreenRecording,
        TileId::ColorPicker,
        TileId::RotationLock,
        TileId::OnScreenKeyboard,
    ];

    pub fn id(&self) -> String {
        let fixed = match self {
            TileId::DarkMode => "dark-mode",
            TileId::NightLight => "night-light",
            TileId::DoNotDisturb => "do-not-disturb",
            TileId::KeepAwake => "keep-awake",
            TileId::PowerProfile => "power-profile",
            TileId::ClipboardHistory => "clipboard-history",
            TileId::Screenshot => "screenshot",
            TileId::ScreenRecording => "screen-recording",
            TileId::ColorPicker => "color-picker",
            TileId::RotationLock => "rotation-lock",
            TileId::OnScreenKeyboard => "on-screen-keyboard",
            TileId::External(descriptor) => return format!("ext:{descriptor}"),
        };
        fixed.to_string()
    }

    pub fn from_id(id: &str) -> Option<TileId> {
        if let Some(descriptor) = id.strip_prefix("ext:") {
            let valid = (1..=255).contains(&descriptor.len())
                && descriptor
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
            return valid.then(|| TileId::External(descriptor.to_string()));
        }
        TileId::ALL.into_iter().find(|tile| tile.id() == id)
    }
}

/// One tile of the grid: which, and how many cells wide (one or two).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub id: TileId,
    pub cells: u8,
}

/// The tiles in force, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tiles(pub Vec<Tile>);

impl Tiles {
    /// The preset's default tiles. The three presets of SH7 differ in panel and dock, not
    /// in controls, so they share one set; the screen-capture tiles wait for
    /// doc_portal.md and the hardware ones are shown by choice.
    // ponytail: one set for every preset, a match on `preset` the day one differs.
    pub fn preset_default(_preset: Preset) -> Tiles {
        Tiles(
            [
                TileId::DarkMode,
                TileId::NightLight,
                TileId::DoNotDisturb,
                TileId::KeepAwake,
                TileId::PowerProfile,
                TileId::ClipboardHistory,
            ]
            .into_iter()
            .map(|id| Tile { id, cells: 1 })
            .collect(),
        )
    }
}

/// Why a file was rejected. Any of these rejects the whole file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TilesError {
    Unreadable(String),
    Malformed(String),
    NewerSchema(i64),
    UnknownKey(String),
    UnknownTile(String),
    DuplicateTile(String),
    InvalidSize(String),
}

impl fmt::Display for TilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TilesError::Unreadable(err) => write!(f, "the file cannot be read: {err}"),
            TilesError::Malformed(err) => write!(f, "the file is malformed: {err}"),
            TilesError::NewerSchema(schema) => write!(
                f,
                "schema {schema} is newer than this build reads ({SCHEMA})"
            ),
            TilesError::UnknownKey(key) => write!(f, "unknown key {key}"),
            TilesError::UnknownTile(id) => write!(f, "unknown tile {id}"),
            TilesError::DuplicateTile(id) => write!(f, "tile {id} appears twice"),
            TilesError::InvalidSize(size) => write!(f, "a tile cannot be {size} cells wide"),
        }
    }
}

impl std::error::Error for TilesError {}

/// Parses one `control-center.toml`.
pub fn parse(text: &str) -> Result<Tiles, TilesError> {
    let mut table: Table = text
        .parse()
        .map_err(|err: toml::de::Error| TilesError::Malformed(err.message().to_string()))?;
    match table.remove("schema") {
        Some(Value::Integer(schema)) if schema > SCHEMA => {
            return Err(TilesError::NewerSchema(schema))
        }
        Some(Value::Integer(SCHEMA)) => {}
        Some(Value::Integer(other)) => {
            return Err(TilesError::Malformed(format!(
                "schema {other} was never shipped"
            )))
        }
        Some(other) => {
            return Err(TilesError::Malformed(format!(
                "schema is a {}, not an integer",
                other.type_str()
            )))
        }
        None => return Err(TilesError::Malformed("there is no schema key".into())),
    }
    let entries = match table.remove("tile") {
        None => Vec::new(),
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            return Err(TilesError::Malformed(
                "tile is not an array of tables".into(),
            ))
        }
    };
    if let Some(key) = table.keys().next() {
        return Err(TilesError::UnknownKey(key.clone()));
    }
    let mut seen = BTreeSet::new();
    let mut tiles = Vec::new();
    for entry in entries {
        let Value::Table(mut entry) = entry else {
            return Err(TilesError::Malformed(
                "tile is not an array of tables".into(),
            ));
        };
        let id = match entry.remove("id") {
            Some(Value::String(id)) => id,
            _ => return Err(TilesError::Malformed("a tile has no string id".into())),
        };
        let tile = TileId::from_id(&id).ok_or_else(|| TilesError::UnknownTile(id.clone()))?;
        if !seen.insert(tile.clone()) {
            return Err(TilesError::DuplicateTile(id));
        }
        let cells = match entry.remove("size") {
            None | Some(Value::Integer(1)) => 1,
            Some(Value::Integer(2)) => 2,
            Some(other) => return Err(TilesError::InvalidSize(other.to_string())),
        };
        if let Some(key) = entry.keys().next() {
            return Err(TilesError::UnknownKey(format!("tile.{key}")));
        }
        tiles.push(Tile { id: tile, cells });
    }
    Ok(Tiles(tiles))
}

/// What one layer holds: nothing, a valid file, or a file rejected (and logged).
fn read_layer(file: &Path) -> Option<Tiles> {
    let parsed = match fs::read_to_string(file) {
        Ok(text) => parse(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => Err(TilesError::Unreadable(err.to_string())),
    };
    match parsed {
        Ok(tiles) => Some(tiles),
        Err(error) => {
            tracing::error!(
                file = %file.display(),
                %error,
                "the control center's tiles are rejected; the layer below applies"
            );
            None
        }
    }
}

/// The tiles in force: the user's file, else the policy's, else the vendor's, else the
/// default of the preset in force. A rejected file counts as absent (SH8).
pub fn load(paths: &Paths) -> Tiles {
    let user = paths.user_file.with_file_name(FILE_NAME);
    [
        user,
        paths.policy_dir.join(FILE_NAME),
        paths.vendor_dir.join(FILE_NAME),
    ]
    .iter()
    .find_map(|file| read_layer(file))
    .unwrap_or_else(|| Tiles::preset_default(loader::resolve(paths).layout.preset()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch;
    use std::path::PathBuf;

    fn paths(name: &str) -> Paths {
        let base = scratch(name);
        Paths {
            vendor_dir: base.join("vendor"),
            policy_dir: base.join("policy"),
            user_file: base.join("config/athanor/layout.toml"),
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn user_file(paths: &Paths) -> PathBuf {
        paths.user_file.with_file_name(FILE_NAME)
    }

    fn ids(tiles: &Tiles) -> Vec<String> {
        tiles.0.iter().map(|tile| tile.id.id()).collect()
    }

    #[test]
    fn a_valid_file_gives_its_tiles_in_its_order() {
        let paths = paths("cc-valid");
        write(
            &user_file(&paths),
            "schema = 1\n[[tile]]\nid = \"power-profile\"\nsize = 2\n[[tile]]\nid = \"dark-mode\"\n",
        );
        let tiles = load(&paths);
        assert_eq!(ids(&tiles), ["power-profile", "dark-mode"]);
        assert_eq!(tiles.0[0].cells, 2);
        assert_eq!(tiles.0[1].cells, 1);
    }

    #[test]
    fn an_unknown_tile_rejects_the_file_and_the_lower_layer_applies() {
        let paths = paths("cc-unknown");
        write(
            &paths.policy_dir.join(FILE_NAME),
            "schema = 1\n[[tile]]\nid = \"keep-awake\"\n",
        );
        write(
            &user_file(&paths),
            "schema = 1\n[[tile]]\nid = \"dark-mode\"\n[[tile]]\nid = \"teleport\"\n",
        );
        assert_eq!(
            parse("schema = 1\n[[tile]]\nid = \"teleport\"\n"),
            Err(TilesError::UnknownTile("teleport".into()))
        );
        assert_eq!(ids(&load(&paths)), ["keep-awake"]);
    }

    #[test]
    fn a_wrong_schema_is_rejected_and_the_preset_default_applies() {
        let paths = paths("cc-schema");
        write(
            &user_file(&paths),
            "schema = 2\n[[tile]]\nid = \"dark-mode\"\n",
        );
        assert_eq!(parse("schema = 2\n"), Err(TilesError::NewerSchema(2)));
        assert_eq!(load(&paths), Tiles::preset_default(Preset::Float));
    }

    #[test]
    fn a_third_party_tile_keeps_its_position_and_a_bad_one_rejects_the_file() {
        let file = |id: &str| {
            format!("schema = 1\n[[tile]]\nid = \"dark-mode\"\n[[tile]]\nid = \"{id}\"\n")
        };
        let tiles = parse(&file("ext:com.tailscale.tile-1")).expect("valid");
        assert_eq!(ids(&tiles), ["dark-mode", "ext:com.tailscale.tile-1"]);
        for bad in [
            "ext:",
            "ext:a b",
            "ext:a/b",
            "ext:\u{e9}",
            "ext",
            "tailscale",
        ] {
            assert!(parse(&file(bad)).is_err(), "{bad} must be rejected");
        }
        assert!(parse(&file(&format!("ext:{}", "a".repeat(256)))).is_err());
        let dup = "schema = 1\n[[tile]]\nid = \"ext:x\"\n[[tile]]\nid = \"ext:x\"\n";
        assert!(matches!(parse(dup), Err(TilesError::DuplicateTile(_))));
    }

    #[test]
    fn duplicates_sizes_and_stray_keys_reject_the_file() {
        let dup = "schema = 1\n[[tile]]\nid = \"dark-mode\"\n[[tile]]\nid = \"dark-mode\"\n";
        assert!(matches!(parse(dup), Err(TilesError::DuplicateTile(_))));
        let size = "schema = 1\n[[tile]]\nid = \"dark-mode\"\nsize = 3\n";
        assert!(matches!(parse(size), Err(TilesError::InvalidSize(_))));
        assert!(matches!(
            parse("schema = 1\nextra = 1\n"),
            Err(TilesError::UnknownKey(_))
        ));
        assert!(matches!(parse("tile = []"), Err(TilesError::Malformed(_))));
    }
}
