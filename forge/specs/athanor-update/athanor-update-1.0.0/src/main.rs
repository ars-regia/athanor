//! `athanor-update`: checks for, downloads and applies system image updates, and publishes
//! the trust state (docs/architecture/doc_update_trust.md). One binary, run by four units:
//! `check` by the timer, `check --offline` at boot, `serve` by D-Bus activation, `migrate`
//! once per machine. `recover-key` is the administrator's command of UT2.
mod check;
mod migrate;
mod policy;
mod recover;
mod requests;
mod secureboot;
mod serve;
mod sigobj;
mod store;
mod tools;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "athanor-update", version, about = "Athanor system image updates and trust state")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify the booted image, ask the registry, download, publish the state.
    Check {
        /// Verify and publish only: no registry, no download (the run at boot).
        #[arg(long)]
        offline: bool,
    },
    /// Serve os.athanor.Update1 on the system bus until idle.
    Serve,
    /// Move this machine onto the signed reference, once.
    Migrate,
    /// Move this machine to a new image signing key (see RECOVERY.md).
    RecoverKey {
        #[command(subcommand)]
        step: RecoverStep,
    },
}

#[derive(Subcommand)]
enum RecoverStep {
    /// Trust NEW_KEY alone from now on.
    Begin { new_key: PathBuf },
    /// Return to the shipped policy, once the booted image ships the new key.
    Finish,
}

const ETC_KEYS: &str = "/etc/athanor/keys";

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

fn failed(what: &str, code: athanor_trust_state::ErrorCode) -> ExitCode {
    tracing::error!(?code, "{what} failed");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).without_time().init();
    let (tools, store) = (tools::System, store::Store::system());
    let ctx = check::Context::system(&tools, &store, now());
    match Cli::parse().command {
        Command::Check { offline } => {
            let _lock = match store.lock() {
                Ok(lock) => lock,
                Err(err) => {
                    tracing::error!(%err, "cannot take the lock: is /run/athanor-update declared in tmpfiles.d?");
                    return ExitCode::FAILURE;
                }
            };
            match check::run(&ctx, offline) {
                Ok(state) => {
                    tracing::info!(update = ?state.update, reason = ?state.verified.reason, error = ?state.last_error, "state published");
                    ExitCode::SUCCESS
                }
                Err(failure) => failed("the check", failure.code),
            }
        }
        Command::Serve => {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::error!(%err, "cannot start the runtime");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(serve::run()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    tracing::error!(%err, "cannot serve {}", serve::BUS_NAME);
                    ExitCode::FAILURE
                }
            }
        }
        Command::Migrate => {
            let Ok(_lock) = store.lock() else { return ExitCode::FAILURE };
            let outcome = migrate::run(&ctx);
            if let Err(failure) = check::run(&ctx, true) {
                return failed("publishing the state", failure.code);
            }
            match outcome {
                Ok(outcome) => {
                    tracing::info!(?outcome, "migration");
                    ExitCode::SUCCESS
                }
                Err(failure) => failed("the migration", failure.code),
            }
        }
        Command::RecoverKey { step } => {
            let paths = policy::PolicyPaths::system();
            let result = match step {
                RecoverStep::Begin { new_key } => recover::begin(&paths, Path::new(ETC_KEYS), &new_key),
                RecoverStep::Finish => recover::finish(&paths, Path::new(ETC_KEYS)),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("athanor-update: {message}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
