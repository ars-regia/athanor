//! The user's window settings, `$XDG_CONFIG_HOME/athanor/sulphur/windows.toml` (doc_own_stack.md
//! decision 38, SE-Q5; doc_settings.md SE14), read at start and after each change through
//! decision 27's watch. Of SE14's keys the file holds those the compositor applies: the active
//! window's hint, `active_hint`, which shows the focus outline of decision 31. A key it does not
//! apply yet is absent, and refused as unknown. A key that is absent takes the compiled default;
//! a file refused whole changes nothing, a key refused is skipped alone, and both are logged.

use crate::input_settings::{follow_file, table};
use crate::state::State;
use crate::xkb_config::{read, root};
use smithay::reexports::calloop::LoopHandle;
use std::path::{Path, PathBuf};
use toml::Value;

/// The file, under the configuration root.
const RELATIVE: &str = "athanor/sulphur/windows.toml";

/// The settings of one `windows.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSettings {
    /// The focus outline on the active window; cosmic-comp's default, on.
    pub active_hint: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        WindowSettings { active_hint: true }
    }
}

/// Reads one `windows.toml`, with what it skipped, each with its reason.
pub fn parse(text: &str) -> Result<(WindowSettings, Vec<String>), String> {
    let mut settings = WindowSettings::default();
    let mut skipped = Vec::new();
    for (key, value) in table(text)? {
        match (key.as_str(), value) {
            ("active_hint", Value::Boolean(on)) => settings.active_hint = on,
            ("active_hint", other) => skipped.push(format!(
                "active_hint: a {}, not a boolean",
                other.type_str()
            )),
            (other, _) => skipped.push(format!("{other}: unknown key")),
        }
    }
    Ok((settings, skipped))
}

/// Reads the file at start and after each change of it. Without a path or a watch the defaults
/// or the first read stay.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let Some(root) = root() else {
        tracing::warn!("no configuration root: the window settings keep their defaults");
        return;
    };
    follow(state, handle, root.join(RELATIVE));
}

/// Watches `path`, then reads it, so a write between the two is not missed.
pub fn follow(state: &mut State, handle: &LoopHandle<'static, State>, path: PathBuf) {
    follow_file(state, handle, path, State::load_window_settings);
}

impl State {
    /// Reads `path` and applies it. An absent file gives the defaults; a refused one changes
    /// nothing.
    pub fn load_window_settings(&mut self, path: &Path) {
        let read = read(path).map_err(|reason| reason.to_string());
        let settings = match read.and_then(|text| text.map(|text| parse(&text)).transpose()) {
            Ok(Some((settings, skipped))) => {
                for reason in skipped {
                    tracing::warn!(?path, %reason, "windows.toml: a key is skipped");
                }
                settings
            }
            Ok(None) => {
                tracing::info!(?path, "no windows.toml: the defaults apply");
                WindowSettings::default()
            }
            Err(reason) => {
                tracing::warn!(?path, %reason, "windows.toml refused: the settings stay");
                return;
            }
        };
        self.focus_theme.hint = settings.active_hint;
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_is_read_and_defaults_to_on() {
        let (settings, skipped) = parse("schema = 1\n").expect("read");
        assert!(skipped.is_empty());
        assert!(settings.active_hint);
        let (settings, _) = parse("schema = 1\nactive_hint = false\n").expect("read");
        assert!(!settings.active_hint);
    }

    /// A key Sulphur does not apply is unknown, not read and ignored.
    #[test]
    fn a_refused_key_is_skipped_alone() {
        let (settings, skipped) = parse(
            "schema = 1\nactive_hint = 0\nedge_snap_threshold = 8\nworkspace_wraparound = true\n",
        )
        .expect("read");
        assert!(settings.active_hint);
        assert_eq!(skipped.len(), 3, "{skipped:?}");
    }

    #[test]
    fn a_file_without_its_schema_is_refused() {
        assert!(parse("active_hint = false\n").is_err());
        assert!(parse("schema = 1\nactive_hint = \n").is_err());
    }
}
