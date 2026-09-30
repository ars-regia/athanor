//! `athanor-update go-back`: the administrator's way to `GoBack()` from a console.
//!
//! It is a client of the running service, as the notifier is: the service asks polkit
//! (`os.athanor.update.rollback`, administrator authentication from every kind of session),
//! goes back with `bootc rollback`, holds the digest it left and asks logind to restart.
//! Run as root, through `sudo`, polkit authorises at once. Any other user needs an
//! authentication agent, which a text console does not have, and the command says so.

use crate::serve::{BUS_NAME, OBJECT_PATH};
use zbus::Connection;

const ERROR_PREFIX: &str = "os.athanor.Update1.Error.";

/// Calls `GoBack()` and describes the outcome for the person at the console.
///
/// # Errors
/// The text to print: the service refused, or could not be reached.
pub async fn run(conn: &Connection) -> Result<String, String> {
    match conn.call_method(Some(BUS_NAME), OBJECT_PATH, Some(BUS_NAME), "GoBack", &()).await {
        Ok(_) => Ok("The previous version boots next. The machine restarts now.".into()),
        Err(zbus::Error::MethodError(name, detail, _)) => Err(describe(name.as_str(), detail.as_deref())),
        Err(err) => Err(format!("cannot reach {BUS_NAME}: {err}")),
    }
}

const NOT_AUTHORIZED: &str = "not authorized: go back as an administrator, with `sudo athanor-update go-back`";

/// What an error means to the person who asked. The service words its own refusals, and
/// those are used. A refusal to authorise, from the service or from the bus, says what to
/// do; a service that is not there, or would not start, says that.
fn describe(name: &str, detail: Option<&str>) -> String {
    let detail = detail.filter(|text| !text.is_empty());
    match name.strip_prefix(ERROR_PREFIX) {
        Some("NotAuthorized") => NOT_AUTHORIZED.into(),
        Some(_) => detail.map_or_else(|| name.to_owned(), str::to_owned),
        None => match name {
            "org.freedesktop.DBus.Error.AccessDenied" => NOT_AUTHORIZED.into(),
            "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner" | "org.freedesktop.DBus.Error.NoReply" => {
                format!("cannot reach {BUS_NAME}: {}; is athanor-update installed?", detail.unwrap_or(name))
            }
            _ => format!("{name}: {}", detail.unwrap_or("no detail")),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::{env, fs, path::PathBuf, process};

    use zbus::connection::Builder;
    use zbus::interface;

    use super::*;
    use crate::serve::Error;

    /// A private dbus-daemon, as the tests of `athanor-shelld` use.
    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(name: &str) -> Bus {
            let dir = env::temp_dir().join(format!("athanor-update-{name}-{}", process::id()));
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
            Bus { daemon, address: address.trim().to_owned(), dir }
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

    /// `os.athanor.Update1` with a fixed answer, in the service's own error type.
    enum Fake {
        Goes,
        Refuses(fn() -> Error),
    }

    #[interface(name = "os.athanor.Update1")]
    impl Fake {
        async fn go_back(&self) -> Result<(), Error> {
            match self {
                Fake::Goes => Ok(()),
                Fake::Refuses(error) => Err(error()),
            }
        }
    }

    async fn ask(name: &str, service: Fake) -> Result<String, String> {
        let bus = Bus::start(name);
        let _service = bus.builder().name(BUS_NAME).expect("name").serve_at(OBJECT_PATH, service).expect("serve").build().await.expect("service");
        let client = bus.builder().build().await.expect("client");
        run(&client).await
    }

    #[tokio::test]
    async fn a_service_that_goes_back_is_reported_as_restarting() {
        let text = ask("goes", Fake::Goes).await.expect("success");
        assert!(text.contains("restarts now"), "{text}");
    }

    #[tokio::test]
    async fn a_refusal_of_authorisation_says_to_use_sudo() {
        let text = ask("denied", Fake::Refuses(|| Error::NotAuthorized("not authorized".into()))).await.expect_err("refused");
        assert!(text.contains("sudo athanor-update go-back"), "{text}");
    }

    #[tokio::test]
    async fn the_other_refusals_use_the_words_of_the_service() {
        let text = ask("noprev", Fake::Refuses(|| Error::NoPreviousVersion("there is no previous version to go back to".into()))).await.expect_err("refused");
        assert_eq!(text, "there is no previous version to go back to");
        let text = ask("blocked", Fake::Refuses(|| Error::Blocked("a restart is blocked by an inhibitor".into()))).await.expect_err("refused");
        assert_eq!(text, "a restart is blocked by an inhibitor");
    }

    #[tokio::test]
    async fn no_service_on_the_bus_is_reported_as_unreachable() {
        let bus = Bus::start("absent");
        let client = bus.builder().build().await.expect("client");
        let text = run(&client).await.expect_err("nobody answers");
        assert!(text.starts_with("cannot reach os.athanor.Update1"), "{text}");
        assert!(text.ends_with("is athanor-update installed?"), "{text}");
    }

    #[test]
    fn an_access_denied_from_the_bus_says_to_use_sudo() {
        assert_eq!(describe("org.freedesktop.DBus.Error.AccessDenied", Some("rejected")), NOT_AUTHORIZED);
    }

    #[test]
    fn an_error_of_another_kind_is_named() {
        assert_eq!(describe("org.freedesktop.DBus.Error.InvalidArgs", Some("no")), "org.freedesktop.DBus.Error.InvalidArgs: no");
        assert_eq!(describe("org.freedesktop.DBus.Error.InvalidArgs", None), "org.freedesktop.DBus.Error.InvalidArgs: no detail");
        // A refusal of the service with no text still names what it was.
        assert_eq!(describe("os.athanor.Update1.Error.Busy", None), "os.athanor.Update1.Error.Busy");
    }
}
