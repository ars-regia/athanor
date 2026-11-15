//! athanor-bar-toolkit: the bar of doc_bar.md drawn with Athanor's own toolkit
//! (doc_own_stack.md, decision 34). It runs beside the GTK bar, which stays the shipped one
//! until the switch, and is installed as `athanor-bar` where it runs, so that it keeps the
//! application name on the bus and in AT-SPI. `--record-exit` is the unit's ExecStopPost, as
//! for the GTK bar.

mod bar;
mod look;
mod modules;

use std::process::ExitCode;

use athanor_bar::start::{self, Prepared};
use athanor_sal::driver;
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_sulphur_client::Client;
use wayland_client::Connection;

fn main() -> ExitCode {
    let (source, favorites_file, first_session) = match start::prepare() {
        Prepared::Run {
            source,
            favorites_file,
            first_session,
        } => (source, favorites_file, first_session),
        Prepared::Exit(true) => return ExitCode::SUCCESS,
        Prepared::Exit(false) => return ExitCode::FAILURE,
    };
    // GLib's main context, which the compositor client, GIO's watches and the D-Bus calls run
    // on, turns in the toolkit's loop.
    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(err) => {
            tracing::error!(error = %err, "cannot run GLib's main context");
            return ExitCode::FAILURE;
        }
    };
    let client = Connection::connect_to_env()
        .map_err(|err| err.to_string())
        .and_then(|connection| Client::connect_wayland(&connection).map_err(|err| err.to_string()));
    let client = match client {
        Ok(client) => {
            // The window list and the windows of the notifications.
            if let Err(err) = client.watch_windows() {
                tracing::warn!(error = %err, "the windows are not followed");
            }
            Some(client)
        }
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; the modules that need it stay hidden");
            None
        }
    };
    let bar = bar::Bar::new(source, favorites_file, first_session, client);
    match driver::run_with(bar, glib) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "the bar stopped");
            ExitCode::FAILURE
        }
    }
}
