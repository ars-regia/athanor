//! `os.athanor.Settings1` (doc_settings.md SE4): `Show(s target, a{sv} platform_data)`, served
//! by the unit, and the forwarder's call to it from a binary started anywhere else.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use gio::glib;
use gio::prelude::*;

pub const NAME: &str = "os.athanor.Settings1";
pub const PATH: &str = "/os/athanor/Settings1";
/// The key of `platform_data` that carries the caller's xdg-activation token (SE-Q9).
pub const ACTIVATION_TOKEN: &str = "activation-token";

/// More `Show` calls than a person makes before the window draws once: a caller that sends
/// more is told so, rather than growing the queue.
const MAX_QUEUED: usize = 16;
/// Longer than any target the registry holds (the window state's own bound).
const MAX_TARGET: usize = 250;

const XML: &str = r#"<node><interface name="os.athanor.Settings1">
<method name="Show"><arg type="s" name="target" direction="in"/><arg type="a{sv}" name="platform_data" direction="in"/></method>
</interface></node>"#;

/// Owns the name and serves `Show`, whose target joins `shows` for the window's next turn; an
/// unknown target is the window's to log (SE4), so `Show` itself always succeeds. `on_ready`
/// runs once the name is ours. Losing the name ends the process, since no caller could reach it.
pub fn own(
    shows: Rc<RefCell<VecDeque<String>>>,
    on_ready: impl FnOnce() + 'static,
) -> gio::OwnerId {
    let on_ready = Cell::new(Some(on_ready));
    gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |connection, _| {
            let interface = gio::DBusNodeInfo::for_xml(XML)
                .ok()
                .and_then(|node| node.lookup_interface(NAME));
            let Some(interface) = interface else {
                tracing::error!("the interface of {NAME} does not parse");
                std::process::exit(1);
            };
            let shows = shows.clone();
            let registered = connection
                .register_object(PATH, &interface)
                .method_call(move |_, _, _, _, method, parameters, invocation| {
                    if method != "Show" {
                        invocation.return_dbus_error(
                            "org.freedesktop.DBus.Error.UnknownMethod",
                            &format!("no method {method}"),
                        );
                        return;
                    }
                    // GDBus checked the signature against the interface before this call.
                    match parameters.child_value(0).str() {
                        Some(target) => match enqueue(&shows, target) {
                            Ok(()) => invocation.return_value(None),
                            Err((name, message)) => invocation.return_dbus_error(name, message),
                        },
                        None => invocation.return_dbus_error(
                            "org.freedesktop.DBus.Error.InvalidArgs",
                            "Show takes a string and a dictionary",
                        ),
                    }
                })
                .build();
            if let Err(err) = registered {
                tracing::error!(error = %err, "cannot export {PATH}");
                std::process::exit(1);
            }
        },
        move |_, _| {
            if let Some(ready) = on_ready.take() {
                ready();
            }
        },
        |_, _| {
            tracing::error!("lost or could not own {NAME}; no Show would reach Settings");
            std::process::exit(1);
        },
    )
}

/// Queues `target` for the window's next turn, or the D-Bus error that refuses it.
fn enqueue(
    shows: &RefCell<VecDeque<String>>,
    target: &str,
) -> Result<(), (&'static str, &'static str)> {
    if target.len() > MAX_TARGET {
        return Err((
            "org.freedesktop.DBus.Error.InvalidArgs",
            "no target of Settings is that long",
        ));
    }
    let mut shows = shows.borrow_mut();
    if shows.len() >= MAX_QUEUED {
        return Err((
            "org.freedesktop.DBus.Error.LimitsExceeded",
            "too many Show calls wait for the window",
        ));
    }
    shows.push_back(target.to_owned());
    Ok(())
}

/// The forwarder of SE4: `Show(target)` on the session bus, which starts the unit when it is
/// down, with the activation token the launcher gave this process, if any.
pub fn forward(target: &str, token: Option<&str>) -> Result<(), glib::Error> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    let platform = glib::VariantDict::new(None);
    if let Some(token) = token {
        platform.insert(ACTIVATION_TOKEN, token);
    }
    let parameters = glib::Variant::tuple_from_iter([target.to_variant(), platform.end()]);
    bus.call_sync(
        Some(NAME),
        PATH,
        NAME,
        "Show",
        Some(&parameters),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_refuses_a_long_target_and_a_flood() {
        let shows = RefCell::default();
        assert!(enqueue(&shows, &"a".repeat(MAX_TARGET + 1)).is_err());
        for _ in 0..MAX_QUEUED {
            assert_eq!(enqueue(&shows, "about"), Ok(()));
        }
        assert!(enqueue(&shows, "about").is_err());
        assert_eq!(shows.borrow().len(), MAX_QUEUED);
    }
}
