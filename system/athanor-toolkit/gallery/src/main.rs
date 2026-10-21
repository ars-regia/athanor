//! Every widget of athanor-toolkit on a page of its own, for an application developer to see
//! what the toolkit offers and for the rig to compare with its GTK 4 twin
//! (`forge/test/shell/gallery_gtk.py`), which shows the same pages from the same strings.
//!
//! `athanor-toolkit-gallery [--lang en|it] [--rtl] [--page <name>]`: `--lang` picks a column of
//! `strings.tsv`, `--rtl` mirrors the window and `--page` names the page shown at the start.

use athanor_toolkit::{driver, Event, Kind, Theme, Ui, WidgetId};
use std::process::ExitCode;

/// One line per string: the key, then its text in each language, tab separated.
const STRINGS: &str = include_str!("../strings.tsv");
/// The languages of `strings.tsv`, in the order of its columns.
const LANGUAGES: [&str; 2] = ["en", "it"];
/// The pages, in the order of the page toggles; the first is shown unless `--page` names another.
const PAGES: [&str; 3] = ["buttons", "choices", "values"];

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

/// A toggle of a group: the first one is active.
fn toggle(label: &str, active: bool) -> Kind {
    Kind::Toggle {
        label: label.into(),
        active,
    }
}

/// The widgets of page `name` under `group`.
fn page(ui: &mut Ui, group: WidgetId, name: &str, lang: usize) -> Result<(), String> {
    let t = |key| tr(lang, key);
    match name {
        "buttons" => {
            ui.add(group, Kind::Label(t("label")?.into()));
            ui.add(group, Kind::Button(t("button")?.into()));
            let row = ui.add(group, Kind::Row);
            for (i, key) in ["one", "two", "three"].into_iter().enumerate() {
                ui.add(row, toggle(t(key)?, i == 0));
            }
            ui.field(group, t("field")?, "");
        }
        "choices" => {
            let check = |label: &str, active| Kind::Check {
                label: label.into(),
                active,
            };
            ui.add(group, check(t("check1")?, true));
            ui.add(group, check(t("check2")?, false));
            ui.add(
                group,
                Kind::Switch {
                    label: t("switch")?.into(),
                    active: true,
                },
            );
        }
        "values" => {
            ui.slider(group, t("slider")?, 0.0, 100.0, 1.0, 50.0)
                .ok_or("the slider's range is broken")?;
        }
        _ => return Err(format!("no page {name:?}")),
    }
    Ok(())
}

/// The window, with the page toggles that show one page each; returns it with each page's
/// toggle and group.
fn build(options: &Options) -> Result<(Ui, Vec<(WidgetId, WidgetId)>), String> {
    let lang = options.lang;
    // The GTK twin's margins and spacings, as the GTK chooser's: 24 around, 18 between the
    // groups, 8 inside one.
    let theme = Theme {
        margin: 24.0,
        section_gap: 18.0,
        gap: 8.0,
        ..Theme::default()
    };
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
            Ok(ui.add(row, toggle(label, i == options.page)))
        })
        .collect::<Result<_, String>>()?;

    let mut shown = Vec::new();
    for (i, name) in PAGES.iter().enumerate() {
        let title = tr(lang, &format!("page.{name}"))?;
        let group = ui.add(Ui::ROOT, Kind::Group(title.into()));
        ui.add(group, Kind::Label(title.into()));
        page(&mut ui, group, name, lang)?;
        ui.set_visible(group, i == options.page);
        shown.push((toggles[i], group));
    }
    ui.set_focus(Some(toggles[options.page]));
    ui.layout();
    Ok((ui, shown))
}

fn main() -> ExitCode {
    let run = || -> Result<(), String> {
        let options = Options::parse(std::env::args().skip(1))?;
        let (ui, pages) = build(&options)?;
        let on_event = move |ui: &mut Ui, event: Event| {
            if let Event::Selected(selected) = event {
                for &(toggle, group) in &pages {
                    ui.set_visible(group, toggle == selected);
                }
            }
        };
        driver::run_window(ui, "athanor-toolkit-gallery", Box::new(on_event))
            .map_err(|error| error.to_string())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-toolkit-gallery: {error}");
            ExitCode::FAILURE
        }
    }
}
