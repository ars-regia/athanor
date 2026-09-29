//! Writing the user document (doc_shell.md, SH8): only when the user changes the layout,
//! at the current schema, through a symlink if the file is one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::apply::write_atomically;
use crate::document::{Document, DocumentError};
use crate::loader::{resolve, Paths, Resolved, UserState};
use crate::preset::{DockKnob, PanelEdge, Preset};

/// One pick in the chooser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Preset(Preset),
    Panel(PanelEdge),
    Dock(DockKnob),
}

/// The user document after `change`. A preset pick writes the preset alone, so the
/// knobs return to its factory values; a knob pick keeps the rest of the document. A
/// rejected document contributes its nearest preset and nothing else.
pub fn edited(resolved: &Resolved, change: Change) -> Document {
    let mut document = match &resolved.user {
        UserState::Valid(document) => document.clone(),
        UserState::Rejected { nearest, .. } => Document {
            preset: *nearest,
            ..Document::default()
        },
        UserState::Absent => Document::default(),
    };
    match change {
        Change::Preset(preset) => {
            document = Document {
                preset: Some(preset),
                ..Document::default()
            }
        }
        Change::Panel(panel) => document.panel = Some(panel),
        Change::Dock(dock) => document.dock = Some(dock),
    }
    // The schema rejects a dock value under the bar, whichever layer chose the bar.
    if document.preset.unwrap_or(resolved.layout.preset()) == Preset::Bar {
        document.dock = None;
    }
    document
}

/// A save the chooser is about to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub document: Document,
    /// The schema of a newer user document this save would replace. The chooser asks
    /// before saving and keeps that file as `layout.toml.<schema>` (SH8).
    pub replaces_newer: Option<i64>,
}

/// Reads the layers as they are now and computes the save for `change`. The chooser
/// calls this on every pick, so an edit made by hand while it was open is built on.
pub fn prepare(paths: &Paths, change: Change) -> Pending {
    let resolved = resolve(paths);
    let replaces_newer = match &resolved.user {
        UserState::Rejected {
            error: DocumentError::NewerSchema(schema),
            ..
        } => Some(*schema),
        _ => None,
    };
    Pending {
        document: edited(&resolved, change),
        replaces_newer,
    }
}

/// The file a save writes: the user file, or what it links to, so that a document kept
/// in a dotfiles repository stays a link. A dangling link is written where it points.
pub fn write_target(user_file: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(user_file) {
        Ok(real) => Ok(real),
        Err(err) if err.kind() == io::ErrorKind::NotFound => match fs::read_link(user_file) {
            Ok(link) => Ok(user_file
                .parent()
                .map_or_else(|| link.clone(), |dir| dir.join(&link))),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
                ) =>
            {
                Ok(user_file.to_path_buf())
            }
            Err(err) => Err(err),
        },
        Err(err) => Err(err),
    }
}

/// Where a newer document of `schema` is kept: beside it, with the schema appended.
pub fn backup_path(target: &Path, schema: i64) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{schema}"));
    target.with_file_name(name)
}

/// Writes `document` as the user document. With `keep_newer`, the file in place is
/// copied to its backup path first.
pub fn save(user_file: &Path, document: &Document, keep_newer: Option<i64>) -> io::Result<()> {
    let target = write_target(user_file)?;
    if let Some(schema) = keep_newer {
        fs::copy(&target, backup_path(&target, schema))?;
    }
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(&target, &document.to_user_toml())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn a_preset_pick_writes_the_preset_alone() {
        let p = paths("preset-alone");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"float\"\npanel = \"bottom\"\ndock = \"auto-hide\"\n");
        let pending = prepare(&p, Change::Preset(Preset::Minimal));
        assert_eq!(
            pending.document,
            Document {
                preset: Some(Preset::Minimal),
                ..Document::default()
            }
        );
    }

    #[test]
    fn a_knob_pick_keeps_the_rest_of_the_document() {
        let p = paths("knob");
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"bottom\"\n",
        );
        let pending = prepare(&p, Change::Dock(DockKnob::AutoHide));
        assert_eq!(
            pending.document,
            Document {
                preset: Some(Preset::Minimal),
                panel: Some(PanelEdge::Bottom),
                dock: Some(DockKnob::AutoHide),
                ..Document::default()
            }
        );
    }

    #[test]
    fn a_hand_edit_made_while_the_chooser_is_open_is_built_on() {
        let p = paths("hand-edit");
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n",
        );
        let opened = resolve(&p);
        assert_eq!(opened.layout.preset(), Preset::Bar);
        write(
            &p.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"bottom\"\n",
        );
        let pending = prepare(&p, Change::Dock(DockKnob::Visible));
        assert_eq!(pending.document.preset, Some(Preset::Minimal));
        assert_eq!(pending.document.panel, Some(PanelEdge::Bottom));
        assert_eq!(pending.document.dock, Some(DockKnob::Visible));
    }

    #[test]
    fn no_dock_is_written_under_the_bar_whichever_layer_names_it() {
        let p = paths("bar-dock");
        write(
            &p.policy_dir.join("50-site.toml"),
            "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n",
        );
        let pending = prepare(&p, Change::Dock(DockKnob::Visible));
        assert_eq!(pending.document.dock, None);
        assert_eq!(pending.document.preset, None);
    }

    #[test]
    fn a_rejected_document_is_replaced_from_its_nearest_preset() {
        let p = paths("rejected");
        write(
            &p.user_file,
            "schema = 1\ncolour = \"red\"\n[output.\"*\"]\npreset = \"minimal\"\n",
        );
        let pending = prepare(&p, Change::Panel(PanelEdge::Bottom));
        assert_eq!(
            pending.document,
            Document {
                preset: Some(Preset::Minimal),
                panel: Some(PanelEdge::Bottom),
                ..Document::default()
            }
        );
        assert_eq!(pending.replaces_newer, None);
    }

    #[test]
    fn a_newer_document_is_named_and_kept_when_saved_over() {
        let p = paths("newer");
        let newer = "schema = 2\n[output.\"*\"]\npreset = \"bar\"\nshelf = true\n";
        write(&p.user_file, newer);
        let pending = prepare(&p, Change::Preset(Preset::Float));
        assert_eq!(pending.replaces_newer, Some(2));
        save(&p.user_file, &pending.document, pending.replaces_newer).expect("save");
        assert_eq!(
            fs::read_to_string(p.user_file.with_file_name("layout.toml.2")).expect("backup"),
            newer
        );
        assert!(matches!(resolve(&p).user, UserState::Valid(_)));
    }

    #[test]
    fn a_symlinked_document_is_written_through_the_link() {
        let p = paths("symlink");
        let real = p
            .user_file
            .parent()
            .expect("dir")
            .parent()
            .expect("config")
            .join("dotfiles/layout.toml");
        write(&real, "schema = 1\n");
        fs::create_dir_all(p.user_file.parent().expect("dir")).expect("mkdir");
        std::os::unix::fs::symlink(&real, &p.user_file).expect("link");
        let doc = Document {
            preset: Some(Preset::Bar),
            ..Document::default()
        };
        save(&p.user_file, &doc, None).expect("save");
        assert!(fs::symlink_metadata(&p.user_file)
            .expect("lstat")
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(&real).expect("target"),
            doc.to_user_toml()
        );
    }

    #[test]
    fn a_dangling_link_is_written_where_it_points() {
        let p = paths("dangling");
        let real = p.user_file.parent().expect("dir").join("elsewhere.toml");
        fs::create_dir_all(p.user_file.parent().expect("dir")).expect("mkdir");
        std::os::unix::fs::symlink("elsewhere.toml", &p.user_file).expect("link");
        save(&p.user_file, &Document::default(), None).expect("save");
        assert!(fs::symlink_metadata(&p.user_file)
            .expect("lstat")
            .file_type()
            .is_symlink());
        assert!(real.exists());
    }

    #[test]
    fn saving_creates_the_directory() {
        let p = paths("mkdir");
        save(
            &p.user_file,
            &Document {
                preset: Some(Preset::Float),
                ..Document::default()
            },
            None,
        )
        .expect("save");
        assert_eq!(resolve(&p).layout, Preset::Float.factory());
    }
}
