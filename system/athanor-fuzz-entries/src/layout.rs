//! The user-editable layout files and the trust-state file.

use athanor_layout::document::{self, Layer};

use crate::Setup;

/// `document::parse`; the first byte picks the layer, the rest is the TOML text.
pub fn document(data: &[u8]) -> Setup {
    if let Some((&layer, rest)) = data.split_first() {
        if let Ok(text) = std::str::from_utf8(rest) {
            let layer = match layer % 3 {
                0 => Layer::Vendor,
                1 => Layer::Policy,
                _ => Layer::User,
            };
            let _ = document::parse(text, layer);
        }
    }
    Ok(())
}

/// `favorites::parse`.
pub fn favorites(data: &[u8]) -> Setup {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = athanor_layout::favorites::parse(text);
    }
    Ok(())
}

/// `athanor_trust_state::parse`.
pub fn trust_state(data: &[u8]) -> Setup {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = athanor_trust_state::parse(text);
    }
    Ok(())
}
