//! The compositor client without GTK, for the end-to-end checks: `cc-launch DESKTOP_ID`
//! connects from a plain Wayland connection, runs GLib's main context inside a calloop
//! loop, launches the desktop entry behind a security context, and prints one JSON object:
//! the outputs read from `wl_output` and the unit started, or the error.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_compositor_client::glib_loop::GlibSource;
use athanor_compositor_client::Client;
use calloop::EventLoop;
use serde_json::{json, Value};
use wayland_client::Connection;

/// Longer than any start job the checks wait for.
const LIMIT: Duration = Duration::from_secs(30);

fn run(id: &str) -> Result<Value, String> {
    let error = |err: &dyn std::fmt::Display| err.to_string();
    let connection = Connection::connect_to_env().map_err(|e| error(&e))?;
    let client = Client::connect_wayland(&connection).map_err(|e| error(&e))?;
    let outputs: Vec<Value> = client
        .outputs()
        .iter()
        .map(|o| json!({"connector": o.connector, "width": o.width, "height": o.height}))
        .collect();
    let app = gio_unix::DesktopAppInfo::new(id).ok_or(format!("no desktop entry {id}"))?;

    let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new().map_err(|e| error(&e))?;
    let source = GlibSource::new().map_err(|e| error(&e))?;
    event_loop
        .handle()
        .insert_source(source, |_, _, _| {})
        .map_err(|e| error(&e.error))?;
    let launched = Rc::new(RefCell::new(None));
    glib::MainContext::default().spawn_local({
        let launched = launched.clone();
        async move {
            let result = client.launch(&app).await.map_err(|e| e.to_string());
            launched.replace(Some(result));
        }
    });
    let deadline = Instant::now() + LIMIT;
    while launched.borrow().is_none() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or("the launch did not end in time")?;
        event_loop.dispatch(left, &mut ()).map_err(|e| error(&e))?;
    }
    let unit = launched.take().ok_or("the launch left no result")??;
    Ok(json!({"outputs": outputs, "launched": unit}))
}

fn main() -> ExitCode {
    let Some(id) = std::env::args().nth(1) else {
        eprintln!("usage: cc-launch DESKTOP_ID");
        return ExitCode::from(2);
    };
    match run(&id) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            println!("{}", json!({"error": err}));
            ExitCode::FAILURE
        }
    }
}
