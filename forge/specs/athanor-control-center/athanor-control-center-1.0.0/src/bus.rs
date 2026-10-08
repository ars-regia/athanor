//! `os.athanor.ControlCenter1` (CC2, CC9): `Show(page)` and `Toggle()`, and the property `Open`
//! the bar follows to hide its notification popups while the panel is shown. The bar's button
//! and Super+C call `Toggle`; the bus starts the unit when it is down.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

pub const NAME: &str = "os.athanor.ControlCenter1";
pub const PATH: &str = "/os/athanor/ControlCenter1";
/// What Super+C runs: a method call on the user bus, which starts the unit when it is down.
pub const TOGGLE_COMMAND: &str =
    "busctl --user call os.athanor.ControlCenter1 /os/athanor/ControlCenter1 os.athanor.ControlCenter1 Toggle";

const XML: &str = r#"<node><interface name="os.athanor.ControlCenter1">
<method name="Show"><arg type="s" name="page" direction="in"/></method>
<method name="Toggle"/>
<property name="Open" type="b" access="read"/>
</interface></node>"#;

/// The `Open` property and the connection its change is announced on.
#[derive(Default)]
pub struct Bus {
    connection: RefCell<Option<gio::DBusConnection>>,
    open: Cell<bool>,
}

impl Bus {
    /// Sets `Open` and, when it changed, emits `PropertiesChanged`.
    pub fn set_open(&self, open: bool) {
        if self.open.replace(open) == open {
            return;
        }
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        let changed = HashMap::from([("Open".to_owned(), open.to_variant())]);
        let parameters = (NAME, changed, Vec::<String>::new()).to_variant();
        if let Err(err) = connection.emit_signal(
            None,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&parameters),
        ) {
            tracing::warn!(error = %err, "cannot announce Open");
        }
    }
}

/// Owns the name and serves the interface. `on_show` takes the page id and refuses a bad one
/// with its reason; `on_ready` runs once the name is ours. Losing the name ends the process,
/// since nothing could reach it.
pub fn own(
    bus: Rc<Bus>,
    on_show: impl Fn(&str) -> Result<(), String> + 'static,
    on_toggle: impl Fn() + 'static,
    on_ready: impl FnOnce() + 'static,
) -> gio::OwnerId {
    let (on_show, on_toggle) = (Rc::new(on_show), Rc::new(on_toggle));
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
            let (on_show, on_toggle, shown) = (on_show.clone(), on_toggle.clone(), bus.clone());
            let registered = connection
                .register_object(PATH, &interface)
                .method_call(
                    move |_, _, _, _, method, parameters, invocation| match method {
                        "Show" => match parameters.child_value(0).str().map(|page| on_show(page)) {
                            Some(Ok(())) => invocation.return_value(None),
                            Some(Err(reason)) => invocation.return_dbus_error(
                                "org.freedesktop.DBus.Error.InvalidArgs",
                                &reason,
                            ),
                            None => invocation.return_dbus_error(
                                "org.freedesktop.DBus.Error.InvalidArgs",
                                "Show takes a string",
                            ),
                        },
                        "Toggle" => {
                            on_toggle();
                            invocation.return_value(None);
                        }
                        other => invocation.return_dbus_error(
                            "org.freedesktop.DBus.Error.UnknownMethod",
                            &format!("no method {other}"),
                        ),
                    },
                )
                .property(move |_, _, _, _, name| match name {
                    "Open" => shown.open.get().to_variant(),
                    _ => glib::Variant::from(false),
                })
                .build();
            if let Err(err) = registered {
                tracing::error!(error = %err, "cannot export {PATH}");
                std::process::exit(1);
            }
            bus.connection.replace(Some(connection));
        },
        move |_, _| {
            if let Some(ready) = on_ready.take() {
                ready();
            }
        },
        |_, _| {
            tracing::error!(
                "lost or could not own {NAME}; the bar's button and Super+C would reach nothing"
            );
            std::process::exit(1);
        },
    )
}
