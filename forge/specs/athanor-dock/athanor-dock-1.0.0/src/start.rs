//! What every dock does before it draws (doc_bar.md, BR7; doc_shell.md SH8): the journal,
//! `--record-exit` and the crash-loop record, the layout's source and the favourites file,
//! Landlock with the dock's write list, and the translations. The GTK dock and the
//! toolkit dock share it, so their inputs and their confinement stay one.

use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use athanor_layout::favorites;
use athanor_layout::loader::{Paths, Source, VENDOR_DIR};
use athanor_layout::user::write_target;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};

use crate::i18n;

/// The outcome of [`prepare`].
pub enum Prepared {
    /// Confined and translated: the dock runs on this layout and these favourites, the
    /// favourites `None` once the dock keeps failing.
    Run {
        source: Source,
        favorites_file: Option<PathBuf>,
    },
    /// The process ends here: `true` after `--record-exit` recorded the run, `false` on a
    /// failure, already logged.
    Exit(bool),
}

/// Runs before any thread starts: Landlock refuses a process that has one.
pub fn prepare() -> Prepared {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-dock", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return Prepared::Exit(false);
    };
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return Prepared::Exit(false);
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => Prepared::Exit(true),
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                Prepared::Exit(false)
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&dirs.failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return Prepared::Exit(false);
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
            return Prepared::Exit(false);
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
    // Writes only (athanor-unit::sandbox): the launch sockets, the dock's own runtime
    // directory and dconf, never the rest of the runtime directory (the compositor's and
    // the bus's sockets); the cache, the favourites, /tmp, and the DRM nodes.
    let write: [&Path; 6] = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ];
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")]));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the dock with Landlock; refusing to run unconfined");
        return Prepared::Exit(false);
    }
    i18n::init();
    Prepared::Run {
        source,
        favorites_file,
    }
}
