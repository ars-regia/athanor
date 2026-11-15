//! athanor-control-center: the panel of doc_control_center.md. athanor-control-center.service
//! runs it, and the session bus starts that unit when `os.athanor.ControlCenter1` is called
//! (CC2). `--bind-shortcut` is the login oneshot that binds Super+C and exits. `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the
//! crash-loop limit (doc_shell.md SH8).

mod airplane;
mod bus;
mod center;
mod i18n;
mod icons;
mod media;
mod panel;

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use athanor_control_center::{grants, launch_dir, parse_mode, parse_page, Mode, RFKILL};
use athanor_layout::loader::{self, Paths, Source};
use athanor_sal::driver;
use athanor_services::battery::BACKLIGHT_ROOT;
use athanor_services::Runtime;
use athanor_style::appearance::Appearance;
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_sulphur_client::shortcuts::{self, Binding};
use athanor_sulphur_client::{theme, Client};
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, dirs, journal, sandbox};
use wayland_client::Connection;

use crate::center::{Center, Input};
use crate::panel::Panel;

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

fn main() -> ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-control-center", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return ExitCode::FAILURE;
    };
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return ExitCode::FAILURE;
        }
    };
    let mode = parse_mode(env::args().nth(1).as_deref());
    if mode == Mode::BindShortcut {
        // The login oneshot (CC9): no crash-loop record, no surface, no bus name.
        let Some(state) = loader::state_home().map(|home| home.join("athanor/control-center")) else {
            tracing::error!("no state directory to keep the Super+C marker in");
            return ExitCode::FAILURE;
        };
        if let Err(err) = std::fs::create_dir_all(&state) {
            tracing::warn!(error = %err, dir = %state.display(), "cannot create the state directory");
        }
        bind_shortcut(&state);
        return ExitCode::SUCCESS;
    }
    if mode == Mode::RecordExit {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&dirs.failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return ExitCode::FAILURE;
    }
    // Created before the ruleset, so the grant has a directory to hold on to.
    let Some(state) = loader::state_home().map(|home| home.join("athanor/control-center")) else {
        tracing::error!("no state directory to write the control center's state to");
        return ExitCode::FAILURE;
    };
    if let Err(err) = std::fs::create_dir_all(&state) {
        tracing::warn!(error = %err, dir = %state.display(), "cannot create the state directory");
    }
    // Before a thread starts. Reads stay open (CC2); writes only where `grants` says.
    // The unit's ConfigurationDirectory= creates it; a run outside it needs it here.
    let theme_dirs = theme::theme_dirs();
    for dir in &theme_dirs {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create the COSMIC theme directory");
        }
    }
    let rfkill_present = Path::new(RFKILL).exists();
    let granted = grants(&dirs, &state, &theme_dirs, rfkill_present);
    // Each is granted by the descriptor its check opened. The launch directory, made the way
    // launch() makes it, and dconf's are private to the user (`dirs::private_dir`).
    let private = [launch_dir(&dirs), dirs.runtime.join("dconf")];
    let names: Vec<&str> = private
        .iter()
        .filter_map(|dir| dir.file_name().and_then(|name| name.to_str()))
        .collect();
    let mut write = dirs::private_dirs(&dirs.runtime, &names);
    let checked: Vec<&Path> = granted
        .write
        .iter()
        .filter(|dir| !private.contains(dir))
        .map(PathBuf::as_path)
        .collect();
    write.extend(dirs::existing(&checked, dirs::owned_dir));
    let devices: Vec<&Path> = granted.devices.iter().map(PathBuf::as_path).collect();
    let devices = dirs::existing(&devices, dirs::owned_node);
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &devices))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the control center with Landlock; refusing to run unconfined");
        return ExitCode::FAILURE;
    }
    // After the confinement, which needs a single thread: the models decode
    // the services' replies on this runtime. Without it the tiles that need one are not built.
    match Runtime::start() {
        Ok(runtime) => athanor_controls::bridge::install(runtime.handle().clone(), backlight_root()),
        Err(err) => tracing::error!(error = %err, "cannot start the runtime of the models"),
    }
    i18n::init();

    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(error) => {
            tracing::error!(%error, "cannot run GLib's main context");
            return ExitCode::FAILURE;
        }
    };
    let inputs: Rc<RefCell<Vec<Input>>> = Rc::default();
    let bus = Rc::new(bus::Bus::default());
    let (shown, toggled) = (inputs.clone(), inputs.clone());
    // Held for the life of the process: dropping it releases the name.
    let _owner = bus::own(
        bus.clone(),
        move |id| {
            let page = parse_page(id)?;
            shown.borrow_mut().push(Input::Show(page));
            Ok(())
        },
        move || toggled.borrow_mut().push(Input::Toggle),
        || {
            if let Err(err) = athanor_unit::notify::notify_ready() {
                tracing::warn!(error = %err, "cannot tell systemd the control center is ready");
            }
        },
    );
    let client = Connection::connect_to_env()
        .map_err(|error| error.to_string())
        .and_then(|connection| {
            Client::connect_wayland(&connection).map_err(|error| error.to_string())
        });
    let client = match client {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::error!(%error, "no compositor client; the panel opens on the first output");
            None
        }
    };
    let panel = Panel::new(
        athanor_controls::bridge::services(),
        airplane::open(),
        Appearance::read().dark,
    );
    let layout = Source::Live(Paths::for_config_home(&dirs.config));
    match driver::run_with(Center::new(inputs, client, layout, bus, panel), glib) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the control center stopped");
            ExitCode::FAILURE
        }
    }
}
