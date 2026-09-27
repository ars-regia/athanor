//! The tiling switch (doc_bar.md, BR3), for the active workspace of this surface's output.
//! Hidden until the compositor reports the workspace's mode.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::tiling::{self, Tiling, WorkspaceView};
use athanor_compositor_client::WorkspaceId;
use gtk4::glib;
use gtk4::prelude::*;

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

struct TilingUi {
    popup: Popup,
    switch: gtk4::Switch,
    connector: Option<String>,
    target: Rc<Cell<Option<(WorkspaceId, Tiling)>>>,
    /// The switch is being set from the compositor's state, not by the user.
    updating: Rc<Cell<bool>>,
}

impl ModuleUi for TilingUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Workspaces {
            return;
        }
        let Some(client) = bar.client() else { return };
        let views: Vec<WorkspaceView<WorkspaceId>> = client
            .workspaces()
            .into_iter()
            .map(|workspace| WorkspaceView {
                id: workspace.id,
                active: workspace.active,
                output: workspace.output,
                tiling: workspace.tiling,
            })
            .collect();
        let target = tiling::active_on(&views, self.connector.as_deref())
            .and_then(|view| view.tiling.map(|mode| (view.id, mode)));
        self.target.set(target);
        self.popup.button.set_visible(target.is_some());
        if let Some((_, mode)) = target {
            self.updating.set(true);
            self.switch.set_active(mode == Tiling::Tiled);
            self.updating.set(false);
        }
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

pub fn new(bar: &Rc<Bar>, connector: Option<&str>) -> Option<Box<dyn ModuleUi>> {
    bar.client()?;
    let icon = gtk4::Image::from_icon_name("view-grid-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Tiling"));
    let label = gtk4::Label::new(Some(&tr("Tile windows")));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let switch = gtk4::Switch::new();
    switch.set_valign(gtk4::Align::Center);
    switch.update_relation(&[gtk4::accessible::Relation::LabelledBy(
        &[label.upcast_ref()],
    )]);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.append(&label);
    row.append(&switch);
    popup.popover.set_child(Some(&row));
    popup.button.set_visible(false);

    let target: Rc<Cell<Option<(WorkspaceId, Tiling)>>> = Rc::new(Cell::new(None));
    let updating = Rc::new(Cell::new(false));
    let (weak, switch_target, switch_updating) =
        (Rc::downgrade(bar), target.clone(), updating.clone());
    switch.connect_state_set(move |_, _| {
        if switch_updating.get() {
            return glib::Propagation::Proceed;
        }
        let (Some(bar), Some((id, mode))) = (weak.upgrade(), switch_target.get()) else {
            return glib::Propagation::Proceed;
        };
        if let Some(Err(err)) = bar
            .client()
            .map(|client| client.set_tiling(id, tiling::toggled(mode)))
        {
            tracing::error!(error = %err, "cannot change the tiling mode");
        }
        // The switch follows the compositor's answer, which arrives as a workspace event.
        glib::Propagation::Proceed
    });
    Some(Box::new(TilingUi {
        popup,
        switch,
        connector: connector.map(str::to_string),
        target,
        updating,
    }))
}
