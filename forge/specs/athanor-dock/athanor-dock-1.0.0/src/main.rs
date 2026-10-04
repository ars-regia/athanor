//! athanor-dock: the dock of doc_bar.md, BR7. athanor-dock.service runs it. `--record-exit`
//! is the unit's ExecStopPost: it counts a failed run towards the crash-loop limit
//! (doc_shell.md SH8).

mod i18n;
mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_layout::favorites;
use athanor_layout::loader::{Paths, Source, VENDOR_DIR};
use athanor_layout::user::write_target;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Dock";

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-dock", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return glib::ExitCode::FAILURE;
    };
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
    let favorites_file = favorites::user_file(&dirs.config);
    // SH8: after five failures the dock still runs, on the vendor layout and without the
    // favourites, the two inputs a user can break.
    let (source, favorites_file) = match crash_loop::given_up(&dirs.failures, now) {
        Ok(false) => (
            Source::Live(Paths::for_config_home(&dirs.config)),
            Some(favorites_file),
        ),
        Ok(true) => {
            tracing::error!(
                failures = crash_loop::GIVE_UP_AFTER,
                window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
                "athanor-dock keeps failing; it runs on the vendor layout, without favourites, until the failures leave the window"
            );
            (Source::Vendor(PathBuf::from(VENDOR_DIR)), None)
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return glib::ExitCode::FAILURE;
        }
    };
    // Created before the ruleset, so the grant has a directory to hold on to. A favourites
    // file that links elsewhere is written where it points, so that directory is granted.
    let favorites_dir = match write_target(&favorites::user_file(&dirs.config)) {
        Ok(target) => target
            .parent()
            .map_or_else(|| dirs.config.join("athanor"), Path::to_path_buf),
        Err(err) => {
            tracing::warn!(error = %err, "cannot resolve the favourites file; it is written in place");
            dirs.config.join("athanor")
        }
    };
    if let Err(err) = std::fs::create_dir_all(&favorites_dir) {
        tracing::warn!(error = %err, dir = %favorites_dir.display(), "cannot create the favourites directory");
    }
    // The parent of the launch sockets (BR2.2), made the way launch() makes it.
    let launch_dir = dirs.runtime.join("athanor");
    if let Err(err) = DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&launch_dir)
    {
        tracing::warn!(error = %err, dir = %launch_dir.display(), "cannot create the directory of the launch sockets");
    }
    let dconf_dir = dirs.runtime.join("dconf");
    // Before GTK starts a thread. Writes only (athanor-unit::sandbox): the launch sockets, the
    // dock's own runtime directory and dconf, never the rest of the runtime directory (the
    // compositor's and the bus's sockets); the cache, the favourites, /tmp, and the DRM
    // nodes.
    let write: [&Path; 6] = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ];
    // Connections: the runtime directory (the compositor, the accessibility bus) and the
    // session bus.
    let session_bus = sandbox::session_bus();
    let connect: Vec<&Path> = std::iter::once(dirs.runtime.as_path())
        .chain(session_bus.iter().map(PathBuf::as_path))
        .collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")], &connect));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the dock with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    // A second `athanor-dock` activates this one and exits; the dock is built once. The
    // handle is kept for the app's lifetime: every watch holds only a weak reference to
    // the dock.
    let handle: Rc<RefCell<Option<Rc<ui::Dock>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if handle.borrow().is_none() {
            let dock = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(dock));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
