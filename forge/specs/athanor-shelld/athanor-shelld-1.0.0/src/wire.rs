//! What the bar and the control center read from `os.athanor.Notifications1`: the type lives in
//! athanor-services so both link it without this crate's daemon. This module builds one from a
//! held notification.

pub use athanor_services::notifications::wire::*;

use crate::icon::Icon;
use crate::markup;
use crate::store::{self, Notification, Visual};

#[must_use]
pub fn from_notification(notification: &Notification, now_ms: u64) -> WireNotification {
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
    let (body, body_spans) = markup::parse(&content.body);
    WireNotification {
        id: notification.id,
        app_id: notification.identity.key().to_owned(),
        app_name: content.app_name.clone(),
        summary: content.summary.clone(),
        body,
        body_spans,
        actions: content.actions.clone(),
        // A sender that is gone, or was on another bus, cannot be told of an action.
        actions_available: !notification.sender.is_empty(),
        urgency: content.urgency as u8,
        transient: content.transient,
        resident: content.resident,
        read: notification.read,
        time: notification.time,
        desktop_entry: content.desktop_entry.clone().unwrap_or_default(),
        icon_name,
        icon_file,
        image_width,
        image_height,
        image_rgba,
        timeout_ms: content.timeout_ms,
        popup_ms_left: store::popup_ms_left(notification, now_ms),
        popup: notification.popup,
        value: content.value.map_or(-1, i32::from),
        reply: content.reply.is_some(),
        reply_placeholder: content.reply.as_ref().map(|r| r.placeholder.clone()).unwrap_or_default(),
        reply_submit: content.reply.as_ref().map(|r| r.submit_text.clone()).unwrap_or_default(),
        reply_icon: content.reply.as_ref().map(|r| r.submit_icon.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use crate::image::Image;
    use crate::store::tests::content;
    use crate::store::Urgency;

    #[test]
    fn pixels_and_names_land_in_their_own_fields() {
        let mut body = content("x", Urgency::Critical, 0);
        body.visual = Visual::Pixels(Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        });
        let wire = from_notification(
            &Notification {
                id: 7,
                arrived_ms: 0,
                popup: true,
                time: 0,
                identity: Identity::App("org.example.Chat".into()),
                sender: ":1.2".into(),
                read: false,
                content: body,
            },
            0,
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
            (
                wire.icon_name.as_str(),
                wire.popup_ms_left,
                wire.app_id.as_str(),
                wire.actions_available
            ),
            ("", u32::MAX, "org.example.Chat", true)
        );
    }

    #[test]
    fn a_gone_sender_makes_the_actions_unavailable_and_the_body_one_span() {
        let mut held = crate::store::tests::notification(1);
        held.content.body = "text".into();
        let wire = from_notification(&held, 0);
        assert!(!wire.actions_available);
        assert_eq!(wire.body_spans, [("text".to_owned(), 0, String::new())]);
        assert_eq!((wire.value, wire.reply), (-1, false));
    }

    #[test]
    fn the_body_goes_out_as_plain_text_and_spans() {
        let mut held = crate::store::tests::notification(1);
        held.content.body = "<b>B</b> <i>x</i>".into();
        let wire = from_notification(&held, 0);
        assert_eq!(wire.body, "B x");
        assert_eq!(wire.body_spans[0], ("B".to_owned(), 1, String::new()));
    }
}
