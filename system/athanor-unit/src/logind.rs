//! The session of this process, found through logind (doc_lock_and_prompts.md, LP2): the lock
//! and the polkit agent find it the same way, with this one copy.

use std::fmt;

use zbus::proxy::CacheProperties;
use zbus::{Connection, OwnedObjectPath};

#[zbus::proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/user/self"
)]
trait User {
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn user(&self) -> zbus::Result<(u32, OwnedObjectPath)>;
}

/// Why the user's `Display` session is not this process's; its `Display` is the line to log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The user has no display session.
    NoDisplay,
    /// The session is not a Wayland session of this process's uid.
    NotOurs {
        id: String,
        kind: String,
        owner: u32,
        uid: u32,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::NoDisplay => write!(f, "the user has no display session"),
            Refused::NotOurs {
                id,
                kind,
                owner,
                uid,
            } => write!(
                f,
                "session {id} is of type {kind:?} and uid {owner}, not a Wayland session of uid {uid}"
            ),
        }
    }
}

/// The session of `Display`, if it is a Wayland session of this process's uid (LP2).
pub async fn our_session(query: &Connection) -> zbus::Result<Result<OwnedObjectPath, Refused>> {
    let user = UserProxy::builder(query)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let (id, path) = user.display().await?;
    if id.is_empty() {
        return Ok(Err(Refused::NoDisplay));
    }
    let session = SessionProxy::builder(query)
        .path(path.clone())
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let kind = session.kind().await?;
    let (owner, _) = session.user().await?;
    // The uid the bus took from the socket: `/proc/self` belongs to root once the process is
    // not dumpable, as the lock makes itself once Sal has connected, while this thread runs.
    let me = query
        .unique_name()
        .ok_or_else(|| zbus::Error::Failure("the connection has no unique name".into()))?;
    let uid = zbus::fdo::DBusProxy::new(query)
        .await?
        .get_connection_unix_user(me.as_ref().into())
        .await?;
    if kind != "wayland" || owner != uid {
        return Ok(Err(Refused::NotOurs {
            id,
            kind,
            owner,
            uid,
        }));
    }
    Ok(Ok(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::{env, fs, process};

    const SESSION: &str = "/org/freedesktop/login1/session/c1";

    /// A private `dbus-daemon`, stopped and its directory removed on drop.
    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start() -> Bus {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let dir = env::temp_dir().join(format!(
                "athanor-unit-logind-{}-{}",
                process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("mkdir");
            // A configuration of its own: `--session` reads the host's, which on a system
            // running dbus-broker has no <listen> element.
            let config = dir.join("bus.conf");
            fs::write(
                &config,
                format!(
                    "<busconfig><type>session</type><listen>unix:path={}</listen>\
                     <policy context=\"default\"><allow send_destination=\"*\"/>\
                     <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>",
                    dir.join("bus").display()
                ),
            )
            .expect("bus.conf");
            let mut daemon = Command::new("dbus-daemon")
                .args(["--nofork", "--nopidfile", "--print-address"])
                .arg(format!("--config-file={}", config.display()))
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon on PATH");
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
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    struct FakeUser {
        display: (String, OwnedObjectPath),
    }

    #[zbus::interface(name = "org.freedesktop.login1.User")]
    impl FakeUser {
        #[zbus(property)]
        fn display(&self) -> (String, OwnedObjectPath) {
            self.display.clone()
        }
    }

    struct FakeSession {
        kind: String,
        uid: u32,
    }

    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl FakeSession {
        #[zbus(property, name = "Type")]
        fn kind(&self) -> String {
            self.kind.clone()
        }

        #[zbus(property)]
        fn user(&self) -> (u32, OwnedObjectPath) {
            (
                self.uid,
                OwnedObjectPath::try_from("/org/freedesktop/login1/user/_1").expect("path"),
            )
        }
    }

    /// `our_session`'s answer and the test's uid, with a fake logind that names session `id`
    /// of type `kind` as the user's `Display`, owned by the test's uid plus `uid_offset`.
    fn ask(id: &str, kind: &str, uid_offset: u32) -> (Result<OwnedObjectPath, Refused>, u32) {
        let bus = Bus::start();
        zbus::block_on(async {
            let logind = zbus::connection::Builder::address(bus.address.as_str())
                .build()
                .await
                .expect("fake logind's connection");
            let query = zbus::connection::Builder::address(bus.address.as_str())
                .build()
                .await
                .expect("query connection");
            let me = query.unique_name().expect("unique name").clone();
            let uid = zbus::fdo::DBusProxy::new(&query)
                .await
                .expect("bus proxy")
                .get_connection_unix_user(me.as_ref().into())
                .await
                .expect("own uid");
            let server = logind.object_server();
            let path = if id.is_empty() { "/" } else { SESSION };
            server
                .at(
                    "/org/freedesktop/login1/user/self",
                    FakeUser {
                        display: (
                            id.to_owned(),
                            OwnedObjectPath::try_from(path).expect("path"),
                        ),
                    },
                )
                .await
                .expect("user");
            server
                .at(
                    SESSION,
                    FakeSession {
                        kind: kind.to_owned(),
                        uid: uid.wrapping_add(uid_offset),
                    },
                )
                .await
                .expect("session");
            logind
                .request_name("org.freedesktop.login1")
                .await
                .expect("name");
            let answer = our_session(&query).await.expect("logind answered");
            (answer, uid)
        })
    }

    #[test]
    fn a_wayland_session_of_this_uid_is_ours() {
        let (answer, _) = ask("c1", "wayland", 0);
        assert_eq!(
            answer,
            Ok(OwnedObjectPath::try_from(SESSION).expect("path"))
        );
    }

    #[test]
    fn a_user_with_no_display_session_is_refused() {
        let (answer, _) = ask("", "wayland", 0);
        assert_eq!(answer, Err(Refused::NoDisplay));
        assert_eq!(
            Refused::NoDisplay.to_string(),
            "the user has no display session"
        );
    }

    #[test]
    fn a_session_that_is_not_wayland_is_refused() {
        let (answer, uid) = ask("c1", "x11", 0);
        let refused = Refused::NotOurs {
            id: "c1".into(),
            kind: "x11".into(),
            owner: uid,
            uid,
        };
        assert_eq!(answer, Err(refused.clone()));
        assert_eq!(
            refused.to_string(),
            format!(
                "session c1 is of type \"x11\" and uid {uid}, not a Wayland session of uid {uid}"
            )
        );
    }

    #[test]
    fn a_session_of_another_uid_is_refused() {
        let (answer, uid) = ask("c1", "wayland", 1);
        assert_eq!(
            answer,
            Err(Refused::NotOurs {
                id: "c1".into(),
                kind: "wayland".into(),
                owner: uid.wrapping_add(1),
                uid,
            })
        );
    }
}
