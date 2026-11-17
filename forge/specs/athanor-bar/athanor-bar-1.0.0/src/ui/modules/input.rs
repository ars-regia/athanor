//! The bar's input source (doc_bar.md, BR3), as the GTK bar's `ui/input.rs` was: shown while the
//! seat has more than one layout, named after the active one, with a radio per layout in its
//! popover.

use std::rc::Rc;

use athanor_bar::i18n::{tr, tr_with};
use athanor_bar::keyboard;
use athanor_sal::{Event, Ui, WidgetId};
use athanor_sulphur_client::Client;

use super::{Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct Input {
    client: Rc<Client>,
    /// The button's name, as last drawn.
    drawn: Option<String>,
    /// Each layout's radio and its group, while the popover is open.
    radios: Vec<(WidgetId, u32)>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let client = start.client?.clone();
    if let Err(err) = client.watch_keyboard_group() {
        tracing::warn!(error = %err, "the keyboard group is not followed");
    }
    Some(Box::new(Input {
        client,
        drawn: None,
        radios: Vec::new(),
    }))
}

/// The button's name: the active layout's; `None` while the module is hidden.
fn name(client: &Client) -> Option<String> {
    let layouts = client.keyboard_layouts();
    if !keyboard::shown(&layouts) {
        return None;
    }
    let active = keyboard::active(&layouts, client.keyboard_group()).unwrap_or_default();
    Some(tr_with("Input source: {name}", "name", active))
}

impl Module for Input {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        self.drawn = name(&self.client);
        let name = self.drawn.as_deref()?;
        let icon = "input-keyboard-symbolic";
        Some(cx.icons.button(ui, group, icon, ICON, cx.scale, name, None))
    }

    fn stale(&self, _cx: &Cx) -> bool {
        name(&self.client) != self.drawn
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let layouts = self.client.keyboard_layouts();
        let group = self.client.keyboard_group();
        let title = name(&self.client).unwrap_or_else(|| tr("Input source"));
        let (mut ui, frame) = look::popover(&title, cx.theme.clone(), cx.rtl, cx.top);
        let labels = keyboard::labels(&layouts, |number| {
            tr_with("Layout {number}", "number", &number.to_string())
        });
        self.radios = (0u32..)
            .zip(labels)
            .map(|(index, label)| (ui.radio(frame, &label, index == group), index))
            .collect();
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, _ui: &mut Ui, event: Event) -> Reply {
        if let Event::Selected(widget) = event {
            if let Some((_, group)) = self.radios.iter().find(|(radio, _)| *radio == widget) {
                if let Err(err) = self.client.set_keyboard_group(*group) {
                    tracing::error!(error = %err, "cannot change the input source");
                }
            }
        }
        Reply::Stay
    }

    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        let group = self.client.keyboard_group();
        if let Some((radio, _)) = self.radios.iter().find(|(_, g)| *g == group) {
            ui.set_active(*radio, true);
        }
        Reply::Stay
    }

    fn closed(&mut self) {
        self.radios.clear();
    }
}
