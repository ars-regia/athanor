//! The start-up assertion doc_shell.md SH4 requires of every layer-shell surface.
//!
//! gtk4-layer-shell interposes libwayland-client. When it is not loaded first it cannot,
//! and the library then degrades in silence: the window maps as an ordinary toplevel
//! with a title bar. For the bar that means a panel that reserves no space and that
//! windows cover.
//! A surface that is not a layer surface is an error, and the process says so and exits.

use gtk4_layer_shell::LayerShell;

/// Call after `init_layer_shell()`. `Err` carries the line to log before exiting.
pub fn require_layer_surface(window: &gtk4::ApplicationWindow) -> Result<(), String> {
    if !gtk4_layer_shell::is_supported() {
        return Err("the compositor does not offer zwlr_layer_shell_v1, or the layer-shell shim was loaded \
                    after libwayland-client"
            .to_string());
    }
    if !window.is_layer_window() {
        return Err("init_layer_shell() did not produce a layer surface".to_string());
    }
    Ok(())
}
