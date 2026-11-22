//! A resident application: no window at start, a window opened at run time for each `show` line
//! on its standard input, as a D-Bus `Show` would open one, closed at its Close, and a run that
//! outlives its last window until a `quit` line: the rig's check of SL1 (doc_settings.md SE5,
//! section 7; `toolkit_window_check.py`). Prints `opened`, `drawn` and `closed` for each window.
//! With `--default` it opens one window at start and keeps the driver's default, under which
//! that window's Close ends the run.

use athanor_sal::driver::{self, Application, Role, Shell, SurfaceId};
use athanor_sal::{Event, Kind, Theme, Ui, WindowButton};
use smithay_client_toolkit::reexports::calloop::ping::make_ping;
use std::io::BufRead;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const APP_ID: &str = "os.athanor.SalResident";

fn window() -> Ui {
    let mut ui = Ui::new("Resident", Theme::default(), false);
    ui.add(Ui::ROOT, Kind::Label("A window opened at run time".into()));
    ui
}

struct Resident {
    /// The lines read from the standard input since the last turn.
    lines: Arc<Mutex<Vec<String>>>,
    default: bool,
}

impl Application for Resident {
    fn start(&mut self) -> Vec<(Ui, Role)> {
        if !self.default {
            return Vec::new();
        }
        println!("opened");
        let app_id = APP_ID.to_owned();
        vec![(window(), Role::Window { app_id })]
    }

    fn close_ends_run(&self) -> bool {
        self.default
    }

    fn turn(&mut self, shell: &mut Shell) {
        let lines = match self.lines.lock() {
            Ok(mut lines) => std::mem::take(&mut *lines),
            Err(_) => return shell.fail("the standard input's reader stopped".into()),
        };
        for line in lines {
            match line.trim() {
                "show" => match shell.open_window(window(), APP_ID) {
                    Ok(_) => println!("opened"),
                    Err(error) => shell.fail(error),
                },
                "quit" => shell.quit(),
                other => shell.fail(format!("unknown command {other:?}").into()),
            }
        }
    }

    fn drawn(&mut self, _: &mut Shell, _: SurfaceId) {
        println!("drawn");
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if event == Event::Window(WindowButton::Close) {
            shell.close(surface);
            println!("closed");
        }
    }
}

fn main() -> ExitCode {
    let default = std::env::args().any(|arg| arg == "--default");
    let lines = Arc::new(Mutex::new(Vec::new()));
    let result = make_ping().map_err(Into::into).and_then(|(ping, source)| {
        let read = Arc::clone(&lines);
        // The reader wakes the event loop for each line; at the end of its input it stops.
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                match read.lock() {
                    Ok(mut lines) => lines.push(line),
                    Err(_) => return,
                }
                ping.ping();
            }
        });
        driver::run_with(Resident { lines, default }, source)
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-resident: {error}");
            ExitCode::FAILURE
        }
    }
}
