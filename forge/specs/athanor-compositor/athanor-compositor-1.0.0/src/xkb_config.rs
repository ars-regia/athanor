//! The keyboard layouts and repeat of cosmic-comp's `xkb_config` (doc_own_stack.md OS5,
//! decision 27; doc_languages.md LN9), read at start and after each change, so that one file
//! gives the same keymap under both compositors. This file is the one exception to the COSMIC
//! boundary (`verify.py boundary`): it reads the record cosmic-config writes and depends on no
//! COSMIC crate. A file that is malformed, lists more than four layouts or does not compile
//! leaves the last good keymap.

use crate::state::State;
use nix::errno::Errno;
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify, WatchDescriptor};
use smithay::input::keyboard::XkbConfig;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use std::ffi::OsStr;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

/// The file, under the configuration root.
const RELATIVE: &str = "cosmic/com.system76.CosmicComp/v1/xkb_config";

/// The largest file read: a record of seven short fields is far below it.
const LIMIT: u64 = 64 * 1024;

/// LN9's bound on the layouts of one keymap.
const MAX_LAYOUTS: usize = 4;

/// The settings of one `xkb_config` record.
#[derive(Debug, PartialEq)]
pub struct Settings {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: Option<String>,
    pub repeat_delay: i32,
    pub repeat_rate: i32,
}

/// Why a file is not applied.
#[derive(Debug)]
pub enum Refused {
    /// Not the record's syntax, at this byte.
    Syntax(usize),
    Unknown(String),
    Duplicate(String),
    Missing(&'static str),
    /// An integer beyond 0 to `i32::MAX`.
    Range(&'static str),
    /// More layouts than LN9 allows.
    Layouts(usize),
    NotAFile,
    TooLarge,
    Read(io::Error),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(at) => write!(f, "not a record at byte {at}"),
            Self::Unknown(name) => write!(f, "unknown field {name}"),
            Self::Duplicate(name) => write!(f, "field {name} given twice"),
            Self::Missing(name) => write!(f, "no field {name}"),
            Self::Range(name) => write!(f, "{name} is not an integer from 0 to {}", i32::MAX),
            Self::Layouts(count) => write!(f, "{count} layouts, more than {MAX_LAYOUTS}"),
            Self::NotAFile => write!(f, "not a regular file"),
            Self::TooLarge => write!(f, "larger than {LIMIT} bytes"),
            Self::Read(error) => write!(f, "unreadable: {error}"),
        }
    }
}

/// The file: under `$XDG_CONFIG_HOME` when it is absolute, else `$HOME/.config`, as
/// cosmic-config resolves it.
pub fn path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(root.join(RELATIVE))
}

/// Reads the record cosmic-config writes (`ron::ser::to_string_pretty` of cosmic-comp-config's
/// `XkbConfig`, or the rig's one-line form): `(`, `name: value` pairs separated by commas with
/// an optional trailing one, `)`. Absent repeat fields take cosmic-comp's defaults.
pub fn parse(text: &str) -> Result<Settings, Refused> {
    let mut reader = Reader { text, at: 0 };
    reader.eat(b'(')?;
    let (mut rules, mut model, mut layout, mut variant) = (None, None, None, None);
    let (mut options, mut repeat_delay, mut repeat_rate) = (None, None, None);
    loop {
        if reader.peek() == Some(b')') {
            reader.at += 1;
            break;
        }
        let name = reader.ident()?;
        reader.eat(b':')?;
        match name {
            "rules" => once(&mut rules, reader.string()?, name)?,
            "model" => once(&mut model, reader.string()?, name)?,
            "layout" => once(&mut layout, reader.string()?, name)?,
            "variant" => once(&mut variant, reader.string()?, name)?,
            "options" => once(&mut options, reader.option()?, name)?,
            "repeat_delay" => once(&mut repeat_delay, reader.integer("repeat_delay")?, name)?,
            "repeat_rate" => once(&mut repeat_rate, reader.integer("repeat_rate")?, name)?,
            _ => return Err(Refused::Unknown(name.into())),
        }
        match reader.peek() {
            Some(b',') => reader.at += 1,
            Some(b')') => {}
            _ => return Err(Refused::Syntax(reader.at)),
        }
    }
    if reader.peek().is_some() {
        return Err(Refused::Syntax(reader.at));
    }
    let layout = layout.ok_or(Refused::Missing("layout"))?;
    let count = layout.split(',').count();
    if count > MAX_LAYOUTS {
        return Err(Refused::Layouts(count));
    }
    Ok(Settings {
        rules: rules.ok_or(Refused::Missing("rules"))?,
        model: model.ok_or(Refused::Missing("model"))?,
        layout,
        variant: variant.ok_or(Refused::Missing("variant"))?,
        options: options.flatten(),
        repeat_delay: repeat_delay.unwrap_or(600),
        repeat_rate: repeat_rate.unwrap_or(25),
    })
}

/// Fills `slot`, refusing a field given twice.
fn once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), Refused> {
    match slot.replace(value) {
        Some(_) => Err(Refused::Duplicate(name.into())),
        None => Ok(()),
    }
}

/// The record's tokens, read from byte `at`.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    /// The next byte after any whitespace, not taken.
    fn peek(&mut self) -> Option<u8> {
        let bytes = self.text.as_bytes();
        while bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> Result<(), Refused> {
        if self.peek() != Some(byte) {
            return Err(Refused::Syntax(self.at));
        }
        self.at += 1;
        Ok(())
    }

    /// A name of letters, digits and underscores.
    fn ident(&mut self) -> Result<&'a str, Refused> {
        self.peek();
        let start = self.at;
        let bytes = self.text.as_bytes();
        while bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.at += 1;
        }
        match &self.text[start..self.at] {
            "" => Err(Refused::Syntax(start)),
            name => Ok(name),
        }
    }

    /// A quoted string, whose only escapes are `\"` and `\\`.
    fn string(&mut self) -> Result<String, Refused> {
        self.eat(b'"')?;
        let bytes = self.text.as_bytes();
        let mut value = Vec::new();
        loop {
            match bytes.get(self.at) {
                Some(b'"') => break,
                Some(b'\\') => match bytes.get(self.at + 1) {
                    Some(escaped @ (b'"' | b'\\')) => {
                        value.push(*escaped);
                        self.at += 2;
                    }
                    _ => return Err(Refused::Syntax(self.at)),
                },
                Some(byte) => {
                    value.push(*byte);
                    self.at += 1;
                }
                None => return Err(Refused::Syntax(self.at)),
            }
        }
        self.at += 1;
        // Only ASCII bytes were taken out of a `str`: the rest is still UTF-8.
        String::from_utf8(value).map_err(|_| Refused::Syntax(self.at))
    }

    /// `None` or `Some("...")`.
    fn option(&mut self) -> Result<Option<String>, Refused> {
        let start = self.at;
        match self.ident()? {
            "None" => Ok(None),
            "Some" => {
                self.eat(b'(')?;
                let value = self.string()?;
                self.eat(b')')?;
                Ok(Some(value))
            }
            _ => Err(Refused::Syntax(start)),
        }
    }

    /// An integer from 0 to `i32::MAX`.
    fn integer(&mut self, name: &'static str) -> Result<i32, Refused> {
        self.peek();
        let start = self.at;
        let bytes = self.text.as_bytes();
        while bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if start == self.at {
            return Err(Refused::Syntax(start));
        }
        self.text[start..self.at]
            .parse()
            .map_err(|_| Refused::Range(name))
    }
}

/// Reads `path` without blocking: a FIFO, a device or a file larger than `LIMIT` is refused,
/// so a file system a client controls never stalls the event loop. `None` when it does not
/// exist.
fn read(path: &Path) -> Result<Option<String>, Refused> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Refused::Read(error)),
    };
    let metadata = file.metadata().map_err(Refused::Read)?;
    if !metadata.is_file() {
        return Err(Refused::NotAFile);
    }
    let mut text = String::new();
    file.take(LIMIT + 1)
        .read_to_string(&mut text)
        .map_err(Refused::Read)?;
    if text.len() as u64 > LIMIT {
        return Err(Refused::TooLarge);
    }
    Ok(Some(text))
}

/// The inotify watch of the file's directory, or of its nearest existing ancestor while the
/// directory does not exist; the compositor creates nothing.
pub struct Watch {
    inotify: Inotify,
    /// The directory that holds the file.
    dir: PathBuf,
    /// The directory watched and its descriptor.
    watched: Option<(PathBuf, WatchDescriptor)>,
}

/// What the file's own directory is watched for.
fn dir_events() -> AddWatchFlags {
    AddWatchFlags::IN_CLOSE_WRITE
        | AddWatchFlags::IN_MOVED_TO
        | AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_DELETE
        | AddWatchFlags::IN_MOVED_FROM
        | AddWatchFlags::IN_DELETE_SELF
        | AddWatchFlags::IN_MOVE_SELF
        | AddWatchFlags::IN_ONLYDIR
}

/// What an ancestor is watched for: the next component appearing, or the ancestor going.
fn ancestor_events() -> AddWatchFlags {
    AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_MOVED_TO
        | AddWatchFlags::IN_DELETE_SELF
        | AddWatchFlags::IN_MOVE_SELF
        | AddWatchFlags::IN_ONLYDIR
}

impl Watch {
    /// Watches the directory of `file`, or its nearest existing ancestor.
    pub fn new(file: &Path) -> nix::Result<Self> {
        let dir = file.parent().ok_or(Errno::EINVAL)?.to_path_buf();
        let inotify = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC)?;
        let mut watch = Self {
            inotify,
            dir,
            watched: None,
        };
        watch.arm()?;
        Ok(watch)
    }

    /// Watches the directory, or its nearest existing ancestor, in place of the last watch:
    /// the same path is watched again, since its directory may have been made anew.
    fn arm(&mut self) -> nix::Result<()> {
        let target = self
            .dir
            .ancestors()
            .find(|path| path.is_dir())
            .ok_or(Errno::ENOENT)?
            .to_path_buf();
        let last = self.watched.take();
        if let Some((_, old)) = &last {
            // A watch whose directory went is already removed: EINVAL is no error.
            match self.inotify.rm_watch(*old) {
                Ok(()) | Err(Errno::EINVAL) => {}
                Err(error) => tracing::debug!(%error, "the last xkb_config watch stays"),
            }
        }
        let events = if target == self.dir {
            dir_events()
        } else {
            ancestor_events()
        };
        let descriptor = self.inotify.add_watch(&target, events)?;
        if last.is_none_or(|(path, _)| path != target) {
            tracing::info!(watched = ?target, "the xkb_config watch moved");
        }
        self.watched = Some((target, descriptor));
        Ok(())
    }

    /// Reads every pending event; true when the file may have changed and is to be read again.
    pub fn changed(&mut self) -> bool {
        let mut reload = false;
        let mut rearm = false;
        loop {
            let events = match self.inotify.read_events() {
                Ok(events) => events,
                Err(Errno::EAGAIN) => break,
                Err(error) => {
                    tracing::warn!(%error, "the xkb_config watch could not be read");
                    break;
                }
            };
            for event in events {
                if event.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                    reload = true;
                    continue;
                }
                let Some((path, descriptor)) = &self.watched else {
                    continue;
                };
                if event.wd != *descriptor {
                    continue;
                }
                let gone = AddWatchFlags::IN_IGNORED
                    | AddWatchFlags::IN_DELETE_SELF
                    | AddWatchFlags::IN_MOVE_SELF;
                if event.mask.intersects(gone) || *path != self.dir {
                    rearm = true;
                } else if event.name.as_deref() == Some(OsStr::new("xkb_config")) {
                    reload = true;
                }
            }
        }
        if rearm {
            if let Err(error) = self.arm() {
                tracing::warn!(%error, "the xkb_config watch was lost: no live reload");
            }
            reload = true;
        }
        reload
    }
}

/// Reads the file at start and after each change of it (D10). Without a path or a watch the
/// keymap of the moment stays, and a warning says why.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let Some(path) = path() else {
        tracing::warn!("no configuration root: the keymap is the environment's");
        return;
    };
    follow(state, handle, path);
}

/// Watches `path`, then reads it, so a write between the two is not missed.
pub fn follow(state: &mut State, handle: &LoopHandle<'static, State>, path: PathBuf) {
    match watch(handle, path.clone()) {
        Ok(()) => {}
        Err(error) => tracing::warn!(%error, ?path, "xkb_config is read once: no live reload"),
    }
    state.load_xkb_config(&path);
}

fn watch(
    handle: &LoopHandle<'static, State>,
    path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut watch = Watch::new(&path)?;
    let fd = watch.inotify.as_fd().try_clone_to_owned()?;
    handle.insert_source(
        Generic::new(fd, Interest::READ, Mode::Level),
        move |_, _, state| {
            if watch.changed() {
                state.load_xkb_config(&path);
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(())
}

impl State {
    /// Reads `path` and applies it; a missing or refused file changes nothing.
    pub fn load_xkb_config(&mut self, path: &Path) {
        match read(path).and_then(|text| text.map(|text| parse(&text)).transpose()) {
            Ok(Some(settings)) => self.apply_xkb_config(&settings, path),
            Ok(None) => tracing::info!(?path, "no xkb_config: the keymap stays"),
            Err(reason) => tracing::warn!(?path, %reason, "xkb_config refused: the keymap stays"),
        }
    }

    /// Gives the seat the keymap of `settings`, keeping the seat's locked modifiers and its
    /// group, clamped to the new layouts (D12). A keymap libxkbcommon cannot compile changes
    /// nothing, a virtual keyboard's current keymap included.
    fn apply_xkb_config(&mut self, settings: &Settings, path: &Path) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let (locked, group) = match &self.virtual_keymap {
            Some(current) => (current.seat_modifiers[2], current.seat_modifiers[3]),
            None => {
                let mods = keyboard.modifier_state().serialized;
                (mods.locked, mods.layout_effective)
            }
        };
        let config = XkbConfig {
            rules: &settings.rules,
            model: &settings.model,
            layout: &settings.layout,
            variant: &settings.variant,
            options: settings.options.clone(),
        };
        if let Err(error) = keyboard.set_xkb_config(self, config) {
            tracing::warn!(
                ?error,
                ?path,
                "xkb_config does not compile: the keymap stays"
            );
            return;
        }
        // The new keymap replaced the virtual keyboard's: its saved seat keymap is stale.
        self.virtual_keymap = None;
        let layouts = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap_or_else(PoisonError::into_inner);
            u32::try_from(xkb.layouts().count()).unwrap_or(u32::MAX)
        });
        let group = group.min(layouts.saturating_sub(1));
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(0, 0, locked, group)
        });
        keyboard.change_repeat_info(settings.repeat_rate, settings.repeat_delay);
        self.repeat = (settings.repeat_rate, settings.repeat_delay);
        tracing::info!(?path, layout = %settings.layout, "keymap from xkb_config");
        self.input_method_keymap_changed();
        self.send_keyboard_group();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file cosmic-config writes, as it stands on a development machine.
    const PRETTY: &str = "(\n    rules: \"\",\n    model: \"\",\n    layout: \"us\",\n    variant: \"intl\",\n    options: None,\n    repeat_delay: 600,\n    repeat_rate: 25,\n)\n";

    /// The rig's one-line seed (`rig.sh seed_bar`).
    const ONE_LINE: &str = "(rules: \"\", model: \"pc105\", layout: \"us,it\", variant: \",\", options: None, repeat_delay: 600, repeat_rate: 25)";

    #[test]
    fn the_written_forms_are_read() {
        let pretty = parse(PRETTY).expect("pretty");
        assert_eq!(pretty.layout, "us");
        assert_eq!(pretty.variant, "intl");
        assert_eq!(pretty.options, None);
        assert_eq!((pretty.repeat_delay, pretty.repeat_rate), (600, 25));
        assert_eq!(parse(PRETTY.trim_end()).expect("no final newline"), pretty);
        let one_line = parse(ONE_LINE).expect("one line");
        assert_eq!(
            (one_line.model.as_str(), one_line.layout.as_str()),
            ("pc105", "us,it")
        );
    }

    #[test]
    fn options_and_defaults() {
        let text = r#"(rules: "", model: "", layout: "us,it", variant: ",", options: Some("grp:alt_shift_toggle"))"#;
        let settings = parse(text).expect("options");
        assert_eq!(settings.options.as_deref(), Some("grp:alt_shift_toggle"));
        assert_eq!((settings.repeat_delay, settings.repeat_rate), (600, 25));
        let escaped = parse(r#"(rules: "a\"b\\", model: "", layout: "us", variant: "")"#);
        assert_eq!(escaped.expect("escapes").rules, "a\"b\\");
    }

    #[test]
    fn malformed_records_are_refused() {
        let base = r#"rules: "", model: "", variant: """#;
        for text in [
            String::new(),
            format!("({base}, layout: \"us\""),
            format!("({base}, layout: \"us\", colour: \"red\")"),
            format!("({base}, layout: \"us\", layout: \"it\")"),
            format!("({base})"),
            format!("({base}, layout: \"u\\s\")"),
            format!("({base}, layout: \"us\", repeat_rate: -1)"),
            format!("({base}, layout: \"us\", repeat_delay: 4294967296)"),
            format!("({base}, layout: \"us,it,de,fr,es\")"),
            format!("({base}, layout: \"us\") trailing"),
        ] {
            assert!(parse(&text).is_err(), "{text:?}");
        }
        let four = format!("({base}, layout: \"us,it,de,fr\")");
        assert!(parse(&four).is_ok(), "four layouts");
    }

    #[test]
    fn every_prefix_before_the_close_is_refused() {
        let close = PRETTY.rfind(')').expect("a close");
        for end in 0..=close {
            assert!(parse(&PRETTY[..end]).is_err(), "{:?}", &PRETTY[..end]);
        }
    }

    /// A temporary directory of its own for each test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-xkb-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_fifo_and_a_large_file_are_refused_without_blocking() {
        let dir = scratch("read");
        let fifo = dir.join("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(made.success());
        assert!(matches!(read(&fifo), Err(Refused::NotAFile)));
        let large = dir.join("large");
        std::fs::write(&large, vec![b' '; LIMIT as usize + 1]).expect("large");
        assert!(matches!(read(&large), Err(Refused::TooLarge)));
        assert!(matches!(read(&dir.join("absent")), Ok(None)));
        std::fs::remove_dir_all(&dir).expect("clean");
    }

    /// Writes `text` as cosmic-config does: a rename into the directory.
    fn write(file: &Path, text: &str) {
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, text).expect("write");
        std::fs::rename(&tmp, file).expect("rename");
    }

    #[test]
    fn the_watch_follows_its_directory() {
        let root = scratch("watch");
        let file = root.join(RELATIVE);
        let dir = file.parent().expect("a directory");
        let mut watch = Watch::new(&file).expect("a watch");
        assert!(!watch.changed(), "nothing happened");
        std::fs::create_dir_all(dir).expect("the directory");
        assert!(watch.changed(), "the directory appeared");
        write(&file, PRETTY);
        assert!(watch.changed(), "the file was renamed in");
        std::fs::remove_dir_all(root.join("cosmic")).expect("removed");
        assert!(watch.changed(), "the directory went");
        std::fs::create_dir_all(dir).expect("the directory again");
        write(&file, ONE_LINE);
        assert!(watch.changed(), "the directory came back");
        write(&file, PRETTY);
        assert!(watch.changed(), "the watch is on the directory again");
        std::fs::remove_dir_all(&root).expect("clean");
    }
}
