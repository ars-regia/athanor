//! The start-up assertion doc_shell.md SH4 requires of every layer-shell surface.
//!
//! gtk4-layer-shell interposes libwayland-client. When it is not loaded first it cannot,
//! and the library then degrades in silence: the window maps as an ordinary toplevel
//! with a title bar. For the dock that means a floating window that other windows
//! cover.
//! A surface that is not a layer surface is an error, and the process says so and exits.

use glib::translate::ToGlibPtr;
use gtk4::prelude::*;

// SH4 makes the DT_NEEDED order a package contract, not an accident: rustc places a
// native library right after the first local object that references it, ahead of any
// native library that only an upstream rlib pulls in. Declaring the shim's own symbol
// here, rather than relying solely on the `gtk4-layer-shell` crate's `-sys` crate to
// bring it in, is what puts `-lgtk4-layer-shell` ahead of `-lgtk-4` and
// `-lwayland-client` in this binary's link line — and this is the one call site SH4
// already requires.
#[allow(unsafe_code)]
#[link(name = "gtk4-layer-shell")]
unsafe extern "C" {
    fn gtk_layer_is_layer_window(window: *mut gtk4::ffi::GtkWindow) -> glib::ffi::gboolean;
}

/// Call after `init_layer_shell()`. `Err` carries the line to log before exiting.
pub fn require_layer_surface(window: &gtk4::ApplicationWindow) -> Result<(), String> {
    if !gtk4_layer_shell::is_supported() {
        return Err("the compositor does not offer zwlr_layer_shell_v1, or the layer-shell shim was loaded \
                    after libwayland-client"
            .to_string());
    }
    // SAFETY: `window` is a live GtkApplicationWindow for the duration of this call;
    // `to_glib_none` only borrows its pointer, and `gtk_layer_is_layer_window` reads the
    // window's own state without taking ownership or retaining the pointer past return.
    #[allow(unsafe_code)]
    let is_layer_window = unsafe {
        gtk_layer_is_layer_window(window.upcast_ref::<gtk4::Window>().to_glib_none().0) != 0
    };
    if !is_layer_window {
        return Err("init_layer_shell() did not produce a layer surface".to_string());
    }
    Ok(())
}
