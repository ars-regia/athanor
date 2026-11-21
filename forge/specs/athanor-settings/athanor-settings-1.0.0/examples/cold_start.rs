//! The probe of spike SE-S1 (doc_settings.md section 4): Settings' window at the landing page,
//! opened as a cold start opens it, without the bus name, the forwarder or Landlock, so that
//! `forge/test/shell/measure_chooser.py run` can time its first frame. Not shipped.

use std::process::ExitCode;

use athanor_settings::window::Settings;
use athanor_settings::{i18n, look};
use athanor_settings_core::registry::Registry;
use athanor_settings_core::target::Target;
use athanor_style::appearance::Appearance;

fn main() -> ExitCode {
    i18n::init();
    let settings = Settings::new(
        Registry::new(),
        look::look(&Appearance::read()),
        i18n::is_rtl(),
        Target::Landing,
        Default::default(),
        Default::default(),
        None,
    );
    match athanor_sal::driver::run(settings) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cold_start: {error}");
            ExitCode::FAILURE
        }
    }
}
