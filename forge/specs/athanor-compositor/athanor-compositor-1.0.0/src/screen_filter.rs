//! The state the own compositor shares with cosmic-comp's session for the magnifier and the
//! colour filters (doc_accessibility.md AX10, ADR-0133): the inversion and the filter of
//! cosmic-comp's `a11y_screen_filter.ron`, read at start and written at each change in the
//! format cosmic-comp writes, and the magnifier's behaviour of cosmic-comp's
//! `accessibility_zoom`, read at start and after each change of it. This file is the exception to
//! the COSMIC boundary for these features (`verify.py boundary`, doc_shell.md SH2,
//! doc_own_stack.md decision 28) and leaves with cosmic-comp's session; it writes nothing else.

use crate::state::State;
use crate::xkb_config::{once, read, Reader, Refused, Watch};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

/// The filter file, under the state root (cosmic-comp's `place_state_file`).
const FILTER: &str = "cosmic-comp/a11y_screen_filter.ron";

/// The magnifier's behaviour, under the configuration root, where cosmic-config keeps it.
const ZOOM: &str = "cosmic/com.system76.CosmicComp/v1/accessibility_zoom";

/// The inversion and the filter as cosmic-comp stores them. The filter is kept by the name
/// cosmic-comp gives it, so that one this compositor does not draw is written back unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenFilter {
    pub inverted: bool,
    pub filter: Option<String>,
}

/// How the magnified view follows the pointer: cosmic-comp's `ZoomMovement`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Movement {
    /// The view moves once the pointer reaches its edge.
    OnEdge,
    /// The pointer stays at the centre of the output while the view can move.
    Centered,
    /// The point under the pointer stays where it is on the output.
    Continuously,
}

/// What the compositor uses of cosmic-comp's `ZoomConfig`: the percentage a zoom step adds,
/// how the view follows, and whether Super with the scroll wheel zooms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub increment: u32,
    pub moves: Movement,
    pub mouse_shortcuts: bool,
}

/// cosmic-comp's defaults (`ZoomConfig::default`).
impl Default for Zoom {
    fn default() -> Self {
        Self {
            increment: 50,
            moves: Movement::Continuously,
            mouse_shortcuts: true,
        }
    }
}

/// `$XDG_STATE_HOME` when it is absolute, else `.local/state` in the home directory, which is
/// `$HOME` when set and not empty, else the password database's, as the xdg crate resolves it
/// for cosmic-comp.
fn filter_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .or_else(|| {
            // xdg's own call, which skips an empty $HOME; deprecated on older toolchains.
            #[allow(deprecated)]
            let home = std::env::home_dir()?;
            Some(home.join(".local/state"))
        })?;
    Some(root.join(FILTER))
}

/// `$XDG_CONFIG_HOME` when it is absolute, else `$HOME/.config`, as cosmic-config resolves it
/// (`xkb_config::path`).
fn zoom_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(root.join(ZOOM))
}

/// The record cosmic-comp writes (`ron::ser::to_string_pretty` of its `ScreenFilter`):
/// `inverted: bool` and an optional `color_filter`, `Some(<name>)` or `None`, in any order.
pub fn parse_filter(text: &str) -> Result<ScreenFilter, Refused> {
    let mut reader = Reader::new(text);
    reader.eat(b'(')?;
    let (mut inverted, mut filter) = (None, None);
    loop {
        if reader.peek() == Some(b')') {
            reader.eat(b')')?;
            break;
        }
        let name = reader.ident()?;
        reader.eat(b':')?;
        let start = reader.at;
        match name {
            "inverted" => once(&mut inverted, boolean(&mut reader)?, name)?,
            "color_filter" => match reader.ident()? {
                "None" => once(&mut filter, None, name)?,
                "Some" => {
                    reader.eat(b'(')?;
                    let named = reader.ident()?.to_string();
                    reader.eat(b')')?;
                    once(&mut filter, Some(named), name)?
                }
                _ => return Err(Refused::Syntax(start)),
            },
            _ => return Err(Refused::Unknown(name.into())),
        }
        separator(&mut reader)?;
    }
    end(&mut reader)?;
    Ok(ScreenFilter {
        inverted: inverted.ok_or(Refused::Missing("inverted"))?,
        filter: filter.flatten(),
    })
}

fn boolean(reader: &mut Reader<'_>) -> Result<bool, Refused> {
    let start = reader.at;
    match reader.ident()? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Refused::Syntax(start)),
    }
}

/// A comma, or the end of the record next.
fn separator(reader: &mut Reader<'_>) -> Result<(), Refused> {
    if reader.peek() == Some(b',') {
        reader.eat(b',')
    } else if reader.peek() != Some(b')') {
        Err(Refused::Syntax(reader.at))
    } else {
        Ok(())
    }
}

fn end(reader: &mut Reader<'_>) -> Result<(), Refused> {
    match reader.peek() {
        Some(_) => Err(Refused::Syntax(reader.at)),
        None => Ok(()),
    }
}

/// The text cosmic-comp writes for `filter`: its pretty RON, which leaves out a filter of
/// `None` and ends with no newline.
pub fn filter_text(filter: &ScreenFilter) -> String {
    match &filter.filter {
        Some(name) => format!(
            "(\n    inverted: {},\n    color_filter: Some({name}),\n)",
            filter.inverted
        ),
        None => format!("(\n    inverted: {},\n)", filter.inverted),
    }
}

/// Writes `filter` to `path` atomically: a temporary file in the same directory, flushed to
/// disk, then renamed over the file, so that a crash never leaves it half written. The
/// directory is made when missing, as cosmic-comp's `place_state_file` makes it.
pub fn write_filter(path: &Path, filter: &ScreenFilter) -> io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a file path",
        ));
    };
    fs::create_dir_all(dir)?;
    let mut temporary = name.to_os_string();
    temporary.push(".new");
    let temporary = dir.join(temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(filter_text(filter).as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

/// The record cosmic-config writes for `accessibility_zoom`: every field of cosmic-comp's
/// `ZoomConfig`, which has no serde default, in any order. `start_on_login` and `show_overlay`
/// are read and not used: the GNOME key starts the magnifier (AX10), and no overlay is drawn.
pub fn parse_zoom(text: &str) -> Result<Zoom, Refused> {
    let mut reader = Reader::new(text);
    reader.eat(b'(')?;
    let (mut login, mut overlay, mut increment, mut moves, mut mouse) =
        (None, None, None, None, None);
    loop {
        if reader.peek() == Some(b')') {
            reader.eat(b')')?;
            break;
        }
        let name = reader.ident()?;
        reader.eat(b':')?;
        let start = reader.at;
        match name {
            "start_on_login" => once(&mut login, boolean(&mut reader)?, name)?,
            "show_overlay" => once(&mut overlay, boolean(&mut reader)?, name)?,
            "enable_mouse_zoom_shortcuts" => once(&mut mouse, boolean(&mut reader)?, name)?,
            "increment" => {
                let value = reader.ident()?;
                if !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(Refused::Syntax(start));
                }
                // Digits alone can only be too large, and the step then ends at the highest
                // level as a large one does.
                let value = value.parse::<i32>().unwrap_or(i32::MAX);
                once(&mut increment, value.unsigned_abs(), name)?
            }
            "view_moves" => {
                let value = match reader.ident()? {
                    "OnEdge" => Movement::OnEdge,
                    "Centered" => Movement::Centered,
                    "Continuously" => Movement::Continuously,
                    _ => return Err(Refused::Syntax(start)),
                };
                once(&mut moves, value, name)?
            }
            _ => return Err(Refused::Unknown(name.into())),
        }
        separator(&mut reader)?;
    }
    end(&mut reader)?;
    login.ok_or(Refused::Missing("start_on_login"))?;
    overlay.ok_or(Refused::Missing("show_overlay"))?;
    Ok(Zoom {
        increment: increment.ok_or(Refused::Missing("increment"))?,
        moves: moves.ok_or(Refused::Missing("view_moves"))?,
        mouse_shortcuts: mouse.ok_or(Refused::Missing("enable_mouse_zoom_shortcuts"))?,
    })
}

/// Reads the filter file and the magnifier's behaviour, and follows the latter's changes. A
/// missing root leaves the filters off and unstored, or cosmic-comp's zoom defaults, logged.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    match filter_path() {
        Some(path) => state.load_screen_filter(path),
        None => tracing::warn!("no state root: the colour filter is neither read nor stored"),
    }
    let Some(path) = zoom_path() else {
        tracing::warn!("no configuration root: the magnifier keeps cosmic-comp's defaults");
        return;
    };
    // Watched first, then read, so that a write between the two is not missed.
    if let Err(error) = watch_zoom(handle, path.clone()) {
        tracing::warn!(%error, ?path, "accessibility_zoom is read once: no live reload");
    }
    state.load_zoom(&path);
}

fn watch_zoom(
    handle: &LoopHandle<'static, State>,
    path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut watch = Watch::new(&path)?;
    let fd = watch.inotify.as_fd().try_clone_to_owned()?;
    handle.insert_source(
        Generic::new(fd, Interest::READ, Mode::Level),
        move |_, _, state| {
            if watch.changed() {
                state.load_zoom(&path);
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(())
}

impl State {
    /// Reads the filter file at `path`, which each change is then written to. A file that
    /// cannot be read or parsed changes nothing on screen and is logged; it is written again
    /// only when the inversion or the filter is changed (AX10, failure behaviour).
    pub fn load_screen_filter(&mut self, path: PathBuf) {
        match read(&path).and_then(|text| text.as_deref().map(parse_filter).transpose()) {
            Ok(Some(filter)) => {
                tracing::info!(?path, ?filter, "colour filter from cosmic-comp's state");
                self.a11y.screen = filter;
            }
            Ok(None) => tracing::info!(?path, "no colour filter stored"),
            Err(reason) => {
                tracing::warn!(?path, %reason, "the colour filter file is refused: nothing changes");
            }
        }
        self.a11y.filter_path = Some(path);
    }

    /// Stores the inversion and the filter; a write that fails is logged, and the screen keeps
    /// the state the user chose.
    pub fn store_screen_filter(&self) {
        let Some(path) = &self.a11y.filter_path else {
            return;
        };
        match write_filter(path, &self.a11y.screen) {
            Ok(()) => tracing::info!(?path, filter = ?self.a11y.screen, "colour filter stored"),
            Err(error) => tracing::warn!(%error, ?path, "the colour filter could not be stored"),
        }
    }

    /// Reads the magnifier's behaviour; one that cannot be read leaves cosmic-comp's defaults,
    /// logged, and an increment of 0, which would never change the level, is 50, logged.
    pub fn load_zoom(&mut self, path: &Path) {
        let mut zoom = match read(path).and_then(|text| text.as_deref().map(parse_zoom).transpose())
        {
            Ok(Some(zoom)) => zoom,
            Ok(None) => {
                tracing::info!(?path, "no accessibility_zoom: cosmic-comp's defaults");
                Zoom::default()
            }
            Err(reason) => {
                tracing::warn!(?path, %reason, "accessibility_zoom refused: cosmic-comp's defaults");
                Zoom::default()
            }
        };
        if zoom.increment == 0 {
            tracing::warn!(
                ?path,
                "accessibility_zoom has an increment of 0: 50 is used"
            );
            zoom.increment = Zoom::default().increment;
        }
        self.a11y.zoom = zoom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What cosmic-config writes, as `ron::ser::to_string_pretty` lays it out.
    const PRETTY_ZOOM: &str = "(\n    start_on_login: false,\n    show_overlay: true,\n    \
        increment: 50,\n    view_moves: Continuously,\n    enable_mouse_zoom_shortcuts: true,\n)";

    fn filter(inverted: bool, name: Option<&str>) -> ScreenFilter {
        ScreenFilter {
            inverted,
            filter: name.map(String::from),
        }
    }

    #[test]
    fn the_written_filters_are_read() {
        assert_eq!(
            parse_filter("(\n    inverted: false,\n)").expect("off"),
            filter(false, None)
        );
        assert_eq!(
            parse_filter("(\n    inverted: true,\n)").expect("inverted"),
            filter(true, None)
        );
        assert_eq!(
            parse_filter("(\n    inverted: false,\n    color_filter: Some(Greyscale),\n)")
                .expect("filter"),
            filter(false, Some("Greyscale"))
        );
        assert_eq!(
            parse_filter("(inverted: false, color_filter: None)").expect("none"),
            filter(false, None)
        );
        assert_eq!(
            parse_filter("(color_filter: Some(Achromatopsia), inverted: true)").expect("unknown"),
            filter(true, Some("Achromatopsia"))
        );
    }

    #[test]
    fn a_filter_that_cannot_be_told_is_refused() {
        for text in [
            "",
            "(",
            "()",
            "(color_filter: None)",
            "(inverted: maybe)",
            "(inverted: false, inverted: true)",
            "(inverted: false, colour: true)",
            "(inverted: false, color_filter: Some(Greyscale)",
            "(inverted: false) trailing",
            "ScreenFilter(inverted: false)",
        ] {
            assert!(parse_filter(text).is_err(), "{text:?}");
        }
    }

    /// The text written is cosmic-comp's, byte for byte, and reads back the same.
    #[test]
    fn the_filter_is_written_as_cosmic_comp_writes_it() {
        for (state, text) in [
            (
                filter(false, Some("Greyscale")),
                "(\n    inverted: false,\n    color_filter: Some(Greyscale),\n)",
            ),
            (filter(true, None), "(\n    inverted: true,\n)"),
        ] {
            assert_eq!(filter_text(&state), text);
            assert_eq!(parse_filter(text).expect("read back"), state);
        }
    }

    /// An increment too large for a number stops at the largest, and the rest of the record
    /// still counts.
    #[test]
    fn a_huge_increment_is_the_largest() {
        for digits in ["2147483648", "4294967296", "340282366920938463463374607431768211456"] {
            let zoom = parse_zoom(&PRETTY_ZOOM.replace("50", digits)).expect("the record");
            assert_eq!(zoom.increment, i32::MAX as u32, "{digits}");
        }
    }

    #[test]
    fn the_zoom_record_is_read() {
        assert_eq!(parse_zoom(PRETTY_ZOOM).expect("defaults"), Zoom::default());
        let other = "(view_moves: OnEdge, increment: 0, enable_mouse_zoom_shortcuts: false, \
            show_overlay: false, start_on_login: true)";
        assert_eq!(
            parse_zoom(other).expect("any order"),
            Zoom {
                increment: 0,
                moves: Movement::OnEdge,
                mouse_shortcuts: false,
            }
        );
        for broken in [
            String::new(),
            "(increment: 50)".into(),
            PRETTY_ZOOM.replace("Continuously", "Sideways"),
            PRETTY_ZOOM.replace("50", "-1"),
            PRETTY_ZOOM.replace("show_overlay", "overlay"),
        ] {
            assert!(parse_zoom(&broken).is_err(), "{broken:?}");
        }
    }
}
