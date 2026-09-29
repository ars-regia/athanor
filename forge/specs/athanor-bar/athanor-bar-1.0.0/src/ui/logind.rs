//! The calls the power menu makes. Manager calls go to logind on the system bus and allow
//! interactive authorisation, so polkit can ask. Logging out stops the session target on
//! the user's systemd, which ends the session and returns to the greeter. Every call has a
//! timeout: a logind that does not answer leaves the bar running and the action unoffered.

use athanor_bar::power::{self, Action};
use gtk4::prelude::*;
use gtk4::{gio, glib};

const TIMEOUT_MS: i32 = 5000;
const LOGIN1: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
/// logind resolves `auto` to the caller's session, or else to the user's display session.
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION: &str = "org.freedesktop.login1.Session";
const SESSION_TARGET: &str = "athanor-session.target";

async fn call(
    bus: gio::BusType,
    name: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
) -> Result<glib::Variant, glib::Error> {
    let connection = gio::bus_get_future(bus).await?;
    connection
        .call_future(
            Some(name),
            path,
            interface,
            method,
            args,
            None,
            gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
            TIMEOUT_MS,
        )
        .await
}

/// Whether logind offers `action`. An error or a malformed answer means it is not.
pub async fn offered(action: Action) -> bool {
    let Some(method) = action.can_method() else {
        return true;
    };
    match call(
        gio::BusType::System,
        LOGIN1,
        MANAGER_PATH,
        MANAGER,
        method,
        None,
    )
    .await
    {
        Ok(reply) => match reply.get::<(String,)>() {
            Some((answer,)) => power::offered(&answer),
            None => {
                tracing::error!(method, reply = %reply.print(true), "logind answered with an unexpected type");
                false
            }
        },
        Err(err) => {
            tracing::warn!(error = %err, method, "logind did not say whether the action is offered; it is hidden");
            false
        }
    }
}

pub async fn run(action: Action) -> Result<(), glib::Error> {
    let reply = match (action, action.manager_method()) {
        (_, Some(method)) => {
            let interactive = (true,).to_variant();
            call(
                gio::BusType::System,
                LOGIN1,
                MANAGER_PATH,
                MANAGER,
                method,
                Some(&interactive),
            )
            .await
        }
        (Action::LogOut, None) => {
            let args = (SESSION_TARGET, "replace").to_variant();
            call(
                gio::BusType::Session,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
                "StopUnit",
                Some(&args),
            )
            .await
        }
        (_, None) => {
            call(
                gio::BusType::System,
                LOGIN1,
                SESSION_PATH,
                SESSION,
                "Lock",
                None,
            )
            .await
        }
    };
    reply.map(|_| ())
}
