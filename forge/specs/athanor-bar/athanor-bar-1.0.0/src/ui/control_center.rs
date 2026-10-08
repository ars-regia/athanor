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
    button.connect_clicked(|_| {
        glib::spawn_future_local(async {
            let result = async {
                let bus = gio::bus_get_future(gio::BusType::Session).await?;
                call(&bus, NAME, PATH, NAME, "Toggle", None, TIMEOUT_MS).await
            }
            .await;
            if let Err(err) = result {
                tracing::error!(error = %err, "the control center did not toggle");
            }
        });
    });
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
