//! The three layers of SH6 -- vendor < policy < user -- merged into one layout.
//!
//! The policy layer is not a security boundary: a user who can write their own files can
//! run their own panel. A mandatory key is honoured here and greyed in the chooser, and
//! that is all it is.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::document::{self, Document, DocumentError, Key, Layer};
use crate::preset::{Layout, Preset};

pub const VENDOR_DIR: &str = "/usr/share/athanor/layout";
pub const POLICY_DIR: &str = "/etc/athanor/layout";

/// Where the three layers live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub vendor_dir: PathBuf,
    pub policy_dir: PathBuf,
    pub user_file: PathBuf,
}

impl Paths {
    pub fn for_config_home(config_home: &Path) -> Paths {
        Paths {
            vendor_dir: PathBuf::from(VENDOR_DIR),
            policy_dir: PathBuf::from(POLICY_DIR),
            user_file: config_home.join("athanor/layout.toml"),
        }
    }

    pub fn from_env() -> Option<Paths> {
        config_home().map(|home| Paths::for_config_home(&home))
    }
}

/// `$XDG_CONFIG_HOME`, else `$HOME/.config`; a relative value is ignored, as the XDG
/// base directory specification says.
pub fn config_home() -> Option<PathBuf> {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_STATE_HOME`, else `$HOME/.local/state`.
pub fn state_home() -> Option<PathBuf> {
    xdg_home("XDG_STATE_HOME", ".local/state")
}

fn xdg_home(variable: &str, below_home: &str) -> Option<PathBuf> {
    let absolute = |name: &str| {
        env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute(variable).or_else(|| absolute("HOME").map(|home| home.join(below_home)))
}

/// What the user layer holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserState {
    Absent,
    Valid(Document),
    /// Rejected whole (SH8); only its preset, read leniently, still counts.
    Rejected {
        error: DocumentError,
        nearest: Option<Preset>,
    },
}

/// The layout in force and what the chooser needs to know about how it came about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub layout: Layout,
    pub mandatory: BTreeSet<Key>,
    pub user: UserState,
    pub policy_names_preset: bool,
}

/// Reads the three layers and merges them.
pub fn resolve(paths: &Paths) -> Resolved {
    let vendor = layer_dir(&paths.vendor_dir, Layer::Vendor).unwrap_or_else(builtin_vendor);
    let policy = layer_dir(&paths.policy_dir, Layer::Policy).unwrap_or_default();
    let user = read_user(&paths.user_file);
    Resolved {
        layout: choose(&vendor, &policy, &user),
        mandatory: policy.mandatory.clone(),
        policy_names_preset: policy.preset.is_some(),
        user,
    }
}

/// The vendor layer alone: what the crash-loop protection falls back to (SH8).
pub fn vendor_layout(vendor_dir: &Path) -> Layout {
    let vendor = layer_dir(vendor_dir, Layer::Vendor).unwrap_or_else(builtin_vendor);
    choose(&vendor, &Document::default(), &UserState::Absent)
}

/// The vendor document compiled in, for an image whose vendor directory is missing.
/// A test keeps it equal to `vendor/10-athanor.toml`, which the translator's RPM ships.
fn builtin_vendor() -> Document {
    Document {
        preset: Some(Preset::Float),
        ..Document::default()
    }
}

/// The `*.toml` files of one layer directory, in lexical order, laid on each other.
/// `None` when the directory holds no readable document. A bad file is logged at error
/// priority and skipped: one broken policy file does not void the others.
fn layer_dir(dir: &Path, layer: Layer) -> Option<Document> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::error!(dir = %dir.display(), error = %err, "cannot list a layout layer");
            return None;
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    let mut merged: Option<Document> = None;
    for file in files {
        let parsed = fs::read_to_string(&file)
            .map_err(|err| DocumentError::Unreadable(err.to_string()))
            .and_then(|text| document::parse(&text, layer));
        match parsed {
            Ok(doc) => merged = Some(merged.map_or(doc.clone(), |below| below.overlaid(&doc))),
            Err(err) => {
                tracing::error!(file = %file.display(), error = %err, "layout document ignored")
            }
        }
    }
    merged
}

fn read_user(file: &Path) -> UserState {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return UserState::Absent,
        Err(err) => {
            tracing::error!(file = %file.display(), error = %err, "the layout document cannot be read; the vendor layout applies");
            return UserState::Rejected {
                error: DocumentError::Unreadable(err.to_string()),
                nearest: None,
            };
        }
    };
    match document::parse(&text, Layer::User) {
        Ok(doc) => UserState::Valid(doc),
        Err(error) => {
            let nearest = document::nearest_preset(&text);
            tracing::error!(
                file = %file.display(),
                error = %error,
                nearest = nearest.map_or("none", Preset::id),
                "the layout document is rejected and left as it is; the nearest preset applies"
            );
            UserState::Rejected { error, nearest }
        }
    }
}

/// Per key: a mandatory key takes policy, then vendor; any other key takes user, then
/// policy, then vendor; a knob still unset takes the preset's factory value.
fn choose(vendor: &Document, policy: &Document, user: &UserState) -> Layout {
    let user = match user {
        UserState::Valid(doc) => doc.clone(),
        UserState::Rejected { nearest, .. } => Document {
            preset: *nearest,
            ..Document::default()
        },
        UserState::Absent => Document::default(),
    };
    let mandatory = &policy.mandatory;
    let preset = layered(
        Key::Preset,
        mandatory,
        user.preset,
        policy.preset,
        vendor.preset,
    )
    .unwrap_or(Preset::Float);
    let factory = preset.factory();
    let panel = layered(
        Key::Panel,
        mandatory,
        user.panel,
        policy.panel,
        vendor.panel,
    )
    .unwrap_or(factory.panel());
    let dock = layered(Key::Dock, mandatory, user.dock, policy.dock, vendor.dock)
        .unwrap_or(factory.dock());
    Layout::new(preset, panel, dock)
}

fn layered<T>(
    key: Key,
    mandatory: &BTreeSet<Key>,
    user: Option<T>,
    policy: Option<T>,
    vendor: Option<T>,
) -> Option<T> {
    if mandatory.contains(&key) {
        policy.or(vendor)
    } else {
        user.or(policy).or(vendor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentError;
    use crate::preset::{DockKnob, PanelEdge};
    use crate::testing::scratch;

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

    #[test]
    fn the_shipped_vendor_file_is_the_built_in_one() {
        let shipped = include_str!("../vendor/10-athanor.toml");
        assert_eq!(
            crate::document::parse(shipped, Layer::Vendor),
            Ok(builtin_vendor())
        );
    }

    #[test]
    fn with_no_file_anywhere_the_factory_island_applies() {
        let resolved = resolve(&paths("nothing"));
        assert_eq!(resolved.layout, Preset::Float.factory());
        assert_eq!(resolved.user, UserState::Absent);
        assert!(!resolved.policy_names_preset);
    }

    #[test]
    fn the_user_preset_brings_its_factory_knobs() {
        let p = paths("user-bar");
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n",
        );
        let resolved = resolve(&p);
        assert_eq!(resolved.layout, Preset::Bar.factory());
        assert!(matches!(resolved.user, UserState::Valid(_)));
    }

    #[test]
    fn a_mandatory_key_ignores_the_user_value_and_nothing_else() {
        let p = paths("mandatory");
        write(
            &p.policy_dir.join("50-site.toml"),
            "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n",
        );
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"top\"\n",
        );
        let resolved = resolve(&p);
        assert_eq!(
            resolved.layout,
            Layout::new(Preset::Minimal, PanelEdge::Bottom, DockKnob::Off)
        );
        assert_eq!(resolved.mandatory, BTreeSet::from([Key::Panel]));
    }

    #[test]
    fn a_rejected_user_document_keeps_its_preset_and_is_not_touched() {
        let p = paths("rejected");
        let text = "schema = 1\ncolour = \"red\"\n[output.\"*\"]\npreset = \"bar\"\n";
        write(&p.user_file, text);
        let resolved = resolve(&p);
        assert_eq!(resolved.layout, Preset::Bar.factory());
        assert_eq!(
            resolved.user,
            UserState::Rejected {
                error: DocumentError::UnknownKey("colour".into()),
                nearest: Some(Preset::Bar)
            }
        );
        assert_eq!(fs::read_to_string(&p.user_file).expect("read"), text);
    }

    #[test]
    fn policy_files_apply_in_lexical_order_and_a_bad_one_is_skipped() {
        let p = paths("policy-order");
        write(
            &p.policy_dir.join("20-b.toml"),
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n",
        );
        write(
            &p.policy_dir.join("10-a.toml"),
            "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n",
        );
        write(&p.policy_dir.join("30-broken.toml"), "schema = 1\n[output");
        write(&p.policy_dir.join("40-ignored.conf"), "not a document");
        let resolved = resolve(&p);
        assert_eq!(resolved.layout.preset(), Preset::Minimal);
        assert!(resolved.policy_names_preset);
    }

    #[test]
    fn a_dock_from_another_layer_is_dropped_under_the_bar() {
        let p = paths("bar-dock");
        write(
            &p.policy_dir.join("50-site.toml"),
            "schema = 1\n[output.\"*\"]\ndock = \"visible\"\n",
        );
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n",
        );
        assert_eq!(resolve(&p).layout.dock(), DockKnob::Off);
    }

    #[test]
    fn the_vendor_layout_ignores_policy_and_user() {
        let p = paths("vendor-only");
        write(
            &p.vendor_dir.join("10-athanor.toml"),
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n",
        );
        assert_eq!(vendor_layout(&p.vendor_dir), Preset::Minimal.factory());
        assert_eq!(
            vendor_layout(&p.vendor_dir.join("absent")),
            Preset::Float.factory()
        );
    }
}
