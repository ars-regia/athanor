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
