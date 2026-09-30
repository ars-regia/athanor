#![allow(clippy::all, warnings)]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::Parser;
use gtk4::prelude::*;
use gtk4::{gio, Application};

mod theme;
mod wayland;
mod ipc;
mod sys;
mod ui;
mod core;
pub mod morphic_pill;
pub mod control_center;
pub mod desktop_canvas;
pub mod appearance_engine;
pub mod launcher;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    file_chooser: bool,
    #[arg(long)]
    topbar: bool,
    #[arg(long)]
    spotlight: bool,
    #[arg(long)]
    launcher: bool,
    #[arg(long)]
    dock: bool,
    #[arg(long)]
    control_center: bool,
    #[arg(long)]
    media_player: bool,
    #[arg(long)]
    sys_monitor: bool,
    #[arg(long)]
    calendar: bool,
    #[arg(long)]
    powermenu: bool,
    #[arg(long)]
    gatekeeper_prompt: Option<String>,
    #[arg(long)]
    privacy_prompt: Option<String>,
    #[arg(long)]
    overview: bool,
    #[arg(long)]
    store: bool,
    #[arg(long)]
    snap_overlay: bool,
    #[arg(long)]
    desktop_stacks: bool,
}


const APP_ID: &str = "os.athanor.Shell";

fn init_telemetry() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,athanor_shell_rs=debug"))
        )
        .with_target(true)
        // Standard output is the answer of a prompt or a chooser to the portal that ran it,
        // so logs go to standard error.
        .with_writer(std::io::stderr)
        .init();
}

#[tracing::instrument]
fn main() -> glib::ExitCode {
    // Confinement comes first, while this is the only thread in the process. Landlock
    // restricts the calling thread and the threads it creates from then on; a thread
    // that already exists -- a Tokio worker, a GLib worker -- would keep running
    // unconfined. Nothing before this line may start a thread, and the check below
    // fails closed if something did. Logging is not set up yet, so errors go to stderr.
    if let Err(err) = crate::sys::sandbox::ensure_single_threaded()
        .and_then(|()| crate::sys::sandbox::apply_landlock_sandbox())
    {
        eprintln!("athanor-shell-rs: cannot apply the Landlock policy, refusing to run unconfined: {err}");
        return glib::ExitCode::FAILURE;
    }

    init_telemetry();
    tracing::info!("Starting Athanor Shell...");

    // The shell drives its futures from GLib's main loop, but the IPC actors and the
    // D-Bus proxies are written against Tokio: AudioActor::spawn calls tokio::spawn and
    // the proxies wrap their calls in tokio::time::timeout. Both need a Tokio runtime to
    // exist and to be the current one on this thread, or they abort the process with
    // "there is no reactor running" -- which is how every greeter start ended in
    // acceptance run 34384585109, three seconds in, before a window was drawn. One
    // runtime for the life of the process, entered here so that whatever the main loop
    // polls can reach it; what is spawned onto it runs on its own worker threads.
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(error = %err, "cannot create the Tokio runtime");
            return glib::ExitCode::FAILURE;
        }
    };
    let _tokio = runtime.enter();

    // Forza il renderer GTK4 NGL (New GL) ad altissime prestazioni / Vulkan e backend puramente Wayland
    std::env::set_var("GSK_RENDERER", "ngl");
    std::env::set_var("GDK_BACKEND", "wayland");
    // Disabilita lo scaling X11 frazionario per evitare blur
    std::env::set_var("GDK_SCALE", "1");

    let args = Args::parse();
    crate::ipc::init_system_controller();

    if let Some(req_info) = args.privacy_prompt {
        // Not unique: with a unique application id a second request, made while a prompt is
        // open, would be forwarded to it and would exit 0 at once, and the portal would have
        // no answer of the user's to read. Each request is its own process with its own prompt.
        let app = Application::builder()
            .application_id("os.athanor.PrivacyPrompt")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        let req_clone = req_info.clone();
        app.connect_activate(move |app| {
            crate::theme::init_css();
            crate::ui::privacy_prompt::build_ui(app, &req_clone);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

        if args.file_chooser {
        let app = Application::builder()
            .application_id("os.athanor.FileChooser")
            .build();
        app.connect_activate(move |app| {
            crate::theme::init_css();
            crate::ui::file_chooser::build_ui(app);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

    if let Some(app_path) = args.gatekeeper_prompt {
        let app = Application::builder()
            .application_id("os.athanor.GatekeeperPrompt")
            .build();
        let path_clone = app_path.clone();
        app.connect_activate(move |app| {
            crate::theme::init_css();
            crate::ui::gatekeeper_prompt::build_ui(app, &path_clone);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

    if args.overview {
        let app = Application::builder()
            .application_id("os.athanor.MissionControl")
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            crate::ui::mission_control::build_ui(app);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

    if args.store {
        let app = Application::builder()
            .application_id("os.athanor.StoreUI")
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            crate::ui::store::show_store_modal(app);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

    if args.snap_overlay {
        let app = Application::builder()
            .application_id("os.athanor.SnapOverlay")
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            crate::ui::snap_overlay::show_snap_overlay(app, None);
        });
        return app.run_with_args(&Vec::<String>::new());
    }

    if args.desktop_stacks {
        let app = Application::builder()
            .application_id("os.athanor.DesktopCanvas")
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            crate::desktop_canvas::build_desktop_canvas(app);
        });
        return app.run_with_args(&Vec::<String>::new());
    }


    if args.launcher {
        let app = Application::builder()
            .application_id("os.athanor.Launcher")
            .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            static ACTIVATED_LAUNCHER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ACTIVATED_LAUNCHER.swap(true, std::sync::atomic::Ordering::SeqCst) {
                launcher::show_launcher_window(app);
            } else {
                launcher::toggle_launcher_visibility();
            }
        });
        app.connect_command_line(|app, _cmdline| {
            app.activate();
            0
        });
        return app.run();
    }

    if args.dock {
        let app = Application::builder()
            .application_id("os.athanor.Dock")
            .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
            .build();
        app.connect_activate(|app| {
            crate::theme::init_css();
            static ACTIVATED_DOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ACTIVATED_DOCK.swap(true, std::sync::atomic::Ordering::SeqCst) {
                ui::dock::build_ui(app);
            } else {
                ui::dock::toggle_dock_visibility();
            }
        });
        app.connect_command_line(|app, _cmdline| {
            app.activate();
            0
        });
        return app.run();
    }

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

thread_local! {
    static TOPBAR_CTRL: std::cell::RefCell<Option<relm4::component::Connector<ui::topbar::TopbarModel>>> = const { std::cell::RefCell::new(None) };
}

    app.connect_activate(move |app| {
        crate::theme::init_css();
        static ACTIVATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !ACTIVATED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            use relm4::Component;
            let ctrl = ui::topbar::TopbarModel::builder()
                .launch(app.clone());
            
            TOPBAR_CTRL.with(|c| {
                *c.borrow_mut() = Some(ctrl);
            });
                
            crate::ui::osd::spawn_osd(app);
            crate::ui::desktop_widgets::build_desktop_widgets(app);
        }
    });

    app.connect_command_line(move |app, cmdline| {
        app.activate();
        let args = cmdline.arguments();
        for arg in args.iter().skip(1) {
            let s = arg.to_string_lossy();
            let clean = s.trim_start_matches("--");
            crate::ui::topbar::handle_command(app, clean);
        }
        0
    });

    // Pass original CLI args to run so GTK forwards them to primary instance
    app.run()
}

