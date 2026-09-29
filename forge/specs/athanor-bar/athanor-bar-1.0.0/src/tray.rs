//! StatusNotifierItem properties as the tray reads them (doc_bar.md BR5). Every item is a
//! peer: names, paths, pixmaps and tooltips are checked and bounded here, and anything that
//! fails a check is dropped, never trusted (BR9).

use athanor_unit::text::{self, NAME_CHARS, TITLE_CHARS};
use glib::{Variant, VariantDict, VariantTy};

use crate::notices;

/// The watcher's own bound; the host never shows more.
pub const MAX_ITEMS: usize = 64;
const MAX_PIXMAPS: usize = 16;
const MAX_PIXMAP_SIDE: u32 = 256;
const MAX_BUS_NAME: usize = 255;
const PIXMAPS_SIGNATURE: &str = "a(iiay)";
const TOOLTIP_SIGNATURE: &str = "(sa(iiay)ss)";

/// A D-Bus bus name, unique (`:1.42`) or well-known (`org.kde.StatusNotifierItem-1-1`).
#[must_use]
pub fn is_bus_name(name: &str) -> bool {
    let (unique, rest) = match name.strip_prefix(':') {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let elements: Vec<&str> = rest.split('.').collect();
    name.len() <= MAX_BUS_NAME
        && elements.len() >= 2
        && elements.iter().all(|element| {
            !element.is_empty()
                && element
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                && (unique || !element.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// The watcher's item id: `"{service}/StatusNotifierItem"` for an item registered by its
/// service name, `"{sender}{path}"` for one registered by its path. `None` when either half
/// is not what D-Bus accepts.
#[must_use]
pub fn split_id(id: &str) -> Option<(String, String)> {
    let slash = id.find('/')?;
    let (service, path) = id.split_at(slash);
    (is_bus_name(service) && Variant::is_object_path(path))
        .then(|| (service.to_owned(), path.to_owned()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passive,
    Active,
    NeedsAttention,
}

/// Straight RGBA, converted from the item's ARGB32 in network byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Icon {
    pub name: Option<String>,
    pub pixmap: Option<Pixmap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub id: String,
    pub status: Status,
    pub icon: Icon,
    /// Title and body of the tooltip, as plain text (ruling 9).
    pub tooltip: String,
    pub item_is_menu: bool,
    /// The dbusmenu object path, when the item has a menu.
    pub menu: Option<String>,
}

impl Item {
    /// The button's accessible name: the title, else the item's id.
    #[must_use]
    pub fn name(&self) -> &str {
        if self.title.is_empty() {
            &self.id
        } else {
            &self.title
        }
    }
}

/// The item behind `props`, the `a{sv}` of `org.freedesktop.DBus.Properties.GetAll`. A
/// property of the wrong type is treated as absent. `wanted_px` is the device pixel size the
/// button draws at: the pixmap chosen is the smallest at least that big.
#[must_use]
pub fn read(props: &Variant, wanted_px: u32) -> Option<Item> {
    if !props.is_type(VariantTy::VARDICT) {
        return None;
    }
    let dict = VariantDict::new(Some(props));
    let string = |key: &str| dict.lookup::<String>(key).ok().flatten();
    let icon = |name_key: &str, pixmap_key: &str| Icon {
        name: string(name_key).filter(|name| notices::is_icon_name(name)),
        pixmap: dict
            .lookup_value(pixmap_key, None)
            .and_then(|value| pixmap(&value, wanted_px)),
    };
    let status = match string("Status").as_deref() {
        Some("Passive") => Status::Passive,
        Some("NeedsAttention") => Status::NeedsAttention,
        _ => Status::Active,
    };
    let normal = icon("IconName", "IconPixmap");
    let attention = icon("AttentionIconName", "AttentionIconPixmap");
    let icon = if status == Status::NeedsAttention
        && (attention.name.is_some() || attention.pixmap.is_some())
    {
        attention
    } else {
        normal
    };
    Some(Item {
        title: text::line(&string("Title").unwrap_or_default(), NAME_CHARS),
        id: text::line(&string("Id").unwrap_or_default(), NAME_CHARS),
        status,
        icon,
        tooltip: dict
            .lookup_value("ToolTip", None)
            .map(|value| tooltip(&value))
            .unwrap_or_default(),
        item_is_menu: dict
            .lookup::<bool>("ItemIsMenu")
            .ok()
            .flatten()
            .unwrap_or(false),
        menu: dict
            .lookup_value("Menu", None)
            .and_then(|value| menu_path(&value)),
    })
}

/// The `Scroll` delta for a GTK scroll step: 120 per notch as Qt counts, positive away
/// from the user (GTK counts down as positive), bounded to ten notches. `None` for no
/// movement (open doubt 4: the sign follows KDE's host).
#[must_use]
pub fn scroll_delta(delta: f64) -> Option<i32> {
    if !delta.is_finite() || delta.abs() < f64::EPSILON {
        return None;
    }
    // In range after the clamp, so the cast neither saturates nor truncates past a notch.
    Some((-(delta * 120.0)).clamp(-1200.0, 1200.0) as i32)
}

/// An object path, or a string holding one, as some items send. `/` and `/NO_DBUSMENU`
/// mean "no menu".
fn menu_path(value: &Variant) -> Option<String> {
    if !(value.is_type(VariantTy::OBJECT_PATH) || value.is_type(VariantTy::STRING)) {
        return None;
    }
    let path = value.str()?;
    (Variant::is_object_path(path) && path != "/" && path != "/NO_DBUSMENU")
        .then(|| path.to_owned())
}

/// Of the item's pixmaps, the smallest whose longer side is at least `wanted`, else the
/// largest. Entries past the sixteenth, of a side outside 1..=256, or whose data is not
/// width × height × 4 bytes, are skipped.
fn pixmap(value: &Variant, wanted: u32) -> Option<Pixmap> {
    if value.type_().as_str() != PIXMAPS_SIGNATURE {
        return None;
    }
    let side = 1..=MAX_PIXMAP_SIDE;
    let mut best: Option<(u32, u32, Variant)> = None;
    for index in 0..value.n_children().min(MAX_PIXMAPS) {
        let Some(entry) = value.try_child_value(index) else {
            continue;
        };
        let width = entry.try_child_value(0).and_then(|v| v.get::<i32>());
        let height = entry.try_child_value(1).and_then(|v| v.get::<i32>());
        let (Some(width), Some(height), Some(data)) = (width, height, entry.try_child_value(2))
        else {
            continue;
        };
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            continue;
        };
        if !side.contains(&width) || !side.contains(&height) {
            continue;
        }
        // Both sides are at most 256, so the product cannot overflow.
        let Ok(expected) = usize::try_from(width * height * 4) else {
            continue;
        };
        if data.n_children() != expected {
            continue;
        }
        let longer = width.max(height);
        if best
            .as_ref()
            .is_none_or(|(w, h, _)| better(longer, (*w).max(*h), wanted))
        {
            best = Some((width, height, data));
        }
    }
    let (width, height, data) = best?;
    let argb = data.fixed_array::<u8>().ok()?;
    let rgba = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
        .collect();
    Some(Pixmap {
        width,
        height,
        rgba,
    })
}

fn better(candidate: u32, current: u32, wanted: u32) -> bool {
    match (candidate >= wanted, current >= wanted) {
        (true, true) => candidate < current,
        (true, false) => true,
        (false, true) => false,
        (false, false) => candidate > current,
    }
}

/// The tooltip's title and body, each one cleaned line, joined by a newline.
fn tooltip(value: &Variant) -> String {
    if value.type_().as_str() != TOOLTIP_SIGNATURE {
        return String::new();
    }
    [2, 3]
        .iter()
        .filter_map(|&index| value.try_child_value(index))
        .filter_map(|part| part.str().map(|s| text::line(s, TITLE_CHARS)))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use glib::prelude::*;

    use super::*;

    fn props(entries: &[(&str, Variant)]) -> Variant {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect::<HashMap<String, Variant>>()
            .to_variant()
    }

    fn object_path(path: &str) -> Variant {
        Variant::parse(None, &format!("objectpath '{path}'")).unwrap()
    }

    fn pixmaps(sides: &[(i32, i32)]) -> Variant {
        sides
            .iter()
            .map(|&(w, h)| {
                let bytes = usize::try_from(w * h * 4).unwrap_or(0);
                // Opaque red in ARGB: a = 255, r = 255.
                let data: Vec<u8> = [255u8, 255, 0, 0].into_iter().cycle().take(bytes).collect();
                (w, h, data)
            })
            .collect::<Vec<_>>()
            .to_variant()
    }

    #[test]
    fn a_plain_item_reads() {
        let item = read(
            &props(&[
                ("Title", "Mail".to_variant()),
                ("Id", "mail".to_variant()),
                ("Status", "Active".to_variant()),
                ("IconName", "mail-unread".to_variant()),
                ("Menu", object_path("/MenuBar")),
                ("ItemIsMenu", true.to_variant()),
            ]),
            32,
        )
        .unwrap();
        assert_eq!(item.name(), "Mail");
        assert_eq!(item.status, Status::Active);
        assert_eq!(item.icon.name.as_deref(), Some("mail-unread"));
        assert_eq!(item.menu.as_deref(), Some("/MenuBar"));
        assert!(item.item_is_menu);
    }

    #[test]
    fn not_a_dictionary_is_refused_and_wrong_types_are_absent() {
        assert!(read(&("x",).to_variant(), 32).is_none());
        let item = read(
            &props(&[("Title", 7u32.to_variant()), ("Id", "7".to_variant())]),
            32,
        )
        .unwrap();
        assert_eq!(item.name(), "7");
        assert_eq!(item.status, Status::Active);
    }

    #[test]
    fn a_bad_icon_name_is_dropped() {
        let item = read(&props(&[("IconName", "../../etc/x".to_variant())]), 32).unwrap();
        assert_eq!(item.icon, Icon::default());
    }

    #[test]
    fn the_pixmap_is_the_smallest_at_least_wanted_in_rgba() {
        let item = read(
            &props(&[(
                "IconPixmap",
                pixmaps(&[(16, 16), (64, 64), (32, 32), (48, 48)]),
            )]),
            24,
        )
        .unwrap();
        let pixmap = item.icon.pixmap.unwrap();
        assert_eq!((pixmap.width, pixmap.height), (32, 32));
        assert_eq!(pixmap.rgba.get(..4), Some([255, 0, 0, 255].as_slice()));
        let largest = read(
            &props(&[("IconPixmap", pixmaps(&[(16, 16), (22, 22)]))]),
            64,
        )
        .unwrap()
        .icon
        .pixmap
        .unwrap();
        assert_eq!(largest.width, 22);
    }

    #[test]
    fn a_pixmap_of_a_wrong_side_or_length_is_skipped() {
        let wrong_length = vec![(4i32, 4i32, vec![0u8; 10])].to_variant();
        for value in [
            pixmaps(&[(0, 16)]),
            pixmaps(&[(-1, 16)]),
            pixmaps(&[(257, 1)]),
            wrong_length,
        ] {
            let item = read(&props(&[("IconPixmap", value)]), 16).unwrap();
            assert!(item.icon.pixmap.is_none());
        }
    }

    #[test]
    fn needs_attention_takes_the_attention_icon_when_there_is_one() {
        let item = read(
            &props(&[
                ("Status", "NeedsAttention".to_variant()),
                ("IconName", "normal".to_variant()),
                ("AttentionIconName", "alert".to_variant()),
            ]),
            16,
        )
        .unwrap();
        assert_eq!(item.icon.name.as_deref(), Some("alert"));
        let item = read(
            &props(&[
                ("Status", "NeedsAttention".to_variant()),
                ("IconName", "normal".to_variant()),
            ]),
            16,
        )
        .unwrap();
        assert_eq!(item.icon.name.as_deref(), Some("normal"));
    }

    #[test]
    fn menu_paths_that_mean_no_menu_are_none() {
        for value in [
            object_path("/"),
            object_path("/NO_DBUSMENU"),
            "not a path".to_variant(),
            1u32.to_variant(),
        ] {
            assert_eq!(read(&props(&[("Menu", value)]), 16).unwrap().menu, None);
        }
        assert_eq!(
            read(&props(&[("Menu", "/Menu".to_variant())]), 16)
                .unwrap()
                .menu
                .as_deref(),
            Some("/Menu")
        );
    }

    #[test]
    fn the_tooltip_is_title_and_body_as_plain_lines() {
        let tip = (
            "",
            Vec::<(i32, i32, Vec<u8>)>::new(),
            "Mail",
            "3 <b>new</b>\n",
        )
            .to_variant();
        let item = read(&props(&[("ToolTip", tip)]), 16).unwrap();
        assert_eq!(item.tooltip, "Mail\n3 <b>new</b>");
    }

    #[test]
    fn bus_names_and_item_ids() {
        for good in [":1.42", "org.kde.StatusNotifierItem-1-1", "a_b.c"] {
            assert!(is_bus_name(good), "{good}");
        }
        // 301 bytes of valid elements: refused for its length alone.
        let long = format!("a{}", ".b".repeat(150));
        for bad in [
            "",
            "org",
            ":1",
            "org..kde",
            "org.1kde",
            "org.k de",
            long.as_str(),
        ] {
            assert!(!is_bus_name(bad), "{bad}");
        }
        assert_eq!(
            split_id(":1.42/StatusNotifierItem"),
            Some((":1.42".into(), "/StatusNotifierItem".into()))
        );
        assert_eq!(
            split_id(":1.42/org/ayatana/NotificationItem/x"),
            Some((":1.42".into(), "/org/ayatana/NotificationItem/x".into()))
        );
        for bad in [
            "",
            ":1.42",
            "/StatusNotifierItem",
            ":1.42/a//b",
            "bad/StatusNotifierItem",
        ] {
            assert_eq!(split_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn scroll_turns_gtk_steps_into_bounded_sni_deltas() {
        assert_eq!(
            scroll_delta(1.0),
            Some(-120),
            "down in GTK is negative in SNI"
        );
        assert_eq!(scroll_delta(-0.5), Some(60));
        assert_eq!(scroll_delta(0.0), None);
        assert_eq!(scroll_delta(1.0e9), Some(-1200));
        assert_eq!(scroll_delta(f64::NAN), None);
    }
}
