//! The control center's button (doc_control_center.md, CC1): the last of the status row, which
//! calls `Toggle()` on the panel's own program. It shows only while the bus knows that name,
//! which is when the package is installed (the unit is started by the call), so a bar without
//! the control center draws no button that does nothing.

use std::rc::Rc;

use athanor_bar::control_center::{NAME, PATH};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::bus::{call, TIMEOUT_MS};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

/// Calls `method` on the control center, with `args` when it takes any. The bus starts the
/// program when it is down; a call that fails, for the name unknown or any other reason, is
/// logged.
fn call_center(method: &'static str, args: Option<glib::Variant>) {
    glib::spawn_future_local(async move {
        let result = async {
            let bus = gio::bus_get_future(gio::BusType::Session).await?;
            call(&bus, NAME, PATH, NAME, method, args.as_ref(), TIMEOUT_MS).await
        }
        .await;
        if let Err(err) = result {
            tracing::error!(method, error = %err, "the control center call failed");
        }
    });
}

/// Asks the control center to show or hide its notification center.
pub(super) fn toggle_notifications() {
    call_center("ToggleNotifications", None);
}

/// Opens the notification center on the row of notification `id`, its reply entry focused: a
/// popup never takes the keyboard (BR4), so the reply is typed there.
pub(super) fn show_notification(id: u32) {
    call_center("Show", Some((format!("notifications:{id}"),).to_variant()));
}

struct ControlCenterUi {
    button: gtk4::Button,
}

impl ModuleUi for ControlCenterUi {
    fn widget(&self) -> gtk4::Widget {
        self.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}
}

pub fn new(_bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let name = tr("Control center");
    let button = gtk4::Button::from_icon_name("preferences-system-symbolic");
    button.add_css_class("bar-button");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    button.set_visible(false);
    button.connect_clicked(|_| call_center("Toggle", None));
    let shown = button.downgrade();
    glib::spawn_future_local(async move {
        let activatable = async {
            let bus = gio::bus_get_future(gio::BusType::Session).await?;
            let reply = call(
                &bus,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ListActivatableNames",
                None,
                TIMEOUT_MS,
            )
            .await?;
            Ok::<_, glib::Error>(
                reply
                    .get::<(Vec<String>,)>()
                    .is_some_and(|(names,)| names.iter().any(|name| name == NAME)),
            )
        }
        .await;
        match (activatable, shown.upgrade()) {
            (Ok(present), Some(button)) => button.set_visible(present),
            (Err(err), _) => {
                tracing::warn!(error = %err, "cannot tell whether the control center is installed; its button stays hidden")
            }
            _ => {}
        }
    });
    Some(Box::new(ControlCenterUi { button }))
}
