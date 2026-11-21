//! athanor-settings: Settings (doc_settings.md). Inside `athanor-settings.service` it owns
//! `os.athanor.Settings1` and opens its window at the first `Show`, or at the target a crashed
//! run left in its state file (SE5). Started anywhere else, by the bar's application unit or a
//! command line, it forwards `--show=<target>` to that name and exits (SE4).

mod bus;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

use athanor_sal::driver;
use athanor_settings::window::{self, First, Settings};
use athanor_settings::{i18n, info, look};
use athanor_settings_core::registry::Registry;
use athanor_settings_core::target;
use athanor_settings_core::window_state;
use athanor_style::appearance::Appearance;
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_unit::dirs::{self, Dirs};
use athanor_unit::{journal, sandbox};

const UNIT: &str = "athanor-settings.service";

/// Whether `/proc/self/cgroup` puts this process in Settings' unit: the last component of its
/// unified-hierarchy path.
fn in_unit(cgroup: &str) -> bool {
    cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .and_then(|path| path.trim_end().rsplit('/').next())
        == Some(UNIT)
}

/// The target of `--show=<target>`, or the argument that is not one, as text. An argument
/// that is not UTF-8 is not one: `env::args` would panic on it.
fn show_target(args: impl IntoIterator<Item = OsString>) -> Result<String, String> {
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|arg| arg.to_string_lossy().into_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    target::from_args(args)
}

fn forward() -> ExitCode {
    let target = match show_target(env::args_os().skip(1)) {
        Ok(target) => target,
        Err(argument) => {
            tracing::error!(%argument, "usage: athanor-settings [--show=<target>]");
            return ExitCode::from(2);
        }
    };
    let token = env::var("XDG_ACTIVATION_TOKEN").ok();
    match bus::forward(&target, token.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "cannot reach {}", bus::NAME);
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    athanor_unit::panic::install();
    journal::init();
    // Before any toolkit or Wayland connection: the window runs only in its unit (SE4).
    match fs::read_to_string("/proc/self/cgroup") {
        Ok(cgroup) if in_unit(&cgroup) => {}
        Ok(_) => return forward(),
        Err(error) => {
            // Neither the window, which may not run outside the unit, nor the forwarder,
            // which from inside it would wait on its own activation.
            tracing::error!(%error, "cannot read /proc/self/cgroup");
            return ExitCode::FAILURE;
        }
    }
    let Some(dirs) = Dirs::from_vars("athanor-settings", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the cache");
        return ExitCode::FAILURE;
    };
    // SE6: before any thread. Step 1 writes its state file and dconf's flag under the runtime
    // directory, and fontconfig's cache; the other grants of SE6 come with the pages that write.
    let mut write = dirs::private_dirs(&dirs.runtime, &["athanor-settings", "dconf"]);
    write.extend(dirs::existing(&[dirs.cache.as_path()], dirs::owned_dir));
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[]))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine Settings with Landlock; refusing to run unconfined");
        return ExitCode::FAILURE;
    }
    i18n::init();
    let registry = Registry::new();
    let state_dir = dirs.unit_runtime.clone();
    // A run that ended with its window open reopens it (SE5).
    let reopen = window_state::reopen(&registry, &state_dir).unwrap_or_else(|error| {
        tracing::warn!(%error, "cannot read the window state; nothing is reopened");
        None
    });

    let shows: Rc<RefCell<VecDeque<String>>> = Rc::default();
    // Held for the life of the process: dropping it releases the name.
    let _owner = bus::own(shows.clone(), || {
        if let Err(err) = athanor_unit::notify::notify_ready() {
            tracing::warn!(error = %err, "cannot tell systemd Settings is ready");
        }
    });
    let first = match reopen {
        // A page a later build no longer draws opens the landing page, as a Show of it would.
        Some(target) => First::Reopen(window::resolve(&registry, &target.to_string())),
        None => First::Show(loop {
            if let Some(target) = window::take_show(&registry, &shows) {
                break target;
            }
            glib::MainContext::default().iteration(true);
        }),
    };

    let mut first = first;
    // A `Show` that was accepted always opens a window: one that arrived while the last window
    // was closing is still queued when the run ends, so the next run opens it.
    loop {
        let glib = match GlibSource::new() {
            Ok(glib) => glib,
            Err(error) => {
                tracing::error!(%error, "cannot run GLib's main context");
                return ExitCode::FAILURE;
            }
        };
        let info = Rc::default();
        info::fetch(&info);
        let settings = Settings::new(
            Registry::new(),
            look::look(&Appearance::read()),
            i18n::is_rtl(),
            first,
            info,
            shows.clone(),
            Some(state_dir.clone()),
        );
        // ponytail: the run ends with the window until Sal keeps a run without one (SL1); each
        // opening is then a cold start, and the unit is started again by the bus.
        if let Err(error) = driver::run_with(settings, glib) {
            tracing::error!(%error, "Settings stopped");
            return ExitCode::FAILURE;
        }
        if let Err(error) = window_state::clear(&state_dir) {
            tracing::warn!(%error, "cannot clear the window state; a restart reopens it");
        }
        // Dispatch the calls the bus has already read, so the queue is complete.
        let context = glib::MainContext::default();
        while context.iteration(false) {}
        match window::take_show(&Registry::new(), &shows) {
            Some(target) => first = First::Show(target),
            None => return ExitCode::SUCCESS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_unit_cgroup_runs_the_window() {
        let unit =
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-settings.service\n";
        assert!(in_unit(unit));
        let bar = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-os.athanor.Settings-4242.scope\n";
        assert!(!in_unit(bar));
        assert!(!in_unit("0::/user.slice/athanor-settings.service/sub\n"));
        assert!(!in_unit("1:name=systemd:/athanor-settings.service\n"));
        assert!(!in_unit(""));
    }

    #[test]
    fn an_argument_that_is_not_utf8_is_a_usage_error() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"--show=\xff".to_vec());
        assert_eq!(show_target([bad]), Err("--show=\u{fffd}".to_owned()));
        assert_eq!(
            show_target([OsString::from("--show=about")]),
            Ok("about".to_owned())
        );
        assert_eq!(show_target([]), Ok(String::new()));
    }
}
