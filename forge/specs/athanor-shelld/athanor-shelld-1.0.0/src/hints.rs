//! The hints of `Notify` the daemon honours; every other hint is skipped without being
//! decoded into memory. A hint of the wrong type is ignored, as if it were absent.

use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use zvariant::{OwnedValue, Signature, Type};

use crate::icon::{self, Icon};
use crate::image::{self, Image, Raw};

/// A theme name is short; the rest of a longer one is never looked up.
const SOUND_NAME_CHARS: usize = 128;

/// A reply hint's text is short; the rest of a longer one is never kept.
const REPLY_HINT_CHARS: usize = 256;

fn bounded(text: &str) -> String {
    text.chars().take(REPLY_HINT_CHARS).collect()
}

#[derive(Debug, Default, PartialEq)]
pub struct Hints {
    pub urgency: Option<u8>,
    pub transient: bool,
    pub resident: bool,
    /// `sound-name`, a name in the sound theme. There is no field for `sound-file`: the
    /// hint is skipped like any other, so no path an application names is ever opened (NC7).
    pub sound_name: Option<String>,
    pub suppress_sound: bool,
    /// `value`: the progress, 0 to 100; anything else is no value (NC9).
    pub value: Option<u8>,
    /// KDE's inline reply hints (NC9), plain text; the icon is a theme name, never a file.
    pub reply_placeholder: Option<String>,
    pub reply_submit_text: Option<String>,
    pub reply_submit_icon: Option<String>,
    pub desktop_entry: Option<String>,
    /// From `image-data`, else `image_data` (1.1), else `icon_data` (1.0).
    pub image: Option<Image>,
    /// From `image-path`, else `image_path` (1.1).
    pub image_path: Option<Icon>,
}

impl Type for Hints {
    const SIGNATURE: &'static Signature = <HashMap<String, OwnedValue> as Type>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Hints {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Hints, D::Error> {
        deserializer.deserialize_map(HintsVisitor)
    }
}

type ImageData<'a> = (i32, i32, i32, bool, i32, i32, &'a [u8]);

struct HintsVisitor;

impl<'de> Visitor<'de> for HintsVisitor {
    type Value = Hints;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the hints of Notify, a{sv}")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Hints, A::Error> {
        let mut hints = Hints::default();
        // Lower rank wins: the name of the newest specification first.
        let mut image: Option<(u8, Image)> = None;
        let mut image_path: Option<(u8, Icon)> = None;
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "urgency" => hints.urgency = map.next_value::<Hint<u8>>()?.0,
                "transient" => hints.transient = map.next_value::<Hint<bool>>()?.0.unwrap_or(false),
                "resident" => hints.resident = map.next_value::<Hint<bool>>()?.0.unwrap_or(false),
                "sound-name" => {
                    hints.sound_name = map
                        .next_value::<Hint<&str>>()?
                        .0
                        .map(|name| name.chars().take(SOUND_NAME_CHARS).collect());
                }
                "suppress-sound" => {
                    hints.suppress_sound = map.next_value::<Hint<bool>>()?.0.unwrap_or(false);
                }
                "value" => {
                    hints.value = map
                        .next_value::<Hint<i32>>()?
                        .0
                        .and_then(|value| u8::try_from(value).ok())
                        .filter(|value| *value <= 100);
                }
                "x-kde-reply-placeholder-text" => {
                    hints.reply_placeholder = map.next_value::<Hint<&str>>()?.0.map(bounded);
                }
                "x-kde-reply-submit-button-text" => {
                    hints.reply_submit_text = map.next_value::<Hint<&str>>()?.0.map(bounded);
                }
                "x-kde-reply-submit-button-icon-name" => {
                    hints.reply_submit_icon = map
                        .next_value::<Hint<&str>>()?
                        .0
                        .and_then(icon::parse)
                        .and_then(|icon| match icon {
                            Icon::Name(name) => Some(name),
                            Icon::File(_) => None,
                        });
                }
                "desktop-entry" => {
                    hints.desktop_entry = map
                        .next_value::<Hint<&str>>()?
                        .0
                        .filter(|id| is_desktop_id(id))
                        .map(str::to_owned);
                }
                "image-data" | "image_data" | "icon_data" => {
                    let rank = match key {
                        "image-data" => 0,
                        "image_data" => 1,
                        _ => 2,
                    };
                    let found = map.next_value::<Hint<ImageData<'de>>>()?.0.and_then(
                        |(width, height, rowstride, has_alpha, bits_per_sample, channels, data)| {
                            image::accept(&Raw {
                                width,
                                height,
                                rowstride,
                                has_alpha,
                                bits_per_sample,
                                channels,
                                data,
                            })
                        },
                    );
                    if let Some(found) = found {
                        if image.as_ref().is_none_or(|(best, _)| rank < *best) {
                            image = Some((rank, found));
                        }
                    }
                }
                "image-path" | "image_path" => {
                    let rank = u8::from(key == "image_path");
                    if let Some(found) = map.next_value::<Hint<&str>>()?.0.and_then(icon::parse) {
                        if image_path.as_ref().is_none_or(|(best, _)| rank < *best) {
                            image_path = Some((rank, found));
                        }
                    }
                }
                _ => {
                    map.next_value::<Skip>()?;
                }
            }
        }
        hints.image = image.map(|(_, image)| image);
        hints.image_path = image_path.map(|(_, icon)| icon);
        Ok(hints)
    }
}

/// One variant: `Some` when it holds a `T`, `None` (skipped, not decoded) otherwise.
struct Hint<T>(Option<T>);

impl<'de, T: Deserialize<'de> + Type> Deserialize<'de> for Hint<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Hint<T>, D::Error> {
        deserializer.deserialize_seq(HintVisitor(PhantomData))
    }
}

/// A hint the daemon does not read: its signature and value are walked, never stored.
struct Skip;

impl<'de> Deserialize<'de> for Skip {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Skip, D::Error> {
        deserializer.deserialize_seq(SkipVisitor)
    }
}

struct SkipVisitor;

impl<'de> Visitor<'de> for SkipVisitor {
    type Value = Skip;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Skip, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Skip)
    }
}

struct HintVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de> + Type> Visitor<'de> for HintVisitor<T> {
    type Value = Hint<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Hint<T>, A::Error> {
        let signature: Signature = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        if signature == *T::SIGNATURE {
            Ok(Hint(seq.next_element::<T>()?))
        } else {
            seq.next_element::<IgnoredAny>()?;
            Ok(Hint(None))
        }
    }
}

/// A desktop file id (`org.gnome.Nautilus`), without the `.desktop` suffix.
pub(crate) fn is_desktop_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zvariant::serialized::Context;
    use zvariant::{to_bytes, Value, LE};

    fn decode(map: HashMap<&str, Value<'_>>) -> Hints {
        let encoded = to_bytes(Context::new_dbus(LE, 0), &map).expect("encode");
        encoded.deserialize::<Hints>().expect("decode").0
    }

    fn image_value(width: i32, height: i32, rowstride: i32, data: Vec<u8>) -> Value<'static> {
        Value::new((width, height, rowstride, false, 8i32, 3i32, data))
    }

    #[test]
    fn known_hints_are_read() {
        let hints = decode(HashMap::from([
            ("urgency", Value::U8(2)),
            ("transient", Value::Bool(true)),
            ("desktop-entry", Value::from("org.gnome.Nautilus")),
            ("image-path", Value::from("file:///a.png")),
            ("image-data", image_value(2, 2, 8, vec![0; 14])),
        ]));
        assert_eq!(hints.urgency, Some(2));
        assert!(hints.transient && !hints.resident);
        assert_eq!(hints.desktop_entry.as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(hints.image_path, Some(Icon::File("/a.png".into())));
        assert_eq!(hints.image.map(|i| (i.width, i.height)), Some((2, 2)));
    }

    #[test]
    fn the_sound_name_and_suppress_sound_are_read_and_sound_file_is_not() {
        let hints = decode(HashMap::from([
            ("sound-name", Value::from("bell")),
            ("suppress-sound", Value::Bool(true)),
            ("sound-file", Value::from("/etc/hostname")),
        ]));
        assert_eq!(hints.sound_name.as_deref(), Some("bell"));
        assert!(hints.suppress_sound);
        let file_only = decode(HashMap::from([(
            "sound-file",
            Value::from("/etc/hostname"),
        )]));
        assert_eq!(file_only, Hints::default(), "sound-file leaves no trace");
    }

    #[test]
    fn progress_and_the_reply_hints_are_read() {
        let hints = decode(HashMap::from([
            ("value", Value::I32(42)),
            ("x-kde-reply-placeholder-text", Value::from("Say it")),
            ("x-kde-reply-submit-button-text", Value::from("Send")),
            ("x-kde-reply-submit-button-icon-name", Value::from("mail-send")),
        ]));
        assert_eq!(hints.value, Some(42));
        assert_eq!(hints.reply_placeholder.as_deref(), Some("Say it"));
        assert_eq!(hints.reply_submit_text.as_deref(), Some("Send"));
        assert_eq!(hints.reply_submit_icon.as_deref(), Some("mail-send"));
    }

    #[test]
    fn a_value_outside_zero_to_a_hundred_and_a_path_as_an_icon_are_ignored() {
        for value in [150, -1, i32::MAX] {
            assert_eq!(decode(HashMap::from([("value", Value::I32(value))])).value, None);
        }
        assert_eq!(decode(HashMap::from([("value", Value::I32(0))])).value, Some(0));
        assert_eq!(decode(HashMap::from([("value", Value::I32(100))])).value, Some(100));
        let hints = decode(HashMap::from([(
            "x-kde-reply-submit-button-icon-name",
            Value::from("/etc/passwd"),
        )]));
        assert_eq!(hints.reply_submit_icon, None, "an icon name, never a path");
    }

    #[test]
    fn a_hint_of_the_wrong_type_is_ignored_not_an_error() {
        let hints = decode(HashMap::from([
            ("urgency", Value::U32(2)),
            (
                "image-data",
                Value::new((2i32, 2i32, 8i32, false, 8i32, 3i32)),
            ),
            ("desktop-entry", Value::from("../evil")),
        ]));
        assert_eq!(hints, Hints::default());
    }

    #[test]
    fn the_newest_image_name_wins_and_a_bad_image_falls_back() {
        let hints = decode(HashMap::from([
            ("icon_data", image_value(1, 1, 3, vec![0; 3])),
            ("image-data", image_value(2, 2, 8, vec![0; 13])), // wrong length: refused
            ("image_data", image_value(2, 2, 8, vec![0; 14])),
        ]));
        assert_eq!(
            hints.image.map(|i| i.width),
            Some(2),
            "image_data, since image-data was refused"
        );
    }

    #[test]
    fn a_large_unknown_hint_is_skipped() {
        let hints = decode(HashMap::from([
            ("x-huge", Value::new(vec![7u8; 256 * 1024])),
            ("resident", Value::Bool(true)),
        ]));
        assert!(hints.resident);
    }
}
