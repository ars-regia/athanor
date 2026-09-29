//! Who may call `os.athanor.Notifications1` (doc_bar.md BR1): a process in the cgroup of
//! athanor-bar.service, read from the caller's credentials on the bus. Informative, as the
//! shield is: a process running as the user can replace that unit.

use std::fs;
use std::path::PathBuf;

pub const BAR_UNIT: &str = "athanor-bar.service";

#[derive(Debug, Clone)]
pub struct BarUnit {
    unit: String,
    proc_root: PathBuf,
}

impl BarUnit {
    #[must_use]
    pub fn from_proc() -> BarUnit {
        BarUnit::with_proc_root(BAR_UNIT, "/proc")
    }

    /// Reads `<proc_root>/<pid>/cgroup`. Tests point it at a directory of their own.
    #[must_use]
    pub fn with_proc_root(unit: &str, proc_root: impl Into<PathBuf>) -> BarUnit {
        BarUnit {
            unit: unit.to_owned(),
            proc_root: proc_root.into(),
        }
    }

    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    #[must_use]
    pub fn admits(&self, pid: u32) -> bool {
        // ponytail: pid can be reused between the credentials call that gave us this pid and
        // this read (TOCTOU); the upgrade path is the `ProcessFD` GetConnectionCredentials can
        // give instead, holding that pidfd open and reconfirming it is still the same process
        // after the read, rather than re-resolving a bare numeric pid.
        match fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup")) {
            Ok(text) => admits_path(&text, &self.unit),
            Err(err) => {
                tracing::warn!(pid, error = %err, "cannot read the caller's cgroup; refused");
                false
            }
        }
    }
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
fn is_slice_or_user_manager(component: &str) -> bool {
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

    #[test]
    fn only_the_bar_unit_is_admitted() {
        let root =
            std::env::temp_dir().join(format!("athanor-shelld-sender-{}", std::process::id()));
        for (pid, cgroup) in [
            (10, BAR),
            (11, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-foo@0123.service\n"),
            (12, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service/sub\n"),
        ] {
            fs::create_dir_all(root.join(pid.to_string())).expect("mkdir");
            fs::write(root.join(pid.to_string()).join("cgroup"), cgroup).expect("write");
        }
        let bar = BarUnit::with_proc_root(BAR_UNIT, &root);
        assert!(bar.admits(10));
        assert!(!bar.admits(11), "an application the bar launched");
        assert!(!bar.admits(12), "a child cgroup");
        assert!(!bar.admits(13), "no such process");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_delegated_or_malformed_path_is_refused_even_when_it_ends_in_the_bar_unit() {
        for (label, cgroup) in [
            (
                "nested under a .service ancestor",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-foo.service/athanor-bar.service\n",
            ),
            (
                "nested under a .scope ancestor",
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/some.scope/athanor-bar.service\n",
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
            assert!(!admits_path(cgroup, BAR_UNIT), "{label}");
        }
    }
}
