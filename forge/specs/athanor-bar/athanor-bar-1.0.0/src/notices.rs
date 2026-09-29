//! Notifications as the bar receives them from athanor-shelld's private interface
//! (doc_bar.md BR1, BR4). The daemon already cleaned and bounded them; it is a peer all the
//! same, so the text is cleaned and the pictures are bounded again here, and nothing here
//! can panic on what the peer sends (BR9, "Without a display").

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use athanor_unit::text::{self, BODY_CHARS, NAME_CHARS, SUMMARY_CHARS};

/// One notification on the private interface, as `athanor-shelld`'s `wire.rs` sends it.
/// The bar does not link that crate (zbus, tokio): this literal and
/// `the_sixteen_fields_keep_their_order` below, which athanor-shelld's `wire.rs` mirrors
/// with the same table of values, pin the field order.
pub const WIRE_SIGNATURE: &str = "(usssa(ss)ybbsssuuayuu)";

/// id, app_name, summary, body, actions (key, label), urgency, transient, resident,
/// desktop_entry, icon_name, icon_file, image_width, image_height, image_rgba (straight
/// RGBA), timeout_ms, popup_ms_left.
pub type Wire = (
    u32,
    String,
    String,
    String,
    Vec<(String, String)>,
    u8,
    bool,
    bool,
    String,
    String,
    String,
    u32,
    u32,
    Vec<u8>,
    u32,
    u32,
);

/// `popup_ms_left` of a popup that shows until the user closes it.
pub const WAITS: u32 = u32::MAX;
/// The daemon's own bound on the notifications it holds.
pub const CAPACITY: usize = 100;
pub const MAX_ACTIONS: usize = 8;
const MAX_PIXELS_SIDE: u32 = 96;
const MAX_ACTION_KEY: usize = 64;
const MAX_PATH: usize = 4096;
const MAX_ICON_NAME: usize = 128;
const MAX_DESKTOP_ENTRY: usize = 255;
const ICON_FILE_BYTES: u64 = 1024 * 1024;
const ICON_FILE_SIDE: u32 = 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    fn from_byte(byte: u8) -> Urgency {
        match byte {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// What the card shows beside the text, in the order of preference of the specification:
/// the image data, then an image file, then an icon name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    None,
    Name(String),
    File(String),
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Goes back to the daemon exactly as it came.
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// The buttons, without "default".
    pub actions: Vec<Action>,
    /// The application offers a "default" action: a click on the text invokes it.
    pub has_default: bool,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    pub picture: Picture,
    /// `WAITS` until the user closes it, 0 for the list only, else the time left.
    pub popup_ms_left: u32,
}

impl Notice {
    /// `None` for a value of another type or the id 0, which the specification never
    /// gives. Every string is cleaned and bounded; actions and pictures that break a
    /// bound are dropped, not the notification.
    #[must_use]
    pub fn decode(value: &glib::Variant) -> Option<Notice> {
        let (
            id,
            app_name,
            summary,
            body,
            actions,
            urgency,
            transient,
            resident,
            desktop_entry,
            icon_name,
            icon_file,
            image_width,
            image_height,
            image_rgba,
            _timeout_ms,
            popup_ms_left,
        ) = value.get::<Wire>()?;
        if id == 0 {
            return None;
        }
        let mut has_default = false;
        let mut kept = Vec::new();
        for (key, label) in actions {
            if !is_action_key(&key) {
                continue;
            }
            if key == "default" {
                has_default = true;
                continue;
            }
            let label = text::line(&label, NAME_CHARS);
            if !label.is_empty() && kept.len() < MAX_ACTIONS {
                kept.push(Action { key, label });
            }
        }
        Some(Notice {
            id,
            app_name: text::line(&app_name, NAME_CHARS),
            summary: text::line(&summary, SUMMARY_CHARS),
            body: text::lines(&body, BODY_CHARS),
            actions: kept,
            has_default,
            urgency: Urgency::from_byte(urgency),
            transient,
            resident,
            desktop_entry: Some(desktop_entry).filter(|entry| is_desktop_entry(entry)),
            picture: picture(image_width, image_height, image_rgba, icon_file, icon_name),
            popup_ms_left,
        })
    }

    #[must_use]
    pub fn critical(&self) -> bool {
        self.urgency == Urgency::Critical
    }

    /// The list groups by application (BR4): the desktop entry when there is one, else the
    /// name the application gave.
    #[must_use]
    pub fn group_key(&self) -> &str {
        self.desktop_entry.as_deref().unwrap_or(&self.app_name)
    }
}

fn picture(width: u32, height: u32, rgba: Vec<u8>, file: String, name: String) -> Picture {
    let side = 1..=MAX_PIXELS_SIDE;
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok());
    if side.contains(&width) && side.contains(&height) && expected == Some(rgba.len()) {
        return Picture::Pixels {
            width,
            height,
            rgba,
        };
    }
    if is_icon_file(&file) {
        return Picture::File(file);
    }
    if is_icon_name(&name) {
        return Picture::Name(name);
    }
    Picture::None
}

fn is_action_key(key: &str) -> bool {
    (1..=MAX_ACTION_KEY).contains(&key.len()) && !key.chars().any(text::is_hidden)
}

fn is_desktop_entry(entry: &str) -> bool {
    (1..=MAX_DESKTOP_ENTRY).contains(&entry.len())
        && !entry.starts_with('.')
        && entry
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A themed icon name: no path, no leading dot, the characters icon themes use.
#[must_use]
pub fn is_icon_name(name: &str) -> bool {
    (1..=MAX_ICON_NAME).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

/// An absolute path with no `..` and no control character: the only files a card reads.
#[must_use]
pub fn is_icon_file(path: &str) -> bool {
    let as_path = Path::new(path);
    path.len() <= MAX_PATH
        && as_path.is_absolute()
        && !path.chars().any(char::is_control)
        && as_path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

/// The bytes of a picture file, only when it is a regular file of at most 1 MiB holding a
/// PNG whose header declares at most 1024 × 1024 (ruling 8). Links are resolved and the
/// type is read before the open, so a FIFO or a device node (opening some has side
/// effects) is never opened. The open refuses a link swapped in since, and cannot block or
/// take a terminal; the open file is checked again. `None` means "show the generic icon";
/// the reason is not worth a log line per redraw.
#[must_use]
pub fn read_icon_file(path: &str) -> Option<Vec<u8>> {
    if !is_icon_file(path) {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    if !std::fs::metadata(&real).ok()?.is_file() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_NOFOLLOW)
        .open(&real)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > ICON_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`: read one byte past the bound to see it.
    file.take(ICON_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let within = u64::try_from(bytes.len()).is_ok_and(|len| len <= ICON_FILE_BYTES);
    (within && png_within(&bytes, ICON_FILE_SIDE)).then_some(bytes)
}

/// A PNG signature, then the IHDR chunk, whose width and height are big-endian at bytes
/// 16 and 20, each within `1..=max_side`. GDK decodes only what passes.
fn png_within(bytes: &[u8], max_side: u32) -> bool {
    let side = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|four| <[u8; 4]>::try_from(four).ok())
            .map(u32::from_be_bytes)
    };
    bytes.starts_with(PNG_SIGNATURE)
        && bytes.get(12..16) == Some(b"IHDR".as_slice())
        && [side(16), side(20)]
            .iter()
            .all(|value| value.is_some_and(|value| (1..=max_side).contains(&value)))
}

/// The list's groups (BR4): the group of the newest notification first, and the newest
/// first inside each group. `notices` is oldest first, as `List` sends it.
#[must_use]
pub fn groups(notices: &[Notice]) -> Vec<Vec<&Notice>> {
    let mut groups: Vec<Vec<&Notice>> = Vec::new();
    for notice in notices.iter().rev() {
        let key = notice.group_key();
        match groups
            .iter_mut()
            .find(|group| group.first().is_some_and(|first| first.group_key() == key))
        {
            Some(group) => group.push(notice),
            None => groups.push(vec![notice]),
        }
    }
    groups
}

/// The notifications the bar holds, oldest first, as the daemon does.
#[derive(Debug, Default)]
pub struct Held {
    notices: Vec<Notice>,
}

impl Held {
    /// The list `List` returned; beyond `CAPACITY` the oldest are dropped.
    pub fn replace_all(&mut self, notices: Vec<Notice>) {
        let mut notices = notices;
        let excess = notices.len().saturating_sub(CAPACITY);
        self.notices = notices.split_off(excess);
    }

    /// A new or replaced notification becomes the newest. Returns the ids pushed out.
    pub fn arrived(&mut self, notice: Notice) -> Vec<u32> {
        self.notices.retain(|held| held.id != notice.id);
        self.notices.push(notice);
        let excess = self.notices.len().saturating_sub(CAPACITY);
        self.notices.drain(..excess).map(|old| old.id).collect()
    }

    pub fn closed(&mut self, id: u32) -> bool {
        let before = self.notices.len();
        self.notices.retain(|held| held.id != id);
        self.notices.len() != before
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Notice> {
        self.notices.iter().find(|held| held.id == id)
    }

    #[must_use]
    pub fn all(&self) -> &[Notice] {
        &self.notices
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use glib::prelude::*;

    use super::*;

    fn wire(id: u32) -> Wire {
        (
            id,
            "Files".into(),
            "Copied".into(),
            "Two files".into(),
            Vec::new(),
            1,
            false,
            false,
            String::new(),
            String::new(),
            String::new(),
            0,
            0,
            Vec::new(),
            5000,
            5000,
        )
    }

    fn decode(wire: Wire) -> Notice {
        Notice::decode(&wire.to_variant()).expect("a valid wire value")
    }

    fn notice(id: u32, app: &str) -> Notice {
        let mut wire = wire(id);
        wire.1 = app.into();
        decode(wire)
    }

    #[test]
    fn the_wire_type_is_the_daemons() {
        let value = wire(7).to_variant();
        assert_eq!(value.type_().as_str(), WIRE_SIGNATURE);
        assert_eq!(Notice::decode(&value).map(|notice| notice.id), Some(7));
    }

    /// The same table of values as athanor-shelld's `wire.rs` test of this name: each field
    /// lands in its own place, so a swap of two fields of one type fails here or there.
    #[test]
    fn the_sixteen_fields_keep_their_order() {
        let table: Wire = (
            1,
            "app".into(),
            "summary".into(),
            "body".into(),
            vec![("key".into(), "label".into())],
            2,
            true,
            false,
            "entry".into(),
            "name".into(),
            "/file".into(),
            3,
            4,
            vec![5; 48],
            6,
            7,
        );
        let notice = decode(table.clone());
        assert_eq!(
            (
                notice.id,
                notice.app_name.as_str(),
                notice.summary.as_str(),
                notice.body.as_str()
            ),
            (1, "app", "summary", "body")
        );
        assert_eq!(
            notice.actions,
            vec![Action {
                key: "key".into(),
                label: "label".into()
            }]
        );
        assert_eq!(
            (notice.urgency, notice.transient, notice.resident),
            (Urgency::Critical, true, false)
        );
        assert_eq!(notice.desktop_entry.as_deref(), Some("entry"));
        assert_eq!(
            notice.picture,
            Picture::Pixels {
                width: 3,
                height: 4,
                rgba: vec![5; 48]
            }
        );
        // timeout_ms (6) is not kept: a swap with popup_ms_left shows here as 6.
        assert_eq!(notice.popup_ms_left, 7);
        let mut table = table;
        table.13 = Vec::new();
        assert_eq!(decode(table.clone()).picture, Picture::File("/file".into()));
        table.10 = String::new();
        assert_eq!(decode(table).picture, Picture::Name("name".into()));
    }

    #[test]
    fn a_wrong_type_or_the_id_zero_is_refused() {
        assert!(Notice::decode(&(1u32, "x").to_variant()).is_none());
        assert!(Notice::decode(&wire(0).to_variant()).is_none());
    }

    #[test]
    fn text_is_cleaned_and_bounded() {
        let mut wire = wire(1);
        wire.1 = "Files\u{202E}\n".into();
        wire.2 = "s".repeat(1000);
        wire.3 = "a\u{0007}b\nc<b>d</b>".into();
        let notice = decode(wire);
        assert_eq!(notice.app_name, "Files");
        assert_eq!(notice.summary.chars().count(), SUMMARY_CHARS);
        assert_eq!(notice.body, "ab\nc<b>d</b>", "markup stays text");
    }

    #[test]
    fn actions_are_bounded_and_default_is_not_a_button() {
        let mut wire = wire(1);
        wire.4 = vec![
            ("default".into(), "Open".into()),
            (String::new(), "Empty key".into()),
            ("k\n".into(), "Control in key".into()),
            ("x".repeat(65), "Long key".into()),
            ("blank".into(), "\u{202E}".into()),
        ];
        wire.4
            .extend((0..20).map(|n| (format!("a{n}"), format!("Action {n}"))));
        let notice = decode(wire);
        assert!(notice.has_default);
        assert_eq!(notice.actions.len(), MAX_ACTIONS);
        assert_eq!(notice.actions.first().map(|a| a.key.as_str()), Some("a0"));
    }

    #[test]
    fn pixels_win_only_when_their_size_matches() {
        let mut wire = wire(1);
        wire.9 = "folder".into();
        wire.11 = 2;
        wire.12 = 2;
        wire.13 = vec![0; 16];
        assert!(matches!(
            decode(wire.clone()).picture,
            Picture::Pixels {
                width: 2,
                height: 2,
                ..
            }
        ));
        wire.13 = vec![0; 15];
        assert_eq!(decode(wire.clone()).picture, Picture::Name("folder".into()));
        wire.11 = 97;
        wire.12 = 1;
        wire.13 = vec![0; 97 * 4];
        assert_eq!(decode(wire.clone()).picture, Picture::Name("folder".into()));
        wire.11 = u32::MAX;
        wire.12 = u32::MAX;
        assert_eq!(decode(wire).picture, Picture::Name("folder".into()));
    }

    #[test]
    fn a_file_comes_before_a_name() {
        let mut wire = wire(1);
        wire.9 = "folder".into();
        wire.10 = "/usr/share/pixmaps/a.png".into();
        assert_eq!(
            decode(wire).picture,
            Picture::File("/usr/share/pixmaps/a.png".into())
        );
    }

    #[test]
    fn only_absolute_clean_paths_are_files() {
        assert!(is_icon_file("/usr/share/icons/a.png"));
        let long = format!("/{}", "a".repeat(MAX_PATH));
        for bad in [
            "",
            "a.png",
            "/usr/../etc/shadow",
            "/usr/a\n.png",
            long.as_str(),
        ] {
            assert!(!is_icon_file(bad), "{bad:?}");
        }
    }

    #[test]
    fn icon_names_and_desktop_entries_are_checked() {
        assert!(is_icon_name("mail-unread-symbolic"));
        let long = "x".repeat(129);
        for bad in ["", ".hidden", "a/b", "a b", long.as_str()] {
            assert!(!is_icon_name(bad), "{bad:?}");
        }
        let mut wire = wire(1);
        wire.8 = "org.gnome.Nautilus".into();
        assert_eq!(
            decode(wire.clone()).desktop_entry.as_deref(),
            Some("org.gnome.Nautilus")
        );
        wire.8 = "../x".into();
        assert_eq!(decode(wire).desktop_entry, None);
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("athanor-bar-notices-{}-{name}", std::process::id()))
    }

    fn read_written(name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        let path = scratch(name);
        fs::write(&path, bytes).unwrap();
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        read
    }

    #[test]
    fn a_png_within_the_bounds_is_read() {
        assert_eq!(read_written("ok.png", &png(48, 48)), Some(png(48, 48)));
    }

    #[test]
    fn a_png_over_the_side_bound_is_refused() {
        assert_eq!(read_written("wide.png", &png(60_000, 60_000)), None);
        assert_eq!(read_written("zero.png", &png(0, 16)), None);
    }

    #[test]
    fn a_file_that_is_not_a_png_is_refused() {
        assert_eq!(read_written("text.png", b"hello"), None);
        assert_eq!(read_written("empty.png", b""), None);
    }

    #[test]
    fn a_file_over_the_byte_bound_is_refused() {
        let mut bytes = png(8, 8);
        bytes.resize(usize::try_from(ICON_FILE_BYTES).unwrap() + 1, 0);
        assert_eq!(read_written("big.png", &bytes), None);
    }

    #[test]
    fn a_fifo_is_not_read() {
        let path = scratch("fifo");
        assert!(Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(
            read, None,
            "and the call returned: nothing blocked on the FIFO"
        );
    }

    #[test]
    fn a_device_is_not_read() {
        assert_eq!(read_icon_file("/dev/zero"), None);
    }

    #[test]
    fn groups_put_the_newest_first() {
        let list = [notice(1, "Files"), notice(2, "Mail"), notice(3, "Files")];
        let ids: Vec<Vec<u32>> = groups(&list)
            .iter()
            .map(|group| group.iter().map(|notice| notice.id).collect())
            .collect();
        assert_eq!(ids, [vec![3, 1], vec![2]]);
    }

    #[test]
    fn the_desktop_entry_groups_before_the_name() {
        let mut first = wire(1);
        first.1 = "Files".into();
        first.8 = "org.gnome.Nautilus".into();
        let mut second = wire(2);
        second.1 = "Nautilus".into();
        second.8 = "org.gnome.Nautilus".into();
        let list = [decode(first), decode(second)];
        assert_eq!(groups(&list).len(), 1);
    }

    #[test]
    fn arrived_moves_a_replaced_notice_last() {
        let mut held = Held::default();
        held.replace_all(vec![notice(1, "Files"), notice(2, "Mail")]);
        assert!(held.arrived(notice(1, "Files")).is_empty());
        let ids: Vec<u32> = held.all().iter().map(|notice| notice.id).collect();
        assert_eq!(ids, [2, 1]);
    }

    #[test]
    fn the_hundred_and_first_evicts_the_oldest() {
        let mut held = Held::default();
        held.replace_all((1..=101).map(|id| notice(id, "Files")).collect());
        assert_eq!(held.all().len(), CAPACITY);
        assert!(held.get(1).is_none());
        assert_eq!(held.arrived(notice(200, "Files")), [2]);
    }

    #[test]
    fn held_replace_then_close_leaves_nothing() {
        let mut held = Held::default();
        held.replace_all(vec![notice(1, "Files")]);
        assert!(held.closed(1));
        assert!(held.all().is_empty());
        assert!(
            !held.closed(1),
            "a second Closed for the same id changes nothing"
        );
    }
}
