//! athanor-launcher: the launcher of doc_launcher.md. athanor-launcher.service runs it, and
//! the session bus starts that unit when `os.athanor.Launcher1` is called (LA8).
//! `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the crash-loop
//! limit (doc_shell.md SH8).

mod bus;
mod i18n;
mod layer_guard;
mod surface;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_compositor_client::shortcuts::{self, Binding};
use athanor_layout::loader;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Launcher";

/// The launcher and the bus name it owns, kept for the life of the app.
type Running = (Rc<ui::Launcher>, gtk4::gio::OwnerId);

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
    let shortcuts_dir = shortcuts::dir();
    for dir in std::iter::once(&usage_dir).chain(shortcuts_dir.iter()) {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create a directory the launcher writes");
        }
    }
    let launch_dir = dirs.runtime.join("athanor");
    if let Err(err) = DirBuilder::new().recursive(true).mode(0o700).create(&launch_dir) {
        tracing::warn!(error = %err, dir = %launch_dir.display(), "cannot create the directory of the launch sockets");
    }
    let dconf_dir = dirs.runtime.join("dconf");
    // Before GTK starts a thread (LA9). Writes: the launch sockets, the unit's runtime
    // directory, dconf, the cache (which holds qalc's per-run configuration directory), usage,
    // cosmic-comp's shortcuts and /tmp; reads stay open, because the preview reads files. The
    // launch directory is also where the window capture buffers live. No TCP for the launcher
    // or for qalc and the preview's systemd-run.
    let write: Vec<&Path> = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        usage_dir.as_path(),
        Path::new("/tmp"),
    ]
    .into_iter()
    .chain(shortcuts_dir.iter().map(PathBuf::as_path))
    .collect();
    let devices: Vec<&Path> = athanor_launcher::DEVICE_WRITES.iter().map(Path::new).collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &devices))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the launcher with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    i18n::init();
    // Super is ours only where the user left it to the system (doc_launcher.md, LA8 and §6).
    match shortcuts::set_system_action("Launcher", bus::SHOW_COMMAND) {
        Ok(Binding::Added) => tracing::info!("Super now calls {}; the user's shortcuts had no entry for it", bus::NAME),
        Ok(Binding::ReplacedDefault) => tracing::info!("Super now calls {} instead of the system's default", bus::NAME),
        Ok(Binding::Unchanged) => tracing::info!("Super already calls {}", bus::NAME),
        Ok(Binding::UserChoice) => tracing::info!("Super keeps the command the user chose; it does not call {}", bus::NAME),
        Err(err) => tracing::warn!(error = %err, "Super is not bound to the launcher; it still opens from the bar"),
    }

    let app = Application::builder().application_id(APP_ID).build();
    // Built once; the handle keeps the launcher for the app's lifetime, every watch holds a
    // weak reference (see athanor-bar's main.rs).
    let handle: Rc<RefCell<Option<Running>>> = Rc::default();
    let options = ui::Options { usage_path, given_up };
    app.connect_activate(move |app| {
        if handle.borrow().is_some() {
            return;
        }
        let launcher = ui::start(app, &options);
        let weak = Rc::downgrade(&launcher);
        let owner = bus::own(
            move || {
                if let Some(launcher) = weak.upgrade() {
                    launcher.toggle();
                }
            },
            || {
                if let Err(err) = athanor_unit::notify::notify_ready() {
                    tracing::warn!(error = %err, "cannot tell systemd the launcher is ready");
                }
            },
        );
        handle.replace(Some((launcher, owner)));
    });
    app.run_with_args(&Vec::<String>::new())
}
