//! Who may call the portal's methods.
//!
//! The methods of this backend are called by `xdg-desktop-portal`, the frontend that owns
//! `org.freedesktop.portal.Desktop`, on behalf of an application it has identified. They
//! sit on the session bus, where any process can call them directly, and then the
//! application id in the call is whatever that process says: the prompt would show the
//! user the name of an application that is not the one asking. So a method answers only
//! the frontend, and every other caller is refused before anything is shown.

use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::Connection;

const FRONTEND: &str = "org.freedesktop.portal.Desktop";

/// Refuses the call unless its sender is the current owner of the frontend's name.
pub async fn authorise(header: &Header<'_>, conn: &Connection) -> fdo::Result<()> {
    let sender = header
        .sender()
        .ok_or_else(|| fdo::Error::AccessDenied("a call with no sender".into()))?;
    if is_frontend(conn, sender).await {
        Ok(())
    } else {
        Err(fdo::Error::AccessDenied(format!(
            "only {FRONTEND} may call this interface"
        )))
    }
}

/// Whether `sender` owns the frontend's name. Fails closed: a bus that cannot say, or a
/// name nobody owns, is `false`.
async fn is_frontend(conn: &Connection, sender: &UniqueName<'_>) -> bool {
    let Ok(name) = BusName::from_static_str(FRONTEND) else {
        return false;
    };
    let Ok(bus) = DBusProxy::new(conn).await else {
        return false;
    };
    match bus.get_name_owner(name).await {
        Ok(owner) => owner.as_str() == sender.as_str(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::{env, fs, path::PathBuf, process};

    use zbus::connection::Builder;
    use zbus::zvariant::{ObjectPath, Value};

    use super::*;
    use crate::portal::{FileChooserPortal, MicrophonePortal};

    const PORTAL_NAME: &str = "org.freedesktop.impl.portal.desktop.athanor";

    /// A private dbus-daemon, as the tests of `athanor-shelld` use.
    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(name: &str) -> Bus {
            let dir = env::temp_dir().join(format!("athanor-portal-{name}-{}", process::id()));
            fs::create_dir_all(&dir).expect("mkdir");
            let mut daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--print-address"])
                .arg(format!("--address=unix:path={}", dir.join("bus").display()))
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon");
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().expect("stdout"))
                .read_line(&mut address)
                .expect("address");
            Bus {
                daemon,
                address: address.trim().to_owned(),
                dir,
            }
        }

        fn builder(&self) -> Builder<'static> {
            Builder::address(self.address.as_str()).expect("address")
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            // Test teardown: a daemon that already exited, or a directory already gone, is fine.
            self.daemon.kill().ok();
            self.daemon.wait().ok();
            fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[tokio::test]
    async fn only_the_owner_of_the_frontend_name_is_the_frontend() {
        let bus = Bus::start("owner");
        let frontend = bus.builder().name(FRONTEND).expect("name").build().await.expect("frontend");
        let other = bus.builder().build().await.expect("other");
        let frontend_id = frontend.unique_name().expect("unique name").clone();
        let other_id = other.unique_name().expect("unique name").clone();

        assert!(is_frontend(&other, &frontend_id).await);
        assert!(!is_frontend(&other, &other_id).await);
    }

    #[tokio::test]
    async fn nobody_is_the_frontend_while_nobody_owns_the_name() {
        let bus = Bus::start("nobody");
        let other = bus.builder().build().await.expect("other");
        let other_id = other.unique_name().expect("unique name").clone();
        assert!(!is_frontend(&other, &other_id).await);
    }

    /// The whole path: a portal method on a bus, called by the frontend and by a stranger.
    #[tokio::test]
    async fn a_method_answers_the_frontend_and_refuses_a_stranger() {
        let bus = Bus::start("method");
        let _portal = bus
            .builder()
            .name(PORTAL_NAME)
            .expect("name")
            .serve_at("/org/freedesktop/portal/desktop", MicrophonePortal)
            .expect("serve")
            .build()
            .await
            .expect("portal");
        let frontend = bus.builder().name(FRONTEND).expect("name").build().await.expect("frontend");
        let stranger = bus.builder().build().await.expect("stranger");

        let call = |conn: Connection| async move {
            conn.call_method(
                Some(PORTAL_NAME),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.impl.portal.Microphone"),
                "AccessMicrophone",
                &(
                    ObjectPath::try_from("/org/freedesktop/portal/desktop/request/1").expect("path"),
                    "org.example.App",
                    HashMap::<String, Value<'_>>::new(),
                ),
            )
            .await
        };

        // The stranger is refused before any prompt.
        let refused = call(stranger).await.expect_err("a stranger must be refused");
        assert!(
            refused.to_string().contains("only org.freedesktop.portal.Desktop"),
            "{refused}"
        );

        // The frontend gets an answer. The prompt program is not installed in the test
        // environment, so the answer is a denial, code 1, and never a grant.
        let reply = call(frontend).await.expect("the frontend is answered");
        let code: u32 = reply.body().deserialize().expect("a status code");
        assert_eq!(code, 1);
    }

    /// Saving is refused, not answered with a made-up path.
    #[tokio::test]
    async fn saving_is_refused_and_names_no_path() {
        let bus = Bus::start("save");
        let _portal = bus
            .builder()
            .name(PORTAL_NAME)
            .expect("name")
            .serve_at("/org/freedesktop/portal/desktop", FileChooserPortal)
            .expect("serve")
            .build()
            .await
            .expect("portal");
        let frontend = bus.builder().name(FRONTEND).expect("name").build().await.expect("frontend");

        for method in ["SaveFile", "SaveFiles"] {
            let reply = frontend
                .call_method(
                    Some(PORTAL_NAME),
                    "/org/freedesktop/portal/desktop",
                    Some("org.freedesktop.impl.portal.FileChooser"),
                    method,
                    &(
                        ObjectPath::try_from("/org/freedesktop/portal/desktop/request/2").expect("path"),
                        "org.example.App",
                        "",
                        "Save",
                        HashMap::<String, Value<'_>>::new(),
                    ),
                )
                .await
                .expect("the frontend is answered");
            let (code, results): (u32, HashMap<String, zbus::zvariant::OwnedValue>) =
                reply.body().deserialize().expect("a response");
            assert_eq!(code, 2, "{method} must end in another way, not succeed");
            assert!(results.is_empty(), "{method} must name no path");
        }
    }
}
