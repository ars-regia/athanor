//! `athanor-preferences-import`, run once per graphical session by
//! `athanor-preferences-import.service` before Sulphur (ADR-0150 points 6 to 8): it imports the
//! user's COSMIC preferences into the preference store, once per user, and writes its record.
//! The import is `athanor_sulphur_client::import`; this binary only confines the process and
//! reports the outcome.
//!
//! The unit sets no sandboxing directive that makes a user namespace under the per-user
//! manager, so the process confines itself before it reads anything: Landlock allows writes
//! beneath the store's directory only, reads stay open (the COSMIC files, and the dotfiles a
//! link may lead to), and TCP is denied.

use std::error::Error;
use std::process::ExitCode;

use athanor_sulphur_client::import::{self, Ran};
use athanor_sulphur_client::store::Store;
use athanor_unit::{dirs, journal, sandbox};

fn main() -> ExitCode {
    athanor_unit::panic::install();
    journal::init();
    match run() {
        Ok(Ran::Done) => {
            tracing::info!("the import record lists every family; nothing imported");
            ExitCode::SUCCESS
        }
        Ok(Ran::Recorded) => {
            tracing::info!("preferences imported and the record written");
            ExitCode::SUCCESS
        }
        Err(err) => {
            tracing::error!("the import failed and wrote no record: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Ran, Box<dyn Error>> {
    let store = Store::from_env()?;
    // The unit's ConfigurationDirectory= makes it; made here too for a run by hand. A dotfiles
    // tree may link the directory: the grant names the directory it leads to.
    let dir = store.dir_for_confinement()?;
    sandbox::ensure_single_threaded()?;
    sandbox::restrict_writes(&[dirs::own_dir(&dir)?], &[])?;
    sandbox::deny_tcp()?;
    Ok(import::run(&Store::at(dir))?)
}
