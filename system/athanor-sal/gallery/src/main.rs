//! Every widget of athanor-sal on a page of its own, for an application developer to see
//! what the toolkit offers and for the rig to compare with its GTK 4 twin
//! (`forge/test/shell/gallery_gtk.py`), which shows the same pages from the same strings.
//!
//! `athanor-sal-gallery [--lang en|it] [--rtl] [--page <name>]`: `--lang` picks a column of
//! `strings.tsv`, `--rtl` mirrors the window and `--page` names the page shown at the start. The
//! dialogs page starts with its dialog open, the focus on the dialog's button.

use athanor_sal::driver::Role;
use athanor_sal::{
    driver, Application, Event, Kind, Shell, SurfaceId, Theme, Ui, WidgetId, WindowButton, Wrap,
};
use std::num::NonZeroU32;
use std::process::ExitCode;

/// One line per string: the key, then its text in each language, tab separated.
const STRINGS: &str = include_str!("../strings.tsv");
/// The languages of `strings.tsv`, in the order of its columns.
const LANGUAGES: [&str; 2] = ["en", "it"];
/// The pages, in the order of the page toggles; the first is shown unless `--page` names another.
const PAGES: [&str; 7] = ["buttons", "choices", "values", "lists", "text", "states", "dialogs"];
/// The page that opens a dialog, with its dialog open at the start.
const DIALOGS: usize = 6;

/// The window's widgets the gallery acts on.
struct Gallery {
    /// Each page's toggle and group.
    pages: Vec<(WidgetId, WidgetId)>,
    /// The states page's switch and the button it makes sensitive.
    states: (WidgetId, WidgetId),
    /// The dialogs page's button, which opens the dialog.
    open: WidgetId,
}

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

/// The widgets of page `name` under `group`: for the states page, its switch and its button, and
/// for the dialogs page its button.
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
            let scroll = ui.scroll(group, 200.0);
            let list = ui.list(scroll, t("list")?);
            let row = t("row")?;
            for n in 1..=50 {
                ui.list_row(list, &format!("{row} {n}"));
            }
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
        "dialogs" => return Ok(vec![ui.add(group, Kind::Button(t("dialog.open")?.into()))]),
        _ => return Err(format!("no page {name:?}")),
    }
    Ok(Vec::new())
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
    let [switch, button, open] = acted[..] else {
        return Err("the states and dialogs pages lost their widgets".into());
    };
    Ok((
        ui,
        Gallery {
            pages: shown,
            states: (switch, button),
            open,
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
}

/// Answers an event of the window: a page toggle shows its page, the states page's switch makes
/// its button sensitive or insensitive, and the dialogs page's button asks for the dialog.
fn on_event(ui: &mut Ui, gallery: &Gallery, event: Event) -> Answer {
    match event {
        Event::Selected(selected) => show_page(ui, &gallery.pages, selected),
        Event::Toggled(id) if id == gallery.states.0 => {
            ui.set_sensitive(gallery.states.1, ui.is_active(id));
        }
        Event::Clicked(id) if id == gallery.open => return Answer::OpenDialog,
        _ => {}
    }
    Answer::Nothing
}

/// The window, and the dialog it opens over itself.
struct App {
    options: Options,
    gallery: Gallery,
    window: Option<Ui>,
    /// The window once open, and its dialog while open.
    open: Option<(SurfaceId, Option<SurfaceId>)>,
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
            _ => {
                let answer = shell
                    .ui_mut(surface)
                    .map(|ui| on_event(ui, &self.gallery, event));
                if answer == Some(Answer::OpenDialog) {
                    self.open_dialog(shell);
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let run = || -> Result<(), String> {
        let options = Options::parse(std::env::args().skip(1))?;
        let (ui, gallery) = build(&options)?;
        let app = App {
            options,
            gallery,
            window: Some(ui),
            open: None,
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
        let (mut ui, gallery) = build(&options)?;
        let (switch, button) = gallery.states;
        assert!(!ui.is_active(switch) && !ui.is_sensitive(button));
        let event = ui.activate(switch).ok_or("the switch does not toggle")?;
        on_event(&mut ui, &gallery, event);
        assert!(ui.is_sensitive(button));
        let event = ui
            .set_active(switch, false)
            .ok_or("the switch does not toggle")?;
        on_event(&mut ui, &gallery, event);
        assert!(!ui.is_sensitive(button));
        Ok(())
    }

    #[test]
    fn the_dialogs_button_asks_for_the_dialog() -> Result<(), String> {
        let options = Options::parse(["--page", "dialogs"].map(String::from).into_iter())?;
        assert_eq!(options.page, DIALOGS);
        let (mut ui, gallery) = build(&options)?;
        let event = ui.activate(gallery.open).ok_or("the button does not click")?;
        assert_eq!(on_event(&mut ui, &gallery, event), Answer::OpenDialog);
        let dialog = dialog(&options)?;
        assert_eq!(dialog.title, tr(0, "dialog.title")?);
        Ok(())
    }
}
