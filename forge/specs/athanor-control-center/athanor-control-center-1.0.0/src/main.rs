//! athanor-control-center: the panel of doc_control_center.md. athanor-control-center.service
//! runs it, and the session bus starts that unit when `os.athanor.ControlCenter1` is called
//! (CC2). `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the
//! crash-loop limit (doc_shell.md SH8).

mod bus;
mod i18n;
mod layer_guard;
mod notifications;
mod surface;
mod ui;

use std::cell::RefCell;
use std::env;
use std::rc::Rc;

use athanor_compositor_client::shortcuts::{self, Binding};
use athanor_control_center::grants;
use athanor_layout::loader;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.ControlCenter";

/// The panel and the bus name it owns, kept for the life of the app.
type Running = (Rc<ui::ControlCenter>, gtk4::gio::OwnerId);

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
    // Created before the ruleset, so the grant has a directory to hold on to.
    let Some(state) = loader::state_home().map(|home| home.join("athanor/control-center")) else {
        tracing::error!("no state directory to write the control center's state to");
        return glib::ExitCode::FAILURE;
    };
    if let Err(err) = std::fs::create_dir_all(&state) {
        tracing::warn!(error = %err, dir = %state.display(), "cannot create the state directory");
    }
    // Super+C is ours only where the user left it free, and only at the first start (CC9):
    // the marker sits in the state directory. It runs before the ruleset, which leaves
    // cosmic-comp's shortcuts read-only. A failing crash-loop gives up nothing here: the
    // panel has no input from outside the image.
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
    // Super+N, the notification center's, the same way (NC1).
    match shortcuts::set_custom_binding_once(
        &["Super"],
        "n",
        bus::TOGGLE_NOTIFICATIONS_COMMAND,
        &state.join("super-n-bound"),
    ) {
        Ok(None) => tracing::info!("Super+N was bound once already"),
        Ok(Some(Binding::Added)) => tracing::info!("Super+N now calls {}", bus::NAME),
        Ok(Some(Binding::Unchanged)) => tracing::info!("Super+N already calls {}", bus::NAME),
        Ok(Some(Binding::UserChoice | Binding::ReplacedDefault)) => {
            tracing::info!(
                "Super+N keeps the action the user chose; it does not call {}",
                bus::NAME
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "Super+N is not bound to the notification center; it still opens from the bar")
        }
    }
    // Before GTK starts a thread. Reads stay open (CC2); writes only where `grants` says.
    let granted = grants(&dirs, &state);
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
    // After the confinement, as the bar does: the models run on a thread of their own.
    let services = match athanor_services::Runtime::start() {
        Ok(runtime) => {
            let handle = runtime.handle().clone();
            Some((handle.clone(), athanor_services::Buses::new(handle)))
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot start the runtime of the models; the notification panel is unavailable");
            None
        }
    };
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    let handle: Rc<RefCell<Option<Running>>> = Rc::default();
    let layout = ui::live_layout(&dirs.config);
    app.connect_activate(move |app| {
        if handle.borrow().is_some() {
            return;
        }
        let bus = Rc::new(bus::Bus::default());
        let center = ui::start(app, bus.clone(), layout.clone(), services.clone());
        let (shown, toggled) = (Rc::downgrade(&center), Rc::downgrade(&center));
        let owner = bus::own(
            bus,
            move |page| match shown.upgrade() {
                Some(center) => center.show_page(page),
                None => Err("the control center is closing".to_owned()),
            },
            move |panel| {
                if let Some(center) = toggled.upgrade() {
                    center.toggle(panel);
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
