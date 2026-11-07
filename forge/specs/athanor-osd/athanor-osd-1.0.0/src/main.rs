//! athanor-osd: the on-screen display of doc_osd.md, step 1 of OD17. It serves the volume,
//! mute and microphone keys on `os.athanor.Osd1`, carries each out through the audio model of
//! athanor-services and shows it on a Sal layer surface.
//!
//! Two threads of its own, both started after Landlock: Tokio's, which serves the bus and runs
//! the audio model and the keys' core, and the toolkit's loop on the main thread, with GLib's
//! main context in it. The core hands what to show to the main thread over a channel, which a
//! task of GLib's context drains into the surfaces' queue. Both of the overlay's channels are
//! bounded: the bus drops a key the core's queue cannot hold (`bus::KEYS`), and the core waits
//! for room in the surfaces' channel ([`VIEWS`]), so a stalled main thread holds the keys at
//! the bus rather than a growing queue.

mod surface;

use std::cell::RefCell;
use std::env;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_osd::logic::{Core, Effect, Key};
use athanor_osd::{bus, confine, i18n};
use athanor_sal::driver;
use athanor_services::audio::{self, AudioCommand, AudioState};
use athanor_sulphur_client::glib_loop::GlibSource;
use athanor_sulphur_client::Client;
use athanor_unit::dirs::Dirs;
use athanor_unit::{journal, sandbox};
use tokio::sync::{mpsc, watch};
use wayland_client::Connection;

use surface::{Input, Osd};

/// CC5's opt-in to amplify above 100 %. It has no store yet (doc_control_center.md CC5), so
/// the keys stop at 100 %.
const AMPLIFY: bool = false;
/// What the core may hand the surfaces ahead of the main thread: past it, the core waits.
const VIEWS: usize = 16;

fn main() -> ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-osd", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the cache");
        return ExitCode::FAILURE;
    };
    let grants = confine::overlay(&dirs, |name| env::var_os(name));
    if let Err(error) = sandbox::ensure_single_threaded().and_then(|()| grants.apply()) {
        tracing::error!(%error, "cannot confine the overlay with Landlock; refusing to run unconfined");
        return ExitCode::FAILURE;
    }
    i18n::init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("athanor-osd-bus")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "cannot start the Tokio runtime");
            return ExitCode::FAILURE;
        }
    };
    let (views, mut view_rx) = mpsc::channel::<Input>(VIEWS);
    runtime.spawn(async move {
        let (state, commands) = audio::spawn(&tokio::runtime::Handle::current());
        let (keys, key_rx) = mpsc::channel(bus::KEYS);
        // Held for the life of the service: dropping them releases the name.
        let _connections = match bus::serve(keys).await {
            Ok(connections) => connections,
            Err(error) => {
                let error = format!("cannot serve {} on the session bus: {error}", bus::NAME);
                if views.send(Input::Fail(error)).await.is_err() {
                    tracing::error!("the overlay's loop ended before the bus failed");
                }
                return;
            }
        };
        drive(Core::new(AMPLIFY), key_rx, state, commands, &views).await;
        // Without its core the overlay would take no key: it ends, and systemd restarts it.
        if views.send(Input::Fail("the keys' core stopped".into())).await.is_err() {
            tracing::error!("the overlay's loop ended before its core");
        }
    });

    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(error) => {
            tracing::error!(%error, "cannot run GLib's main context");
            return ExitCode::FAILURE;
        }
    };
    let inputs: Rc<RefCell<Vec<Input>>> = Rc::default();
    {
        let inputs = inputs.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Some(input) = view_rx.recv().await {
                inputs.borrow_mut().push(input);
            }
        });
    }
    let client = Connection::connect_to_env()
        .map_err(|error| error.to_string())
        .and_then(|connection| {
            Client::connect_wayland(&connection).map_err(|error| error.to_string())
        });
    let client = match client {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::warn!(%error, "no compositor client; the overlay shows on the first output");
            None
        }
    };
    let result = driver::run_with(Osd::new(inputs, client), glib);
    runtime.shutdown_timeout(Duration::from_secs(1));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the overlay stopped");
            ExitCode::FAILURE
        }
    }
}

/// The keys' core: each key against the model's state, the model's changes, and the clock of
/// the timeout and the announcement. Ends when the bus, the model or the surfaces do.
async fn drive(
    mut core: Core,
    mut keys: mpsc::Receiver<Key>,
    mut state: watch::Receiver<AudioState>,
    commands: mpsc::UnboundedSender<AudioCommand>,
    views: &mpsc::Sender<Input>,
) {
    loop {
        let deadline = core.deadline();
        // Never reached while `deadline` is `None`: the branch is off then.
        let wake = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
        let effects = tokio::select! {
            key = keys.recv() => match key {
                Some(key) => {
                    let seen = state.borrow().clone();
                    core.key(key, &seen, Instant::now())
                }
                None => {
                    tracing::error!("the bus stopped; the overlay takes no more keys");
                    return;
                }
            },
            changed = state.changed() => match changed {
                Ok(()) => {
                    let seen = state.borrow_and_update().clone();
                    core.observe(&seen)
                }
                Err(_) => {
                    tracing::error!("the audio model stopped");
                    return;
                }
            },
            () = tokio::time::sleep_until(wake.into()), if deadline.is_some() => {
                let seen = state.borrow().clone();
                core.tick(&seen, Instant::now())
            }
        };
        for effect in effects {
            match effect {
                Effect::Command(command) => {
                    if commands.send(command).is_err() {
                        tracing::error!("the audio model stopped; the key is not carried out");
                    }
                }
                Effect::View(view) => {
                    if views.send(Input::View(view)).await.is_err() {
                        tracing::error!("the overlay's surfaces are gone");
                        return;
                    }
                }
            }
        }
    }
}
