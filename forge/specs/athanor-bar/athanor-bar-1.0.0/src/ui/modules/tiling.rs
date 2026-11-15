//! The bar's tiling switch (doc_bar.md, BR3), as the GTK bar's `ui/tiling.rs` was: shown on a
//! surface while the active workspace of its output reports its mode, with a switch in its
//! popover that tiles that workspace's windows or floats them.

use std::collections::HashMap;
use std::rc::Rc;

use athanor_bar::i18n::tr;
use athanor_bar::tiling::{self, Tiling, WorkspaceView};
use athanor_sal::{Event, Ui, WidgetId};
use athanor_sulphur_client::{Client, WorkspaceId};

use super::{Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct TilingModule {
    client: Rc<Client>,
    /// Whether each output's button was drawn, by connector.
    drawn: HashMap<Option<String>, bool>,
    /// The open popover's switch and the workspace it changes, with the mode it had when
    /// drawn.
    open: Option<(WidgetId, (WorkspaceId, Tiling))>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let client = start.client?.clone();
    if let Err(err) = client.watch_tiling() {
        tracing::warn!(error = %err, "the tiling of the workspaces is not followed");
    }
    Some(Box::new(TilingModule {
        client,
        drawn: HashMap::new(),
        open: None,
    }))
}

/// The workspace whose tiling the switch of `connector`'s surface changes, and its mode: the
/// active one of that output, while the compositor reports its mode.
fn target(client: &Client, connector: Option<&str>) -> Option<(WorkspaceId, Tiling)> {
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
    tiling::active_on(&views, connector).and_then(|view| view.tiling.map(|mode| (view.id, mode)))
}

impl Module for TilingModule {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let shown = target(&self.client, cx.connector).is_some();
        self.drawn.insert(cx.connector.map(str::to_owned), shown);
        if !shown {
            return None;
        }
        let icon = "view-grid-symbolic";
        Some(
            cx.icons
                .button(ui, group, icon, ICON, cx.scale, &tr("Tiling"), None),
        )
    }

    fn stale(&self, cx: &Cx) -> bool {
        let shown = target(&self.client, cx.connector).is_some();
        self.drawn.get(&cx.connector.map(str::to_owned)) != Some(&shown)
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let target = target(&self.client, cx.connector)?;
        let (mut ui, frame) = look::popover(&tr("Tiling"), cx.theme.clone(), cx.rtl, cx.top);
        let switch = ui.switch(frame, &tr("Tile windows"), target.1 == Tiling::Tiled);
        self.open = Some((switch, target));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        let Some((switch, (id, mode))) = self.open else {
            return Reply::Stay;
        };
        if !matches!(event, Event::Toggled(widget) if widget == switch) {
            return Reply::Stay;
        }
        if let Err(err) = self.client.set_tiling(id, tiling::toggled(mode)) {
            tracing::error!(error = %err, "cannot change the tiling mode");
            // No workspace event follows a request that was not sent: show the mode the
            // workspace still has.
            ui.set_active(switch, mode == Tiling::Tiled);
        }
        Reply::Stay
    }

    fn refresh(&mut self, cx: &mut Cx, ui: &mut Ui) -> Reply {
        let Some((switch, drawn)) = &mut self.open else {
            return Reply::Stay;
        };
        match target(&self.client, cx.connector) {
            Some(target) if target.0 == drawn.0 => {
                ui.set_active(*switch, target.1 == Tiling::Tiled);
                *drawn = target;
                Reply::Stay
            }
            // Another workspace is active, or none reports its mode.
            _ => Reply::Close,
        }
    }

    fn closed(&mut self) {
        self.open = None;
    }
}
