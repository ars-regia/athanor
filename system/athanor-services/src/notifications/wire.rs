//! One notification on the private interface, flat, with no optional field: an empty string,
//! an empty array or a zero size means absent (doc_notification_center.md, NC8). The daemon
//! cleans and bounds every field; a reader is a peer all the same and checks again.

use serde::{Deserialize, Serialize};
use zvariant::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WireNotification {
    pub id: u32,
    /// The desktop file id of the proven application; `""` is Other.
    pub app_id: String,
    pub app_name: String,
    pub summary: String,
    /// Plain text.
    pub body: String,
    /// (text, style bits bold = 1 italic = 2 underline = 4, href with `""` for none).
    pub body_spans: Vec<(String, u32, String)>,
    /// (key, label); the key goes back to the application exactly as it sent it.
    pub actions: Vec<(String, String)>,
    /// False once the sender is gone: the actions can no longer be invoked.
    pub actions_available: bool,
    pub urgency: u8,
    pub transient: bool,
    pub resident: bool,
    pub read: bool,
    /// Unix seconds.
    pub time: i64,
    pub desktop_entry: String,
    pub icon_name: String,
    pub icon_file: String,
    pub image_width: u32,
    pub image_height: u32,
    /// Straight RGBA, `image_width * 4` bytes a row.
    pub image_rgba: Vec<u8>,
    /// 0 when the popup waits for the user.
    pub timeout_ms: u32,
    /// `u32::MAX` while the popup waits for the user, 0 once it has ended.
    pub popup_ms_left: u32,
    /// The notification is to be shown as a popup.
    pub popup: bool,
    /// Progress, 0 to 100; -1 for none.
    pub value: i32,
    pub reply: bool,
    pub reply_placeholder: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zvariant::{serialized::Context, Structure, Value, LE};

    fn table() -> WireNotification {
        WireNotification {
            id: 1,
            app_id: "org.example.Chat".into(),
            app_name: "app".into(),
            summary: "summary".into(),
            body: "body".into(),
            body_spans: vec![("span".into(), 5, "https://example.org".into())],
            actions: vec![("key".into(), "label".into())],
            actions_available: true,
            urgency: 2,
            transient: true,
            resident: false,
            read: true,
            time: -8,
            desktop_entry: "entry".into(),
            icon_name: "name".into(),
            icon_file: "/file".into(),
            image_width: 3,
            image_height: 4,
            image_rgba: vec![5; 48],
            timeout_ms: 6,
            popup_ms_left: 7,
            popup: true,
            value: -1,
            reply: true,
            reply_placeholder: "placeholder".into(),
        }
    }

    #[test]
    fn the_signature_is_the_one_the_readers_decode() {
        assert_eq!(
            WireNotification::SIGNATURE.to_string(),
            "(ussssa(sus)a(ss)bybbbxsssuuayuubibs)"
        );
    }

    /// The same table of values as athanor-bar's `notices.rs` test of this name: each field
    /// lands in its own place, so a swap of two fields of one type fails here or there.
    #[test]
    fn the_fields_keep_their_order() {
        let wire = table();
        let bytes = zvariant::to_bytes(Context::new_dbus(LE, 0), &wire).expect("serializes");
        let value: Structure = bytes
            .deserialize_for_dynamic_signature(WireNotification::SIGNATURE)
            .expect("a dynamic structure reads it back")
            .0;
        let fields: Vec<Value> = value.into_fields();
        let expected: Vec<Value> = vec![
            1u32.into(),
            "org.example.Chat".into(),
            "app".into(),
            "summary".into(),
            "body".into(),
            Value::new(vec![("span", 5u32, "https://example.org")]),
            Value::new(vec![("key", "label")]),
            true.into(),
            2u8.into(),
            true.into(),
            false.into(),
            true.into(),
            (-8i64).into(),
            "entry".into(),
            "name".into(),
            "/file".into(),
            3u32.into(),
            4u32.into(),
            Value::new(vec![5u8; 48]),
            6u32.into(),
            7u32.into(),
            true.into(),
            (-1i32).into(),
            true.into(),
            "placeholder".into(),
        ];
        assert_eq!(fields.len(), expected.len());
        for (at, (got, want)) in fields.iter().zip(&expected).enumerate() {
            assert_eq!(got, want, "field {at}");
        }
    }
}
