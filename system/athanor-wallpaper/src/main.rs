//! `athanor-wallpaper`, the wallpaper service (doc_session_daemons.md SD6, SD16, SD17): a Sal
//! client with one background surface per output. It reads `org.athanor.desktop.background`
//! and the colour scheme, migrates cosmic-bg's choice once, opens each picture as a regular
//! file and hands it to its decoder unit, which returns the pixels scaled to each output.

mod confine;
mod decoder;
mod surfaces;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use athanor_sal::driver;
use athanor_sulphur_client::background;
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_unit::{journal, sandbox};
use athanor_wallpaper::{migrate, protocol, ColorScheme, Settings};
use gio::prelude::*;

use surfaces::{Inbox, Wallpaper};

const SCHEMA: &str = "org.athanor.desktop.background";
const INTERFACE: &str = "org.gnome.desktop.interface";

fn main() -> ExitCode {
    journal::init();
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    else {
        tracing::error!("no absolute XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    // Before any GIO object and any thread: GLib's worker threads are confined from their
    // first instruction (SD17).
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| Ok(confine::dconf_dir(&runtime)?))
        .and_then(|dconf| confine::apply(&confine::reads(|name| std::env::var_os(name)), &dconf));
    if let Err(error) = confined {
        tracing::error!(%error, "refusing to run unconfined");
        return ExitCode::FAILURE;
    }
    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(error) => {
            tracing::error!(%error, "cannot run GLib's main context");
            return ExitCode::FAILURE;
        }
    };
    let ours = lookup(SCHEMA);
    if ours.is_none() {
        tracing::error!("{SCHEMA} is not installed; the hearth is shown on every output");
    }
    let interface = lookup(INTERFACE);
    if let Some(ours) = &ours {
        migrate_once(ours);
    }
    let inbox = Rc::new(RefCell::new(Inbox::default()));
    let watch = |settings: &gio::Settings| {
        let inbox = Rc::clone(&inbox);
        settings.connect_changed(None, move |_, _| inbox.borrow_mut().settings_changed = true)
    };
    let _watches = (ours.as_ref().map(watch), interface.as_ref().map(watch));
    let read = Box::new(move || {
        let settings = ours.as_ref().map(read_settings).unwrap_or_default();
        let scheme = interface
            .as_ref()
            .map(|interface| ColorScheme::from_key(&interface.string("color-scheme")))
            .unwrap_or(ColorScheme::Light);
        (settings, scheme)
    });
    let socket = runtime.join(protocol::SOCKET);
    match driver::run_with(Wallpaper::new(read, socket, inbox), glib) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the wallpaper stopped");
            ExitCode::FAILURE
        }
    }
}

/// The schema `id`, or `None` when it is not installed (`Settings::new` would abort).
fn lookup(id: &str) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(id, true)?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

fn read_settings(settings: &gio::Settings) -> Settings {
    Settings {
        picture: settings.string("picture").into(),
        per_output: settings
            .value("per-output")
            .get::<BTreeMap<String, String>>()
            .unwrap_or_default(),
        same_on_all: settings.boolean("same-on-all"),
        migrated: settings.boolean("migrated"),
    }
}

/// SD16: cosmic-bg's choice, once. `migrated` is written last, and only once the other keys
/// are, so a failed write is tried again at the next start.
fn migrate_once(ours: &gio::Settings) {
    let current = read_settings(ours);
    if current.migrated {
        return;
    }
    let next = migrate(current, background::read_user().as_ref());
    let written = ours
        .set_string("picture", &next.picture)
        .and_then(|()| ours.set_value("per-output", &next.per_output.to_variant()))
        .and_then(|()| ours.set_boolean("same-on-all", next.same_on_all))
        .and_then(|()| ours.set_boolean("migrated", true));
    match written {
        Ok(()) => tracing::info!(picture = %next.picture, "cosmic-bg's choice migrated"),
        Err(error) => {
            tracing::warn!(%error, "the migration from cosmic-bg is not written; it runs again at the next start")
        }
    }
}
