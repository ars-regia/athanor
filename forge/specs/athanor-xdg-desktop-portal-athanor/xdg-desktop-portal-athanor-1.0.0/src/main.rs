//! xdg-desktop-portal-athanor, Athanor's backend for xdg-desktop-portal (doc_portal.md).
//!
//! It serves `org.freedesktop.impl.portal.Settings` (PT5). The main thread runs the GLib
//! main loop that GSettings reports changes on; D-Bus runs on a Tokio thread of its own
//! (PT2). Landlock confines the process before either thread starts (PT3).

mod caller;
mod settings;

use std::env;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, RwLock};

use athanor_unit::{journal, sandbox};
use tokio::sync::mpsc::UnboundedReceiver;

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.athanor";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// The errors the backend answers with.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The caller is not the frontend (PT3).
    #[zbus(name = "DBus.Error.AccessDenied")]
    AccessDenied(String),
    /// A setting that is not served; the frontend then asks the next backend.
    #[zbus(name = "portal.Error.NotFound")]
    NotFound(String),
}

fn main() -> ExitCode {
    journal::init();
    let Some(runtime) = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    else {
        tracing::error!("no absolute XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    if let Err(err) = confine(&runtime) {
        tracing::error!(error = %err, "cannot confine the portal with Landlock; refusing to run unconfined");
        return ExitCode::FAILURE;
    }

    let (changes, received) = tokio::sync::mpsc::unbounded_channel();
    let (store, _watched) = settings::watch(changes);
    std::thread::spawn(move || {
        if let Err(err) = serve(store, received) {
            tracing::error!(error = %err, "the portal's D-Bus service ended");
            std::process::exit(1);
        }
    });
    glib::MainLoop::new(None, false).run();
    ExitCode::SUCCESS
}

/// Before any thread starts. Reads stay open (PT3); the only write is dconf's shared-memory
/// flag beneath `$XDG_RUNTIME_DIR/dconf`, which GSettings maps to learn that a value changed.
/// No TCP. The chooser's and the screenshots' grants of PT3 join with the first window.
fn confine(runtime: &Path) -> Result<(), Box<dyn StdError>> {
    sandbox::ensure_single_threaded()?;
    sandbox::restrict_writes(&[&runtime.join("dconf")], &[])?;
    sandbox::deny_tcp()
}

/// Serves the Settings interface on the session bus and forwards every change the GLib
/// thread reports as `SettingChanged`. Returns only on failure.
fn serve(
    store: Arc<RwLock<settings::Store>>,
    mut received: UnboundedReceiver<settings::Change>,
) -> Result<(), Box<dyn StdError>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let conn = zbus::connection::Builder::session()?
            .serve_at(PATH, settings::Portal::new(store))?
            .name(BUS_NAME)?
            .build()
            .await?;
        tracing::info!("serving {BUS_NAME}");
        let portal = conn
            .object_server()
            .interface::<_, settings::Portal>(PATH)
            .await?;
        while let Some(change) = received.recv().await {
            settings::emit(portal.signal_emitter(), &change).await;
        }
        Err("the settings watcher stopped".into())
    })
}
