//! The launcher, application-library and workspaces buttons (doc_bar.md, BR3), built by
//! athanor-apps, which the dock shares.

use std::rc::Rc;

use athanor_compositor_client::Opener;
use gtk4::prelude::*;

use super::{Bar, Changed, ModuleUi};

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
    let button = athanor_apps::openers::button(bar, opener)?;
    Some(Box::new(OpenerUi { button }))
}
