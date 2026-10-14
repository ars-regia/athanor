//! athanor-greeter-ui: the Athanor greeter, a program of its own (doc_shell.md, SH4).
//!
//! greetd starts cosmic-comp, cosmic-comp starts athanor-greeter-client, and that wrapper
//! execs this binary inside a bubblewrap sandbox. It takes no arguments.

mod auth;
mod i18n;
mod layer_guard;
mod power;
mod sandbox;
mod seal;
mod ui;

use gtk4::prelude::*;
use gtk4::Application;

const APP_ID: &str = "os.athanor.Greeter";

/// The renderer the greeter has always shipped with.
const GREETER_RENDERER: &str = "ngl";

/// What to write into `GSK_RENDERER`, or `None` when the environment already chose.
///
/// GTK 4.18 probes Vulkan first. The old greeter pinned NGL instead
/// (`athanor-shell-rs-1.0.0/src/main.rs` at 458d2584) and the extraction dropped the
/// line, so the login screen silently changed renderer. On a driver whose Vulkan loads
/// but draws wrongly -- the NVK/nouveau path this project ships by default, or llvmpipe
/// in the GPU-less acceptance VM -- the greeter draws black and the machine cannot be
/// logged into except from a text console. Keep the renderer that has been seen to work
/// until an ISO acceptance run has proved GTK's own default on real hardware. An
/// operator who exports `GSK_RENDERER` wins: this only fills in a choice nobody made.
fn renderer_to_set(current: Option<&str>) -> Option<&'static str> {
    match current {
        Some(value) if !value.is_empty() => None,
        _ => Some(GREETER_RENDERER),
    }
}

fn main() -> glib::ExitCode {
    // Confinement comes first, while this is the only thread in the process. Landlock
    // restricts the calling thread and the threads it creates from then on; a thread
    // that already exists -- a Tokio worker, a GLib worker -- would keep running
    // unconfined. Nothing before this line may start a thread, and the check fails
    // closed if something did. Logging is not set up yet, so errors go to stderr.
    // Refusing core dumps comes straight after, before anything can abort with a
    // password in the heap.
    if let Err(err) = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::apply_landlock_sandbox())
        .and_then(|()| sandbox::forbid_core_dumps())
    {
        eprintln!(
            "athanor-greeter-ui: cannot confine the process, refusing to run unconfined: {err}"
        );
        return glib::ExitCode::FAILURE;
    }

    // Before GTK initialises, and while this is still the only thread: setting an
    // environment variable is not thread-safe, and the Tokio runtime below starts a
    // worker.
    let current_renderer = std::env::var("GSK_RENDERER").ok();
    if let Some(renderer) = renderer_to_set(current_renderer.as_deref()) {
        std::env::set_var("GSK_RENDERER", renderer);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    i18n::init();

    // The workspace builds zbus on Tokio, and the greetd conversation sleeps on a Tokio
    // timer: both need a runtime that is current on this thread, or they abort with
    // "there is no reactor running" (acceptance run 34384585109). One worker is enough
    // for three logind calls.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(error = %err, "cannot create the Tokio runtime");
            return glib::ExitCode::FAILURE;
        }
    };
    let _tokio = runtime.enter();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_ui);
    app.run_with_args(&Vec::<String>::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_renderer_is_pinned_only_when_the_environment_left_it_open() {
        assert_eq!(renderer_to_set(None), Some(GREETER_RENDERER));
        // An empty value picks no renderer either: GTK would fall back to its probe.
        assert_eq!(renderer_to_set(Some("")), Some(GREETER_RENDERER));
        assert_eq!(renderer_to_set(Some("vulkan")), None);
        assert_eq!(renderer_to_set(Some(GREETER_RENDERER)), None);
    }
}
