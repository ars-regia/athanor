//! The layout chooser on the toolkit, beside the GTK one until it replaces it (doc_own_stack.md,
//! milestone T1). `ATHANOR_RTL=1` mirrors it.

mod tree;

use athanor_toolkit::{driver, Event};
use std::process::ExitCode;

fn main() -> ExitCode {
    let rtl = std::env::var_os("ATHANOR_RTL").is_some_and(|v| v == "1");
    let chooser = tree::build(rtl);
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
