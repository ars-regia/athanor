//! The battery page's words, which the GTK page and the one drawn with athanor-sal both say.

use athanor_services::battery::{self, Charge};

use crate::i18n::{tr, tr_with};

/// The charge as the button and the page say it.
pub fn percent_text(percent: f64) -> String {
    // At most 100 after `battery::battery`, so the cast cannot truncate.
    tr_with(
        "{percent} %",
        "percent",
        &(percent.round() as u32).to_string(),
    )
}

/// A translated `text` with `{hours}` and `{minutes}` filled in. The callers pass `tr(...)`
/// with the literal message id, so xgettext finds it.
fn duration(text: &str, seconds: u64) -> String {
    let (hours, minutes) = battery::hours_minutes(seconds);
    text.replace("{hours}", &hours.to_string())
        .replace("{minutes}", &minutes.to_string())
}

/// The line under the charge: the time left, the time until full, or the state.
pub fn note(battery: &battery::Battery) -> Option<String> {
    match (battery.charge, battery.seconds) {
        (Charge::Discharging, Some(seconds)) => {
            Some(duration(&tr("{hours} h {minutes} min left"), seconds))
        }
        (Charge::Charging, Some(seconds)) => {
            Some(duration(&tr("{hours} h {minutes} min until full"), seconds))
        }
        (Charge::Charging, None) => Some(tr("Charging")),
        (Charge::Full, _) => Some(tr("Fully charged")),
        (Charge::Discharging | Charge::Unknown, _) => None,
    }
}

pub(crate) fn profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => tr("Power saver"),
        "performance" => tr("Performance"),
        _ => tr("Balanced"),
    }
}
