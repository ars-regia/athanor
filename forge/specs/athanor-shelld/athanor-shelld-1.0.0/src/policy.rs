//! What a notification does, decided from facts already gathered (doc_notification_center.md,
//! NC3): no bus, no clock, no file. The caller reads the identity, the rule, the settings and
//! the do-not-disturb state, and this module answers.

use crate::identity::Identity;
use crate::rules::{Rule, Settings, Timeout};
use crate::store::Urgency;

pub struct Facts<'a> {
    pub identity: &'a Identity,
    pub rule: &'a Rule,
    pub settings: &'a Settings,
    pub dnd_on: bool,
    pub urgency: Urgency,
    pub transient: bool,
    /// The application's own request: `-1` for the default, `0` for none, else milliseconds.
    pub expire_timeout: i32,
    pub suppress_sound: bool,
    pub rate_limited: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    /// Written to the history.
    pub keep: bool,
    /// Held in the list.
    pub list: bool,
    pub popup: bool,
    pub sound: bool,
    /// How long the popup shows; 0 until the user closes it.
    pub timeout_ms: u32,
}

#[must_use]
pub fn decide(facts: &Facts) -> Decision {
    let rule = facts.rule;
    if !rule.allowed {
        return Decision {
            keep: false,
            list: false,
            popup: false,
            sound: false,
            timeout_ms: 0,
        };
    }
    // A bypass is the user's word about one proven application: `Other` is every program
    // that could not be proven, so it never has one.
    let bypass = rule.bypass_dnd && matches!(facts.identity, Identity::App(_));
    let critical = facts.urgency == Urgency::Critical;
    let heard = !facts.rate_limited && (!facts.dnd_on || critical || bypass);
    Decision {
        keep: !facts.transient,
        list: true,
        popup: rule.popups && heard,
        sound: facts.settings.sound && rule.sound && !facts.suppress_sound && heard,
        timeout_ms: timeout_ms(facts),
    }
}

fn timeout_ms(facts: &Facts) -> u32 {
    if facts.urgency == Urgency::Critical {
        return 0;
    }
    if let Timeout::Seconds(seconds) = facts.rule.timeout {
        return seconds.saturating_mul(1000);
    }
    match facts.expire_timeout {
        requested if requested > 0 => requested.unsigned_abs(),
        0 => 0,
        _ => match facts.urgency {
            Urgency::Low => facts.settings.timeout_low_s,
            _ => facts.settings.timeout_normal_s,
        }
        .saturating_mul(1000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(identity: &'a Identity, rule: &'a Rule, settings: &'a Settings) -> Facts<'a> {
        Facts {
            identity,
            rule,
            settings,
            dnd_on: false,
            urgency: Urgency::Normal,
            transient: false,
            expire_timeout: -1,
            suppress_sound: false,
            rate_limited: false,
        }
    }

    #[test]
    fn the_decision_table() {
        let app = Identity::App("org.example.Chat".into());
        let other = Identity::Other;
        let settings = Settings::default();
        let plain = Rule::default();
        let bypass = Rule {
            bypass_dnd: true,
            ..Rule::default()
        };
        let muted = Rule {
            allowed: false,
            ..Rule::default()
        };
        let quiet = Rule {
            sound: false,
            ..Rule::default()
        };
        let listed = Rule {
            popups: false,
            ..Rule::default()
        };

        let d = decide(&facts(&app, &plain, &settings));
        assert_eq!(
            d,
            Decision {
                keep: true,
                list: true,
                popup: true,
                sound: true,
                timeout_ms: 5_000
            }
        );
        assert_eq!(
            decide(&facts(&app, &muted, &settings)),
            Decision {
                keep: false,
                list: false,
                popup: false,
                sound: false,
                timeout_ms: 0
            }
        );
        assert!(!decide(&facts(&app, &listed, &settings)).popup);
        assert!(
            !decide(&Facts {
                transient: true,
                ..facts(&app, &plain, &settings)
            })
            .keep
        );

        let dnd = |identity, rule| {
            decide(&Facts {
                dnd_on: true,
                ..facts(identity, rule, &settings)
            })
        };
        assert!(!dnd(&app, &plain).popup && !dnd(&app, &plain).sound);
        assert!(dnd(&app, &bypass).popup && dnd(&app, &bypass).sound);
        assert!(
            !dnd(&other, &bypass).popup,
            "bypass never applies without a proven identity"
        );
        let critical = |rule| {
            decide(&Facts {
                dnd_on: true,
                urgency: Urgency::Critical,
                ..facts(&app, rule, &settings)
            })
        };
        assert!(critical(&plain).popup && critical(&plain).sound);
        assert!(
            !critical(&quiet).sound,
            "critical still respects sound=false"
        );
        assert_eq!(critical(&plain).timeout_ms, 0);
        assert!(
            !decide(&Facts {
                suppress_sound: true,
                ..facts(&app, &plain, &settings)
            })
            .sound
        );
        let silent = Settings {
            sound: false,
            ..Settings::default()
        };
        assert!(!decide(&facts(&app, &plain, &silent)).sound);
        let limited = decide(&Facts {
            rate_limited: true,
            urgency: Urgency::Critical,
            ..facts(&app, &plain, &settings)
        });
        assert!(
            !limited.popup && !limited.sound && limited.list,
            "a rate-limited notification is listed, never shown or heard"
        );
    }

    #[test]
    fn the_timeout_order_is_rule_then_application_then_urgency() {
        let app = Identity::App("a".into());
        let settings = Settings {
            timeout_low_s: 3,
            timeout_normal_s: 8,
            ..Settings::default()
        };
        let ruled = Rule {
            timeout: Timeout::Seconds(12),
            ..Rule::default()
        };
        let plain = Rule::default();
        assert_eq!(
            decide(&Facts {
                expire_timeout: 2_000,
                ..facts(&app, &ruled, &settings)
            })
            .timeout_ms,
            12_000
        );
        assert_eq!(
            decide(&Facts {
                expire_timeout: 2_000,
                ..facts(&app, &plain, &settings)
            })
            .timeout_ms,
            2_000
        );
        assert_eq!(
            decide(&Facts {
                expire_timeout: 0,
                ..facts(&app, &plain, &settings)
            })
            .timeout_ms,
            0
        );
        assert_eq!(decide(&facts(&app, &plain, &settings)).timeout_ms, 8_000);
        assert_eq!(
            decide(&Facts {
                urgency: Urgency::Low,
                ..facts(&app, &plain, &settings)
            })
            .timeout_ms,
            3_000
        );
    }
}
