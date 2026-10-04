//! Which application sent a notification (doc_bar.md BR1), read from the sender's cgroup.
//! systemd's desktop-environment convention names an application's unit
//! `app-[<launcher>-]<id>[@<instance>].service` or `app-[<launcher>-]<id>-<random>.scope`,
//! with `-` inside a name part escaped as `\x2d`. Informative, like `sender`: a process
//! running as the user can start a unit of any name.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::hints::is_desktop_id;
use crate::sender::is_slice_or_user_manager;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Identity {
    /// A desktop file id, without the `.desktop` suffix.
    App(String),
    Other,
}

impl Identity {
    /// The application id, or `""` for `Other`, as on the bus and the wire.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Identity::App(id) => id,
            Identity::Other => "",
        }
    }
}

/// Reads `<proc_root>/<pid>/cgroup`. A process that has gone, or a cgroup that is no
/// application's, is `Other`.
#[must_use]
pub fn of_pid(proc_root: &Path, pid: u32) -> Identity {
    match fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup")) {
        Ok(text) => from_cgroup(&text),
        Err(err) => {
            tracing::debug!(pid, error = %err, "cannot read the sender's cgroup");
            Identity::Other
        }
    }
}

#[must_use]
pub fn from_cgroup(cgroup: &str) -> Identity {
    let Some(path) = cgroup.lines().find_map(|line| line.strip_prefix("0::")) else {
        return Identity::Other;
    };
    let Some(rest) = path.strip_prefix('/') else {
        return Identity::Other;
    };
    // Everything before the application's unit must be a slice or the user manager, and
    // at least one component must precede it; anything below it is still that application.
    for (depth, component) in rest.split('/').enumerate() {
        if is_app_unit(component) {
            return if depth == 0 { Identity::Other } else { unit_id(component) };
        }
        if !is_slice_or_user_manager(component) {
            return Identity::Other;
        }
    }
    Identity::Other
}

fn is_app_unit(component: &str) -> bool {
    component.starts_with("app-")
        && (component.ends_with(".service") || component.ends_with(".scope"))
}

fn unit_id(unit: &str) -> Identity {
    let Some(name) = unit.strip_prefix("app-") else {
        return Identity::Other;
    };
    let id = if let Some(name) = name.strip_suffix(".service") {
        let name = name.split('@').next().unwrap_or(name);
        let mut parts = name.split('-');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(id), None, _) | (Some(_), Some(id), None) => id,
            _ => return Identity::Other,
        }
    } else if let Some(name) = name.strip_suffix(".scope") {
        let mut parts = name.split('-');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(id), Some(_), None, _) | (Some(_), Some(id), Some(_), None) => id,
            _ => return Identity::Other,
        }
    } else {
        return Identity::Other;
    };
    let id = id.replace("\\x2d", "-");
    if is_desktop_id(&id) {
        Identity::App(id)
    } else {
        Identity::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPS: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/";

    fn app(id: &str) -> Identity {
        Identity::App(id.to_owned())
    }

    #[test]
    fn units_and_scopes_name_the_application() {
        for (last, expected) in [
            ("app-org.gnome.Calculator@1a2b.service", app("org.gnome.Calculator")),
            ("app-cosmic-firefox@1a.service", app("firefox")),
            ("app-firefox-1234.scope", app("firefox")),
            ("app-cosmic-firefox-1234.scope", app("firefox")),
            ("app-flatpak-org.mozilla.firefox-98765.scope", app("org.mozilla.firefox")),
            ("app-gnome-code\\x2doss-77.scope", app("code-oss")),
        ] {
            assert_eq!(from_cgroup(&format!("{APPS}{last}\n")), expected, "{last}");
        }
    }

    #[test]
    fn a_sub_cgroup_of_an_application_is_that_application() {
        let cgroup = format!("{APPS}app-org.gnome.Calculator@1a2b.service/worker\n");
        assert_eq!(from_cgroup(&cgroup), app("org.gnome.Calculator"));
    }

    #[test]
    fn everything_else_is_other() {
        for cgroup in [
            format!("{APPS}athanor-bar.service\n"),
            "0::/user.slice/user-1000.slice/session-2.scope\n".to_owned(),
            format!("{APPS}app-a-b-c-d.scope\n"),
            format!("{APPS}app-.service\n"),
            format!("{APPS}app-..@1.service\n"),
            format!("{APPS}app-foo.service.d\n"),
            format!("{APPS}some.scope/app-org.gnome.Calculator@1.service\n"),
            "0::app-org.gnome.Calculator@1.service\n".to_owned(),
            String::new(),
        ] {
            assert_eq!(from_cgroup(&cgroup), Identity::Other, "{cgroup}");
        }
    }

    #[test]
    fn a_process_that_has_gone_is_other() {
        let root = std::env::temp_dir().join(format!("athanor-shelld-identity-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("mkdir");
        assert_eq!(of_pid(&root, 4_000_000), Identity::Other);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
