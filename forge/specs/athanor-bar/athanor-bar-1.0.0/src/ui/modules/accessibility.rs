//! The bar's accessibility menu (doc_bar.md, BR3), as the GTK bar's `ui/accessibility.rs` was:
//! high contrast; the screen reader with its GNOME key; with the compositor's state, the
//! magnifier, inverted colours and the colour filter; and the keyboard aids (AX11), whose
//! state the button shows beside its icon and whose offer the menu accepts.

use std::rc::Rc;

use athanor_bar::i18n::tr;
use athanor_bar::{magnifier, screen_reader};
use athanor_sal::{Event, Kind, Ui, WidgetId};
use athanor_style::appearance::{self, Appearance};
use athanor_sulphur_client::{theme, Accessibility, Client, Event as Compositor, ScreenFilter};
use gio::prelude::SettingsExt;

use super::aids;
use super::{Cx, Module, Reply, Start, ICON};
use crate::ui::look;

/// The filters a user can choose. `Unknown` is read, never offered.
const FILTERS: [ScreenFilter; 5] = [
    ScreenFilter::None,
    ScreenFilter::Greyscale,
    ScreenFilter::Protanopia,
    ScreenFilter::Deuteranopia,
    ScreenFilter::Tritanopia,
];

fn filter_label(filter: ScreenFilter) -> String {
    match filter {
        ScreenFilter::Greyscale => tr("Greyscale"),
        ScreenFilter::Protanopia => tr("Red-green (protanopia)"),
        ScreenFilter::Deuteranopia => tr("Green-red (deuteranopia)"),
        ScreenFilter::Tritanopia => tr("Blue-yellow (tritanopia)"),
        ScreenFilter::None | ScreenFilter::Unknown => tr("No colour filter"),
    }
}

struct AccessModule {
    client: Option<Rc<Client>>,
    /// One keyboard aids' service for the bar: a notice is announced once.
    aids: aids::Service,
    /// What the button showed beside its icon at the last drawing.
    shown: String,
    /// The screen reader's key; without it, no row.
    reader_key: Option<gio::Settings>,
    /// The magnifier's key, followed for the compositor (doc_accessibility.md AX10).
    magnifier_key: Option<gio::Settings>,
    /// The open menu's widgets.
    open: Option<Widgets>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let client = start.client.cloned();
    let aids = aids::Service::default();
    let mut magnifier_key = None;
    if let Some(client) = &client {
        // athanor-sulphur's manager is bound at the first watch, before the magnifier's key
        // is sent; the keyboard aids' start asks for it.
        aids.start(client);
        magnifier_key = follow_magnifier(client);
    }
    let reader_key = screen_reader::settings(gio::SettingsSchemaSource::default().as_ref(), None);
    if reader_key.is_none() {
        tracing::warn!(
            "{} has no {}; the screen reader row is hidden",
            screen_reader::SCHEMA,
            screen_reader::KEY
        );
    }
    Some(Box::new(AccessModule {
        client,
        aids,
        shown: String::new(),
        reader_key,
        magnifier_key,
        open: None,
    }))
}

/// The magnifier's key (doc_accessibility.md AX10): true at the start turns the magnifier on,
/// and each change is sent to the compositor. `None` without the schema.
fn follow_magnifier(client: &Rc<Client>) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(magnifier::SCHEMA, true);
    if !schema.is_some_and(|schema| schema.has_key(magnifier::KEY)) {
        tracing::warn!(
            "{} has no {}; the magnifier row switches the compositor alone",
            magnifier::SCHEMA,
            magnifier::KEY
        );
        return None;
    }
    let settings = gio::Settings::new(magnifier::SCHEMA);
    let weak = Rc::downgrade(client);
    magnifier::follow(&settings, move |enabled| {
        if let Some(Err(err)) = weak.upgrade().map(|client| client.set_magnifier(enabled)) {
            tracing::info!(error = %err, enabled, "the magnifier's key cannot reach the compositor");
        }
    });
    Some(settings)
}

/// The menu's widgets.
struct Widgets {
    high_contrast: WidgetId,
    /// Without the screen reader's key, none.
    reader: Option<WidgetId>,
    /// Without the compositor's state, none.
    magnifier: Option<WidgetId>,
    inverted: Option<WidgetId>,
    filters: Vec<(WidgetId, ScreenFilter)>,
    /// The offer's button, while the keyboard aids offer one.
    accept: Option<WidgetId>,
}

/// What the menu shows: the screen reader's key, the compositor's state and the keyboard aid
/// offered.
struct State<'a> {
    reader: Option<&'a gio::Settings>,
    compositor: Option<Accessibility>,
    offer: Option<athanor_sulphur_client::KeyboardAid>,
}

/// The menu: high contrast, the screen reader with its key, the magnifier, inverted colours
/// and the colour filter with the compositor's state, and the keyboard aid offered with its
/// button.
fn menu(cx: &Cx, state: &State) -> (Ui, Widgets) {
    let (mut ui, frame) = look::popover(&tr("Accessibility"), cx.theme.clone(), cx.rtl, cx.top);
    let high_contrast = ui.switch(
        frame,
        &tr("High contrast"),
        Appearance::read().high_contrast,
    );
    let reader = state
        .reader
        .map(|key| ui.switch(frame, &tr("Screen reader"), key.boolean(screen_reader::KEY)));
    let (mut magnifier, mut inverted, mut filters) = (None, None, Vec::new());
    if let Some(compositor) = state.compositor {
        magnifier = Some(ui.switch(frame, &tr("Magnifier"), compositor.magnifier));
        inverted = Some(ui.switch(frame, &tr("Invert colours"), compositor.inverted));
        let heading = ui.add(frame, Kind::Label(tr("Colour filter")));
        look::note(&mut ui, heading);
        let group = ui.add(frame, Kind::Column);
        for filter in FILTERS {
            let radio = ui.radio(group, &filter_label(filter), filter == compositor.filter);
            filters.push((radio, filter));
        }
    }
    let accept = state.offer.map(|aid| {
        let row = ui.add(frame, Kind::Row);
        ui.add(row, Kind::Label(aids::offer_text(aid)));
        ui.add(row, Kind::Button(tr("Turn on")))
    });
    let widgets = Widgets {
        high_contrast,
        reader,
        magnifier,
        inverted,
        filters,
        accept,
    };
    (ui, widgets)
}

impl Widgets {
    /// Sets the compositor's switches and the filter to `state` without raising events.
    fn show(&self, ui: &mut Ui, state: Accessibility) {
        for (widget, on) in [
            (self.magnifier, state.magnifier),
            (self.inverted, state.inverted),
        ] {
            if let Some(widget) = widget {
                ui.set_active(widget, on);
            }
        }
        for (radio, filter) in &self.filters {
            if *filter == state.filter {
                ui.set_active(*radio, true);
            }
        }
    }

    /// The filter chosen, with whether colours are inverted, as the compositor takes them.
    fn screen_filter(&self, ui: &Ui) -> Option<(bool, ScreenFilter)> {
        let inverted = self.inverted.is_some_and(|widget| ui.is_active(widget));
        self.filters
            .iter()
            .find(|(radio, _)| ui.is_active(*radio))
            .map(|(_, filter)| (inverted, *filter))
    }
}

impl Module for AccessModule {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        // The keyboard aids' state beside the icon (AX11).
        self.shown = self.aids.shown();
        let name = tr("Accessibility");
        let icon = "preferences-desktop-accessibility-symbolic";
        let button = match cx.icons.image(icon, ICON, cx.scale) {
            Some(image) if !self.shown.is_empty() => {
                let button =
                    ui.labelled_button(group, &name, Some(image), f32::from(ICON), &self.shown);
                ui.set_tooltip(button, Some(&name));
                button
            }
            _ => cx
                .icons
                .button(ui, group, icon, ICON, cx.scale, &name, None),
        };
        Some(button)
    }

    /// A rejected key or an offer expires with time, not with an event.
    fn stale(&self, _cx: &Cx) -> bool {
        self.aids.shown() != self.shown
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let state = State {
            reader: self.reader_key.as_ref(),
            compositor: self
                .client
                .as_ref()
                .and_then(|client| client.accessibility()),
            offer: self.aids.offer(),
        };
        let (ui, widgets) = menu(cx, &state);
        self.open = Some(widgets);
        Some(ui)
    }

    /// Acts on the menu's widget, as the GTK menu's rows do.
    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        let (Event::Toggled(widget) | Event::Selected(widget) | Event::Clicked(widget)) = event
        else {
            return Reply::Stay;
        };
        let Some(widgets) = &self.open else {
            return Reply::Stay;
        };
        let on = ui.is_active(widget);
        let compositor = self
            .client
            .as_ref()
            .and_then(|client| client.accessibility());
        if widget == widgets.high_contrast {
            if let Err(err) =
                appearance::apply(|choice| choice.high_contrast = on, theme::write_cosmic)
            {
                tracing::error!(error = %err, "cannot switch high contrast");
                ui.set_active(widget, Appearance::read().high_contrast);
            }
        } else if Some(widget) == widgets.reader {
            if let Some(key) = &self.reader_key {
                if let Err(err) = key.set_boolean(screen_reader::KEY, on) {
                    tracing::error!(error = %err, "cannot switch the screen reader");
                    ui.set_active(widget, key.boolean(screen_reader::KEY));
                }
            }
        } else if Some(widget) == widgets.magnifier {
            let client = self.client.as_ref();
            let sent = magnifier::switch(self.magnifier_key.as_ref(), on, |enabled| {
                client
                    .map_or(Ok(()), |client| client.set_magnifier(enabled))
                    .map_err(|err| err.to_string())
            });
            if let Err(err) = sent {
                tracing::error!(error = %err, "cannot switch the magnifier");
                // Show the compositor's state again.
                if let Some(state) = compositor {
                    widgets.show(ui, state);
                }
            }
        } else if Some(widget) == widgets.accept {
            self.aids.accept();
            // The menu again, without the offer; the button follows once it closes.
            return Reply::Reopen;
        } else if Some(widget) == widgets.inverted
            || widgets.filters.iter().any(|(radio, _)| *radio == widget)
        {
            // Inversion and the filter are one compositor request: each sends the other's
            // state.
            if let (Some((inverted, filter)), Some(client)) =
                (widgets.screen_filter(ui), &self.client)
            {
                if let Err(err) = client.set_screen_filter(inverted, filter) {
                    tracing::error!(error = %err, "cannot change the screen filter");
                    if let Some(state) = compositor {
                        widgets.show(ui, state);
                    }
                }
            }
        }
        Reply::Stay
    }

    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        let state = self
            .client
            .as_ref()
            .and_then(|client| client.accessibility());
        if let (Some(widgets), Some(state)) = (&self.open, state) {
            widgets.show(ui, state);
        }
        Reply::Stay
    }

    fn closed(&mut self) {
        self.open = None;
    }

    fn heard(&mut self, event: &Compositor) -> Option<String> {
        self.aids.heard(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_apps::sal::Icons;
    use athanor_sal::Theme;
    use athanor_sulphur_client::KeyboardAid;

    #[test]
    fn the_menu_shows_the_compositors_rows_only_with_its_state() {
        let mut icons = Icons::new("Adwaita", Theme::default().text);
        let theme = Theme::default();
        let cx = Cx {
            icons: &mut icons,
            theme: &theme,
            secondary: theme.text,
            rtl: false,
            top: true,
            scale: 1,
            connector: None,
        };
        let empty = State {
            reader: None,
            compositor: None,
            offer: None,
        };
        let (_, widgets) = menu(&cx, &empty);
        assert!(widgets.magnifier.is_none() && widgets.filters.is_empty());
        assert!(widgets.reader.is_none() && widgets.accept.is_none());
        let state = Accessibility {
            inverted: true,
            filter: ScreenFilter::Tritanopia,
            ..Accessibility::default()
        };
        let full = State {
            reader: None,
            compositor: Some(state),
            offer: Some(KeyboardAid::Sticky),
        };
        let (ui, widgets) = menu(&cx, &full);
        assert_eq!(widgets.filters.len(), FILTERS.len());
        assert!(widgets.accept.is_some());
        assert_eq!(
            widgets.screen_filter(&ui),
            Some((true, ScreenFilter::Tritanopia))
        );
    }
}
