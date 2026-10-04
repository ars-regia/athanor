//! athanor-bar: the bar of doc_bar.md. athanor-bar.service runs it. `--record-exit` is the
//! unit's ExecStopPost: it counts a failed run towards the crash-loop limit (doc_shell.md SH8).

mod first_layout;
mod i18n;
mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_compositor_client::theme;
use athanor_layout::favorites;
use athanor_layout::loader::{self, Paths, Source, VENDOR_DIR};
use athanor_layout::user::write_target;
use athanor_services::Runtime;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Bar";

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-bar", |name| env::var_os(name)) else {
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
    // SH8: after five failures the bar still runs, on the vendor layout and without the
    // favourites, the two inputs a user can break. The shell is never lost.
    let (source, favorites_file) = match crash_loop::given_up(&dirs.failures, now) {
        Ok(false) => (
            Source::Live(Paths::for_config_home(&dirs.config)),
            Some(favorites_file),
        ),
        Ok(true) => {
            tracing::error!(
                failures = crash_loop::GIVE_UP_AFTER,
                window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
                "athanor-bar keeps failing; it runs on the vendor layout, without favourites, until the failures leave the window"
            );
            (Source::Vendor(PathBuf::from(VENDOR_DIR)), None)
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return glib::ExitCode::FAILURE;
        }
    };
    // SH10: the first-session pick writes the user's layout file and a marker in the state
    // directory. Only on the live source: a bar in give-up mode leaves the user's files alone.
    let first_session = match &source {
        Source::Live(paths) => match loader::state_home() {
            Some(state) => Some((paths.clone(), state.join("athanor"))),
            None => {
                tracing::error!("no state directory; the first-session pick is skipped");
                None
            }
        },
        Source::Vendor(_) => None,
    };
    // Created before the ruleset, so the grant has a directory to hold on to. A layout file
    // that links elsewhere is written where it points, so that directory is granted.
    let first_session_dirs: Vec<PathBuf> = match &first_session {
        Some((paths, state)) => {
            let layout_dir = match write_target(&paths.user_file) {
                Ok(target) => target
                    .parent()
                    .map_or_else(|| dirs.config.join("athanor"), Path::to_path_buf),
                Err(err) => {
                    tracing::warn!(error = %err, "cannot resolve the layout file; it is written in place");
                    dirs.config.join("athanor")
                }
            };
            for dir in [state, &layout_dir] {
                if let Err(err) = std::fs::create_dir_all(dir) {
                    tracing::warn!(error = %err, dir = %dir.display(), "cannot create a first-session directory");
                }
            }
            vec![state.clone(), layout_dir]
        }
        None => Vec::new(),
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
    // The unit's ConfigurationDirectory= creates these; a run outside it, as in the rig,
    // needs them here.
    let high_contrast_dirs = theme::high_contrast_dirs();
    for dir in &high_contrast_dirs {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create the high contrast directory");
        }
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
    // bar's own runtime directory and dconf, never the rest of the runtime directory (the
    // compositor's and the bus's sockets); the caches, the favourites, the COSMIC high
    // contrast keys, the first-session layout and marker, /tmp, and the DRM nodes.
    let write: Vec<&Path> = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ]
    .into_iter()
    .chain(high_contrast_dirs.iter().map(PathBuf::as_path))
    .chain(first_session_dirs.iter().map(PathBuf::as_path))
    .collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")]));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the bar with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    // After the confinement, which needs a single thread, and before GTK: the models of
    // athanor-services decode the services' replies here, off the interface thread. Without
    // it the modules that read a service are hidden, and the rest of the bar runs.
    match Runtime::start() {
        Ok(runtime) => ui::bridge::install(runtime.handle().clone()),
        Err(err) => tracing::error!(error = %err, "cannot start the runtime of the models"),
    }
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    // A second `athanor-bar` activates this one and exits; the bar is built once. The
    // handle is kept for the app's lifetime: every live-reload watch holds only a weak
    // reference to the bar, so nothing else keeps it alive once `connect_activate`
    // returns.
    let handle: Rc<RefCell<Option<Rc<ui::Bar>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if handle.borrow().is_none() {
            if let (Some((paths, state)), Some(display)) =
                (first_session.clone(), gtk4::gdk::Display::default())
            {
                first_layout::arm(&display, paths, state.join("layout-first-session"));
            }
            let bar = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(bar));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
