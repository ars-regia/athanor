//! Writing a plan into cosmic-panel's configuration (doc_shell.md, SH7): idempotent,
//! atomic per key, `entries` last.
//!
//! The render record holds the last plan written. When the new plan is the same, nothing
//! is written at all, so an edit made in COSMIC Settings stays until the layout or an
//! output's shape changes; then the plan wins, key by key. That is the "two editors" risk
//! the spec accepts: cosmic-panel's configuration is not the user's layout document.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cosmic::Plan;

/// What an `apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The files written, in the order they were written.
    pub written: Vec<PathBuf>,
    /// Whether the running panel must be restarted to show the new entries.
    pub restart_panel: bool,
}

/// Writes `plan` below `cosmic_dir`, then records it in `record`.
pub fn apply(plan: &Plan, cosmic_dir: &Path, record: &Path) -> io::Result<Applied> {
    let text = plan.to_record();
    // A record that cannot be read counts as absent: the worst case is one full rewrite.
    if fs::read_to_string(record).is_ok_and(|previous| previous == text) {
        return Ok(Applied::default());
    }
    let mut written = Vec::new();
    for entry in &plan.entries {
        let dir = cosmic_dir.join(format!("com.system76.CosmicPanel.{}/v1", entry.name));
        fs::create_dir_all(&dir)?;
        for (key, value) in &entry.keys {
            let path = dir.join(key);
            if write_if_changed(&path, value)? {
                written.push(path);
            }
        }
    }
    // Last: cosmic-panel reacts to this file, and every entry it names is complete by now.
    let entries_dir = cosmic_dir.join("com.system76.CosmicPanel/v1");
    fs::create_dir_all(&entries_dir)?;
    let entries = entries_dir.join("entries");
    let entries_changed = write_if_changed(&entries, &plan.entries_value())?;
    if entries_changed {
        written.push(entries);
    }
    if let Some(dir) = record.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(record, &text)?;
    Ok(Applied {
        written,
        restart_panel: entries_changed && plan.pins_outputs(),
    })
}

/// Writes `value` to `path` unless the file already holds the same RON value. Returns
/// whether it wrote.
///
/// cosmic-panel 1.8.0 rewrites its configuration in pretty RON when it starts, so the
/// comparison ignores layout: a byte compare would rewrite `entries` after every start and
/// restart a panel with per-output docks for nothing.
fn write_if_changed(path: &Path, value: &str) -> io::Result<bool> {
    match fs::read_to_string(path) {
        Ok(current) if compact(&current) == compact(value) => return Ok(false),
        Ok(_) => {}
        // Missing, or not text: either way the plan's value replaces it.
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::InvalidData
            ) => {}
        Err(err) => return Err(err),
    }
    write_atomically(path, value)?;
    Ok(true)
}

/// `text` without what RON ignores: whitespace outside strings, and a comma before a
/// closing bracket.
fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut quoted, mut escaped) = (false, false);
    for c in text.chars() {
        if quoted {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                out.push(c);
            }
            ']' | ')' | '}' => {
                if out.ends_with(',') {
                    out.pop();
                }
                out.push(c);
            }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

/// Replaces `path` with `text` in one step: a reader sees the old file or the new one,
/// never half of either. The temporary file lives in the same directory, so the rename
/// never crosses a filesystem.
pub fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temporary = path.with_file_name(format!(".{}.athanor-tmp", name.to_string_lossy()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmic::render;
    use crate::placement::Output;
    use crate::preset::{DockKnob, Layout, PanelEdge, Preset};
    use crate::testing::scratch;

    fn screen(connector: &str, width: i32, height: i32) -> Output {
        Output {
            connector: Some(connector.into()),
            width,
            height,
        }
    }

    fn dirs(name: &str) -> (PathBuf, PathBuf) {
        let base = scratch(name);
        (
            base.join("config/cosmic"),
            base.join("state/athanor/layout-cosmic-panel"),
        )
    }

    fn key(cosmic: &Path, entry: &str, key: &str) -> PathBuf {
        cosmic.join(format!("com.system76.CosmicPanel.{entry}/v1/{key}"))
    }

    fn relative(cosmic: &Path, written: &[PathBuf]) -> Vec<String> {
        written
            .iter()
            .map(|path| {
                path.strip_prefix(cosmic)
                    .expect("under cosmic")
                    .display()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn the_first_apply_writes_every_key_and_the_entries_last() {
        let (cosmic, record) = dirs("first");
        let applied = apply(
            &render(&Preset::Float.factory(), &[screen("HDMI-A-1", 1920, 1080)]),
            &cosmic,
            &record,
        )
        .expect("apply");
        assert_eq!(applied.written.len(), 22 * 2 + 1);
        assert_eq!(
            applied.written.last(),
            Some(&cosmic.join("com.system76.CosmicPanel/v1/entries"))
        );
        assert_eq!(
            fs::read_to_string(cosmic.join("com.system76.CosmicPanel/v1/entries"))
                .expect("entries"),
            "[\"Panel\",\"Dock\"]"
        );
        assert_eq!(
            fs::read_to_string(key(&cosmic, "Dock", "anchor")).expect("anchor"),
            "Bottom"
        );
        assert!(!applied.restart_panel);
        assert!(record.exists());
    }

    #[test]
    fn an_unchanged_plan_writes_nothing() {
        let (cosmic, record) = dirs("unchanged");
        let plan = render(&Preset::Bar.factory(), &[screen("HDMI-A-1", 1920, 1080)]);
        apply(&plan, &cosmic, &record).expect("first");
        assert!(apply(&plan, &cosmic, &record)
            .expect("second")
            .written
            .is_empty());
    }

    #[test]
    fn a_same_shape_resolution_change_writes_nothing() {
        let (cosmic, record) = dirs("resolution");
        let layout = Preset::Float.factory();
        apply(
            &render(&layout, &[screen("HDMI-A-1", 1920, 1080)]),
            &cosmic,
            &record,
        )
        .expect("first");
        let again = apply(
            &render(&layout, &[screen("HDMI-A-1", 2560, 1440)]),
            &cosmic,
            &record,
        )
        .expect("second");
        assert!(again.written.is_empty());
    }

    #[test]
    fn a_cosmic_settings_edit_survives_until_the_layout_changes() {
        let (cosmic, record) = dirs("two-editors");
        let screens = [screen("HDMI-A-1", 1920, 1080)];
        apply(
            &render(&Preset::Float.factory(), &screens),
            &cosmic,
            &record,
        )
        .expect("first");
        fs::write(key(&cosmic, "Panel", "size"), "S").expect("edit as COSMIC Settings would");
        assert!(apply(
            &render(&Preset::Float.factory(), &screens),
            &cosmic,
            &record
        )
        .expect("same")
        .written
        .is_empty());
        assert_eq!(
            fs::read_to_string(key(&cosmic, "Panel", "size")).expect("size"),
            "S"
        );

        let hidden = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide);
        let applied = apply(&render(&hidden, &screens), &cosmic, &record).expect("changed");
        assert_eq!(
            relative(&cosmic, &applied.written),
            [
                "com.system76.CosmicPanel.Panel/v1/size",
                "com.system76.CosmicPanel.Dock/v1/autohide",
                "com.system76.CosmicPanel.Dock/v1/exclusive_zone",
            ]
        );
        assert_eq!(
            fs::read_to_string(key(&cosmic, "Panel", "size")).expect("size"),
            "XS"
        );
    }

    /// Rewrites `entries` the way cosmic-panel 1.8.0 does when it starts: pretty RON.
    fn pretty_print_entries(cosmic: &Path) {
        let path = cosmic.join("com.system76.CosmicPanel/v1/entries");
        let compact = fs::read_to_string(&path).expect("entries");
        let names: Vec<&str> = compact
            .trim_matches(['[', ']'])
            .split(',')
            .map(|name| name.trim_matches('"'))
            .collect();
        let pretty: String = names
            .iter()
            .map(|name| format!("    \"{name}\",\n"))
            .collect();
        fs::write(&path, format!("[\n{pretty}]")).expect("pretty entries");
    }

    #[test]
    fn a_pretty_printed_entries_file_is_not_rewritten() {
        let (cosmic, record) = dirs("pretty-entries");
        let mixed = [screen("HDMI-A-1", 1920, 1080), screen("DP-1", 1080, 1920)];
        apply(&render(&Preset::Float.factory(), &mixed), &cosmic, &record).expect("first");
        pretty_print_entries(&cosmic);

        let hidden = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide);
        let applied = apply(&render(&hidden, &mixed), &cosmic, &record).expect("changed");
        assert!(!applied
            .written
            .contains(&cosmic.join("com.system76.CosmicPanel/v1/entries")));
        assert!(!applied.restart_panel);
    }

    #[test]
    fn a_pretty_printed_entries_file_with_other_names_is_rewritten() {
        let (cosmic, record) = dirs("pretty-other-entries");
        let screens = [screen("HDMI-A-1", 1920, 1080)];
        apply(&render(&Preset::Bar.factory(), &screens), &cosmic, &record).expect("first");
        pretty_print_entries(&cosmic);

        let applied = apply(
            &render(&Preset::Float.factory(), &screens),
            &cosmic,
            &record,
        )
        .expect("changed");
        assert_eq!(
            applied.written.last(),
            Some(&cosmic.join("com.system76.CosmicPanel/v1/entries"))
        );
    }

    #[test]
    fn compact_drops_only_what_ron_ignores() {
        assert_eq!(
            compact("[\n    \"a b\",\n    \"c,]\",\n]"),
            "[\"a b\",\"c,]\"]"
        );
        assert_eq!(compact("Some( ( 1 , 2 ) )"), "Some((1,2))");
        assert_ne!(compact("[\"Panel\"]"), compact("[\"Panel\",\"Dock\"]"));
    }

    #[test]
    fn new_per_output_docks_restart_the_panel_once() {
        let (cosmic, record) = dirs("restart");
        let layout = Preset::Float.factory();
        let shared = [screen("HDMI-A-1", 1920, 1080)];
        let mixed = [screen("HDMI-A-1", 1920, 1080), screen("DP-1", 1080, 1920)];
        assert!(
            !apply(&render(&layout, &shared), &cosmic, &record)
                .expect("shared")
                .restart_panel
        );
        assert!(
            apply(&render(&layout, &mixed), &cosmic, &record)
                .expect("mixed")
                .restart_panel
        );
        assert!(
            !apply(&render(&layout, &mixed), &cosmic, &record)
                .expect("mixed again")
                .restart_panel
        );
        assert!(
            !apply(&render(&layout, &shared), &cosmic, &record)
                .expect("back to shared")
                .restart_panel
        );
    }

    #[test]
    fn an_atomic_write_leaves_only_the_file() {
        let dir = scratch("atomic");
        let file = dir.join("entries");
        write_atomically(&file, "[\"Panel\"]").expect("write");
        write_atomically(&file, "[\"Panel\",\"Dock\"]").expect("rewrite");
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "[\"Panel\",\"Dock\"]"
        );
        let names: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names, ["entries"]);
    }
}
