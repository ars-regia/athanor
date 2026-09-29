//! The launcher, application-library and workspaces buttons (doc_bar.md, BR3, BR7). They
//! show COSMIC's components through the compositor client, which starts them when they do
//! not run; pressing again leaves them shown, and closing them is the component's job.

use std::rc::Rc;

use athanor_compositor_client::Opener;
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;

use crate::i18n::tr;
use crate::Host;

/// `None` without the compositor client (SH1).
pub fn button<H: Host>(host: &Rc<H>, opener: Opener) -> Option<gtk4::Button> {
    host.client()?;
    let (icon, name) = match opener {
        Opener::Launcher => ("system-search-symbolic", tr("Launcher")),
        Opener::AppLibrary => ("view-app-grid-symbolic", tr("Applications")),
        Opener::Workspaces => ("view-paged-symbolic", tr("Workspaces")),
    };
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-button");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    let weak = Rc::downgrade(host);
    button.connect_clicked(move |_| {
        let Some(host) = weak.upgrade() else { return };
        glib::spawn_future_local(async move {
            let Some(client) = host.client() else { return };
            if let Err(err) = client.open(opener).await {
                tracing::error!(error = %err, opener = ?opener, "cannot open the component");
            }
        });
    });
    Some(button)
}
