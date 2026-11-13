//! Every widget of athanor-sal on a page of its own, for an application developer to see
//! what the toolkit offers and for the rig to compare with its GTK 4 twin
//! (`forge/test/shell/gallery_gtk.py`), which shows the same pages from the same strings.
//!
//! `athanor-sal-gallery [--lang en|it] [--rtl] [--page <name>]`: `--lang` picks a column of
//! `strings.tsv`, `--rtl` mirrors the window and `--page` names the page shown at the start.

use athanor_sal::{driver, Event, Kind, Theme, Ui, WidgetId};
use std::process::ExitCode;

/// One line per string: the key, then its text in each language, tab separated.
const STRINGS: &str = include_str!("../strings.tsv");
/// The languages of `strings.tsv`, in the order of its columns.
const LANGUAGES: [&str; 2] = ["en", "it"];
/// The pages, in the order of the page toggles; the first is shown unless `--page` names another.
const PAGES: [&str; 6] = ["buttons", "choices", "values", "lists", "text", "states"];

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
        .ok_or(format!(
            "strings.tsv has no {:?} for {key:?}",
            LANGUAGES[lang]
        ))
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

/// The widgets of page `name` under `group`; for the states page, its switch and its button,
/// and for the lists page, its button and its list.
fn page(
    ui: &mut Ui,
    group: WidgetId,
    name: &str,
    lang: usize,
) -> Result<Option<(WidgetId, WidgetId)>, String> {
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
            return Ok(Some((replace, list)));
        }
        "text" => {
            let text = t("notes.text")?.replace("\\n", "\n");
            ui.text_area(group, t("notes")?, &text, 4);
        }
        "states" => {
            // The button takes input once the switch is on.
            let switch = ui.switch(group, t("enable")?, false);
            let button = ui.add(group, Kind::Button(t("waiting")?.into()));
            ui.set_sensitive(button, false);
            return Ok(Some((switch, button)));
        }
        _ => return Err(format!("no page {name:?}")),
    }
    Ok(None)
}

/// The window, with the page toggles that show one page each, and the widgets the gallery acts
/// on.
fn build(options: &Options) -> Result<(Ui, Gallery), String> {
    let lang = options.lang;
    // The GTK twin's margins and spacings, as the GTK chooser's: 24 around, 18 between the
    // groups, 8 inside one.
    let mut theme = Theme::default();
    theme.margin = 24.0;
    theme.section_gap = 18.0;
    theme.gap = 8.0;
    let mut ui = Ui::new(tr(lang, "title")?, theme, options.rtl);
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
    let (mut states, mut lists) = (None, None);
    for (i, name) in PAGES.iter().enumerate() {
        let title = tr(lang, &format!("page.{name}"))?;
        let group = ui.add(Ui::ROOT, Kind::Group(title.into()));
        ui.add(group, Kind::Label(title.into()));
        let acted = page(&mut ui, group, name, lang)?;
        match *name {
            "states" => states = acted,
            "lists" => lists = acted,
            _ => {}
        }
        ui.set_visible(group, i == options.page);
        shown.push((toggles[i], group));
    }
    ui.set_focus(Some(toggles[options.page]));
    ui.layout();
    Ok((
        ui,
        Gallery {
            pages: shown,
            states: states.ok_or("no states page")?,
            lists: lists.ok_or("no lists page")?,
            first: 0,
            lang,
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

/// Answers an event of the window: a page toggle shows its page, the states page's switch
/// makes its button sensitive or insensitive, and the lists page's button replaces the rows of
/// its list with the next fifty, or the first fifty again.
fn on_event(ui: &mut Ui, gallery: &mut Gallery, event: Event) {
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
        _ => {}
    }
}

fn main() -> ExitCode {
    let run = || -> Result<(), String> {
        let options = Options::parse(std::env::args().skip(1))?;
        let (ui, mut gallery) = build(&options)?;
        let on_event = move |ui: &mut Ui, event: Event| on_event(ui, &mut gallery, event);
        driver::run_window(ui, "athanor-sal-gallery", Box::new(on_event))
            .map_err(|error| error.to_string())
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
