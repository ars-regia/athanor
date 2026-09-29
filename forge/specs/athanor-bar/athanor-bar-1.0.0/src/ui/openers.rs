//! The launcher, application-library and workspaces buttons (doc_bar.md, BR3). They show
//! COSMIC's components through the compositor client, which starts them when they do not
//! run; pressing again leaves them shown, and closing them is the component's job.

use std::rc::Rc;

use athanor_compositor_client::Opener;
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;

use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct OpenerUi {
    button: gtk4::Button,
}

impl ModuleUi for OpenerUi {
    fn widget(&self) -> gtk4::Widget {
        self.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}
}

pub fn new(bar: &Rc<Bar>, opener: Opener) -> Option<Box<dyn ModuleUi>> {
    bar.client()?;
    let (icon, name) = match opener {
        Opener::Launcher => ("system-search-symbolic", tr("Launcher")),
        Opener::AppLibrary => ("view-app-grid-symbolic", tr("Applications")),
        Opener::Workspaces => ("view-paged-symbolic", tr("Workspaces")),
    };
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-button");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    let weak = Rc::downgrade(bar);
    button.connect_clicked(move |_| {
        let Some(bar) = weak.upgrade() else { return };
        glib::spawn_future_local(async move {
            let Some(client) = bar.client() else { return };
            if let Err(err) = client.open(opener).await {
                tracing::error!(error = %err, opener = ?opener, "cannot open the component");
            }
        });
    });
    Some(Box::new(OpenerUi { button }))
}
