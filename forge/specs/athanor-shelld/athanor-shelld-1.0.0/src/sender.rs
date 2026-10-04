//! Who may call `os.athanor.Notifications1` (doc_bar.md BR1, doc_notification_center.md NC8):
//! a process in the cgroup of athanor-bar.service or of athanor-control-center.service, read
//! from the caller's credentials on the bus. Informative, as the shield is: a process running
//! as the user can replace either unit.

use std::fs;
use std::path::PathBuf;

pub const BAR_UNIT: &str = "athanor-bar.service";
pub const CONTROL_CENTER_UNIT: &str = "athanor-control-center.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    Bar,
    ControlCenter,
}

/// Whether `caller` may call `method` of the private interface (the table of NC8). `SetRule`
/// is admitted for the control center; which keys it may set is checked with the call.
#[must_use]
pub fn admits(caller: Caller, method: &str) -> bool {
    match method {
        "List"
        | "Close"
        | "InvokeAction"
        | "Reply"
        | "MarkRead"
        | "DoNotDisturb"
        | "SetDoNotDisturb"
        | "SetDoNotDisturbUntil"
        | "Settings" => true,
        "History" | "ClearAll" | "ClearGroup" | "Rules" | "SetRule" => {
            caller == Caller::ControlCenter
        }
        "ReportFullscreen" => caller == Caller::Bar,
        // `SetSetting` waits for the Settings application; anything else is no method.
        _ => false,
    }
}

type Rule = Box<dyn Fn(&str, u32) -> Option<Caller> + Send + Sync>;

/// Which unit a connection belongs to, given its unique name and the pid the bus reports.
pub struct Admitted(Rule);

impl Admitted {
    /// The production rule: the unit in the cgroup of the pid, read from
    /// `<proc_root>/<pid>/cgroup`. The unique name plays no part.
    #[must_use]
    pub fn from_proc_root(proc_root: impl Into<PathBuf>) -> Admitted {
        let proc_root = proc_root.into();
        Admitted(Box::new(move |_name, pid| {
            // ponytail: pid can be reused between the credentials call that gave us this pid
            // and this read (TOCTOU); the upgrade path is the `ProcessFD` GetConnectionCredentials
            // can give instead, holding that pidfd open and reconfirming it is still the same
            // process after the read, rather than re-resolving a bare numeric pid.
            match fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup")) {
                Ok(text) => caller_of(&text),
                Err(err) => {
                    tracing::warn!(pid, error = %err, "cannot read the caller's cgroup; refused");
                    None
                }
            }
        }))
    }

    /// Tests: every connection of a test belongs to the test process, so one pid cannot
    /// stand for two units and the rule looks at the unique name instead.
    #[must_use]
    pub fn from_fn(f: impl Fn(&str, u32) -> Option<Caller> + Send + Sync + 'static) -> Admitted {
        Admitted(Box::new(f))
    }

    #[must_use]
    pub fn caller(&self, unique_name: &str, pid: u32) -> Option<Caller> {
        (self.0)(unique_name, pid)
    }
}

fn caller_of(cgroup: &str) -> Option<Caller> {
    [
        (BAR_UNIT, Caller::Bar),
        (CONTROL_CENTER_UNIT, Caller::ControlCenter),
    ]
    .into_iter()
    .find_map(|(unit, caller)| admits_path(cgroup, unit).then_some(caller))
}

/// Whether the cgroup text's unified hierarchy (`0::`) path names `unit` exactly: the path is
/// absolute, its last component is `unit` verbatim (no trimming, no descendant of it — a
/// delegated subtree or a scope under it is refused), and every component before it has the
/// shape systemd gives a unit of the user manager, a `.slice` or `user@<digits>.service` —
/// never another `.service` or a `.scope`, which would mean `unit` sits under something else.
#[must_use]
fn admits_path(cgroup: &str, unit: &str) -> bool {
    let Some(path) = cgroup.lines().find_map(|line| line.strip_prefix("0::")) else {
        return false;
    };
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let mut components = rest.split('/');
    components.next_back() == Some(unit) && components.all(is_slice_or_user_manager)
}

/// A `.slice`, or the `user@<uid>.service` systemd gives the user manager itself.
pub(crate) fn is_slice_or_user_manager(component: &str) -> bool {
    component.ends_with(".slice")
        || component
            .strip_prefix("user@")
            .and_then(|rest| rest.strip_suffix(".service"))
            .is_some_and(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service\n";
    const CENTER: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-control-center.service\n";

    #[test]
    fn only_the_two_units_are_admitted() {
        let root =
            std::env::temp_dir().join(format!("athanor-shelld-sender-{}", std::process::id()));
        for (pid, cgroup) in [
            (10, BAR),
            (11, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-foo@0123.service\n"),
            (12, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service/sub\n"),
            (14, CENTER),
            (15, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-control-center.service/sub\n"),
        ] {
            fs::create_dir_all(root.join(pid.to_string())).expect("mkdir");
            fs::write(root.join(pid.to_string()).join("cgroup"), cgroup).expect("write");
        }
        let admitted = Admitted::from_proc_root(&root);
        assert_eq!(admitted.caller(":1.1", 10), Some(Caller::Bar));
        assert_eq!(admitted.caller(":1.2", 14), Some(Caller::ControlCenter));
        assert_eq!(
            admitted.caller(":1.3", 11),
            None,
            "an application a unit launched"
        );
        assert_eq!(
            admitted.caller(":1.4", 12),
            None,
            "a child cgroup of the bar"
        );
        assert_eq!(
            admitted.caller(":1.5", 15),
            None,
            "a child cgroup of the center"
        );
        assert_eq!(admitted.caller(":1.6", 13), None, "no such process");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_delegated_or_malformed_path_is_refused_even_when_it_ends_in_a_unit() {
        for (label, cgroup) in [
            (
                "nested under a .service ancestor",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-foo.service/athanor-bar.service\n",
            ),
            (
                "nested under a .scope ancestor",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/some.scope/athanor-control-center.service\n",
            ),
            (
                "a trailing space",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service \n",
            ),
            (
                "a sibling unit with a suffix",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service.d\n",
            ),
            ("a relative path", "0::athanor-bar.service\n"),
        ] {
            assert_eq!(caller_of(cgroup), None, "{label}");
        }
    }

    #[test]
    fn each_cell_of_the_table() {
        use Caller::{Bar, ControlCenter};
        // (method, bar, control center)
        let table = [
            ("List", true, true),
            ("History", false, true),
            ("Close", true, true),
            ("InvokeAction", true, true),
            ("Reply", true, true),
            ("MarkRead", true, true),
            ("ClearAll", false, true),
            ("ClearGroup", false, true),
            ("DoNotDisturb", true, true),
            ("SetDoNotDisturb", true, true),
            ("SetDoNotDisturbUntil", true, true),
            ("Rules", false, true),
            ("SetRule", false, true),
            ("Settings", true, true),
            ("SetSetting", false, false),
            ("ReportFullscreen", true, false),
        ];
        for (method, bar, center) in table {
            assert_eq!(admits(Bar, method), bar, "bar {method}");
            assert_eq!(admits(ControlCenter, method), center, "center {method}");
        }
        assert!(!admits(Bar, "NoSuchMethod") && !admits(ControlCenter, "NoSuchMethod"));
    }
}
