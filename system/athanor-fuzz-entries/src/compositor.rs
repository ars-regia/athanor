//! Files under the user's COSMIC configuration, which any application with a config
//! permission can rewrite.

use athanor_compositor_client::shortcuts;

use crate::Setup;

/// `shortcuts::parse`, the hand-written RON scanner.
pub fn shortcuts(data: &[u8]) -> Setup {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = shortcuts::parse(text);
    }
    Ok(())
}
