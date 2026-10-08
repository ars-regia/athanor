//! The arguments of `org.freedesktop.Notifications.Notify`, as any session client sends them.

use athanor_shelld::hints::Hints;
use athanor_shelld::image::{self, Raw};
use athanor_shelld::{icon, notifications};
use zvariant::serialized::{Context, Data};
use zvariant::LE;

use crate::Setup;

/// `hints::Hints`, deserialised from the D-Bus bytes of an `a{sv}`; `notifications::content`
/// then runs over what was accepted, with the text fields taken from the input too.
pub fn hints(data: &[u8]) -> Setup {
    let ctxt = Context::new_dbus(LE, 0);
    let Ok((hints, _)) = Data::new(data.to_vec(), ctxt).deserialize::<Hints>() else {
        return Ok(());
    };
    let [app, icon, summary, body, action_text] = crate::fields::<5>(data);
    let actions: Vec<&str> = action_text.split('\n').collect();
    let _ = notifications::content(&app, &icon, &summary, &body, &actions, hints, i32::try_from(data.len()).unwrap_or(-1));
    Ok(())
}

/// `image::accept`: five little-endian i32 (width, height, rowstride, bits per sample,
/// channels), a flags byte whose low bit is `has_alpha`, then the pixel bytes. The documented
/// bound is that no product overflows, so an accepted image must have exactly the size it states.
pub fn image(data: &[u8]) -> Setup {
    let Some((head, tail)) = data.split_at_checked(21) else {
        return Ok(());
    };
    let field = |at: usize| i32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    let raw = Raw {
        width: field(0),
        height: field(4),
        rowstride: field(8),
        bits_per_sample: field(12),
        channels: field(16),
        has_alpha: head[20] & 1 == 1,
        data: tail,
    };
    if let Some(accepted) = image::accept(&raw) {
        let (width, height) = (accepted.width as usize, accepted.height as usize);
        assert_eq!(accepted.rgba.len(), width * height * 4, "an accepted image is width * height RGBA");
        assert!(width <= 1024 && height <= 1024);
    }
    Ok(())
}

/// `icon::parse`.
pub fn icon(data: &[u8]) -> Setup {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = icon::parse(text);
    }
    Ok(())
}
