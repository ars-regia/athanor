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
struct Clock {
    texts: Texts,
    /// The calendar's first weekday, 0 for Sunday, the time locale's as GTK's calendar takes it.
    first_weekday: u8,
    /// The short text and the long name now, formatted at a tick when the minute, the zone or
    /// the 12/24-hour choice changed: [`Module::stale`] reads only what is held.
    current: Option<(String, String)>,
    /// The short text and the long name, as last drawn.
    drawn: Option<(String, String)>,
}

pub fn new(_start: &Start) -> Option<Box<dyn Module>> {
    let texts = Texts::new();
    let current = texts.changed_texts();
    Some(Box::new(Clock {
        texts,
        first_weekday: athanor_i18n::first_weekday(),
        current,
        drawn: None,
    }))
}

impl Module for Clock {
    fn button(&mut self, _cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let (short, long) = self.current.clone().unwrap_or_default();
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
        self.drawn = self.current.clone();
        Some(button)
    }

    fn tick(&mut self) {
        if let Some(texts) = self.texts.changed_texts() {
            self.current = Some(texts);
        }
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| self.drawn.as_ref() != Some(current))
    }

    fn update(&mut self, _cx: &mut Cx, ui: &mut Ui, button: WidgetId) -> bool {
        if let Some((short, long)) = &self.current {
            ui.set_shown_text(button, short);
            ui.set_text(button, long);
            ui.set_tooltip(button, Some(long));
            ui.layout();
            self.drawn = self.current.clone();
        }
        true
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let now = self.texts.now().ok();
        let (Some(text), Some(today)) = (calendar_text(), now.as_ref().and_then(today)) else {
            tracing::error!("cannot read the date or the names of the months; no calendar");
            return None;
        };
        let [start, end] = arrow_icons(cx.rtl);
        let arrows = [
            cx.icons.image(start, ARROW, cx.scale)?,
            cx.icons.image(end, ARROW, cx.scale)?,
        ];
        let (mut ui, frame) = look::popover(&tr("Calendar"), cx.theme.clone(), cx.rtl, cx.top);
        let calendar = ui.calendar(frame, &text, &arrows, self.first_weekday, today);
        ui.set_focus(Some(calendar));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, _ui: &mut Ui, _event: Event) -> Reply {
        Reply::Stay
    }
}

/// The icons of the calendar's start and end arrows, each pointing to its edge: in a
/// right-to-left language the start arrow points right, as GTK's `pan-start-symbolic-rtl`.
fn arrow_icons(rtl: bool) -> [&'static str; 2] {
    if rtl {
        ["pan-end-symbolic", "pan-start-symbolic"]
    } else {
        ["pan-start-symbolic", "pan-end-symbolic"]
    }
}

/// The date of `now`, the time the clock's button shows: GLib's local zone is read once for
/// the life of the process, so the calendar takes the clock's zone, which follows
/// /etc/localtime, and its today is the button's.
fn today(now: &glib::DateTime) -> Option<Date> {
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
    fn the_arrows_point_to_their_edges_in_both_directions() {
        assert_eq!(arrow_icons(false), ["pan-start-symbolic", "pan-end-symbolic"]);
        assert_eq!(arrow_icons(true), ["pan-end-symbolic", "pan-start-symbolic"]);
    }

    #[test]
    fn today_is_the_date_in_the_clocks_zone() {
        let zone = glib::TimeZone::from_identifier(Some("+09:00")).expect("an offset is a zone");
        let now = glib::DateTime::from_utc(2026, 1, 1, 23, 30, 0.0)
            .and_then(|utc| utc.to_timezone(&zone))
            .expect("a valid time");
        assert_eq!(
            today(&now),
            Some(Date {
                year: 2026,
                month: 1,
                day: 2
            })
        );
    }

    #[test]
    fn the_calendar_names_its_months_from_january_and_its_days_from_sunday() {
        let text = calendar_text().expect("GLib formats a date");
        assert_eq!(text.months[0], "January");
        assert_eq!(text.months[11], "December");
        assert_eq!(text.weekdays[0], "Sun");
        assert_eq!(text.weekdays[6], "Sat");
    }
}
