//! Every widget of athanor-sal on a page of its own, for an application developer to see
//! what the toolkit offers and for the rig to compare with its GTK 4 twin
//! (`forge/test/shell/gallery_gtk.py`), which shows the same pages from the same strings.
//!
//! `athanor-sal-gallery [--lang en|it] [--rtl] [--page <name>]`: `--lang` picks a column of
//! `strings.tsv`, `--rtl` mirrors the window and `--page` names the page shown at the start. The
//! dialogs page starts with its dialog open, the focus on the dialog's button, and the menus page
//! with its menu open, the focus on the menu's first item.

use athanor_sal::driver::Role;
use athanor_sal::{
    driver, Application, CalendarText, Date, Event, IconTheme, Kind, Rgba, Shell, Side, SurfaceId,
    Theme, Ui, WidgetId, WindowButton, Wrap,
};
use std::num::NonZeroU32;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

/// One line per string: the key, then its text in each language, tab separated.
const STRINGS: &str = include_str!("../strings.tsv");
/// The languages of `strings.tsv`, in the order of its columns.
const LANGUAGES: [&str; 2] = ["en", "it"];
/// The pages, in the order of the page toggles; the first is shown unless `--page` names another.
const PAGES: [&str; 9] = [
    "buttons", "choices", "values", "lists", "text", "states", "calendar", "dialogs", "menus",
];
/// The page that opens a dialog, with its dialog open at the start.
const DIALOGS: usize = 7;
/// The page that opens a menu, with its menu open at the start.
const MENUS: usize = 8;
/// How far the menu's arrow stands out of its frame, as GTK's popover tail (12 px).
const ARROW: f32 = 12.0;

/// The window's widgets the gallery acts on.
struct Gallery {
    /// Each page's toggle and group.
    pages: Vec<(WidgetId, WidgetId)>,
    /// The states page's switch and the button it makes sensitive.
    states: (WidgetId, WidgetId),
    /// The lists page's button and the list whose rows it replaces.
    lists: (WidgetId, WidgetId),
    /// The number of the lists page's first row less one: 0 or [`ROWS`].
    first: u32,
    /// The column of `strings.tsv` the window shows.
    lang: usize,
    /// The dialogs page's button, which opens the dialog.
    open: WidgetId,
    /// The menus page's button, which opens the menu.
    menu: WidgetId,
}

/// What the menu's check and radios show: the menu takes them from here each time it opens,
/// as GTK's menu takes them from its actions' states.
#[derive(Clone, Copy)]
struct Choices {
    check: bool,
    /// The radio selected, 0 or 1.
    radio: usize,
}

/// The rows of the lists page, a heading among them every [`ROWS_PER_HEADING`].
const ROWS: u32 = 50;
const ROWS_PER_HEADING: u32 = 25;

/// The command line.
struct Options {
    lang: usize,
    rtl: bool,
    page: usize,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            lang: 0,
            rtl: false,
            page: 0,
        };
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
            match arg.as_str() {
                "--rtl" => options.rtl = true,
                "--lang" => {
                    let lang = value("--lang")?;
                    options.lang = LANGUAGES
                        .iter()
                        .position(|&l| l == lang)
                        .ok_or(format!("--lang is one of {LANGUAGES:?}, not {lang:?}"))?;
                }
                "--page" => {
                    let page = value("--page")?;
                    options.page = PAGES
                        .iter()
                        .position(|&p| p == page)
                        .ok_or(format!("--page is one of {PAGES:?}, not {page:?}"))?;
                }
                _ => return Err(format!("unknown argument {arg:?}")),
            }
        }
        Ok(options)
    }
}

/// The text of a key in the chosen language.
fn tr(lang: usize, key: &str) -> Result<&'static str, String> {
    STRINGS
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(k, _)| *k == key)
        .and_then(|(_, texts)| texts.split('\t').nth(lang))
        .ok_or(format!("strings.tsv has no {:?} for {key:?}", LANGUAGES[lang]))
}

/// Fills `list` with the rows numbered from `first + 1`, which hold their name and a second line
/// in a column, and a heading that opens each [`ROWS_PER_HEADING`] past the first ones: among
/// the rows, not first, so that Tab enters the list at its first row, as in GTK's twin.
fn fill_rows(ui: &mut Ui, list: WidgetId, lang: usize, first: u32) -> Result<(), String> {
    let (row, rows, detail) = (tr(lang, "row")?, tr(lang, "rows")?, tr(lang, "row.detail")?);
    for n in first + 1..=first + ROWS {
        if n > first + 1 && (n - first - 1).is_multiple_of(ROWS_PER_HEADING) {
            let last = (n + ROWS_PER_HEADING - 1).min(first + ROWS);
            ui.list_heading(list, &format!("{rows} {n}–{last}"));
        }
        let name = format!("{row} {n}");
        let item = ui.list_row(list, &name);
        let column = ui.add(item, Kind::Column);
        ui.add(column, Kind::Label(name));
        ui.add(column, Kind::Label(detail.into()));
    }
    Ok(())
}

/// The widgets of page `name` under `group`: for the lists page its button and its list, for the
/// states page its switch and its button, and for the dialogs page its button.
fn page(ui: &mut Ui, group: WidgetId, name: &str, lang: usize) -> Result<Vec<WidgetId>, String> {
    let t = |key| tr(lang, key);
    match name {
        "buttons" => {
            ui.add(group, Kind::Label(t("label")?.into()));
            ui.add(group, Kind::Button(t("button")?.into()));
            let row = ui.add(group, Kind::Row);
            for (i, key) in ["one", "two", "three"].into_iter().enumerate() {
                ui.toggle(row, t(key)?, i == 0);
            }
            ui.field(group, t("field")?, "");
        }
        "choices" => {
            ui.check(group, t("check1")?, true);
            ui.check(group, t("check2")?, false);
            ui.switch(group, t("switch")?, true);
        }
        "values" => {
            ui.slider(group, t("slider")?, 0.0, 100.0, 1.0, 50.0)
                .ok_or("the slider's range is broken")?;
        }
        "lists" => {
            // The button comes first: Tab steps through the GTK twin's rows, not past them.
            let replace = ui.add(group, Kind::Button(t("replace")?.into()));
            let scroll = ui.scroll(group, 200.0);
            let list = ui.list(scroll, t("list")?);
            fill_rows(ui, list, lang, 0)?;
            return Ok(vec![replace, list]);
        }
        "text" => {
            // GTK's label with wrap, max-width-chars 40, lines 3 and an end ellipsis.
            let label = ui.add(group, Kind::Label(t("wrap")?.into()));
            let mut wrap = Wrap::default();
            wrap.max_width_chars = NonZeroU32::new(40);
            wrap.lines = NonZeroU32::new(3);
            ui.set_wrap(label, Some(wrap));
            let text = t("notes.text")?.replace("\\n", "\n");
            ui.text_area(group, t("notes")?, &text, 4);
        }
        "states" => {
            // The button takes input once the switch is on.
            let switch = ui.switch(group, t("enable")?, false);
            let button = ui.add(group, Kind::Button(t("waiting")?.into()));
            ui.set_sensitive(button, false);
            return Ok(vec![switch, button]);
        }
        "calendar" => {
            let text = CalendarText {
                months: words(t("months")?)?,
                weekdays: words(t("weekdays")?)?,
                arrows: [
                    t("calendar.previous-month")?.into(),
                    t("calendar.next-month")?.into(),
                    t("calendar.previous-year")?.into(),
                    t("calendar.next-year")?.into(),
                ],
            };
            let first = t("weekday.first")?.parse().map_err(|_| "weekday.first is no number")?;
            let icons = IconTheme::system("Adwaita");
            let arrow = |name| icons.image(name, 16, 2).map_err(|error| format!("{name}: {error}"));
            let arrows = [arrow("pan-start-symbolic")?, arrow("pan-end-symbolic")?];
            ui.calendar(group, &text, &arrows, first, today());
        }
        "dialogs" => return Ok(vec![ui.add(group, Kind::Button(t("dialog.open")?.into()))]),
        "menus" => {
            let button = ui.add(group, Kind::Button(t("menu.open")?.into()));
            ui.set_has_popup(button, true);
            return Ok(vec![button]);
        }
        _ => return Err(format!("no page {name:?}")),
    }
    Ok(Vec::new())
}

/// The space-separated words of `text`, one for each entry of the array.
fn words<const N: usize>(text: &str) -> Result<[String; N], String> {
    let words: Vec<String> = text.split(' ').map(String::from).collect();
    words.try_into().map_err(|words: Vec<String>| format!("{N} words, not {}", words.len()))
}

/// Today in UTC, from the system clock (the rig's frozen date under libfaketime).
// ponytail: UTC, not the local zone; a time-zone database when the gallery shows one.
fn today() -> Date {
    civil(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

/// The day of `secs` past 1970-01-01 in UTC: Howard Hinnant's `civil_from_days`.
fn civil(secs: u64) -> Date {
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Date { year, month, day }
}


/// The gallery's margins and spacings, as the GTK twin's and the GTK chooser's: 24 around, 18
/// between the groups, 8 inside one.
fn theme() -> Theme {
    let mut theme = Theme::default();
    theme.margin = 24.0;
    theme.section_gap = 18.0;
    theme.gap = 8.0;
    theme
}

/// The dialog of the dialogs page: a sentence that wraps and a button that closes it, which holds
/// the focus.
fn dialog(options: &Options) -> Result<Ui, String> {
    let lang = options.lang;
    let mut ui = Ui::new(tr(lang, "dialog.title")?, theme(), options.rtl);
    ui.min_width = 360.0;
    let text = ui.add(Ui::ROOT, Kind::Label(tr(lang, "dialog.text")?.into()));
    ui.set_wrap(text, Some(Wrap::default()));
    let close = ui.add(Ui::ROOT, Kind::Button(tr(lang, "dialog.close")?.into()));
    ui.set_focus(Some(close));
    ui.layout();
    Ok(ui)
}

/// The menu of the menus page, laid out, and its check and radios: an item, a check, two radios,
/// an item that takes no input and a submenu of one item, in three sections, as GTK draws a menu
/// model's sections, in a frame whose outline and arrow are GTK's popover's, as the bar's popovers.
fn menu(options: &Options, choices: Choices) -> Result<(Ui, [WidgetId; 3]), String> {
    let t = |key| tr(options.lang, key);
    let mut theme = Theme::default();
    // The popup's surface is transparent around the frame and its arrow, as the bar's popovers.
    theme.background = Rgba([0, 0, 0, 0]);
    theme.island_padding = 8.0;
    theme.island_border = 1.0;
    theme.island_arrow = ARROW;
    let mut ui = Ui::new(t("menu.open")?, theme, options.rtl);
    let frame = ui.island(Ui::ROOT, true, [ARROW, 0.0, 0.0, 0.0]);
    ui.set_arrow(frame);
    let menu = ui.add(frame, Kind::Menu);
    ui.add(menu, Kind::MenuItem(t("menu.item")?.into()));
    let check = ui.menu_check(menu, t("menu.check")?, choices.check);
    ui.add(menu, Kind::Separator);
    let first = ui.menu_radio(menu, t("menu.radio1")?, choices.radio == 0);
    let second = ui.menu_radio(menu, t("menu.radio2")?, choices.radio == 1);
    ui.add(menu, Kind::Separator);
    let off = ui.add(menu, Kind::MenuItem(t("menu.off")?.into()));
    ui.set_sensitive(off, false);
    let (_, page) = ui.submenu(menu, t("menu.more")?);
    ui.add(page, Kind::MenuItem(t("menu.about")?.into()));
    ui.layout();
    Ok((ui, [check, first, second]))
}

/// The window, with the page toggles that show one page each, and the widgets the gallery acts
/// on.
fn build(options: &Options) -> Result<(Ui, Gallery), String> {
    let lang = options.lang;
    let mut ui = Ui::new(tr(lang, "title")?, theme(), options.rtl);
    ui.min_width = 420.0;

    let pages = tr(lang, "pages")?;
    let selector = ui.add(Ui::ROOT, Kind::Group(pages.into()));
    ui.add(selector, Kind::Label(pages.into()));
    let row = ui.add(selector, Kind::Row);
    let toggles: Vec<WidgetId> = PAGES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let label = tr(lang, &format!("page.{name}"))?;
            Ok(ui.toggle(row, label, i == options.page))
        })
        .collect::<Result<_, String>>()?;

    let mut shown = Vec::new();
    let mut acted = Vec::new();
    for (i, name) in PAGES.iter().enumerate() {
        let title = tr(lang, &format!("page.{name}"))?;
        let group = ui.add(Ui::ROOT, Kind::Group(title.into()));
        ui.add(group, Kind::Label(title.into()));
        acted.extend(page(&mut ui, group, name, lang)?);
        ui.set_visible(group, i == options.page);
        shown.push((toggles[i], group));
    }
    ui.set_focus(Some(toggles[options.page]));
    ui.layout();
    let [replace, list, switch, button, open, menu] = acted[..] else {
        return Err("the lists, states, dialogs and menus pages lost their widgets".into());
    };
    Ok((
        ui,
        Gallery {
            pages: shown,
            states: (switch, button),
            lists: (replace, list),
            first: 0,
            lang,
            open,
            menu,
        },
    ))
}

/// Shows the page of `selected` and hides the others; a selection that is not a page toggle,
/// such as a toggle inside a page, changes nothing.
fn show_page(ui: &mut Ui, pages: &[(WidgetId, WidgetId)], selected: WidgetId) {
    if pages.iter().any(|&(toggle, _)| toggle == selected) {
        for &(toggle, group) in pages {
            ui.set_visible(group, toggle == selected);
        }
    }
}

/// What an event of the window asks of the application.
#[derive(Debug, PartialEq)]
enum Answer {
    Nothing,
    OpenDialog,
    OpenMenu,
}

/// Answers an event of the window: a page toggle shows its page, the states page's switch makes
/// its button sensitive or insensitive, the lists page's button replaces the rows of its list
/// with the next fifty, or the first fifty again, and the dialogs and menus pages' buttons ask
/// for the dialog and the menu.
fn on_event(ui: &mut Ui, gallery: &mut Gallery, event: Event) -> Answer {
    match event {
        Event::Selected(selected) => show_page(ui, &gallery.pages, selected),
        Event::Toggled(id) if id == gallery.states.0 => {
            ui.set_sensitive(gallery.states.1, ui.is_active(id));
        }
        Event::Clicked(id) if id == gallery.lists.0 => {
            let list = gallery.lists.1;
            gallery.first = ROWS - gallery.first;
            ui.remove_children(list);
            if let Err(error) = fill_rows(ui, list, gallery.lang, gallery.first) {
                eprintln!("athanor-sal-gallery: {error}");
            }
            ui.layout();
        }
        Event::Clicked(id) if id == gallery.open => return Answer::OpenDialog,
        Event::Clicked(id) if id == gallery.menu => return Answer::OpenMenu,
        _ => {}
    }
    Answer::Nothing
}

/// The window, and the dialog and the menu it opens over itself.
struct App {
    options: Options,
    gallery: Gallery,
    window: Option<Ui>,
    /// The window once open, and its dialog while open.
    open: Option<(SurfaceId, Option<SurfaceId>)>,
    choices: Choices,
    /// The menu's check and radios, from its last opening.
    entries: [WidgetId; 3],
}

impl App {
    fn open_dialog(&mut self, shell: &mut Shell) {
        let Some((window, None)) = self.open else {
            return;
        };
        let opened = dialog(&self.options).map_err(Into::into);
        match opened.and_then(|ui| shell.open_dialog(window, ui)) {
            Ok(id) => self.open = Some((window, Some(id))),
            Err(error) => eprintln!("athanor-sal-gallery: the dialog did not open: {error}"),
        }
    }

    fn open_menu(&mut self, shell: &mut Shell) {
        let Some((window, None)) = self.open else {
            return;
        };
        let opened = menu(&self.options, self.choices).map_err(Into::into);
        match opened.and_then(|(ui, entries)| {
            self.entries = entries;
            shell.open_popup(window, self.gallery.menu, ui, Side::Bottom)
        }) {
            Ok(_) => {}
            Err(error) => eprintln!("athanor-sal-gallery: the menu did not open: {error}"),
        }
    }

    /// An entry of the menu: an item closes it, as a check or a radio does once it has changed
    /// the choices. The submenu's entries slide its pages in the menu itself.
    fn menu_event(&mut self, shell: &mut Shell, event: Event) {
        let [check, first, second] = self.entries;
        match event {
            Event::Clicked(_) => {}
            Event::Toggled(id) if id == check => self.choices.check = !self.choices.check,
            Event::Toggled(id) if id == first => self.choices.radio = 0,
            Event::Toggled(id) if id == second => self.choices.radio = 1,
            _ => return,
        }
        shell.close_popup();
    }
}

impl Application for App {
    fn start(&mut self) -> Vec<(Ui, Role)> {
        let role = Role::Window {
            app_id: "athanor-sal-gallery".into(),
        };
        self.window.take().map(|ui| (ui, role)).into_iter().collect()
    }

    fn drawn(&mut self, shell: &mut Shell, surface: SurfaceId) {
        if self.open.is_none() {
            self.open = Some((surface, None));
            if self.options.page == DIALOGS {
                self.open_dialog(shell);
            } else if self.options.page == MENUS {
                self.open_menu(shell);
            }
        }
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        match self.open {
            // The dialog's button, Escape, its Close and the compositor's request close it.
            Some((window, Some(dialog))) if surface == dialog => {
                if matches!(event, Event::Clicked(_) | Event::Window(WindowButton::Close)) {
                    shell.close(dialog);
                    self.open = Some((window, None));
                }
            }
            Some((window, None)) if surface != window => self.menu_event(shell, event),
            _ => {
                let answer = shell
                    .ui_mut(surface)
                    .map(|ui| on_event(ui, &mut self.gallery, event));
                match answer {
                    Some(Answer::OpenDialog) => self.open_dialog(shell),
                    Some(Answer::OpenMenu) => self.open_menu(shell),
                    _ => {}
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let run = || -> Result<(), String> {
        let options = Options::parse(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()).skip(1))?;
        let (ui, gallery) = build(&options)?;
        let app = App {
            options,
            gallery,
            window: Some(ui),
            open: None,
            choices: Choices {
                check: true,
                radio: 0,
            },
            entries: [Ui::ROOT; 3],
        };
        driver::run(app).map_err(|error| error.to_string())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-gallery: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_sal::Mark;

    #[test]
    fn every_page_builds_in_every_language() -> Result<(), String> {
        for lang in LANGUAGES {
            build(&Options::parse(["--lang", lang].map(String::from).into_iter())?)?;
        }
        Ok(())
    }


    #[test]
    fn a_selection_inside_a_page_keeps_the_page() -> Result<(), String> {
        let (mut ui, gallery) = build(&Options::parse(std::iter::empty())?)?;
        let pages = gallery.pages;
        let (first, second) = (pages[0], pages[1]);
        assert!(ui.is_visible(first.1) && !ui.is_visible(second.1));
        // Not a page toggle: the group's own id stands for any widget inside a page.
        show_page(&mut ui, &pages, first.1);
        assert!(ui.is_visible(first.1) && !ui.is_visible(second.1));
        show_page(&mut ui, &pages, second.0);
        assert!(!ui.is_visible(first.1) && ui.is_visible(second.1));
        Ok(())
    }

    #[test]
    fn the_states_switch_makes_its_button_sensitive() -> Result<(), String> {
        let options = Options::parse(["--page", "states"].map(String::from).into_iter())?;
        let (mut ui, mut gallery) = build(&options)?;
        let (switch, button) = gallery.states;
        assert!(!ui.is_active(switch) && !ui.is_sensitive(button));
        let event = ui.activate(switch).ok_or("the switch does not toggle")?;
        on_event(&mut ui, &mut gallery, event);
        assert!(ui.is_sensitive(button));
        let event = ui
            .set_active(switch, false)
            .ok_or("the switch does not toggle")?;
        on_event(&mut ui, &mut gallery, event);
        assert!(!ui.is_sensitive(button));
        Ok(())
    }

    #[test]
    fn the_dialogs_button_asks_for_the_dialog() -> Result<(), String> {
        let options = Options::parse(["--page", "dialogs"].map(String::from).into_iter())?;
        assert_eq!(options.page, DIALOGS);
        let (mut ui, mut gallery) = build(&options)?;
        let event = ui.activate(gallery.open).ok_or("the button does not click")?;
        assert_eq!(on_event(&mut ui, &mut gallery, event), Answer::OpenDialog);
        let dialog = dialog(&options)?;
        assert_eq!(dialog.title, tr(0, "dialog.title")?);
        Ok(())
    }

    #[test]
    fn the_menus_page_holds_each_kind_of_menu_entry() -> Result<(), String> {
        let options = Options::parse(["--page", "menus"].map(String::from).into_iter())?;
        assert_eq!(options.page, MENUS);
        let (mut ui, mut gallery) = build(&options)?;
        let event = ui.activate(gallery.menu).ok_or("the button does not click")?;
        assert_eq!(on_event(&mut ui, &mut gallery, event), Answer::OpenMenu);
        let choices = Choices {
            check: false,
            radio: 1,
        };
        let (menu, [check, first, second]) = menu(&options, choices)?;
        let marks: Vec<_> = [check, first, second]
            .map(|id| match &menu.widget(id).kind {
                Kind::MenuEntry { mark, .. } => Some(*mark),
                _ => None,
            })
            .into();
        assert_eq!(
            marks,
            [
                Some(Mark::Check(false)),
                Some(Mark::Radio(false)),
                Some(Mark::Radio(true))
            ]
        );
        let entries = &menu.widget(menu.widget(check).parent.ok_or("no menu")?).children;
        let kinds: Vec<_> = entries.iter().map(|&id| &menu.widget(id).kind).collect();
        assert_eq!(
            kinds.iter().filter(|kind| matches!(kind, Kind::Separator)).count(),
            2
        );
        let off = entries[entries.len() - 2];
        assert!(!menu.is_sensitive(off), "the last section's item takes no input");
        assert!(matches!(
            kinds.last(),
            Some(Kind::MenuEntry {
                mark: Mark::Submenu(_),
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn the_day_of_a_time_counts_the_gregorian_calendar() {
        let date = |year, month, day| Date { year, month, day };
        assert_eq!(civil(0), date(1970, 1, 1));
        assert_eq!(civil(951_782_400), date(2000, 2, 29));
        // The rig's frozen time, scene.sh's 2026-09-18 10:00.
        assert_eq!(civil(1_789_725_600), date(2026, 9, 18));
    }


    /// The name of each row of `list`, the headings left out.
    fn row_names(ui: &Ui, list: WidgetId) -> Vec<String> {
        let rows = ui.widget(list).children.iter();
        rows.filter_map(|&id| match &ui.widget(id).kind {
            Kind::ListRow(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
    }

    #[test]
    fn the_lists_button_replaces_the_rows_and_back() -> Result<(), String> {
        let options = Options::parse(["--page", "lists"].map(String::from).into_iter())?;
        let (mut ui, mut gallery) = build(&options)?;
        let (button, list) = gallery.lists;
        let count = ui.widget(list).children.len();
        // Fifty rows and a heading among them, each row a column of its name and a second
        // line; the first is a row, where Tab enters the list.
        assert_eq!(count, 51);
        let first_child = ui.widget(list).children[0];
        assert!(matches!(ui.widget(first_child).kind, Kind::ListRow(_)));
        let first = row_names(&ui, list);
        assert_eq!((first[0].as_str(), first[49].as_str()), ("Row 1", "Row 50"));
        for expected in [("Row 51", "Row 100"), ("Row 1", "Row 50")] {
            let event = ui.activate(button).ok_or("the button does not click")?;
            on_event(&mut ui, &mut gallery, event);
            let names = row_names(&ui, list);
            assert_eq!((names[0].as_str(), names[49].as_str()), expected);
            assert_eq!(ui.widget(list).children.len(), count);
        }
        Ok(())
    }
}
