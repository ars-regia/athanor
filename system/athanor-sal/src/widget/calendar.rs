//! [`Kind::Calendar`]: a month as GTK's `Calendar` shows it. A heading with the month and the
//! year, each between its previous and next buttons; a line of the weekdays' short names from
//! the locale's first; and six weeks of [`Kind::CalendarCell`]s, the days of the month among
//! the last ones of the month before and the first ones of the month after, which are dimmed.
//! Today is marked and the selected day filled with the accent. The days are one stop of the
//! focus, as GTK's calendar is one widget: the arrows move the focus between them, Ctrl+Left
//! and Ctrl+Right change the month, Ctrl+Up and Ctrl+Down the year, and Enter, Space or a click
//! select the focused day, from the month it belongs to. The wheel changes the month.

use super::{Event, Key, Kind, Ui, WidgetId};
use crate::icon::Image;

/// A day of the proleptic Gregorian calendar, as GLib's `GDateTime` counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    /// The year, from 1 to 9999.
    pub year: i32,
    /// The month, from 1 for January to 12.
    pub month: u8,
    /// The day of the month, from 1.
    pub day: u8,
}

/// The years a calendar shows, as GTK's.
const YEARS: std::ops::RangeInclusive<i32> = 1..=9999;
/// Six weeks: enough for every month, wherever it starts.
const DAYS: usize = 42;

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days in `month` of `year`.
fn length(year: i32, month: u8) -> u8 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The weekday of `date`, from 0 for Sunday to 6 (Sakamoto's method).
fn weekday(date: Date) -> u8 {
    const OFFSET: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if date.month < 3 {
        date.year - 1
    } else {
        date.year
    };
    let month = usize::from(date.month.clamp(1, 12)) - 1;
    let n = y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + OFFSET[month];
    (n + i32::from(date.day)).rem_euclid(7) as u8
}

/// `date` moved by `months`, its day kept within the new month, as GLib's
/// `g_date_time_add_months`; `None` outside [`YEARS`].
fn add_months(date: Date, months: i32) -> Option<Date> {
    let index = date.year * 12 + i32::from(date.month) - 1 + months;
    let (year, month) = (index.div_euclid(12), index.rem_euclid(12) as u8 + 1);
    YEARS.contains(&year).then(|| Date {
        year,
        month,
        day: date.day.min(length(year, month)),
    })
}

/// `date` within [`YEARS`], its month from 1 to 12 and its day within the month.
fn valid(date: Date) -> Date {
    let (year, month) = (
        date.year.clamp(*YEARS.start(), *YEARS.end()),
        date.month.clamp(1, 12),
    );
    Date {
        year,
        month,
        day: date.day.clamp(1, length(year, month)),
    }
}

/// The 42 days a calendar on `year` and `month` shows, from its first weekday `first` (0 for
/// Sunday): each day of the month with the month it belongs to, -1 the one before, 0 its own,
/// 1 the one after. A month that starts on the first weekday starts on the second line, as in
/// GTK's `calendar_compute_days`.
fn days(year: i32, month: u8, first: u8) -> [(u8, i8); DAYS] {
    let mut lead = usize::from(
        (weekday(Date {
            year,
            month,
            day: 1,
        }) + 7
            - first % 7)
            % 7,
    );
    if lead == 0 {
        lead = 7;
    }
    let before = if month == 1 {
        length(year - 1, 12)
    } else {
        length(year, month - 1)
    };
    let own = usize::from(length(year, month));
    std::array::from_fn(|i| {
        // Every number fits in a u8: at most 31 + 42.
        if i < lead {
            (before - (lead - i) as u8 + 1, -1)
        } else if i - lead < own {
            ((i - lead + 1) as u8, 0)
        } else {
            ((i - lead - own + 1) as u8, 1)
        }
    })
}

/// The words a calendar shows and says, in the user's language.
pub struct CalendarText {
    /// The months' names, January first, as GTK's `%OB`.
    pub months: [String; 12],
    /// The weekdays' short names, Sunday first, as GTK's `%a`.
    pub weekdays: [String; 7],
    /// The names of the previous month's, the next month's, the previous year's and the next
    /// year's buttons.
    pub arrows: [String; 4],
}

/// A calendar's state and the widgets it is made of. See [`Kind::Calendar`].
pub struct Calendar {
    selected: Date,
    today: Date,
    first: u8,
    months: [String; 12],
    /// The previous month's, the next month's, the previous year's and the next year's buttons.
    arrows: [WidgetId; 4],
    month: WidgetId,
    year: WidgetId,
    days: Vec<WidgetId>,
}

/// A label of a calendar: a weekday's name, or a day that takes the focus. See
/// [`Kind::CalendarCell`].
pub struct Cell {
    pub(crate) text: String,
    pub(crate) day: Option<Mark>,
}

/// How a day is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mark {
    /// Of the month before or the month after.
    pub(crate) other: bool,
    pub(crate) today: bool,
    pub(crate) selected: bool,
}

/// A cell's size: its text, with room for the selection's fill around it.
pub(super) fn measure_cell(w: f32, h: f32, pad: f32) -> (f32, f32) {
    (w + pad, h + pad / 2.0)
}

impl Ui {
    /// Adds a calendar on `today`, selected and marked, whose weeks start on `first_weekday`
    /// (0 for Sunday, 1 for Monday, and so on), with `text` in the user's language and
    /// `arrows`, the start and end arrows, on its buttons. See [`Kind::Calendar`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn calendar(
        &mut self,
        parent: WidgetId,
        text: &CalendarText,
        arrows: &[Image; 2],
        first_weekday: u8,
        today: Date,
    ) -> WidgetId {
        let today = valid(today);
        let first = first_weekday % 7;
        let id = self.add(
            parent,
            Kind::Calendar(Box::new(Calendar {
                selected: today,
                today,
                first,
                months: text.months.clone(),
                arrows: [parent; 4],
                month: parent,
                year: parent,
                days: Vec::with_capacity(DAYS),
            })),
        );
        let heading = self.add(id, Kind::CenterBox);
        // The month at the start, nothing in the middle, the year at the end.
        let groups = [0; 3].map(|_| self.inline(heading, [0.0, 0.0]));
        let mut ids = Vec::with_capacity(6);
        for (side, group) in [(0, groups[0]), (2, groups[2])] {
            ids.push(self.icon_button(group, &text.arrows[side], arrows[0].clone(), 16.0, None));
            ids.push(self.add(group, Kind::Label(String::new())));
            ids.push(self.icon_button(
                group,
                &text.arrows[side + 1],
                arrows[1].clone(),
                16.0,
                None,
            ));
        }
        let names = self.add(id, Kind::Row);
        for n in 0..7 {
            let name = text.weekdays[(n + usize::from(first)) % 7].clone();
            self.add(
                names,
                Kind::CalendarCell(Cell {
                    text: name,
                    day: None,
                }),
            );
        }
        let mut days = Vec::with_capacity(DAYS);
        for _ in 0..6 {
            let week = self.add(id, Kind::Row);
            for _ in 0..7 {
                let cell = Cell {
                    text: String::new(),
                    day: Some(Mark::default()),
                };
                days.push(self.add(week, Kind::CalendarCell(cell)));
            }
        }
        if let Kind::Calendar(calendar) = &mut self.widgets[id].kind {
            calendar.arrows = [ids[0], ids[2], ids[3], ids[5]];
            (calendar.month, calendar.year) = (ids[1], ids[4]);
            calendar.days = days;
        }
        self.refresh_calendar(id);
        id
    }

    /// Marks `today` on calendar `id` and selects it, on its month, as the bar's calendar does
    /// each time it opens; `false`, and nothing changes, for another kind. Call
    /// [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn select_today(&mut self, id: WidgetId, today: Date) -> bool {
        let Kind::Calendar(calendar) = &mut self.widgets[id].kind else {
            return false;
        };
        let today = valid(today);
        (calendar.today, calendar.selected) = (today, today);
        self.refresh_calendar(id);
        true
    }

    fn calendar_state(&self, id: WidgetId) -> Option<&Calendar> {
        match &self.widgets[id].kind {
            Kind::Calendar(calendar) => Some(calendar),
            _ => None,
        }
    }

    /// The calendar `id` is part of, its heading's buttons and its days, if any.
    pub(crate) fn calendar_of(&self, id: WidgetId) -> Option<WidgetId> {
        std::iter::successors(self.widgets[id].parent, |&p| self.widgets[p].parent)
            .take(3)
            .find(|&p| self.calendar_state(p).is_some())
    }

    /// The day of calendar `id` the focus stops on: the focused one, else the selected one.
    pub(crate) fn calendar_stop(&self, id: WidgetId) -> Option<WidgetId> {
        let calendar = self.calendar_state(id)?;
        let focused = self.focus.filter(|f| calendar.days.contains(f));
        focused.or_else(|| self.selected_day(id))
    }

    /// Whether the focus may stop on `id`: on a calendar's days, only on its stop.
    pub(crate) fn is_calendar_stop(&self, id: WidgetId) -> bool {
        !matches!(
            self.widgets[id].kind,
            Kind::CalendarCell(Cell { day: Some(_), .. })
        ) || self
            .calendar_of(id)
            .is_some_and(|c| self.calendar_stop(c) == Some(id))
    }

    /// The cell of calendar `id` that shows its selected day.
    fn selected_day(&self, id: WidgetId) -> Option<WidgetId> {
        let calendar = self.calendar_state(id)?;
        let s = calendar.selected;
        let index = days(s.year, s.month, calendar.first)
            .iter()
            .position(|&(day, month)| month == 0 && day == s.day)?;
        calendar.days.get(index).copied()
    }

    /// Shows calendar `id`'s month: the heading, each day's number and mark, and the buttons
    /// that would go past [`YEARS`] insensitive, as GTK's.
    fn refresh_calendar(&mut self, id: WidgetId) {
        let Some(calendar) = self.calendar_state(id) else {
            return;
        };
        let (selected, today) = (calendar.selected, calendar.today);
        let shown = days(selected.year, selected.month, calendar.first);
        let month_name = calendar
            .months
            .get(usize::from(selected.month) - 1)
            .cloned()
            .unwrap_or_default();
        let (month, year, arrows) = (calendar.month, calendar.year, calendar.arrows);
        let cells = calendar.days.clone();
        for (&cell, (day, other)) in cells.iter().zip(shown) {
            if let Kind::CalendarCell(c) = &mut self.widgets[cell].kind {
                c.text = day.to_string();
                let own = other == 0;
                c.day = Some(Mark {
                    other: !own,
                    today: own && Date { day, ..selected } == today,
                    selected: own && day == selected.day,
                });
            }
        }
        self.set_text(month, &month_name);
        self.set_text(year, &selected.year.to_string());
        for (arrow, by) in arrows.into_iter().zip([-1, 1, -12, 12]) {
            self.set_sensitive(arrow, add_months(selected, by).is_some());
        }
    }

    /// Moves calendar `id` by `months`, as its buttons do: `None` past [`YEARS`].
    fn move_calendar(&mut self, id: WidgetId, months: i32) -> Option<Event> {
        let Kind::Calendar(calendar) = &mut self.widgets[id].kind else {
            return None;
        };
        calendar.selected = add_months(calendar.selected, months)?;
        self.refresh_calendar(id);
        Some(Event::Changed(id))
    }

    /// Activates `widget` of calendar `id`: a button moves the month or the year; a day is
    /// selected, from the month it belongs to, and focused. [`Event::Changed`] when the
    /// selected day changed.
    pub(super) fn activate_in_calendar(&mut self, id: WidgetId, widget: WidgetId) -> Option<Event> {
        let calendar = self.calendar_state(id)?;
        if let Some(n) = calendar.arrows.iter().position(|&a| a == widget) {
            return self.move_calendar(id, [-1, 1, -12, 12][n]);
        }
        let index = calendar.days.iter().position(|&d| d == widget)?;
        let (selected, first) = (calendar.selected, calendar.first);
        let (day, month) = days(selected.year, selected.month, first)[index];
        let date = add_months(selected, i32::from(month))?;
        let date = Date { day, ..date };
        let Kind::Calendar(calendar) = &mut self.widgets[id].kind else {
            return None;
        };
        calendar.selected = date;
        self.refresh_calendar(id);
        let stop = self.selected_day(id);
        self.set_focus(stop);
        (date != selected).then_some(Event::Changed(id))
    }

    /// A key on the focused day of a calendar, as GTK's calendar takes it: the arrows move the
    /// focus, Left and Right across the ends of the weeks, Up and Down a week and not past the
    /// first or the last; the Ctrl arrows move the month or the year. `None` when the focus is
    /// on no day or the key is another.
    pub(super) fn calendar_key(&mut self, key: &Key) -> Option<Option<Event>> {
        let focused = self.focus?;
        let id = self.calendar_of(focused)?;
        let calendar = self.calendar_state(id)?;
        let at = calendar.days.iter().position(|&d| d == focused)?;
        // Left is the day before in reading order: the one after when mirrored.
        let back = if self.rtl { 1 } else { -1 };
        let to = match key {
            Key::Left => at as isize + back,
            Key::Right => at as isize - back,
            Key::Up if at >= 7 => at as isize - 7,
            Key::Down if at + 7 < DAYS => at as isize + 7,
            Key::Up | Key::Down => at as isize,
            Key::ControlLeft => return Some(self.move_calendar(id, -1)),
            Key::ControlRight => return Some(self.move_calendar(id, 1)),
            Key::ControlUp => return Some(self.move_calendar(id, -12)),
            Key::ControlDown => return Some(self.move_calendar(id, 12)),
            _ => return None,
        };
        let to = usize::try_from(to)
            .ok()
            .and_then(|i| calendar.days.get(i).copied());
        if let Some(to) = to {
            self.set_focus(Some(to));
        }
        Some(None)
    }

    /// The wheel over a calendar at the point changes its month, down to the next one, and
    /// focuses its day, as GTK's; `None` when no calendar is there.
    pub(super) fn calendar_wheel(&mut self, x: f32, y: f32, dy: i32) -> Option<Option<Event>> {
        let id = self.ids().find(|&id| {
            self.calendar_state(id).is_some()
                && self.widgets[id].rect.contains(x, y)
                && self.is_visible(id)
                && self.is_sensitive(id)
        })?;
        if self.focus.and_then(|f| self.calendar_of(f)) != Some(id) {
            let stop = self.calendar_stop(id);
            self.set_focus(stop);
        }
        Some(match dy.signum() {
            0 => None,
            sign => self.move_calendar(id, sign),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{add_months, days, weekday, Date};
    use crate::{Event, Key, Kind, Theme, Ui};

    fn date(year: i32, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    fn image() -> crate::Image {
        crate::icon::Image {
            pixmap: std::sync::Arc::new(vello_cpu::Pixmap::new(1, 1)),
        }
    }

    fn text() -> super::CalendarText {
        let months = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        let weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let arrows = ["Previous month", "Next month", "Previous year", "Next year"];
        super::CalendarText {
            months: months.map(String::from),
            weekdays: weekdays.map(String::from),
            arrows: arrows.map(String::from),
        }
    }

    fn calendar(rtl: bool, first: u8, today: Date) -> (Ui, super::super::WidgetId) {
        let mut ui = Ui::new("Calendar", Theme::default(), rtl);
        let id = ui.calendar(Ui::ROOT, &text(), &[image(), image()], first, today);
        ui.layout();
        (ui, id)
    }

    fn cells(ui: &Ui, id: super::super::WidgetId) -> Vec<String> {
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        c.days
            .iter()
            .map(|&d| match &ui.widget(d).kind {
                Kind::CalendarCell(cell) => cell.text.clone(),
                _ => panic!("a cell"),
            })
            .collect()
    }

    fn selected(ui: &Ui, id: super::super::WidgetId) -> Date {
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        c.selected
    }

    fn texts(ui: &Ui, id: super::super::WidgetId) -> (String, String) {
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        let label = |w| match &ui.widget(w).kind {
            Kind::Label(t) => t.clone(),
            _ => panic!("a label"),
        };
        (label(c.month), label(c.year))
    }

    #[test]
    fn the_date_arithmetic_is_glibs() {
        assert_eq!(weekday(date(2026, 9, 18)), 5, "a Friday");
        assert_eq!(weekday(date(2000, 1, 1)), 6, "a Saturday");
        assert_eq!(weekday(date(1, 1, 1)), 1, "a Monday");
        assert_eq!(add_months(date(2026, 1, 31), 1), Some(date(2026, 2, 28)));
        assert_eq!(add_months(date(2024, 3, 31), -1), Some(date(2024, 2, 29)));
        assert_eq!(add_months(date(2026, 12, 5), 1), Some(date(2027, 1, 5)));
        assert_eq!(add_months(date(2026, 1, 5), -13), Some(date(2024, 12, 5)));
        assert_eq!(add_months(date(1, 1, 1), -1), None);
        assert_eq!(add_months(date(9999, 12, 1), 1), None);
    }

    #[test]
    fn september_2026_shows_as_gtks_golden() {
        let (ui, id) = calendar(false, 0, date(2026, 9, 18));
        let shown = cells(&ui, id);
        assert_eq!(shown[..3], ["30", "31", "1"]);
        assert_eq!(shown[35..], ["4", "5", "6", "7", "8", "9", "10"]);
        assert_eq!(texts(&ui, id), ("September".into(), "2026".into()));
        // Friday 18 is the sixth day of the third week; the days around it are not marked.
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        let mark = |i: usize| match &ui.widget(c.days[i]).kind {
            Kind::CalendarCell(cell) => cell.day.unwrap_or_default(),
            _ => panic!("a cell"),
        };
        assert!(mark(19).today && mark(19).selected && !mark(19).other);
        assert!(mark(0).other && mark(36).other && !mark(18).today);
        assert_eq!(ui.calendar_stop(id), Some(c.days[19]));
    }

    #[test]
    fn a_month_that_starts_on_the_first_weekday_starts_on_the_second_line() {
        // 1 February 2026 is a Sunday.
        assert_eq!(days(2026, 2, 0)[7], (1, 0));
        assert_eq!(days(2026, 2, 0)[0], (25, -1));
        // On weeks from Monday it is the last day of the first line.
        assert_eq!(days(2026, 2, 1)[6], (1, 0));
        assert_eq!(days(2026, 2, 1)[0], (26, -1));
    }

    #[test]
    fn the_weekday_names_start_on_the_locales_first_weekday() {
        let (ui, id) = calendar(false, 1, date(2026, 9, 18));
        let names = ui.widget(id).children[1];
        let first = ui.widget(names).children[0];
        let Kind::CalendarCell(cell) = &ui.widget(first).kind else {
            panic!("a cell");
        };
        assert_eq!(cell.text, "Mon");
        assert!(!ui.widget(first).is_focusable(), "a name takes no focus");
    }

    #[test]
    fn the_buttons_move_the_month_and_the_year_and_keep_the_day_in_the_month() {
        let (mut ui, id) = calendar(false, 0, date(2026, 1, 31));
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        let [previous, next, _, next_year] = c.arrows;
        assert_eq!(ui.activate(next), Some(Event::Changed(id)));
        assert_eq!(selected(&ui, id), date(2026, 2, 28));
        assert_eq!(texts(&ui, id).0, "February");
        ui.activate(next_year);
        assert_eq!(selected(&ui, id), date(2027, 2, 28));
        ui.activate(previous);
        assert_eq!(texts(&ui, id), ("January".into(), "2027".into()));
    }

    #[test]
    fn the_buttons_stop_at_the_first_and_the_last_year() {
        let (ui, id) = calendar(false, 0, date(1, 1, 1));
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        assert!(!ui.is_sensitive(c.arrows[0]) && !ui.is_sensitive(c.arrows[2]));
        assert!(ui.is_sensitive(c.arrows[1]) && ui.is_sensitive(c.arrows[3]));
    }

    #[test]
    fn a_day_of_another_month_selects_it_on_its_month() {
        let (mut ui, id) = calendar(false, 0, date(2026, 9, 18));
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        let (last, oct_10) = (c.days[0], c.days[41]);
        assert_eq!(ui.activate(oct_10), Some(Event::Changed(id)));
        assert_eq!(selected(&ui, id), date(2026, 10, 10));
        let stop = ui.calendar_stop(id);
        assert_eq!(ui.focus(), stop, "the selected day takes the focus");
        ui.activate(last);
        assert_eq!(
            selected(&ui, id),
            date(2026, 9, 27),
            "the 27th, first of October's grid"
        );
        let today = ui.select_today(id, date(2026, 9, 18));
        assert!(today);
        assert_eq!(selected(&ui, id), date(2026, 9, 18));
    }

    #[test]
    fn the_arrows_move_the_focus_as_gtks_and_the_ctrl_arrows_the_month() {
        for rtl in [false, true] {
            let (mut ui, id) = calendar(rtl, 0, date(2026, 9, 18));
            let Kind::Calendar(c) = &ui.widget(id).kind else {
                panic!("a calendar");
            };
            let days = c.days.clone();
            ui.set_focus(Some(days[7]));
            let left = if rtl { Key::Right } else { Key::Left };
            let right = if rtl { Key::Left } else { Key::Right };
            assert_eq!(ui.key(left.clone()), None);
            assert_eq!(ui.focus(), Some(days[6]), "across the end of the week");
            ui.key(right.clone());
            ui.key(right);
            assert_eq!(ui.focus(), Some(days[8]));
            ui.key(Key::Up);
            ui.key(Key::Up);
            assert_eq!(ui.focus(), Some(days[1]), "not past the first week");
            ui.set_focus(Some(days[0]));
            ui.key(left);
            assert_eq!(ui.focus(), Some(days[0]), "not before the first day");
            ui.set_focus(Some(days[38]));
            ui.key(Key::Down);
            assert_eq!(ui.focus(), Some(days[38]), "not past the last week");
            assert_eq!(
                selected(&ui, id),
                date(2026, 9, 18),
                "the focus selects nothing"
            );
            assert_eq!(ui.key(Key::ControlRight), Some(Event::Changed(id)));
            assert_eq!(ui.key(Key::ControlDown), Some(Event::Changed(id)));
            assert_eq!(selected(&ui, id), date(2027, 10, 18));
            assert_eq!(ui.focus(), Some(days[38]), "the focus stays on its cell");
            assert_eq!(ui.key(Key::Enter), Some(Event::Changed(id)));
        }
    }

    #[test]
    fn the_days_are_one_stop_of_the_focus_and_the_ctrl_arrows_elsewhere_are_arrows() {
        let (mut ui, id) = calendar(false, 0, date(2026, 9, 18));
        let field = ui.field(Ui::ROOT, "Name", "ab");
        ui.layout();
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        let mut stops = c.arrows.to_vec();
        stops.extend([c.days[19], field]);
        assert_eq!(
            crate::focus::chain(&ui),
            stops,
            "the buttons, then the selected day"
        );
        ui.set_focus(Some(field));
        // A new field's caret is at its start: to its end, then one back.
        ui.key(Key::Right);
        ui.key(Key::Right);
        ui.key(Key::ControlLeft);
        assert_eq!(ui.key(Key::Text("x".into())), Some(Event::Edited(field)));
        assert_eq!(
            ui.field_text(field).as_deref(),
            Some("axb"),
            "Ctrl+Left moved the caret"
        );
    }

    #[test]
    fn the_wheel_changes_the_month_and_focuses_the_day() {
        let (mut ui, id) = calendar(false, 0, date(2026, 9, 18));
        let r = ui.widget(id).rect;
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h - 2.0);
        assert_eq!(ui.pointer_wheel(x, y, 0, 120), Some(Event::Changed(id)));
        assert_eq!(selected(&ui, id), date(2026, 10, 18));
        assert_eq!(ui.calendar_of(ui.focus().unwrap_or(Ui::ROOT)), Some(id));
        ui.pointer_wheel(x, y, 0, -240);
        assert_eq!(selected(&ui, id), date(2026, 9, 18));
    }

    #[test]
    fn the_days_read_as_labels_and_the_buttons_by_their_names() {
        let (ui, id) = calendar(false, 0, date(2026, 9, 18));
        let Kind::Calendar(c) = &ui.widget(id).kind else {
            panic!("a calendar");
        };
        // GTK's calendar is an unnamed panel over AT-SPI; a GenericContainer would leave no node.
        assert_eq!(crate::a11y::node(&ui, id, 1.0).role(), accesskit::Role::Group);
        let day = crate::a11y::node(&ui, c.days[19], 1.0);
        assert_eq!(day.role(), accesskit::Role::Label);
        assert_eq!(day.value(), Some("18"));
        assert!(day.is_selected() == Some(true));
        let next = crate::a11y::node(&ui, c.arrows[1], 1.0);
        assert_eq!(next.label(), Some("Next month"));
        let update = crate::a11y::tree_update(&ui, 1.0);
        assert!(crate::a11y::missing(&ui, &update).is_empty());
    }
}
