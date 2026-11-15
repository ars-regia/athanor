//! What every bar does before it draws (doc_bar.md; doc_shell.md SH8, SH10): the journal,
//! `--record-exit` and the crash-loop record, the layout's source, the favourites file and
//! the first-session pick, Landlock with the bar's write list, the models' runtime and the
//! translations. The GTK bar and the toolkit bar share it, so their inputs and their
//! confinement stay one.

use std::env;
use std::path::{Path, PathBuf};

use athanor_layout::favorites;
use athanor_layout::first_session::{self, Outcome};
use athanor_layout::loader::{self, Paths, Source, VENDOR_DIR};
use athanor_layout::placement::Output;
use athanor_layout::user::write_target;
use athanor_services::battery::BACKLIGHT_ROOT;
use athanor_services::Runtime;
use athanor_sulphur_client::theme;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, dirs, journal, sandbox};

use crate::i18n;

/// The outcome of [`prepare`].
pub enum Prepared {
    /// Confined and translated: the bar runs on this layout and these favourites, the
    /// favourites `None` once the bar keeps failing. `first_session` holds the layout paths
    /// and the state directory of the first-session pick, `None` when it is skipped.
    Run {
        source: Source,
        favorites_file: Option<PathBuf>,
        first_session: Option<(Paths, PathBuf)>,
    },
    /// The process ends here: `true` after `--record-exit` recorded the run, `false` on a
    /// failure, already logged.
    Exit(bool),
}

/// Where the battery page reads the backlight. The rig points the bar at a fake sysfs
/// directory; the level still goes through logind, which checks the device itself.
fn backlight_root() -> PathBuf {
    env::var_os("ATHANOR_BAR_BACKLIGHT_DIR")
        .map_or_else(|| PathBuf::from(BACKLIGHT_ROOT), PathBuf::from)
}

/// Runs before any thread starts: Landlock refuses a process that has one.
pub fn prepare() -> Prepared {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-bar", |name| env::var_os(name)) else {
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
            return Prepared::Exit(false);
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
    let cosmic_dirs = theme::theme_dirs();
    for dir in &cosmic_dirs {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create the COSMIC theme directory");
        }
    }
    // Before any thread starts. Writes only (athanor-unit::sandbox): the launch sockets, the
    // bar's own runtime directory and dconf, never the rest of the runtime directory (the
    // compositor's and the bus's sockets); the caches, the favourites, the COSMIC theme
    // keys, the first-session layout and marker, /tmp, and the DRM nodes. Each is granted by
    // the descriptor its check opened: the parent of the launch sockets (BR2.2), made the way
    // launch() makes it, and dconf's are private to the user (`dirs::private_dir`).
    let mut write = dirs::private_dirs(&dirs.runtime, &["athanor", "dconf"]);
    let checked: Vec<&Path> = [
        dirs.unit_runtime.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ]
    .into_iter()
    .chain(cosmic_dirs.iter().map(PathBuf::as_path))
    .chain(first_session_dirs.iter().map(PathBuf::as_path))
    .collect();
    write.extend(dirs::existing(&checked, dirs::owned_dir));
    let devices = dirs::existing(&[Path::new("/dev/dri")], dirs::owned_node);
    let confined =
        sandbox::ensure_single_threaded().and_then(|()| sandbox::restrict_writes(&write, &devices));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the bar with Landlock; refusing to run unconfined");
        return Prepared::Exit(false);
    }
    // After the confinement, which needs a single thread, and before the interface: the
    // models of athanor-services decode the services' replies here, off the interface thread.
    // Without it the modules that read a service are hidden, and the rest of the bar runs.
    match Runtime::start() {
        Ok(runtime) => {
            athanor_controls::bridge::install(runtime.handle().clone(), backlight_root())
        }
        Err(err) => tracing::error!(error = %err, "cannot start the runtime of the models"),
    }
    i18n::init();
    Prepared::Run {
        source,
        favorites_file,
        first_session,
    }
}

/// SH10, one attempt at the first-session pick on `outputs`: the preset they suit, written
/// to the user's layout file once, under the marker the translator used until the switch of
/// stage 2, so a desktop upgraded from it never picks again. True when nothing is left to
/// do in this process; false while no output is sized yet.
pub fn pick_first_layout(paths: &Paths, marker: &Path, outputs: &[Output]) -> bool {
    match first_session::run(&loader::resolve(paths), &paths.user_file, marker, outputs) {
        Ok(Outcome::NotYet) => false,
        Ok(Outcome::Wrote(preset)) => {
            tracing::info!(preset = preset.id(), "first session: default layout picked");
            true
        }
        Ok(Outcome::AlreadyRan | Outcome::LeftToPolicyOrUser) => true,
        Err(err) => {
            tracing::error!(error = %err, "cannot record the first-session layout; the vendor layout applies");
            true
        }
    }
}

/// The marker of the first-session pick in `state`, the bar's state directory.
pub fn first_session_marker(state: &Path) -> PathBuf {
    state.join("layout-first-session")
}
