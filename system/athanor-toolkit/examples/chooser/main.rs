//! The layout chooser on the toolkit, beside the GTK one until it replaces it (doc_own_stack.md,
//! milestone T1). It speaks the language of the GTK chooser's catalogue, which athanor-i18n
//! finds as it does for the GTK chooser (`ATHANOR_I18N_CATALOG` names one in the rig), and
//! mirrors itself when that language is written right to left.

mod tree;

use athanor_i18n::Catalog;
use athanor_toolkit::{driver, Event};
use std::process::ExitCode;

/// The GTK chooser's gettext domain, whose catalogue this chooser reads.
const DOMAIN: &str = "athanor-layout-chooser";

fn main() -> ExitCode {
    // As the GTK chooser: a catalogue that cannot be read is reported, and the chooser speaks
    // English.
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, error)| {
        eprintln!(
            "athanor-layout-chooser: translations are unavailable: {}: {error}",
            path.display()
        );
        Catalog::empty()
    });
    let chooser = tree::build(&catalog, catalog.is_rtl());
    let choices = chooser.choices;
    let on_event = move |_: &mut athanor_toolkit::Ui, event: Event| {
        if let Event::Selected(id) = event {
            if let Some((_, group, name)) = choices.iter().find(|c| c.0 == id) {
                println!("{group}: {name}");
            }
        }
    };
    match driver::run(chooser.ui, "athanor-layout-chooser", Box::new(on_event)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-layout-chooser: {error}");
            ExitCode::FAILURE
        }
    }
}
