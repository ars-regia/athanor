//! athanor-dock-toolkit: the dock of doc_bar.md, BR7, drawn with Athanor's own toolkit (the
//! dock plan, task 6). It runs beside the GTK dock, which stays the shipped one until the
//! switch (task 10), and is installed as `athanor-dock` where it runs, so that it keeps the
//! application name on the bus and in AT-SPI. `--record-exit` is the unit's ExecStopPost,
//! as for the GTK dock.

mod dock;
mod edge;
mod look;

use std::process::ExitCode;

use athanor_compositor_client::glib_loop::GlibSource;
use athanor_compositor_client::Client;
use athanor_dock::start::{self, Prepared};
use athanor_toolkit::driver;
use wayland_client::Connection;

fn main() -> ExitCode {
    let (source, favorites_file) = match start::prepare() {
        Prepared::Run {
            source,
            favorites_file,
        } => (source, favorites_file),
        Prepared::Exit(true) => return ExitCode::SUCCESS,
        Prepared::Exit(false) => return ExitCode::FAILURE,
    };
    // GLib's main context, which the compositor client and GIO's watches run on, turns in
    // the toolkit's loop.
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
        Ok(client) => Some(client),
        Err(err) => {
            // No windows to list and no secure launch (SH1, BR2): no dock surface at all.
            tracing::error!(error = %err, "no compositor client; the dock shows no surface");
            None
        }
    };
    let dock = dock::Dock::new(source, favorites_file, client);
    match driver::run_with(dock, glib) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "the dock stopped");
            ExitCode::FAILURE
        }
    }
}
