//! The athanor-sulphur binary: the log, then the compositor of the library crate.

#![deny(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    athanor_unit::panic::install();
    // RUST_LOG adds to the log, as `smithay::backend::drm::compositor=trace` for where each
    // element of a frame goes; what it does not name stays at info. Its directives follow
    // `info`, and a later directive replaces an earlier one of the same target, so a bare level
    // in RUST_LOG replaces `info` too. (EnvFilter's own default applies only to an empty one.)
    let directives = match std::env::var(tracing_subscriber::EnvFilter::DEFAULT_ENV) {
        Ok(directives) => Ok(format!("info,{directives}")),
        Err(std::env::VarError::NotPresent) => Ok(String::from("info")),
        Err(error) => Err(error.to_string()),
    };
    let filter = directives.and_then(|directives| {
        tracing_subscriber::EnvFilter::try_new(directives).map_err(|error| error.to_string())
    });
    let (filter, malformed) = match filter {
        Ok(filter) => (filter, None),
        Err(error) => (tracing_subscriber::EnvFilter::new("info"), Some(error)),
    };
    if let Err(error) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        eprintln!("athanor-sulphur: no log: {error}");
    }
    if let Some(error) = malformed {
        tracing::warn!(%error, "RUST_LOG is malformed: the log is at info");
    }
    match athanor_sulphur::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}
