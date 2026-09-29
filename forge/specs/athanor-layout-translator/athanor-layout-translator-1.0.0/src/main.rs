//! athanor-layout-translator: the layout document (doc_shell.md, SH6-SH8, SH10) rendered
//! as cosmic-panel configuration.
//!
//! athanor-layout.service starts it before cosmic-panel. `--record-exit` is the unit's
//! ExecStopPost: it counts a failed run towards the crash-loop limit.

mod journal;
mod resident;
mod supervision;

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use athanor_layout::apply::{self, Applied};
use athanor_layout::placement::Output;
use athanor_layout::{cosmic, first_session, loader};
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

/// Every path the translator reads or writes.
pub(crate) struct Dirs {
    pub(crate) paths: loader::Paths,
    /// `$XDG_CONFIG_HOME/cosmic`.
    pub(crate) cosmic: PathBuf,
    /// The last plan written.
    pub(crate) record: PathBuf,
    /// The first-session marker (SH10).
    pub(crate) marker: PathBuf,
    /// The crash-loop record.
    pub(crate) failures: PathBuf,
}

impl Dirs {
    fn from_env() -> Option<Dirs> {
        let config = loader::config_home()?;
        let state = loader::state_home()?.join("athanor");
        let runtime = env::var_os("RUNTIME_DIRECTORY")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("athanor-layout"))
            })
            .filter(|dir| dir.is_absolute())?;
        Some(Dirs {
            paths: loader::Paths::for_config_home(&config),
            cosmic: config.join("cosmic"),
            record: state.join("layout-cosmic-panel"),
            marker: state.join("layout-first-session"),
            failures: runtime.join("failures"),
        })
    }
}

fn main() -> ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_env() else {
        tracing::error!("no absolute XDG_CONFIG_HOME, XDG_STATE_HOME, XDG_RUNTIME_DIR or HOME");
        return ExitCode::FAILURE;
    };
    let now = match supervision::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match supervision::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = supervision::record_start(&dirs.failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return ExitCode::FAILURE;
    }
    match supervision::given_up(&dirs.failures, now) {
        Ok(true) => return give_up(&dirs),
        Ok(false) => {}
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return ExitCode::FAILURE;
        }
    }
    if let Err(err) = gtk4::init() {
        tracing::error!(error = %err, "cannot connect to the display");
        return ExitCode::FAILURE;
    }
    let Some(display) = gdk::Display::default() else {
        tracing::error!("GTK has no default display");
        return ExitCode::FAILURE;
    };
    let _resident = match resident::Resident::start(dirs, display) {
        Ok(resident) => resident,
        Err(err) => {
            tracing::error!(error = %err, "cannot apply the layout");
            return ExitCode::FAILURE;
        }
    };
    glib::MainLoop::new(None, false).run();
    ExitCode::SUCCESS
}

/// One pass: the first-session pick, then the layout rendered and applied.
pub(crate) fn pass(dirs: &Dirs, outputs: &[Output]) -> io::Result<Applied> {
    let mut resolved = loader::resolve(&dirs.paths);
    let outcome = first_session::run(&resolved, &dirs.paths.user_file, &dirs.marker, outputs)?;
    if let first_session::Outcome::Wrote(preset) = outcome {
        tracing::info!(preset = preset.id(), "first session: default layout picked");
        resolved = loader::resolve(&dirs.paths);
    }
    let plan = cosmic::render(&resolved.layout, outputs);
    let applied = apply::apply(&plan, &dirs.cosmic, &dirs.record)?;
    if !applied.written.is_empty() {
        tracing::info!(files = applied.written.len(), layout = ?resolved.layout, "cosmic-panel configuration updated");
    }
    if applied.restart_panel {
        restart_panel();
    }
    Ok(applied)
}

/// The outputs as GDK reports them, in logical pixels.
pub(crate) fn outputs(display: &gdk::Display) -> Vec<Output> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
        .map(|monitor| {
            let geometry = monitor.geometry();
            Output {
                connector: monitor.connector().map(|connector| connector.to_string()),
                width: geometry.width(),
                height: geometry.height(),
            }
        })
        .collect()
}

pub(crate) fn notify_ready() {
    if let Err(err) = supervision::notify_ready() {
        tracing::error!(error = %err, "cannot tell systemd that the layout is applied");
    }
}

/// cosmic-panel 1.8.0 binds an entry pinned to a named output only when it starts.
/// `try-restart` does nothing while the panel is not running, as at login, where this
/// unit is ordered before it.
fn restart_panel() {
    match Command::new("systemctl")
        .args(["--user", "try-restart", "cosmic-panel.service"])
        .status()
    {
        Ok(status) if status.success() => {
            tracing::info!("cosmic-panel restarted to show one dock per output")
        }
        Ok(status) => {
            tracing::error!(%status, "cosmic-panel was not restarted; its per-output docks appear at its next start")
        }
        Err(err) => tracing::error!(error = %err, "cannot run systemctl to restart cosmic-panel"),
    }
}

/// The crash-loop limit was reached: the vendor layout alone, then stop until the next
/// session (SH8). Placed for landscape, whatever the outputs are: this path must not
/// depend on anything that might be what keeps failing.
fn give_up(dirs: &Dirs) -> ExitCode {
    tracing::error!(
        failures = supervision::GIVE_UP_AFTER,
        window_seconds = supervision::FAILURE_WINDOW_SECONDS,
        "the layout translator keeps failing; the vendor layout applies until the next session"
    );
    let plan = cosmic::render(&loader::vendor_layout(&dirs.paths.vendor_dir), &[]);
    let status = match apply::apply(&plan, &dirs.cosmic, &dirs.record) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "cannot write the vendor layout");
            ExitCode::FAILURE
        }
    };
    notify_ready();
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dirs(name: &str) -> Dirs {
        let base = env::temp_dir().join(format!(
            "athanor-layout-translator-{}-{name}",
            std::process::id()
        ));
        Dirs {
            paths: loader::Paths {
                vendor_dir: base.join("vendor"),
                policy_dir: base.join("policy"),
                user_file: base.join("config/athanor/layout.toml"),
            },
            cosmic: base.join("config/cosmic"),
            record: base.join("state/athanor/layout-cosmic-panel"),
            marker: base.join("state/athanor/layout-first-session"),
            failures: base.join("run/failures"),
        }
    }

    #[test]
    fn a_first_pass_on_a_small_screen_applies_the_bar_and_a_second_writes_nothing() {
        let dirs = dirs("small-screen");
        let screens = [Output {
            connector: Some("eDP-1".into()),
            width: 1366,
            height: 768,
        }];
        let first = pass(&dirs, &screens).expect("first pass");
        assert!(!first.written.is_empty());
        let read = |path: &str| fs::read_to_string(dirs.cosmic.join(path)).expect("key file");
        assert_eq!(read("com.system76.CosmicPanel/v1/entries"), "[\"Panel\"]");
        assert_eq!(read("com.system76.CosmicPanel.Panel/v1/size"), "M");
        assert!(dirs.marker.exists());
        assert!(pass(&dirs, &screens)
            .expect("second pass")
            .written
            .is_empty());
    }
}
