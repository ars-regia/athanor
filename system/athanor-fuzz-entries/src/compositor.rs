//! Files under the user's COSMIC configuration, which any application with a config
//! permission can rewrite.

use athanor_compositor_client::{shortcuts, theme};

use crate::Setup;

/// `shortcuts::parse`, the hand-written RON scanner.
pub fn shortcuts(data: &[u8]) -> Setup {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = shortcuts::parse(text);
    }
    Ok(())
}

/// `theme::read_from` over a configuration directory whose three keys come from the input,
/// split at NUL: `is_dark`, `is_high_contrast`, `accent`, the last two in both modes.
pub fn theme(data: &[u8]) -> Setup {
    let root = crate::scratch_dir("theme")?;
    let put = |component: &str, name: &str, value: &str| -> Setup {
        let path = theme::key_path(&root, component, name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, value)
    };
    let [is_dark, high_contrast, accent] = crate::fields::<3>(data);
    put(theme::MODE, "is_dark", &is_dark)?;
    for mode in [theme::DARK, theme::LIGHT] {
        put(mode, "is_high_contrast", &high_contrast)?;
        put(mode, "accent", &accent)?;
    }
    let _ = theme::read_from(&[root]);
    Ok(())
}
