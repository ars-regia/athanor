//! The control center's decisions, without a GTK type: tested without a display.

use std::path::{Path, PathBuf};

use athanor_layout::preset::PanelEdge;
use athanor_unit::dirs::Dirs;

pub mod rfkill;
pub mod tiles;

/// The detail pages of CC5; the panel itself is the empty page id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Network,
    Bluetooth,
    Audio,
    Battery,
}

impl Page {
    pub const ALL: [Page; 4] = [
        Page::Network,
        Page::Bluetooth,
        Page::Audio,
        Page::Battery,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Page::Network => "network",
            Page::Bluetooth => "bluetooth",
            Page::Audio => "audio",
            Page::Battery => "battery",
        }
    }
}

/// The argument of `Show(page)`: empty is the panel (`Ok(None)`), a page id is that page, and
/// anything else is refused rather than shown as the panel.
pub fn parse_page(id: &str) -> Result<Option<Page>, String> {
    if id.is_empty() {
        return Ok(None);
    }
    Page::ALL
        .into_iter()
        .find(|page| page.id() == id)
        .map(Some)
        .ok_or_else(|| format!("{id:?} is not a page of the control center"))
}

/// Which end of an axis the panel sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Start,
    End,
}

/// Where the panel sits in the usable area of its output, which the bar's exclusive zone has
/// already shrunk (CC9): under the bar when it is at the top, above it when it is at the
/// bottom, and at the end edge, where the bar's button is (CC1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub vertical: Side,
    pub horizontal: Side,
}

pub fn anchor(bar: PanelEdge, rtl: bool) -> Anchor {
    Anchor {
        vertical: match bar {
            PanelEdge::Top => Side::Start,
            PanelEdge::Bottom => Side::End,
        },
        horizontal: if rtl { Side::Start } else { Side::End },
    }
}

/// The gap between the panel and the bar, and between the panel and the output's side.
pub const GAP: i32 = 8;

// ponytail: the launcher's `place::focused_output`, copied; share it through the compositor
// client when a third surface needs it.
/// The output the activated window entered first, else the first output.
pub fn focused_output(activated: Option<&[String]>, outputs: &[Option<String>]) -> usize {
    activated
        .and_then(|entered| entered.first())
        .and_then(|first| {
            outputs
                .iter()
                .position(|output| output.as_deref() == Some(first.as_str()))
        })
        .unwrap_or(0)
}

/// The write grants of the Landlock ruleset (CC2). Reads are never restricted (a GTK program
/// cannot name what it reads, `sandbox::restrict_writes`), which covers `/proc` for the
/// system page, the icon and theme directories, and the artwork files a player names; TCP is
/// denied separately, which keeps `https:` artwork out.
#[derive(Debug, PartialEq, Eq)]
pub struct Grants {
    /// Directories written below: the control center's own state, what GTK writes at start
    /// (the bar's list: its runtime directory, dconf, the cache, `/tmp`), the launch sockets,
    /// and the directories
    /// of the COSMIC theme that the dark-mode tile writes.
    pub write: Vec<PathBuf>,
    /// Device nodes opened for writing.
    pub devices: Vec<PathBuf>,
}

/// `theme_dirs` are the user's COSMIC theme directories the appearance apply function
/// writes; `rfkill` is whether `/dev/rfkill` exists, since Landlock refuses to add a rule
/// for a path that does not.
pub fn grants(dirs: &Dirs, state: &Path, theme_dirs: &[PathBuf], rfkill: bool) -> Grants {
    let mut write = vec![
        state.to_path_buf(),
        dirs.unit_runtime.clone(),
        dirs.runtime.join("dconf"),
        launch_dir(dirs),
        dirs.cache.clone(),
        PathBuf::from("/tmp"),
    ];
    write.extend_from_slice(theme_dirs);
    // /dev/rfkill is CC8's; /dev/dri is what GDK opens at start, as the bar's ruleset says.
    let mut devices = Vec::new();
    if rfkill {
        devices.push(PathBuf::from(RFKILL));
    }
    devices.push(PathBuf::from("/dev/dri"));
    Grants { write, devices }
}

/// The parent of the launch sockets of BR2.2, where `Client::launch` makes one per application:
/// the Settings button writes there.
pub fn launch_dir(dirs: &Dirs) -> PathBuf {
    dirs.runtime.join("athanor")
}

/// What the first argument of the command line asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The panel itself.
    Panel,
    /// The unit's ExecStopPost: count a failed run.
    RecordExit,
    /// The login oneshot: bind Super+C and exit, before GTK and before the bus name (CC9).
    BindShortcut,
}

pub fn parse_mode(first: Option<&str>) -> Mode {
    match first {
        Some("--record-exit") => Mode::RecordExit,
        Some("--bind-shortcut") => Mode::BindShortcut,
        _ => Mode::Panel,
    }
}

/// The kernel's rfkill device.
pub const RFKILL: &str = "/dev/rfkill";

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_unit::sandbox;
    use std::ffi::OsString;

    #[test]
    fn the_flags_select_their_mode() {
        assert_eq!(parse_mode(None), Mode::Panel);
        assert_eq!(parse_mode(Some("--bind-shortcut")), Mode::BindShortcut);
        assert_eq!(parse_mode(Some("--record-exit")), Mode::RecordExit);
        assert_eq!(parse_mode(Some("--other")), Mode::Panel);
    }

    #[test]
    fn a_page_id_is_empty_a_page_or_refused() {
        assert_eq!(parse_page(""), Ok(None));
        for page in Page::ALL {
            assert_eq!(parse_page(page.id()), Ok(Some(page)));
        }
        assert!(parse_page("Network").is_err(), "ids are lower case");
        assert!(parse_page("wifi").is_err());
        // Pages that are not built are not shown as empty ones.
        for unbuilt in ["display", "devices", "system"] {
            assert!(parse_page(unbuilt).is_err(), "{unbuilt}");
        }
        assert!(parse_page(" ").is_err());
    }

    #[test]
    fn the_panel_sits_under_a_top_bar_and_above_a_bottom_one() {
        let ltr = Anchor {
            vertical: Side::Start,
            horizontal: Side::End,
        };
        assert_eq!(anchor(PanelEdge::Top, false), ltr);
        assert_eq!(
            anchor(PanelEdge::Bottom, false),
            Anchor {
                vertical: Side::End,
                ..ltr
            }
        );
    }

    #[test]
    fn right_to_left_text_mirrors_the_panel_to_the_start_edge() {
        assert_eq!(
            anchor(PanelEdge::Top, true),
            Anchor {
                vertical: Side::Start,
                horizontal: Side::Start
            }
        );
        assert_eq!(
            anchor(PanelEdge::Bottom, true),
            Anchor {
                vertical: Side::End,
                horizontal: Side::Start
            }
        );
    }

    #[test]
    fn the_focused_output_is_the_first_one_the_activated_window_entered() {
        let outputs = [Some("DP-1".to_owned()), Some("HDMI-A-1".to_owned()), None];
        assert_eq!(focused_output(Some(&["HDMI-A-1".to_owned()]), &outputs), 1);
        assert_eq!(focused_output(None, &outputs), 0);
        assert_eq!(focused_output(Some(&["DP-9".to_owned()]), &outputs), 0);
    }

    fn dirs() -> Dirs {
        let vars = [("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "/run/user/1000")];
        Dirs::from_vars("athanor-control-center", |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
        .expect("absolute HOME and XDG_RUNTIME_DIR")
    }

    fn paths(list: &[PathBuf]) -> Vec<String> {
        list.iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
    }

    /// The theme directories under `/home/u`, named through the compositor client, which
    /// owns COSMIC's configuration names.
    fn theme_dirs() -> Vec<PathBuf> {
        athanor_compositor_client::theme::theme_dirs_in(Path::new("/home/u/.config/cosmic"))
    }

    #[test]
    fn the_ruleset_grants_the_state_the_theme_what_gtk_writes_and_two_devices() {
        let state = Path::new("/home/u/.local/state/athanor/control-center");
        let theme = theme_dirs();
        let grants = grants(&dirs(), state, &theme, true);
        let mut expected = [
            "/home/u/.local/state/athanor/control-center",
            "/run/user/1000/athanor-control-center",
            "/run/user/1000/dconf",
            "/run/user/1000/athanor",
            "/home/u/.cache",
            "/tmp",
        ]
        .map(String::from)
        .to_vec();
        expected.extend(paths(&theme));
        assert_eq!(theme.len(), 3, "the theme's mode and its two built variants");
        assert_eq!(paths(&grants.write), expected);
        assert_eq!(paths(&grants.devices), ["/dev/rfkill", "/dev/dri"]);
        assert!(
            !grants
                .write
                .iter()
                .any(|path| path.starts_with("/home/u/.config") && !theme.contains(path)),
            "no write to the configuration but the theme"
        );
    }

    /// The launch directory the ruleset grants is one the unit makes writable: under
    /// ProtectSystem=strict, /run/user is read-only but for the unit's runtime directories.
    #[test]
    fn the_unit_declares_the_launch_directory_as_a_runtime_directory() {
        let unit = include_str!("../data/athanor-control-center.service");
        let declared: Vec<&str> = unit
            .lines()
            .filter_map(|line| line.strip_prefix("RuntimeDirectory="))
            .flat_map(str::split_whitespace)
            .collect();
        let dirs = dirs();
        let launch = launch_dir(&dirs);
        let name = launch
            .strip_prefix(&dirs.runtime)
            .expect("the launch directory is under the runtime directory");
        assert!(
            declared.iter().any(|dir| Path::new(dir) == name),
            "{declared:?} lacks {name:?}"
        );
    }

    /// The theme directories the ruleset grants are ones the unit makes writable: under
    /// ProtectHome=read-only, only its configuration directories are.
    #[test]
    fn the_unit_declares_every_theme_directory_as_a_configuration_directory() {
        let unit = include_str!("../data/athanor-control-center.service");
        let declared: Vec<&str> = unit
            .lines()
            .filter_map(|line| line.strip_prefix("ConfigurationDirectory="))
            .flat_map(str::split_whitespace)
            .collect();
        for dir in athanor_compositor_client::theme::theme_dirs_in(Path::new("cosmic")) {
            assert!(
                declared.iter().any(|name| Path::new(name) == dir),
                "{declared:?} lacks {dir:?}"
            );
        }
    }

    #[test]
    fn a_machine_without_rfkill_is_granted_no_rfkill_device() {
        let state = Path::new("/home/u/.local/state/athanor/control-center");
        let grants = grants(&dirs(), state, &[], false);
        assert_eq!(paths(&grants.devices), ["/dev/dri"]);
        assert_eq!(grants.write.len(), 6, "no theme directory to grant either");
    }

    /// The ruleset the grants build is enforceable and confines a thread: a write outside the
    /// grants is refused, one inside them is not. A thread of its own, since Landlock stays.
    #[test]
    fn the_ruleset_confines_writes_to_its_grants() {
        let root = std::env::temp_dir().join(format!("athanor-cc-landlock-{}", std::process::id()));
        let (inside, outside) = (root.join("state"), root.join("elsewhere"));
        std::fs::create_dir_all(&inside).expect("test directory");
        std::fs::create_dir_all(&outside).expect("test directory");
        let result = std::thread::spawn({
            let (inside, outside) = (inside.clone(), outside.clone());
            move || {
                let grants = Grants {
                    write: vec![inside.clone()],
                    devices: Vec::new(),
                };
                let write: Vec<&Path> = grants.write.iter().map(PathBuf::as_path).collect();
                sandbox::restrict_writes(&write, &[]).map_err(|err| err.to_string())?;
                std::fs::write(inside.join("ok"), "x").map_err(|err| format!("inside: {err}"))?;
                match std::fs::write(outside.join("no"), "x") {
                    Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
                    other => Err(format!("outside: {other:?}")),
                }
            }
        })
        .join();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(result.map_err(|_| "panic".to_owned()), Ok(Ok(())));
    }
}
