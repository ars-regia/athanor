//! Drives the compositor client from the command line for the end-to-end checks:
//! `cc-probe STEP...`, each step in turn, one JSON object per line on stdout.
//!
//! Steps: `snapshot`, `watch SECONDS`, `activate|minimize|unminimize|close APP_ID`,
//! `react APP_ID` (from then on, the handler unminimizes that window from inside the callback),
//! `tiling on|off`, `group N`, `magnifier on|off`, `filter none|greyscale|protanopia|
//! deuteranopia|tritanopia`, `drain` (from then on, each step is followed, before the main
//! loop runs again, by a second of display syncs: another reader that empties the socket),
//! `launch DESKTOP_ID`, `open launcher|app-library|workspaces`.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use athanor_compositor_client::{
    outputs, Client, Event, Opener, ScreenFilter, Tiling, Window, Workspace,
};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use serde_json::{json, Value};

fn window(window: &Window) -> Value {
    json!({
        "app_id": window.app_id,
        "title": window.title,
        "activated": window.state.activated,
        "minimized": window.state.minimized,
        "maximized": window.state.maximized,
        "fullscreen": window.state.fullscreen,
    })
}

fn workspace(workspace: &Workspace) -> Value {
    json!({
        "name": workspace.name,
        "active": workspace.active,
        "tiling": workspace.tiling.map(|tiling| format!("{tiling:?}")),
        "output": workspace.output,
    })
}

fn event(event: &Event) -> Value {
    match event {
        Event::WindowAdded(w) => json!({"window_added": window(w)}),
        Event::WindowChanged(w) => json!({"window_changed": window(w)}),
        Event::WindowRemoved(_) => json!({"window_removed": true}),
        Event::WorkspaceAdded(w) => json!({"workspace_added": workspace(w)}),
        Event::WorkspaceChanged(w) => json!({"workspace_changed": workspace(w)}),
        Event::WorkspaceRemoved(_) => json!({"workspace_removed": true}),
        Event::KeyboardLayouts(names) => json!({"keyboard_layouts": names}),
        Event::KeyboardGroup(group) => json!({"keyboard_group": group}),
        Event::Accessibility(a11y) => json!({"accessibility": format!("{a11y:?}")}),
    }
}

fn snapshot(client: &Client, display: &gdk::Display) -> Value {
    json!({"snapshot": {
        "windows": client.windows().iter().map(window).collect::<Vec<_>>(),
        "workspaces": client.workspaces().iter().map(workspace).collect::<Vec<_>>(),
        "keyboard_layouts": client.keyboard_layouts(),
        "keyboard_group": client.keyboard_group(),
        "accessibility": client.accessibility().map(|a11y| format!("{a11y:?}")),
        "outputs": outputs::current(display).iter()
            .map(|o| json!({"connector": o.connector, "width": o.width, "height": o.height}))
            .collect::<Vec<_>>(),
    }})
}

/// GDK's roundtrips read every event on the socket, those for the client's queue included,
/// and leave the socket empty, as the EGL and Vulkan WSI do while they swap.
fn drain(display: &gdk::Display) {
    let end = Instant::now() + Duration::from_secs(1);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
        display.sync();
    }
}

fn on(value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, got {value}")),
    }
}

async fn step(
    client: &Client,
    display: &gdk::Display,
    steps: &mut impl Iterator<Item = String>,
    verb: &str,
) -> Result<(), String> {
    let mut arg = || steps.next().ok_or(format!("{verb} needs an argument"));
    let by_app_id = |app_id: &str| {
        client
            .windows()
            .into_iter()
            .find(|w| w.app_id == app_id)
            .map(|w| w.id)
            .ok_or(format!("no window with app id {app_id}"))
    };
    let error = |err: &dyn std::fmt::Display| err.to_string();
    match verb {
        "snapshot" => println!("{}", snapshot(client, display)),
        "watch" => {
            let seconds: u64 = arg()?.parse().map_err(|err| error(&err))?;
            glib::timeout_future(Duration::from_secs(seconds)).await;
        }
        "activate" => client.activate(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "minimize" => client.minimize(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "unminimize" => client.unminimize(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "close" => client.close(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "react" => {
            // A handler that reads and acts from inside the callback (re-entrancy).
            let app_id = arg()?;
            client.connect_events(move |client, e| {
                println!("{}", json!({"event": event(e)}));
                if let Event::WindowChanged(w) = e {
                    if w.app_id == app_id && w.state.minimized {
                        let listed = client.windows().iter().any(|other| other.id == w.id);
                        let unminimized = client.unminimize(w.id).is_ok();
                        println!("{}", json!({"reacted": listed && unminimized}));
                    }
                }
            });
        }
        "tiling" => {
            let tiling = if on(&arg()?)? { Tiling::Tiled } else { Tiling::Floating };
            let active = client
                .workspaces()
                .into_iter()
                .find(|w| w.active)
                .ok_or("no active workspace")?;
            client.set_tiling(active.id, tiling).map_err(|e| error(&e))?;
        }
        "group" => {
            let group = arg()?.parse().map_err(|err| error(&err))?;
            client.set_keyboard_group(group).map_err(|e| error(&e))?;
        }
        "magnifier" => client.set_magnifier(on(&arg()?)?).map_err(|e| error(&e))?,
        "filter" => {
            let filter = match arg()?.as_str() {
                "none" => ScreenFilter::None,
                "greyscale" => ScreenFilter::Greyscale,
                "protanopia" => ScreenFilter::Protanopia,
                "deuteranopia" => ScreenFilter::Deuteranopia,
                "tritanopia" => ScreenFilter::Tritanopia,
                other => return Err(format!("no filter named {other}")),
            };
            client.set_screen_filter(false, filter).map_err(|e| error(&e))?;
        }
        "launch" => {
            let id = arg()?;
            let app = gio_unix::DesktopAppInfo::new(&id).ok_or(format!("no desktop entry {id}"))?;
            let unit = client.launch(&app).await.map_err(|e| error(&e))?;
            println!("{}", json!({"launched": unit}));
        }
        "open" => {
            let opener = match arg()?.as_str() {
                "launcher" => Opener::Launcher,
                "app-library" => Opener::AppLibrary,
                "workspaces" => Opener::Workspaces,
                other => return Err(format!("no opener named {other}")),
            };
            client.open(opener).await.map_err(|e| error(&e))?;
            println!("{}", json!({"opened": format!("{opener:?}")}));
        }
        other => return Err(format!("no step named {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    if let Err(err) = gtk4::init() {
        eprintln!("cc-probe: {err}");
        return ExitCode::FAILURE;
    }
    let Some(display) = gdk::Display::default() else {
        eprintln!("cc-probe: no display");
        return ExitCode::FAILURE;
    };
    let client = match Client::connect(&display) {
        Ok(client) => client,
        Err(err) => {
            println!("{}", json!({"error": err.to_string()}));
            return ExitCode::FAILURE;
        }
    };
    client.connect_events(|_, e| println!("{}", json!({"event": event(e)})));
    let main_loop = glib::MainLoop::new(None, false);
    let result = std::rc::Rc::new(std::cell::Cell::new(ExitCode::SUCCESS));
    glib::spawn_future_local({
        let (main_loop, result) = (main_loop.clone(), result.clone());
        async move {
            let mut steps = std::env::args().skip(1);
            let mut draining = false;
            while let Some(verb) = steps.next() {
                if verb == "drain" {
                    draining = true;
                    continue;
                }
                if let Err(err) = step(&client, &display, &mut steps, &verb).await {
                    println!("{}", json!({"error": err, "step": verb}));
                    result.set(ExitCode::FAILURE);
                    break;
                }
                if draining {
                    drain(&display);
                }
                // Let the compositor answer before the next step reads the state.
                glib::timeout_future(Duration::from_millis(300)).await;
            }
            main_loop.quit();
        }
    });
    main_loop.run();
    result.get()
}
