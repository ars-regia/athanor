//! The session lock: one lock surface on each output, naming it, as outputs come and go: the rig's
//! check of the driver's lock role (doc_lock_and_prompts.md LP18, the Sal prerequisites;
//! `toolkit_lock_check.py`). Asks for the lock at its first turn and prints `locked` when the
//! compositor confirms it, or `finished` when it refuses or ends it. Escape on a lock surface ends
//! the lock and prints `unlocked`; the client then stays, as a lock between locks does.

use athanor_sal::driver::{self, Application, OutputInfo, Shell, SurfaceId};
use athanor_sal::{Event, Kind, Theme, Ui};
use std::process::ExitCode;

#[derive(Default)]
struct Lock {
    asked: bool,
}

impl Application for Lock {
    fn lock_surface(&mut self, output: &OutputInfo) -> Option<Ui> {
        let name = output.name.clone().unwrap_or_else(|| output.id.to_string());
        let mut ui = Ui::new(&name, Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Label(name.clone()));
        Some(ui)
    }

    fn turn(&mut self, shell: &mut Shell) {
        if !self.asked {
            self.asked = true;
            if let Err(error) = shell.lock() {
                shell.fail(error);
            }
        }
    }

    fn locked(&mut self, _: &mut Shell) {
        println!("locked");
    }

    fn finished(&mut self, _: &mut Shell) {
        println!("finished");
    }

    fn event(&mut self, shell: &mut Shell, _: SurfaceId, event: Event) {
        if event != Event::Escape {
            return;
        }
        match shell.unlock() {
            Ok(()) => println!("unlocked"),
            Err(error) => shell.fail(error),
        }
    }
}

fn main() -> ExitCode {
    match driver::run(Lock::default()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-lock: {error}");
            ExitCode::FAILURE
        }
    }
}
