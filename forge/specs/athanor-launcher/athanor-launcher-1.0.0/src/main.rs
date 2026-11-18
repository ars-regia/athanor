//! athanor-launcher: the launcher of doc_launcher.md. athanor-launcher.service runs it, and
//! the session bus starts that unit when `os.athanor.Launcher1` is called (LA8).
//! `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the crash-loop
//! limit (doc_shell.md SH8).

mod bus;
mod i18n;
mod ui;

use std::env;
use std::path::Path;

use athanor_layout::loader;
use athanor_sal::driver;
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_sulphur_client::shortcuts::{self, Binding};
use athanor_sulphur_client::Client;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, dirs, journal, sandbox};
use gio::glib;
use wayland_client::Connection;

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-launcher", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return glib::ExitCode::FAILURE;
    };
    // `now`, `--record-exit` and `record_start` exactly as in athanor-bar's main.rs.
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return glib::ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                glib::ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&dirs.failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return glib::ExitCode::FAILURE;
    }
    // SH8: after five failures the launcher still runs, without the inputs that come from
    // outside the image and can break it: no files, no search providers, and a usage file
    // of its own for this session. The shell is never lost.
    let given_up = match crash_loop::given_up(&dirs.failures, now) {
        Ok(given_up) => given_up,
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return glib::ExitCode::FAILURE;
        }
    };
    if given_up {
        tracing::error!(
            failures = crash_loop::GIVE_UP_AFTER,
            window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
            "athanor-launcher keeps failing; it runs without files, search providers and usage until the failures leave the window"
        );
    }
    let session_usage = dirs.unit_runtime.join("usage.json");
    let usage_path = match (given_up, loader::state_home()) {
        (false, Some(state)) => state.join("athanor/search/usage.json"),
        (false, None) => {
            tracing::error!("no state directory; usage is kept for this session only");
            session_usage
        }
        (true, _) => session_usage,
    };
    // Created before the ruleset, so each grant has a directory to hold on to.
    let usage_dir = usage_path.parent().map_or_else(|| dirs.unit_runtime.clone(), Path::to_path_buf);
    if let Err(err) = std::fs::create_dir_all(&usage_dir) {
        tracing::warn!(error = %err, dir = %usage_dir.display(), "cannot create a directory the launcher writes");
    }
    // Super is ours only where the user left it to the system, and only at the first start
    // with a state directory (doc_launcher.md, LA8 and §6): the marker sits beside the usage.
    // It runs before the ruleset, which then leaves cosmic-comp's shortcuts read-only.
    let bound = match (given_up, loader::state_home()) {
        (false, Some(state)) => {
            shortcuts::set_system_action_once("Launcher", bus::SHOW_COMMAND, &state.join("athanor/search/super-bound"))
        }
        _ => Ok(None),
    };
    match bound {
        Ok(None) => tracing::info!("Super was bound once already, or there is no state directory: the shortcuts are not read"),
        Ok(Some(Binding::Added)) => tracing::info!("Super now calls {}; the user's shortcuts had no entry for it", bus::NAME),
        Ok(Some(Binding::ReplacedDefault)) => tracing::info!("Super now calls {} instead of the system's default", bus::NAME),
        Ok(Some(Binding::Unchanged)) => tracing::info!("Super already calls {}", bus::NAME),
        Ok(Some(Binding::UserChoice)) => tracing::info!("Super keeps the command the user chose; it does not call {}", bus::NAME),
        Err(err) => tracing::warn!(error = %err, "Super is not bound to the launcher; it still opens from the bar"),
    }
    // Before GLib starts a thread (LA9). Writes: the launch sockets, the unit's runtime
    // directory, dconf, the cache (which holds qalc's per-run configuration directory), usage
    // and /tmp; reads stay open, because the preview reads files. The launch directory is also
    // where the window capture buffers live. No TCP for the launcher or for qalc and the
    // preview's systemd-run. Each is granted by the descriptor its check opened: the launch
    // directory, made the way launch() makes it, and dconf's are private to the user
    // (`dirs::private_dir`).
    let mut write = dirs::private_dirs(&dirs.runtime, &["athanor", "dconf"]);
    write.extend(dirs::existing(
        &[
            dirs.unit_runtime.as_path(),
            dirs.cache.as_path(),
            usage_dir.as_path(),
            Path::new("/tmp"),
        ],
        dirs::owned_dir,
    ));
    let devices: Vec<&Path> = athanor_launcher::DEVICE_WRITES.iter().map(Path::new).collect();
    let devices = dirs::existing(&devices, dirs::owned_node);
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &devices))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the launcher with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    i18n::init();

    // GLib's main context, which the compositor client, the bus name and GIO's watches run
    // on, turns in the toolkit's loop.
    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(err) => {
            tracing::error!(error = %err, "cannot run GLib's main context");
            return glib::ExitCode::FAILURE;
        }
    };
    let client = Connection::connect_to_env()
        .map_err(|err| err.to_string())
        .and_then(|connection| Client::connect_wayland(&connection).map_err(|err| err.to_string()));
    let client = match client {
        Ok(client) => {
            // The open windows among the results, their actions and their pictures.
            if let Err(err) = client.watch_windows() {
                tracing::warn!(error = %err, "the windows are not followed");
            }
            Some(client)
        }
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; windows, launching and the focused output are unavailable");
            None
        }
    };
    let options = ui::Options {
        usage_path,
        given_up,
        show: env::var("ATHANOR_LAUNCHER_SHOW").ok(),
    };
    match driver::run_with(ui::Launcher::new(options, client), glib) {
        Ok(()) => glib::ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "the launcher stopped");
            glib::ExitCode::FAILURE
        }
    }
}
