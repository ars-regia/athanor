//! One focused text field, for the rig's input-method scene (doc_own_stack.md, milestone T1,
//! task 6). Prints the field's committed text after every change.

use athanor_toolkit::{driver, Event, Theme, Ui};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut ui = Ui::new("Field", Theme::default(), false);
    let field = ui.field(Ui::ROOT, "Name", "");
    ui.set_focus(Some(field));
    let on_event = |ui: &mut Ui, event: Event| {
        if let Event::Edited(id) = event {
            println!("{}", ui.field_text(id).unwrap_or_default());
        }
    };
    match driver::run(ui, "athanor-toolkit-field", Box::new(on_event)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-toolkit-field: {error}");
            ExitCode::FAILURE
        }
    }
}
