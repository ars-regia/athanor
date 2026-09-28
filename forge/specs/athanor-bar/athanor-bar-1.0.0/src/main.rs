//! athanor-bar: the bar of doc_bar.md. athanor-bar.service runs it. `--record-exit` is the
//! unit's ExecStopPost: it counts a failed run towards the crash-loop limit (doc_shell.md SH8).

mod i18n;
mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_bar::dirs::Dirs;
use athanor_compositor_client::theme;
use athanor_layout::favorites;
use athanor_layout::loader::{Paths, VENDOR_DIR};
use athanor_layout::user::write_target;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

use crate::ui::Source;

const APP_ID: &str = "os.athanor.Bar";

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars(|name| env::var_os(name)) else {
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
    let high_contrast_dirs = theme::high_contrast_dirs();
    for dir in &high_contrast_dirs {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create the high contrast directory");
        }
    }
    // Before GTK starts a thread. Writes only (athanor-unit::sandbox): the launch sockets and
    // dconf under the runtime directory, the caches, the favourites, the COSMIC high
    // contrast keys, /tmp, and the DRM nodes.
    let write: Vec<&Path> = [
        dirs.runtime.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ]
    .into_iter()
    .chain(high_contrast_dirs.iter().map(PathBuf::as_path))
    .collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")]));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the bar with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
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
            let bar = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(bar));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
