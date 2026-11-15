//! The popovers of the toolkit bar's modules (doc_bar.md, BR3), as `ui/` draws the GTK ones:
//! the clock's calendar, the input sources, the tiling switch and the accessibility menu. Each
//! is built from its source's state when it opens, and its widgets are kept so that a change
//! of the source sets them in place.

use athanor_apps::sal::Icons;
use gio::prelude::SettingsExt;
use athanor_bar::i18n::{tr, tr_with};
use athanor_bar::keyboard;
use athanor_bar::screen_reader;
use athanor_bar::tiling::{self, Tiling, WorkspaceView};
use athanor_sal::{CalendarText, Date, Kind, Theme, Ui, WidgetId};
use athanor_style::appearance::Appearance;
use athanor_sulphur_client::{Accessibility, Client, KeyboardAid, ScreenFilter, WorkspaceId};

use crate::look;

/// The size of the calendar's arrows, as GTK's.
const ARROW: u16 = 16;
/// The first weekday of the calendar, 0 for Sunday.
// ponytail: Sunday, what GTK shows under the C locale, until the locale's own day is read
// through nl_langinfo(_NL_TIME_FIRST_WEEKDAY); the process locale is not set, so that call
// would answer the C locale's day too.
const FIRST_WEEKDAY: u8 = 0;

/// The filters a user can choose. `Unknown` is read, never offered.
pub const FILTERS: [ScreenFilter; 5] = [
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

/// Today, in the local time zone.
pub fn today() -> Option<Date> {
    let now = glib::DateTime::now_local().ok()?;
    Some(Date {
        year: now.year(),
        month: u8::try_from(now.month()).ok()?,
        day: u8::try_from(now.day_of_month()).ok()?,
    })
}

/// The calendar's names in the locale, as GTK's calendar takes them: the months with `%OB`,
/// the weekdays with `%a`, from Sunday; `None` when GLib cannot format a date.
fn calendar_text() -> Option<CalendarText> {
    // 7 January 2001 was a Sunday.
    let format = |month: i32, day: i32, pattern: &str| {
        let date = glib::DateTime::from_local(2001, month, day, 0, 0, 0.0).ok()?;
        date.format(pattern).ok().map(String::from)
    };
    let months: Vec<String> = (1..=12)
        .map(|month| format(month, 1, "%OB"))
        .collect::<Option<_>>()?;
    let weekdays: Vec<String> = (7..14)
        .map(|day| format(1, day, "%a"))
        .collect::<Option<_>>()?;
    Some(CalendarText {
        months: months.try_into().ok()?,
        weekdays: weekdays.try_into().ok()?,
        arrows: [
            tr("Previous month"),
            tr("Next month"),
            tr("Previous year"),
            tr("Next year"),
        ],
    })
}

/// The clock's popover: a calendar on today, which takes the focus, as the GTK clock's.
/// `None`, logged, without the names or the arrows to draw it with.
pub fn calendar(theme: Theme, rtl: bool, top: bool, icons: &mut Icons, scale: u16) -> Option<Ui> {
    let (Some(text), Some(today)) = (calendar_text(), today()) else {
        tracing::error!("cannot read the date or the names of the months; no calendar");
        return None;
    };
    let arrows = [
        icons.image("pan-start-symbolic", ARROW, scale)?,
        icons.image("pan-end-symbolic", ARROW, scale)?,
    ];
    let (mut ui, frame) = look::popover(&tr("Calendar"), theme, rtl, top);
    let calendar = ui.calendar(frame, &text, &arrows, FIRST_WEEKDAY, today);
    ui.set_focus(Some(calendar));
    Some(ui)
}

/// The input source button's name: the active layout's.
pub fn input_name(client: &Client) -> Option<String> {
    let layouts = client.keyboard_layouts();
    if !keyboard::shown(&layouts) {
        return None;
    }
    let active = keyboard::active(&layouts, client.keyboard_group()).unwrap_or_default();
    Some(tr_with("Input source: {name}", "name", active))
}

/// The input sources' popover: a radio per layout, the active one set, and the group each
/// selects.
pub fn input(theme: Theme, rtl: bool, top: bool, client: &Client) -> (Ui, Vec<(WidgetId, u32)>) {
    let layouts = client.keyboard_layouts();
    let group = client.keyboard_group();
    let name = input_name(client).unwrap_or_else(|| tr("Input source"));
    let (mut ui, frame) = look::popover(&name, theme, rtl, top);
    let labels = keyboard::labels(&layouts, |number| {
        tr_with("Layout {number}", "number", &number.to_string())
    });
    let radios = (0u32..)
        .zip(labels)
        .map(|(index, label)| (ui.radio(frame, &label, index == group), index))
        .collect();
    (ui, radios)
}

/// The workspace whose tiling the switch of `connector`'s surface changes, and its mode:
/// the active one of that output, while the compositor reports its mode.
pub fn tiling_target(client: &Client, connector: Option<&str>) -> Option<(WorkspaceId, Tiling)> {
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

/// The tiling popover: its switch, on while the workspace tiles.
pub fn tiling(theme: Theme, rtl: bool, top: bool, mode: Tiling) -> (Ui, WidgetId) {
    let (mut ui, frame) = look::popover(&tr("Tiling"), theme, rtl, top);
    let switch = ui.switch(frame, &tr("Tile windows"), mode == Tiling::Tiled);
    (ui, switch)
}

/// The accessibility menu's widgets.
pub struct AccessUi {
    pub high_contrast: WidgetId,
    /// Without the screen reader's key, none.
    pub reader: Option<WidgetId>,
    /// Without the compositor's state, none.
    pub magnifier: Option<WidgetId>,
    pub inverted: Option<WidgetId>,
    pub filters: Vec<(WidgetId, ScreenFilter)>,
    /// The offer's button, while the keyboard aids offer one.
    pub accept: Option<WidgetId>,
}

/// What the accessibility menu shows: the screen reader's key, the compositor's state and
/// the keyboard aid offered.
pub struct AccessState<'a> {
    pub reader: Option<&'a gio::Settings>,
    pub compositor: Option<Accessibility>,
    pub offer: Option<KeyboardAid>,
}

/// The accessibility menu: high contrast, the screen reader with its key, the magnifier,
/// inverted colours and the colour filter with the compositor's state, and the keyboard aid
/// offered with its button.
pub fn accessibility(theme: Theme, rtl: bool, top: bool, state: &AccessState) -> (Ui, AccessUi) {
    let (mut ui, frame) = look::popover(&tr("Accessibility"), theme, rtl, top);
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
        ui.add(frame, Kind::Label(tr("Colour filter")));
        let group = ui.add(frame, Kind::Column);
        for filter in FILTERS {
            let radio = ui.radio(group, &filter_label(filter), filter == compositor.filter);
            filters.push((radio, filter));
        }
    }
    let accept = state.offer.map(|aid| {
        let row = ui.add(frame, Kind::Row);
        ui.add(row, Kind::Label(crate::aids::offer_text(aid)));
        ui.add(row, Kind::Button(tr("Turn on")))
    });
    let widgets = AccessUi {
        high_contrast,
        reader,
        magnifier,
        inverted,
        filters,
        accept,
    };
    (ui, widgets)
}

impl AccessUi {
    /// Sets the compositor's switches and the filter to `state` without raising events.
    pub fn show(&self, ui: &mut Ui, state: Accessibility) {
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
    pub fn screen_filter(&self, ui: &Ui) -> Option<(bool, ScreenFilter)> {
        let inverted = self.inverted.is_some_and(|widget| ui.is_active(widget));
        self.filters
            .iter()
            .find(|(radio, _)| ui.is_active(*radio))
            .map(|(_, filter)| (inverted, *filter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_names_its_months_from_january_and_its_days_from_sunday() {
        let text = calendar_text().expect("GLib formats a date");
        assert_eq!(text.months[0], "January");
        assert_eq!(text.months[11], "December");
        assert_eq!(text.weekdays[0], "Sun");
        assert_eq!(text.weekdays[6], "Sat");
    }

    #[test]
    fn the_accessibility_menu_shows_the_compositors_rows_only_with_its_state() {
        let empty = AccessState {
            reader: None,
            compositor: None,
            offer: None,
        };
        let (_, widgets) = accessibility(Theme::default(), false, true, &empty);
        assert!(widgets.magnifier.is_none() && widgets.filters.is_empty());
        assert!(widgets.reader.is_none() && widgets.accept.is_none());
        let state = Accessibility {
            inverted: true,
            filter: ScreenFilter::Tritanopia,
            ..Accessibility::default()
        };
        let full = AccessState {
            reader: None,
            compositor: Some(state),
            offer: Some(KeyboardAid::Sticky),
        };
        let (ui, widgets) = accessibility(Theme::default(), false, true, &full);
        assert_eq!(widgets.filters.len(), FILTERS.len());
        assert!(widgets.accept.is_some());
        assert_eq!(
            widgets.screen_filter(&ui),
            Some((true, ScreenFilter::Tritanopia))
        );
    }
}
