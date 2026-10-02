//! `os.athanor.Launcher1` (LA8): one method, `Show`, which shows the launcher or hides it when
//! shown. Super calls it through cosmic-comp's `Launcher` system action, the bar's launcher
//! module through `Opener::Launcher`; the bus starts the unit when it is down.

use gtk4::gio;

pub const NAME: &str = "os.athanor.Launcher1";
pub const PATH: &str = "/os/athanor/Launcher1";
/// The system action written into cosmic-comp's shortcuts at start.
pub const SHOW_COMMAND: &str =
    "gdbus call --session --dest os.athanor.Launcher1 --object-path /os/athanor/Launcher1 --method os.athanor.Launcher1.Show";

const XML: &str = r#"<node><interface name="os.athanor.Launcher1"><method name="Show"/></interface></node>"#;

/// Owns the name and serves `Show`. `on_ready` runs once the name is ours, which is when a
/// `Show` can arrive; losing the name ends the process, since nothing could reach it.
pub fn own(on_show: impl Fn() + 'static, on_ready: impl FnOnce() + 'static) -> gio::OwnerId {
    let on_show = std::rc::Rc::new(on_show);
    let on_ready = std::cell::Cell::new(Some(on_ready));
    gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |connection, _| {
            let interface = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|node| node.lookup_interface(NAME));
            let Some(interface) = interface else {
                tracing::error!("the interface of {NAME} does not parse");
                std::process::exit(1);
            };
            let on_show = on_show.clone();
            let registered = connection
                .register_object(PATH, &interface)
                .method_call(move |_, _, _, _, method, _, invocation| match method {
                    "Show" => {
                        on_show();
                        invocation.return_value(None);
                    }
                    other => invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        &format!("no method {other}"),
                    ),
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
            tracing::error!("lost or could not own {NAME}; Super would reach nothing");
            std::process::exit(1);
        },
    )
}
