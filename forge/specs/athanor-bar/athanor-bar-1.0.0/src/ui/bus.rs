//! D-Bus calls of the modules that still reach a service through GLib's `gio` (doc_bar.md,
//! BR3): a call with a timeout. Every call allows interactive authorisation, so polkit can
//! ask. The models of athanor-services speak D-Bus through zbus instead.

use std::future::Future;

use gtk4::{gio, glib};

pub const TIMEOUT_MS: i32 = 5000;

pub fn call(
    connection: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
    timeout: i32,
) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static {
    connection.call_future(
        Some(name),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
        timeout,
    )
}
