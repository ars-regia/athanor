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
use zbus::blocking::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

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

    // Held until the window closes: the bus releases the name with the connection.
    let _bus = match claim() {
        Ok(Claim::First(bus)) => bus,
        Ok(Claim::Taken) => {
            tracing::info!("a layout chooser is already open");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot register {} on the session bus", ui::APP_ID);
            return ExitCode::FAILURE;
        }
    };
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

/// Whether this chooser is the only one.
enum Claim {
    /// It owns the chooser's name on the session bus, through this connection, or there is no
    /// session bus to ask.
    First(Option<Connection>),
    /// Another chooser owns the name.
    Taken,
}

/// Owns [`ui::APP_ID`] on the session bus, as GTK's GApplication registered the GTK chooser,
/// so that a second chooser finds the first and leaves: two would both write the layout
/// document. Without a session bus the chooser runs alone, as GApplication does.
fn claim() -> zbus::Result<Claim> {
    let bus = match Connection::session() {
        Ok(bus) => bus,
        Err(err) => {
            tracing::warn!(error = %err, "no session bus: another layout chooser may be open");
            return Ok(Claim::First(None));
        }
    };
    claim_on(bus)
}

fn claim_on(bus: Connection) -> zbus::Result<Claim> {
    // zbus answers a name another connection owns with NameTaken, not with Exists.
    match bus.request_name_with_flags(ui::APP_ID, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            Ok(Claim::First(Some(bus)))
        }
        Ok(RequestNameReply::InQueue | RequestNameReply::Exists) | Err(zbus::Error::NameTaken) => {
            Ok(Claim::Taken)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // It needs a session bus of its own: `dbus-run-session -- cargo test -p
    // athanor-layout-chooser -- --ignored`.
    #[test]
    #[ignore = "needs a session bus"]
    fn a_second_chooser_finds_the_name_taken_until_the_first_leaves() -> zbus::Result<()> {
        let Claim::First(Some(first)) = claim_on(Connection::session()?)? else {
            panic!("the first chooser does not own the name");
        };
        assert!(matches!(claim_on(Connection::session()?)?, Claim::Taken));
        // As the first chooser's exit does, without waiting for the bus to see it disconnect.
        first.release_name(ui::APP_ID)?;
        assert!(matches!(
            claim_on(Connection::session()?)?,
            Claim::First(Some(_))
        ));
        Ok(())
    }
}
