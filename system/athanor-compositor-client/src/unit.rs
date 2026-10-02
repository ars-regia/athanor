//! Transient services of the user manager (doc_bar.md, BR2.3). A process the shell starts
//! runs as a child of the user manager, never of the shell: it inherits neither the
//! shell's Landlock ruleset nor its cgroup.

use std::os::fd::OwnedFd;

use futures_util::StreamExt;
use gtk4::gio::{self, prelude::*};
use gtk4::glib::{self, variant::Handle, variant::ObjectPath, Variant};

use crate::launch::LaunchError;

/// systemd's `UNIT_NAME_MAX`.
const NAME_MAX: usize = 255;

const SYSTEMD: &str = "org.freedesktop.systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
/// The error the manager answers a client that subscribed already.
const ALREADY_SUBSCRIBED: &str = "org.freedesktop.systemd1.AlreadySubscribed";

/// The application id a desktop id stands for: without `.desktop`, as the unit name and
/// Wayland's `app_id` carry it.
pub(crate) fn app_id(desktop_id: &str) -> &str {
    desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id)
}

/// A desktop id as unit names carry it: without `.desktop`, escaped as `systemd-escape`
/// does. `/` becomes `-` (a desktop id never holds one), a leading `.` and every other byte
/// outside `[A-Za-z0-9:_.]` become `\xNN`. The dash is escaped too, so the id reads back
/// unambiguously from between the name's dashes.
pub(crate) fn escape(desktop_id: &str) -> String {
    let id = app_id(desktop_id);
    let mut escaped = String::with_capacity(id.len());
    for (index, byte) in id.bytes().enumerate() {
        match byte {
            b'/' => escaped.push('-'),
            b'.' if index == 0 => escaped.push_str(r"\x2e"),
            b if b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.') => {
                escaped.push(char::from(b));
            }
            b => escaped.push_str(&format!("\\x{b:02x}")),
        }
    }
    escaped
}

/// `app-athanor-<escaped id>@<random>.service`, the XDG naming of application units;
/// `None` when the id is too long for a unit name.
pub(crate) fn app_unit_name(desktop_id: &str, random: &str) -> Option<String> {
    let name = format!("app-athanor-{}@{random}.service", escape(desktop_id));
    (name.len() <= NAME_MAX).then_some(name)
}

/// 32 hexadecimal digits from GLib's random UUID.
pub(crate) fn random() -> String {
    glib::uuid_string_random().replace('-', "")
}

/// A transient service: what `StartTransientUnit` receives.
#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) argv: Vec<String>,
    /// `NAME=value` pairs, over the user manager's environment.
    pub(crate) environment: Vec<String>,
    /// An absolute path, or `~` for the user's home directory.
    pub(crate) working_directory: String,
    /// A directory under `$XDG_RUNTIME_DIR`, private to the user, that systemd removes
    /// when the unit stops. It may exist already: systemd keeps what it holds.
    pub(crate) runtime_directory: Option<String>,
}

impl Unit {
    /// The parameters of `StartTransientUnit`. With `pass_fd`, the first descriptor of the
    /// call's descriptor list reaches the service through `ExtraFileDescriptors`; the
    /// manager holds it until the unit stops. An empty `argv` is refused.
    pub(crate) fn parameters(&self, pass_fd: bool) -> Result<Variant, glib::Error> {
        let (program, _) = self.argv.split_first().ok_or_else(|| {
            glib::Error::new(
                gio::IOErrorEnum::InvalidArgument,
                "a unit needs a program to run",
            )
        })?;
        let mut properties: Vec<(&str, Variant)> = vec![
            ("Description", self.description.to_variant()),
            (
                "ExecStart",
                vec![(program.as_str(), self.argv.clone(), false)].to_variant(),
            ),
            ("Environment", self.environment.to_variant()),
            ("WorkingDirectory", self.working_directory.to_variant()),
            // The start job fails when the program cannot be executed, not only when
            // systemd cannot fork: `start` waits for that job.
            ("Type", "exec".to_variant()),
            // The unit, and so the descriptor, lasts while any of its processes does.
            ("ExitType", "cgroup".to_variant()),
            ("CollectMode", "inactive-or-failed".to_variant()),
        ];
        if let Some(dir) = &self.runtime_directory {
            properties.push(("RuntimeDirectory", vec![dir.as_str()].to_variant()));
            properties.push(("RuntimeDirectoryMode", 0o700_u32.to_variant()));
        }
        if pass_fd {
            properties.push((
                "ExtraFileDescriptors",
                vec![(Handle(0), "wayland-context")].to_variant(),
            ));
        }
        let aux: Vec<(&str, Vec<(&str, Variant)>)> = Vec::new();
        Ok((self.name.as_str(), "fail", properties, aux).to_variant())
    }

    /// Asks the user manager to start the unit, and returns when its start job ends. With
    /// `Type=exec` the job ends once the program runs, so a program that cannot be executed
    /// is an error, as is any other result than `done`. The wait never blocks the main loop.
    pub(crate) async fn start(&self, fd: Option<OwnedFd>) -> Result<(), LaunchError> {
        let parameters = self.parameters(fd.is_some())?;
        let fds = gio::UnixFDList::new();
        if let Some(fd) = &fd {
            fds.append(fd)?;
        }
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        // Subscribed before the job exists: the stream keeps what arrives before the reply
        // names the job, so its end cannot be missed.
        let mut removed = bus.receive_signal_parameters::<(u32, ObjectPath, String, String)>(
            Some(SYSTEMD),
            Some(MANAGER),
            Some("JobRemoved"),
            Some(MANAGER_PATH),
            None,
            gio::DBusSignalFlags::NONE,
        );
        subscribe(&bus).await?;
        let (reply, _) = bus
            .call_with_unix_fd_list_future(
                Some(SYSTEMD),
                MANAGER_PATH,
                MANAGER,
                "StartTransientUnit",
                Some(&parameters),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                Some(&fds),
            )
            .await?;
        let job = reply.child_value(0);
        let job = job.str().ok_or_else(|| {
            glib::Error::new(
                gio::IOErrorEnum::InvalidData,
                "StartTransientUnit answered with no job",
            )
        })?;
        while let Some(signal) = removed.next().await {
            match signal {
                Ok((_, path, unit, result)) if path.as_str() == job => {
                    return if result == "done" {
                        Ok(())
                    } else {
                        Err(LaunchError::Start { unit, result })
                    };
                }
                Ok(_) => {}
                Err(err) => tracing::warn!("a JobRemoved signal was ignored: {err}"),
            }
        }
        // The stream ends only with its subscription, which it owns.
        Err(LaunchError::Start {
            unit: self.name.clone(),
            result: "unknown".to_owned(),
        })
    }
}

/// The manager sends its signals only while a client is subscribed. A subscription lasts
/// as long as the bus connection, which the process shares, so a second one is refused
/// with an error that means success.
async fn subscribe(bus: &gio::DBusConnection) -> Result<(), glib::Error> {
    match bus
        .call_future(
            Some(SYSTEMD),
            MANAGER_PATH,
            MANAGER,
            "Subscribe",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
    {
        Err(err)
            if gio::DBusError::remote_error(&err)
                .is_some_and(|name| name == ALREADY_SUBSCRIBED) =>
        {
            Ok(())
        }
        result => result.map(drop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_id_is_escaped_like_systemd_escape() {
        assert_eq!(escape("org.gnome.Nautilus.desktop"), "org.gnome.Nautilus");
        assert_eq!(escape("google-chrome.desktop"), r"google\x2dchrome");
        assert_eq!(escape("a b/c@d"), r"a\x20b-c\x40d");
        assert_eq!(escape("caffè"), r"caff\xc3\xa8");
        assert_eq!(escape(".hidden.desktop"), r"\x2ehidden");
        assert_eq!(escape("x..y:a_b"), "x..y:a_b");
    }

    #[test]
    fn the_unit_name_follows_the_xdg_convention() {
        assert_eq!(
            app_unit_name("google-chrome.desktop", "0123").as_deref(),
            Some(r"app-athanor-google\x2dchrome@0123.service")
        );
        let random = random();
        assert_eq!(random.len(), 32);
        assert!(random.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn a_name_beyond_systemds_limit_is_refused() {
        let random = random();
        let fits = "a".repeat(NAME_MAX - "app-athanor-@.service".len() - random.len());
        assert!(app_unit_name(&fits, &random).is_some());
        assert!(app_unit_name(&format!("{fits}a"), &random).is_none());
        // Escaping counts: one dash is four characters.
        assert!(app_unit_name(&format!("{}-", &fits[1..]), &random).is_none());
    }

    #[test]
    fn the_parameters_match_start_transient_unit() {
        let unit = Unit {
            name: "app-athanor-x@1.service".into(),
            description: "X".into(),
            argv: vec!["/usr/bin/x".into(), "--flag".into()],
            environment: vec!["WAYLAND_DISPLAY=/run/user/1000/athanor/1/wayland".into()],
            working_directory: "~".into(),
            runtime_directory: Some("athanor/1".into()),
        };
        let with_fd = unit.parameters(true).unwrap();
        assert_eq!(with_fd.type_().as_str(), "(ssa(sv)a(sa(sv)))");
        let text = with_fd.print(false);
        assert!(
            text.contains("('ExecStart', <[('/usr/bin/x', ['/usr/bin/x', '--flag'], false)]>)"),
            "{text}"
        );
        assert!(
            text.contains("('ExtraFileDescriptors', <[(handle 0, 'wayland-context')]>)"),
            "{text}"
        );
        assert!(
            text.contains(
                "('RuntimeDirectory', <['athanor/1']>), ('RuntimeDirectoryMode', <uint32 448>)"
            ),
            "{text}"
        );
        assert!(!unit
            .parameters(false)
            .unwrap()
            .print(false)
            .contains("ExtraFileDescriptors"));
    }

    #[test]
    fn a_unit_with_no_program_is_refused() {
        let unit = Unit {
            name: "app-athanor-x@1.service".into(),
            description: "X".into(),
            argv: Vec::new(),
            environment: Vec::new(),
            working_directory: "~".into(),
            runtime_directory: None,
        };
        assert!(unit.parameters(false).is_err());
    }

    #[test]
    fn the_app_id_is_the_desktop_id_without_its_suffix() {
        assert_eq!(
            app_id("org.gnome.TextEditor.desktop"),
            "org.gnome.TextEditor"
        );
        assert_eq!(
            app_id("com.system76.CosmicLauncher"),
            "com.system76.CosmicLauncher"
        );
        assert_eq!(app_id("x.desktop.desktop"), "x.desktop");
    }
}
