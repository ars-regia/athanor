//! Who may call the portal's methods.
//!
//! The methods of this backend are called by `xdg-desktop-portal`, the frontend that owns
//! `org.freedesktop.portal.Desktop`, on behalf of an application it has identified. They
//! sit on the session bus, where any process can call them directly, and then the
//! application id in the call is whatever that process says. So every method answers only
//! the frontend, and every other caller is refused with a journal line (doc_portal.md, PT3).
//! The frontend is the owner of that name which is also the main process of the image's
//! `xdg-desktop-portal.service`, as [`athanor_portal::frontend::check`] admits it: a process
//! of the user that took the name, or that runs from a unit it defined or overrode, is refused
//! (issue #1560).

use std::time::Duration;

use athanor_portal::eis::OnBus;
use athanor_portal::frontend::{self, NAME};
use athanor_unit::bus_caller::Questions;
use zbus::fdo::DBusProxy;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::Connection;

use crate::Error;

/// Bound on the whole check: the owner of the name, and every question of the admission to
/// the bus and to both systemds.
const ASK: Duration = Duration::from_secs(2);

/// The frontend check, asked on `query`, the query connection, which serves nothing and
/// subscribes to nothing, so that a reply is never queued behind the calls this daemon is
/// answering (issue #1035), and through `questions`, on the same connection and on a system
/// bus connection of their own.
pub struct Frontend<Q = OnBus> {
    query: Connection,
    questions: Q,
}

impl Frontend {
    /// Asks on `query` and on the system bus at its fixed socket.
    pub async fn new(query: Connection) -> zbus::Result<Frontend> {
        let questions = OnBus::new(&query, None).await?;
        Ok(Frontend { query, questions })
    }
}

impl<Q: Questions> Frontend<Q> {
    /// Refuses the call unless its sender is the frontend. Fails closed: a call with no
    /// sender, a name nobody owns, a bus or a systemd that cannot say or does not answer
    /// within [`ASK`], is refused.
    pub async fn authorise(&self, header: &Header<'_>) -> Result<(), Error> {
        self.authorise_within(header, ASK).await
    }

    async fn authorise_within(&self, header: &Header<'_>, limit: Duration) -> Result<(), Error> {
        let Some(sender) = header.sender() else {
            tracing::warn!("refused a call with no sender");
            return Err(Error::AccessDenied("a call with no sender".into()));
        };
        let why = match tokio::time::timeout(limit, self.check(sender.as_str())).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(why)) => why,
            Err(_) => "the bus or systemd did not answer in time".into(),
        };
        tracing::warn!(%sender, member = ?header.member(), %why, "refused a caller that is not {NAME}");
        Err(Error::AccessDenied(format!(
            "only {NAME} may call this interface"
        )))
    }

    async fn check(&self, sender: &str) -> Result<(), String> {
        let name = BusName::from_static_str(NAME).map_err(|e| e.to_string())?;
        let bus = DBusProxy::new(&self.query)
            .await
            .map_err(|e| e.to_string())?;
        let owner = bus
            .get_name_owner(name)
            .await
            .map_err(|e| format!("the owner of {NAME}: {e}"))?;
        frontend::check(&self.questions, sender, owner.as_str()).await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, RwLock};
    use std::{env, fs, path::PathBuf, process};

    use std::os::fd::{AsFd, BorrowedFd};
    use std::os::unix::fs::MetadataExt;

    use athanor_portal::frontend::FRONTEND;
    use athanor_unit::bus_caller::{Credentials, Manager, Unit, IMAGE_USER_UNITS};
    use zbus::connection::Builder;
    use zbus::names::UniqueName;
    use zbus::{OwnedObjectPath, OwnedVariant};

    use super::*;
    use crate::settings::{Portal, Store};

    const PORTAL_NAME: &str = "org.freedesktop.impl.portal.desktop.athanor";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const SETTINGS: &str = "org.freedesktop.impl.portal.Settings";

    /// A private dbus-daemon, as the tests of `athanor-shelld` use.
    pub(crate) struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        pub(crate) fn start(name: &str) -> Bus {
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

        pub(crate) fn builder(&self) -> Builder<'static> {
            Builder::address(self.address.as_str())
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

    /// The fixture of the admission's questions: the bus's real answers for a caller's
    /// credentials, and fixture answers for both systemds. The user's systemd is a connection of
    /// this process, whose pid is the main process of `user@<uid>.service`; every connection of
    /// the test runs in `xdg-desktop-portal.service`, as its main process, defined by
    /// `fragment` with `drop_ins`.
    pub(crate) struct Image {
        bus: DBusProxy<'static>,
        manager: String,
        fragment: String,
        drop_ins: Vec<String>,
    }

    const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/fixture";

    impl Questions for Image {
        async fn user_manager(&self) -> Result<String, String> {
            Ok(self.manager.clone())
        }

        async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
            let name = BusName::try_from(peer).map_err(|e| e.to_string())?;
            let found = self
                .bus
                .get_connection_credentials(name)
                .await
                .map_err(|e| e.to_string())?;
            let pidfd = match found.process_fd() {
                Some(fd) => Some(fd.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?),
                None => None,
            };
            Ok(Credentials {
                uid: found.unix_user_id(),
                pid: found.process_id(),
                pidfd,
            })
        }

        async fn unit_by_pidfd(
            &self,
            manager: Manager<'_>,
            _pidfd: BorrowedFd<'_>,
        ) -> Result<(OwnedObjectPath, String), String> {
            let unit = match manager {
                Manager::System => {
                    let uid = fs::metadata("/proc/self").map_err(|e| e.to_string())?.uid();
                    format!("user@{uid}.service")
                }
                Manager::User(_) => FRONTEND.unit.into(),
            };
            let path = OwnedObjectPath::try_from(UNIT_PATH).map_err(|e| e.to_string())?;
            Ok((path, unit))
        }

        async fn main_pid(&self, _: Manager<'_>, _: &OwnedObjectPath) -> Result<u32, String> {
            Ok(process::id())
        }

        async fn unit(&self, _: Manager<'_>, _: &OwnedObjectPath) -> Result<Unit, String> {
            Ok(Unit {
                main_pid: process::id(),
                exec_start: vec![FRONTEND.exec.into()],
                fragment_path: self.fragment.clone(),
                drop_in_paths: self.drop_ins.clone(),
            })
        }
    }

    /// The frontend check on `bus`, the image's unit answering for every caller.
    pub(crate) async fn image(bus: &Bus) -> Arc<Frontend<Image>> {
        defined_by(
            bus,
            format!("{IMAGE_USER_UNITS}/{}", FRONTEND.unit),
            Vec::new(),
        )
        .await
    }

    /// The frontend check on `bus`, the unit defined by `fragment` with `drop_ins`.
    async fn defined_by(
        bus: &Bus,
        fragment: String,
        drop_ins: Vec<String>,
    ) -> Arc<Frontend<Image>> {
        let query = bus.builder().build().await.expect("query");
        let questions = Image {
            bus: DBusProxy::new(&query).await.expect("proxy"),
            manager: query.unique_name().expect("name").to_string(),
            fragment,
            drop_ins,
        };
        Arc::new(Frontend { query, questions })
    }

    /// A call from `sender`, as the backend's handlers see its header.
    fn call(sender: &UniqueName<'_>) -> zbus::Message {
        zbus::Message::method_call(PATH, "Read")
            .sender(sender)
            .build(&())
            .expect("message")
    }

    #[tokio::test]
    async fn only_the_owner_of_the_frontend_name_is_the_frontend() {
        let bus = Bus::start("owner");
        let frontend = bus.builder().name(NAME).build().await.expect("frontend");
        let other = bus.builder().build().await.expect("other");
        let check = image(&bus).await;

        let message = call(frontend.unique_name().expect("unique name"));
        assert!(check.authorise(&message.header()).await.is_ok());
        let message = call(other.unique_name().expect("unique name"));
        assert!(check.authorise(&message.header()).await.is_err());
    }

    #[tokio::test]
    async fn nobody_is_the_frontend_while_nobody_owns_the_name() {
        let bus = Bus::start("nobody");
        let other = bus.builder().build().await.expect("other");
        let message = call(other.unique_name().expect("unique name"));
        assert!(image(&bus)
            .await
            .authorise(&message.header())
            .await
            .is_err());
    }

    /// A bus daemon that accepts the connection and never answers a call to it.
    #[tokio::test]
    async fn a_bus_that_does_not_answer_is_not_the_frontend() {
        let bus = Bus::start("silent");
        let check = image(&bus).await;
        let message = call(check.query.unique_name().expect("unique name"));
        // Stopped: the socket stays open, the process answers nothing.
        let pid = bus.daemon.id().to_string();
        assert!(Command::new("kill")
            .args(["-STOP", &pid])
            .status()
            .expect("kill")
            .success());
        let started = std::time::Instant::now();
        let frontend = check
            .authorise_within(&message.header(), Duration::from_millis(300))
            .await;
        Command::new("kill")
            .args(["-CONT", &pid])
            .status()
            .expect("kill");
        assert!(frontend.is_err());
        assert!(started.elapsed() < Duration::from_secs(5), "the check hung");
    }

    /// A portal serving one appearance value, on `bus`, behind `frontend`.
    async fn portal(bus: &Bus, frontend: Arc<Frontend<Image>>) -> Connection {
        let mut store = Store::new();
        store.insert(
            "org.freedesktop.appearance".into(),
            BTreeMap::from([("color-scheme".to_owned(), glib::Variant::from(1u32))]),
        );
        bus.builder()
            .name(PORTAL_NAME)
            .serve_at(PATH, Portal::new(Arc::new(RwLock::new(store)), frontend))
            .build()
            .await
            .expect("portal")
    }

    async fn read(conn: &Connection, namespace: &str, key: &str) -> zbus::Result<zbus::Message> {
        conn.call_method(Some(PORTAL_NAME), PATH, Some(SETTINGS), "Read", &(namespace, key))
            .await
    }

    /// The whole path: a portal method on a bus, called by the frontend and by a stranger.
    #[tokio::test]
    async fn a_method_answers_the_frontend_and_refuses_a_stranger() {
        let bus = Bus::start("method");
        let _portal = portal(&bus, image(&bus).await).await;
        let frontend = bus.builder().name(NAME).build().await.expect("frontend");
        let stranger = bus.builder().build().await.expect("stranger");

        let refused = read(&stranger, "org.freedesktop.appearance", "color-scheme")
            .await
            .expect_err("a stranger must be refused");
        assert!(
            matches!(&refused, zbus::Error::FDO(err) if matches!(**err, zbus::fdo::Error::AccessDenied(_))),
            "{refused}"
        );
        let refused = stranger
            .call_method(Some(PORTAL_NAME), PATH, Some(SETTINGS), "ReadAll", &(Vec::<String>::new(),))
            .await
            .expect_err("a stranger must be refused");
        assert!(refused.to_string().contains("only org.freedesktop.portal.Desktop"), "{refused}");

        let reply = read(&frontend, "org.freedesktop.appearance", "color-scheme")
            .await
            .expect("the frontend is answered");
        let value: OwnedVariant = reply.body().deserialize().expect("a variant");
        assert_eq!(u32::try_from(value).expect("a u32"), 1);

        let reply = frontend
            .call_method(Some(PORTAL_NAME), PATH, Some(SETTINGS), "ReadAll", &(vec!["org.freedesktop.*"],))
            .await
            .expect("the frontend is answered");
        let all: HashMap<String, HashMap<String, OwnedVariant>> =
            reply.body().deserialize().expect("a{sa{sv}}");
        assert_eq!(all.len(), 1);
        assert!(all["org.freedesktop.appearance"].contains_key("color-scheme"));
    }

    /// The frontend's name owned by a process of a unit the user defined, or overrode, under
    /// `~/.config/systemd/user`: refused, as a stranger is (issue #1560).
    #[tokio::test]
    async fn a_frontend_of_a_unit_the_user_defined_or_overrode_is_refused() {
        let user_units = "/home/user/.config/systemd/user";
        let image_unit = format!("{IMAGE_USER_UNITS}/{}", FRONTEND.unit);
        let cases = [
            (
                "defined",
                format!("{user_units}/{}", FRONTEND.unit),
                Vec::new(),
            ),
            (
                "overridden",
                image_unit,
                vec![format!("{user_units}/{}.d/override.conf", FRONTEND.unit)],
            ),
        ];
        for (what, fragment, drop_ins) in cases {
            let bus = Bus::start(what);
            let _portal = portal(&bus, defined_by(&bus, fragment, drop_ins).await).await;
            let frontend = bus.builder().name(NAME).build().await.expect("frontend");
            let refused = read(&frontend, "org.freedesktop.appearance", "color-scheme")
                .await
                .expect_err("a frontend outside the image must be refused");
            assert!(
                matches!(&refused, zbus::Error::FDO(err) if matches!(**err, zbus::fdo::Error::AccessDenied(_))),
                "{what}: {refused}"
            );
        }
    }

    /// A key that is not served is NotFound, the error the frontend falls through on.
    #[tokio::test]
    async fn a_setting_that_is_not_served_is_not_found() {
        let bus = Bus::start("missing");
        let _portal = portal(&bus, image(&bus).await).await;
        let frontend = bus.builder().name(NAME).build().await.expect("frontend");
        for (namespace, key) in [("org.freedesktop.appearance", "missing"), ("org.example", "color-scheme")] {
            let missing = read(&frontend, namespace, key).await.expect_err("not served");
            assert!(
                matches!(&missing, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.freedesktop.portal.Error.NotFound"),
                "{missing}"
            );
        }
    }
}
