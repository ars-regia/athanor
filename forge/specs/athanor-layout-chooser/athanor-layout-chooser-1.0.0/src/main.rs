//! athanor-layout-chooser: the layout chooser of doc_shell.md, stage 1c. A small window
//! that writes the user's layout document; athanor-bar and athanor-dock apply it. It draws
//! with Athanor's toolkit, athanor-sal.

mod i18n;
mod look;
mod sandbox;
mod ui;

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use athanor_layout::loader::Paths;
use athanor_style::appearance::Appearance;

fn main() -> ExitCode {
    // Confinement first, while this is the only thread; logging is not set up yet.
    let Some(paths) = Paths::from_env() else {
        eprintln!("athanor-layout-chooser: no absolute XDG_CONFIG_HOME or HOME");
        return ExitCode::FAILURE;
    };
    if let Err(err) =
        sandbox::ensure_single_threaded().and_then(|()| sandbox::apply(&paths.user_file))
    {
        eprintln!(
            "athanor-layout-chooser: cannot confine the process, refusing to run unconfined: {err}"
        );
        return ExitCode::FAILURE;
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

    let look = look::look(&Appearance::read());
    let chooser = ui::Chooser::new(paths, look, i18n::is_rtl());
    match athanor_sal::driver::run(chooser) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "the layout chooser stopped");
            ExitCode::FAILURE
        }
    }
}
