//! The accessibility menu (doc_bar.md, BR3). High contrast through COSMIC's own theme keys,
//! read back by the bar's theme watch. The screen reader through the AT-SPI bus's status.
//! The magnifier, inverted colours and the colour filter through the compositor. Each row
//! shows only when its source answers; with none, the button is hidden (SH1).

use std::cell::Cell;
use std::rc::Rc;

use athanor_compositor_client::{theme, ScreenFilter};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

const A11Y_BUS: &str = "org.a11y.Bus";
const A11Y_PATH: &str = "/org/a11y/bus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const STATUS: &str = "org.a11y.Status";
const READER: &str = "ScreenReaderEnabled";
const TIMEOUT_MS: i32 = 5000;

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

async fn a11y_call(method: &str, args: glib::Variant) -> Result<glib::Variant, glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    bus.call_future(
        Some(A11Y_BUS),
        A11Y_PATH,
        PROPERTIES,
        method,
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
    )
    .await
}

/// `None` when the AT-SPI bus does not answer, or answers something else than a boolean.
async fn reader_enabled() -> Option<bool> {
    match a11y_call("Get", (STATUS, READER).to_variant()).await {
        Ok(reply) => reply
            .get::<(glib::Variant,)>()
            .and_then(|(value,)| value.get::<bool>()),
        Err(err) => {
            tracing::warn!(error = %err, "the AT-SPI bus does not answer; the screen reader row is hidden");
            None
        }
    }
}

fn switch_row(text: &str) -> (gtk4::Box, gtk4::Switch) {
    let label = gtk4::Label::new(Some(text));
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
    (row, switch)
}

struct Inner {
    popup: Popup,
    high_contrast_row: gtk4::Box,
    high_contrast: gtk4::Switch,
    reader_row: gtk4::Box,
    reader: gtk4::Switch,
    compositor_rows: gtk4::Box,
    magnifier: gtk4::Switch,
    inverted: gtk4::Switch,
    filters: Vec<(ScreenFilter, gtk4::CheckButton)>,
    /// A switch or a radio is being set from its source, not by the user.
    updating: Cell<bool>,
}

impl Inner {
    /// A row's own `visible` flag, not `is_visible()`: the rows live inside a `Popover`,
    /// whose `visible` is false until it is popped up, so the ancestor-aware check would
    /// read every row as invisible while the menu is closed, which is most of the time.
    fn fit(&self) {
        let any = self.high_contrast_row.get_visible()
            || self.reader_row.get_visible()
            || self.compositor_rows.get_visible();
        self.popup.button.set_visible(any);
    }

    /// The high contrast switch follows COSMIC's own theme keys, not an event: read it
    /// again whenever the popover is about to be seen.
    fn sync_high_contrast(&self) {
        self.updating.set(true);
        self.high_contrast
            .set_active(theme::read().is_high_contrast);
        self.updating.set(false);
    }

    fn show_reader(self: &Rc<Self>, bar: &Rc<Bar>) {
        let (inner, weak) = (self.clone(), Rc::downgrade(bar));
        glib::spawn_future_local(async move {
            let enabled = reader_enabled().await;
            inner.reader_row.set_visible(enabled.is_some());
            if let Some(enabled) = enabled {
                inner.updating.set(true);
                inner.reader.set_active(enabled);
                inner.updating.set(false);
            }
            inner.fit();
            if let Some(bar) = weak.upgrade() {
                // The group may have gained or lost its only visible button.
                bar.refresh(Changed::Accessibility);
            }
        });
    }
}

struct AccessibilityUi {
    inner: Rc<Inner>,
}

impl ModuleUi for AccessibilityUi {
    fn widget(&self) -> gtk4::Widget {
        self.inner.popup.button.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Accessibility {
            return;
        }
        let state = bar.client().and_then(|client| client.accessibility());
        let inner = &self.inner;
        inner.compositor_rows.set_visible(state.is_some());
        if let Some(state) = state {
            inner.updating.set(true);
            inner.magnifier.set_active(state.magnifier);
            inner.inverted.set_active(state.inverted);
            for (filter, check) in &inner.filters {
                check.set_active(*filter == state.filter);
            }
            inner.updating.set(false);
        }
        inner.fit();
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.inner.popup.open(bar);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let icon = gtk4::Image::from_icon_name("preferences-desktop-accessibility-symbolic");
    let popup = Popup::new(bar, &icon, &tr("Accessibility"));
    let (high_contrast_row, high_contrast) = switch_row(&tr("High contrast"));
    let (reader_row, reader) = switch_row(&tr("Screen reader"));
    let (magnifier_row, magnifier) = switch_row(&tr("Magnifier"));
    let (inverted_row, inverted) = switch_row(&tr("Invert colours"));
    let compositor_rows = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    compositor_rows.append(&magnifier_row);
    compositor_rows.append(&inverted_row);
    let heading = gtk4::Label::new(Some(&tr("Colour filter")));
    heading.add_css_class("bar-popover-note");
    heading.set_xalign(0.0);
    compositor_rows.append(&heading);
    let mut filters: Vec<(ScreenFilter, gtk4::CheckButton)> = Vec::new();
    for filter in FILTERS {
        let check = gtk4::CheckButton::with_label(&filter_label(filter));
        check.set_group(filters.first().map(|(_, first)| first));
        compositor_rows.append(&check);
        filters.push((filter, check));
    }
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.append(&high_contrast_row);
    content.append(&reader_row);
    content.append(&compositor_rows);
    popup.popover.set_child(Some(&content));
    high_contrast_row.set_visible(!theme::high_contrast_dirs().is_empty());
    reader_row.set_visible(false);
    compositor_rows.set_visible(false);

    let inner = Rc::new(Inner {
        popup,
        high_contrast_row,
        high_contrast,
        reader_row,
        reader,
        compositor_rows,
        magnifier,
        inverted,
        filters,
        updating: Cell::new(false),
    });
    inner.sync_high_contrast();
    inner.fit();

    let weak_inner = Rc::downgrade(&inner);
    inner.high_contrast.connect_state_set(move |_, enabled| {
        if weak_inner
            .upgrade()
            .is_some_and(|inner| !inner.updating.get())
        {
            if let Err(err) = theme::set_high_contrast(enabled) {
                tracing::error!(error = %err, "cannot switch high contrast");
            }
        }
        glib::Propagation::Proceed
    });
    let weak_inner = Rc::downgrade(&inner);
    inner.reader.connect_state_set(move |_, enabled| {
        if weak_inner
            .upgrade()
            .is_some_and(|inner| !inner.updating.get())
        {
            glib::spawn_future_local(async move {
                let args = (STATUS, READER, enabled.to_variant()).to_variant();
                if let Err(err) = a11y_call("Set", args).await {
                    tracing::error!(error = %err, "cannot switch the screen reader");
                }
            });
        }
        glib::Propagation::Proceed
    });
    let (weak_bar, weak_inner) = (Rc::downgrade(bar), Rc::downgrade(&inner));
    inner.magnifier.connect_state_set(move |_, enabled| {
        if let (Some(bar), Some(inner)) = (weak_bar.upgrade(), weak_inner.upgrade()) {
            if !inner.updating.get() {
                if let Some(Err(err)) = bar.client().map(|client| client.set_magnifier(enabled)) {
                    tracing::error!(error = %err, "cannot switch the magnifier");
                }
            }
        }
        glib::Propagation::Proceed
    });
    // Inversion and the filter are one compositor request: each sends the other's state.
    let send_filter = {
        let (weak_bar, weak_inner) = (Rc::downgrade(bar), Rc::downgrade(&inner));
        move || {
            let (Some(bar), Some(inner)) = (weak_bar.upgrade(), weak_inner.upgrade()) else {
                return;
            };
            if inner.updating.get() {
                return;
            }
            let Some(filter) = inner
                .filters
                .iter()
                .find(|(_, check)| check.is_active())
                .map(|(filter, _)| *filter)
            else {
                return;
            };
            if let Some(Err(err)) = bar
                .client()
                .map(|client| client.set_screen_filter(inner.inverted.is_active(), filter))
            {
                tracing::error!(error = %err, "cannot change the screen filter");
            }
        }
    };
    let send = send_filter.clone();
    inner.inverted.connect_active_notify(move |_| send());
    for (_, check) in &inner.filters {
        let send = send_filter.clone();
        check.connect_toggled(move |check| {
            if check.is_active() {
                send();
            }
        });
    }

    inner.show_reader(bar);
    let (weak_bar, weak_inner) = (Rc::downgrade(bar), Rc::downgrade(&inner));
    inner.popup.popover.connect_show(move |_| {
        if let Some(inner) = weak_inner.upgrade() {
            inner.sync_high_contrast();
        }
        if let (Some(bar), Some(inner)) = (weak_bar.upgrade(), weak_inner.upgrade()) {
            inner.show_reader(&bar);
        }
    });
    Some(Box::new(AccessibilityUi { inner }))
}
