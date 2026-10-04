//! What the bar reads from `os.athanor.Notifications1`: one notification, flat, with no
//! optional field. An empty string, an empty array or a zero size means absent. The bar
//! links this crate for this type (doc_bar.md BR1, "Shared code").

use serde::{Deserialize, Serialize};
use zvariant::Type;

use crate::icon::Icon;
use crate::store::{self, Notification, Visual};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WireNotification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: String,
    pub icon_name: String,
    pub icon_file: String,
    pub image_width: u32,
    pub image_height: u32,
    /// Straight RGBA, `image_width * 4` bytes a row.
    pub image_rgba: Vec<u8>,
    /// 0 when the popup waits for the user.
    pub timeout_ms: u32,
    /// See `store::popup_ms_left`.
    pub popup_ms_left: u32,
}

impl WireNotification {
    #[must_use]
    pub fn new(notification: &Notification, now_ms: u64, dnd: bool) -> WireNotification {
        let content = &notification.content;
        let (icon_name, icon_file) = match &content.visual {
            Visual::Icon(Icon::Name(name)) => (name.clone(), String::new()),
            Visual::Icon(Icon::File(file)) => (String::new(), file.clone()),
            Visual::None | Visual::Pixels(_) => (String::new(), String::new()),
        };
        let (image_width, image_height, image_rgba) = match &content.visual {
            Visual::Pixels(image) => (image.width, image.height, image.rgba.clone()),
            _ => (0, 0, Vec::new()),
        };
        WireNotification {
            id: notification.id,
            app_name: content.app_name.clone(),
            summary: content.summary.clone(),
            body: content.body.clone(),
            actions: content.actions.clone(),
            urgency: content.urgency as u8,
            transient: content.transient,
            resident: content.resident,
            desktop_entry: content.desktop_entry.clone().unwrap_or_default(),
            icon_name,
            icon_file,
            image_width,
            image_height,
            image_rgba,
            timeout_ms: content.timeout_ms,
            popup_ms_left: store::popup_ms_left(notification, now_ms, dnd),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Image;
    use crate::store::tests::content;
    use crate::store::Urgency;

    #[test]
    fn the_signature_is_the_one_the_bar_decodes() {
        assert_eq!(
            WireNotification::SIGNATURE.to_string(),
            "(usssa(ss)ybbsssuuayuu)"
        );
    }

    /// The same table of values as athanor-bar's `notices.rs` test of this name: the struct
    /// serializes in the order the bar's plain tuple reads it.
    #[test]
    fn the_sixteen_fields_keep_their_order() {
        type BarTuple = (
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
        let wire = WireNotification {
            id: 1,
            app_name: "app".into(),
            summary: "summary".into(),
            body: "body".into(),
            actions: vec![("key".into(), "label".into())],
            urgency: 2,
            transient: true,
            resident: false,
            desktop_entry: "entry".into(),
            icon_name: "name".into(),
            icon_file: "/file".into(),
            image_width: 3,
            image_height: 4,
            image_rgba: vec![5; 48],
            timeout_ms: 6,
            popup_ms_left: 7,
        };
        let ctxt = zvariant::serialized::Context::new_dbus(zvariant::LE, 0);
        let bytes = zvariant::to_bytes(ctxt, &wire).expect("the wire value serializes");
        let (tuple, _): (BarTuple, usize) =
            bytes.deserialize().expect("the bar's tuple reads it back");
        // Tuples above twelve fields implement neither `PartialEq` nor `Debug`: compare
        // them in two halves.
        let (id, app, summary, body, actions, urgency, transient, resident) = (
            tuple.0, tuple.1, tuple.2, tuple.3, tuple.4, tuple.5, tuple.6, tuple.7,
        );
        assert_eq!(
            (id, app.as_str(), summary.as_str(), body.as_str()),
            (1, "app", "summary", "body")
        );
        assert_eq!(actions, vec![("key".to_string(), "label".to_string())]);
        assert_eq!((urgency, transient, resident), (2, true, false));
        let (entry, name, file, width, height, rgba, timeout, left) = (
            tuple.8, tuple.9, tuple.10, tuple.11, tuple.12, tuple.13, tuple.14, tuple.15,
        );
        assert_eq!(
            (entry.as_str(), name.as_str(), file.as_str()),
            ("entry", "name", "/file")
        );
        assert_eq!(
            (width, height, rgba, timeout, left),
            (3, 4, vec![5; 48], 6, 7)
        );
    }

    #[test]
    fn pixels_and_names_land_in_their_own_fields() {
        let mut body = content("x", Urgency::Critical, 0);
        body.visual = Visual::Pixels(Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        });
        let wire = WireNotification::new(
            &Notification {
                id: 7,
                arrived_ms: 0,
                time: 0,
                identity: crate::identity::Identity::Other,
                sender: String::new(),
                read: false,
                content: body,
            },
            0,
            true,
        );
        assert_eq!(
            (
                wire.id,
                wire.urgency,
                wire.image_width,
                wire.image_rgba.len()
            ),
            (7, 2, 1, 4)
        );
        assert_eq!(
            (wire.icon_name.as_str(), wire.popup_ms_left),
            ("", u32::MAX)
        );
    }
}
