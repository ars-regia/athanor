//! The power menu's actions, and what logind offers (doc_bar.md, BR3). The UI puts every
//! action behind a confirmation; this module only says which exist and how to ask for them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Lock,
    LogOut,
    Suspend,
    Reboot,
    PowerOff,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::Lock,
        Action::LogOut,
        Action::Suspend,
        Action::Reboot,
        Action::PowerOff,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::Lock => "lock",
            Action::LogOut => "log-out",
            Action::Suspend => "suspend",
            Action::Reboot => "reboot",
            Action::PowerOff => "power-off",
        }
    }

    /// The `org.freedesktop.login1.Manager` method that says whether the action is offered.
    /// A session can always lock and log out.
    pub fn can_method(self) -> Option<&'static str> {
        match self {
            Action::Lock | Action::LogOut => None,
            Action::Suspend => Some("CanSuspend"),
            Action::Reboot => Some("CanReboot"),
            Action::PowerOff => Some("CanPowerOff"),
        }
    }

    /// The `org.freedesktop.login1.Manager` method that does it.
    pub fn manager_method(self) -> Option<&'static str> {
        match self {
            Action::Lock | Action::LogOut => None,
            Action::Suspend => Some("Suspend"),
            Action::Reboot => Some("Reboot"),
            Action::PowerOff => Some("PowerOff"),
        }
    }
}

/// logind answers `yes`, `challenge` (allowed once polkit authenticates), `no`, or `na`
/// (the hardware or the configuration cannot). Anything else is not offered.
pub fn offered(answer: &str) -> bool {
    matches!(answer, "yes" | "challenge")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_yes_and_challenge_are_offered() {
        assert!(offered("yes"));
        assert!(offered("challenge"));
        for answer in ["no", "na", "", "YES", "yes "] {
            assert!(!offered(answer), "{answer:?}");
        }
    }

    #[test]
    fn lock_and_log_out_are_always_offered_and_are_not_manager_calls() {
        for action in [Action::Lock, Action::LogOut] {
            assert_eq!(action.can_method(), None);
            assert_eq!(action.manager_method(), None);
        }
    }

    #[test]
    fn the_manager_actions_ask_logind_first() {
        let pairs: Vec<_> = [Action::Suspend, Action::Reboot, Action::PowerOff]
            .into_iter()
            .map(|action| (action.can_method(), action.manager_method()))
            .collect();
        assert_eq!(
            pairs,
            [
                (Some("CanSuspend"), Some("Suspend")),
                (Some("CanReboot"), Some("Reboot")),
                (Some("CanPowerOff"), Some("PowerOff")),
            ]
        );
    }

    #[test]
    fn ids_are_distinct() {
        let ids: std::collections::HashSet<_> =
            Action::ALL.iter().map(|action| action.id()).collect();
        assert_eq!(ids.len(), Action::ALL.len());
    }
}
