//! athanor-layout-chooser: the layout chooser of doc_shell.md, stage 1c. A small window
//! that writes the user's layout document; athanor-layout-translator applies it.

mod i18n;
mod sandbox;
mod ui;

use std::io::{self, IsTerminal};

use athanor_layout::loader::Paths;
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Layout";

fn main() -> glib::ExitCode {
    // Confinement first, while this is the only thread; logging is not set up yet.
    let Some(paths) = Paths::from_env() else {
        eprintln!("athanor-layout-chooser: no absolute XDG_CONFIG_HOME or HOME");
        return glib::ExitCode::FAILURE;
    };
    if let Err(err) =
        sandbox::ensure_single_threaded().and_then(|()| sandbox::apply(&paths.user_file))
    {
        eprintln!(
            "athanor-layout-chooser: cannot confine the process, refusing to run unconfined: {err}"
        );
        return glib::ExitCode::FAILURE;
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        // Launched from the desktop, stderr goes to the journal, which stores colour codes
        // as they come: colour only a terminal.
        .with_ansi(io::stderr().is_terminal())
        .init();
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| ui::build_ui(app, paths.clone()));
    app.run_with_args(&Vec::<String>::new())
}
