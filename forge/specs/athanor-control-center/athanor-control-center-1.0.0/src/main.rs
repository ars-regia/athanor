//! athanor-control-center: the panel of doc_control_center.md. athanor-control-center.service
//! runs it, and the session bus starts that unit when `os.athanor.ControlCenter1` is called
//! (CC2). `--bind-shortcut` is the login oneshot that binds Super+C and exits. `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the
//! crash-loop limit (doc_shell.md SH8).

mod bus;
mod i18n;
mod layer_guard;
mod panel;
mod surface;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_compositor_client::shortcuts::{self, Binding};
use athanor_compositor_client::theme;
use athanor_services::battery::BACKLIGHT_ROOT;
use athanor_services::Runtime;
use athanor_control_center::{grants, launch_dir, parse_mode, Mode, RFKILL};
use athanor_layout::loader;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.ControlCenter";

/// The panel and the bus name it owns, kept for the life of the app.
type Running = (Rc<ui::ControlCenter>, gtk4::gio::OwnerId);

/// The bar's `ATHANOR_BAR_BACKLIGHT_DIR` for the rig, else sysfs.
fn backlight_root() -> PathBuf {
    env::var_os("ATHANOR_CONTROL_CENTER_BACKLIGHT_DIR")
        .map_or_else(|| PathBuf::from(BACKLIGHT_ROOT), PathBuf::from)
}

/// Super+C is ours only where the user left it free, and only once per user (CC9): the marker
/// sits in the state directory. Only the login oneshot calls it: the panel's unit cannot
/// write cosmic-comp's shortcuts.
fn bind_shortcut(state: &Path) {
    match shortcuts::set_custom_binding_once(
        &["Super"],
        "c",
        bus::TOGGLE_COMMAND,
        &state.join("super-c-bound"),
    ) {
        Ok(None) => tracing::info!("Super+C was bound once already"),
        Ok(Some(Binding::Added)) => tracing::info!("Super+C now calls {}", bus::NAME),
        Ok(Some(Binding::Unchanged)) => tracing::info!("Super+C already calls {}", bus::NAME),
        Ok(Some(Binding::UserChoice | Binding::ReplacedDefault)) => {
            tracing::info!(
                "Super+C keeps the action the user chose; it does not call {}",
                bus::NAME
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "Super+C is not bound to the control center; it still opens from the bar")
        }
    }
}

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-control-center", |name| env::var_os(name)) else {
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
    let mode = parse_mode(env::args().nth(1).as_deref());
    if mode == Mode::BindShortcut {
        // The login oneshot (CC9): no crash-loop record, no GTK, no bus name.
        let Some(state) = loader::state_home().map(|home| home.join("athanor/control-center")) else {
            tracing::error!("no state directory to keep the Super+C marker in");
            return glib::ExitCode::FAILURE;
        };
        if let Err(err) = std::fs::create_dir_all(&state) {
            tracing::warn!(error = %err, dir = %state.display(), "cannot create the state directory");
        }
        bind_shortcut(&state);
        return glib::ExitCode::SUCCESS;
    }
    if mode == Mode::RecordExit {
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
    // Created before the ruleset, so the grant has a directory to hold on to.
    let Some(state) = loader::state_home().map(|home| home.join("athanor/control-center")) else {
        tracing::error!("no state directory to write the control center's state to");
        return glib::ExitCode::FAILURE;
    };
    if let Err(err) = std::fs::create_dir_all(&state) {
        tracing::warn!(error = %err, dir = %state.display(), "cannot create the state directory");
    }
    // Before GTK starts a thread. Reads stay open (CC2); writes only where `grants` says.
    let launch = launch_dir(&dirs);
    if let Err(err) = DirBuilder::new().recursive(true).mode(0o700).create(&launch) {
        tracing::warn!(error = %err, dir = %launch.display(), "cannot create the directory of the launch sockets");
    }
    // The unit's ConfigurationDirectory= creates it; a run outside it needs it here.
    let mode_dir = theme::mode_dir();
    if let Some(dir) = &mode_dir {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create the theme mode directory");
        }
    }
    let rfkill_present = Path::new(RFKILL).exists();
    let granted = grants(&dirs, &state, mode_dir.as_deref(), rfkill_present);
    let write: Vec<&std::path::Path> = granted
        .write
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();
    let devices: Vec<&std::path::Path> = granted
        .devices
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &devices))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the control center with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    // After the confinement, which needs a single thread, and before GTK: the models decode
    // the services' replies on this runtime. Without it the tiles that need one are not built.
    match Runtime::start() {
        Ok(runtime) => athanor_controls::bridge::install(runtime.handle().clone(), backlight_root()),
        Err(err) => tracing::error!(error = %err, "cannot start the runtime of the models"),
    }
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    let handle: Rc<RefCell<Option<Running>>> = Rc::default();
    let layout = ui::live_layout(&dirs.config);
    app.connect_activate(move |app| {
        if handle.borrow().is_some() {
            return;
        }
        let bus = Rc::new(bus::Bus::default());
        let center = ui::start(app, bus.clone(), layout.clone(), panel::open_rfkill());
        let (shown, toggled) = (Rc::downgrade(&center), Rc::downgrade(&center));
        let owner = bus::own(
            bus,
            move |page| match shown.upgrade() {
                Some(center) => center.show_page(page),
                None => Err("the control center is closing".to_owned()),
            },
            move || {
                if let Some(center) = toggled.upgrade() {
                    center.toggle();
                }
            },
            || {
                if let Err(err) = athanor_unit::notify::notify_ready() {
                    tracing::warn!(error = %err, "cannot tell systemd the control center is ready");
                }
            },
        );
        handle.replace(Some((center, owner)));
    });
    app.run_with_args(&Vec::<String>::new())
}
