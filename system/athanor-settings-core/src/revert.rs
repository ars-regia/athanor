//! Keep or revert (doc_settings.md SE12): after an apply that changes resolution, refresh,
//! scale, rotation or the set of enabled outputs, the previous configuration comes back after
//! 15 seconds unless the user keeps the new one. The previous configuration and the deadline
//! are written to `$XDG_RUNTIME_DIR/athanor-settings/display-revert` before the apply, so a
//! Settings restarted after a crash (SE5) still reverts on time.
//!
//! The configuration is the text the compositor client serialises; this machine only keeps
//! it. Times are milliseconds of a clock that does not stop while the session runs and is the
//! same across a restart of Settings (CLOCK_BOOTTIME), which the caller reads.

/// The file's name in Settings' runtime directory.
pub const FILE: &str = "display-revert";
/// How long the dialog waits before it reverts.
pub const COUNTDOWN_MS: u64 = 15_000;

/// The user's answer in the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Keep,
    /// "Revert", which has the focus, or Escape.
    Revert,
}

/// What the caller does now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Keep counting: this many milliseconds are left.
    Wait(u64),
    /// Remove the file and close the dialog: the new configuration stays.
    Keep,
    /// Apply this previous configuration, then remove the file.
    Revert(String),
}

/// An apply waiting for its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub previous: String,
    pub deadline_ms: u64,
}

impl Pending {
    /// The pending state for an apply made at `now_ms` over `previous`. The caller writes
    /// [`Pending::encode`] to [`FILE`] before it applies.
    pub fn begin(previous: String, now_ms: u64) -> Self {
        Self {
            previous,
            deadline_ms: now_ms.saturating_add(COUNTDOWN_MS),
        }
    }

    /// What to do at `now_ms`, with the user's answer if one came. The countdown's end
    /// reverts as Revert does; a later answer changes nothing.
    pub fn decide(&self, now_ms: u64, answer: Option<Answer>) -> Decision {
        if now_ms >= self.deadline_ms {
            return Decision::Revert(self.previous.clone());
        }
        match answer {
            Some(Answer::Keep) => Decision::Keep,
            Some(Answer::Revert) => Decision::Revert(self.previous.clone()),
            None => Decision::Wait(self.deadline_ms - now_ms),
        }
    }

    /// The file's text: the deadline on the first line, the configuration after it.
    pub fn encode(&self) -> String {
        format!("{}\n{}", self.deadline_ms, self.previous)
    }

    /// The pending apply a restarted Settings finds in the file, if the text is one.
    pub fn decode(text: &str) -> Option<Self> {
        let (deadline, previous) = text.split_once('\n')?;
        Some(Self {
            previous: previous.to_owned(),
            deadline_ms: deadline.parse().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "eDP-1 1920x1080@60 scale 1.25\nHDMI-A-1 off";

    #[test]
    fn keep_keeps_and_revert_or_the_countdown_reverts() {
        let pending = Pending::begin(CONFIG.into(), 1_000);
        assert_eq!(pending.decide(1_000, None), Decision::Wait(15_000));
        assert_eq!(pending.decide(6_000, Some(Answer::Keep)), Decision::Keep);
        assert_eq!(
            pending.decide(6_000, Some(Answer::Revert)),
            Decision::Revert(CONFIG.into())
        );
        assert_eq!(pending.decide(15_999, None), Decision::Wait(1));
        assert_eq!(
            pending.decide(16_000, None),
            Decision::Revert(CONFIG.into())
        );
        assert_eq!(
            pending.decide(16_000, Some(Answer::Keep)),
            Decision::Revert(CONFIG.into()),
            "a Keep after the deadline comes too late"
        );
    }

    #[test]
    fn a_restart_before_the_deadline_counts_on_and_one_after_it_reverts_at_once() {
        let written = Pending::begin(CONFIG.into(), 50_000).encode();
        // Settings crashed and its unit restarted it, 4 s later.
        let found = Pending::decode(&written).expect("the file holds a pending apply");
        assert_eq!(
            found.previous, CONFIG,
            "the configuration survives its own newlines"
        );
        assert_eq!(found.decide(54_000, None), Decision::Wait(11_000));
        // A restart that came after the deadline.
        assert_eq!(found.decide(70_000, None), Decision::Revert(CONFIG.into()));
    }

    #[test]
    fn a_torn_file_is_no_pending_apply() {
        assert_eq!(Pending::decode(""), None);
        assert_eq!(Pending::decode("65000"), None);
        assert_eq!(Pending::decode("soon\nconfig"), None);
        assert_eq!(
            Pending::begin(String::new(), u64::MAX).deadline_ms,
            u64::MAX
        );
    }
}
