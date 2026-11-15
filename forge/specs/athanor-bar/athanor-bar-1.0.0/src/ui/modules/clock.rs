//! The bar's clock and its calendar (doc_bar.md, BR3), as the GTK bar's `ui/clock.rs` was: the
//! date and time in semibold figures of one width, the long form as the button's name and
//! tooltip, and a calendar on today, which takes the focus, in its popover.

use athanor_bar::clock::Clock as Texts;
use athanor_bar::i18n::tr;
use athanor_sal::{CalendarText, Date, Event, TextStyle, Ui, WidgetId};

use super::{Cx, Module, Reply, Start};
use crate::ui::look;

/// The size of the calendar's arrows, as GTK's.
const ARROW: u16 = 16;
/// The first weekday of the calendar, 0 for Sunday.
// ponytail: Sunday, what GTK shows under the C locale, until the locale's own day is read
// through nl_langinfo(_NL_TIME_FIRST_WEEKDAY); the process locale is not set, so that call
// would answer the C locale's day too.
const FIRST_WEEKDAY: u8 = 0;

struct Clock {
    texts: Texts,
    /// The short text and the long name, as last drawn.
    drawn: Option<(String, String)>,
}

pub fn new(_start: &Start) -> Option<Box<dyn Module>> {
    Some(Box::new(Clock {
        texts: Texts::new(),
        drawn: None,
    }))
}

impl Module for Clock {
    fn button(&mut self, _cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let texts = self.texts.texts();
        let (short, long) = texts.clone().unwrap_or_default();
        let button = ui.labelled_button(group, &long, None, 0.0, &short);
        ui.set_tooltip(button, Some(&long));
        // `.bar-clock`: semibold figures of one width.
        ui.set_text_style(
            button,
            TextStyle {
                weight: 600,
                features: vec![*b"tnum"],
                ..TextStyle::default()
            },
        );
        self.drawn = texts;
        Some(button)
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.texts
            .texts()
            .is_some_and(|texts| self.drawn.as_ref() != Some(&texts))
    }

    fn update(&mut self, _cx: &mut Cx, ui: &mut Ui, button: WidgetId) -> bool {
        if let Some((short, long)) = self.texts.texts() {
            ui.set_shown_text(button, &short);
            ui.set_text(button, &long);
            ui.set_tooltip(button, Some(&long));
            ui.layout();
            self.drawn = Some((short, long));
        }
        true
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let (Some(text), Some(today)) = (calendar_text(), today()) else {
            tracing::error!("cannot read the date or the names of the months; no calendar");
            return None;
        };
        let arrows = [
            cx.icons.image("pan-start-symbolic", ARROW, cx.scale)?,
            cx.icons.image("pan-end-symbolic", ARROW, cx.scale)?,
        ];
        let (mut ui, frame) = look::popover(&tr("Calendar"), cx.theme.clone(), cx.rtl, cx.top);
        let calendar = ui.calendar(frame, &text, &arrows, FIRST_WEEKDAY, today);
        ui.set_focus(Some(calendar));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, _ui: &mut Ui, _event: Event) -> Reply {
        Reply::Stay
    }
}

/// Today, in the local time zone.
fn today() -> Option<Date> {
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
}
