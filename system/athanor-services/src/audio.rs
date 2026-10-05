//! The sound server (doc_bar.md, BR3; doc_control_center.md, CC3): output and input volumes,
//! mutes and the default devices, as the audio module shows them. `pipewire-pulse` serves the
//! PulseAudio protocol; libpulse's threaded main loop runs it on a thread of its own, so no
//! call on the sound server ever waits on a Tokio worker or on the thread that draws.
//!
//! The thread owns the context. A libpulse callback only copies data out: into the state, or
//! as an [`Event`] for the thread. Commands and introspection run under the main loop's lock.
//! A lost server publishes the empty state, and the thread connects again with [`backoff`];
//! while the server is healthy nothing retries.

use std::collections::HashMap;
use std::sync::mpsc::{self as events, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use athanor_unit::text::{line, NAME_CHARS};
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::{properties, Proplist};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

/// The sound server's 100 %, `PA_VOLUME_NORM`.
pub const NORMAL: u32 = 0x10000;
pub const MAX_DEVICES: usize = 16;

/// A volume as a percentage. Above 100 % the slider shows 100 %: the bar does not amplify.
pub fn percent(raw: u32) -> f64 {
    (f64::from(raw) * 100.0 / f64::from(NORMAL)).clamp(0.0, 100.0)
}

pub fn raw(percent: f64) -> u32 {
    let percent = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    // At most NORMAL, so the cast cannot truncate.
    (percent * f64::from(NORMAL) / 100.0).round() as u32
}

pub fn output_icon(percent: f64, muted: bool) -> &'static str {
    match percent {
        _ if muted || percent <= 0.0 => "audio-volume-muted-symbolic",
        p if p < 34.0 => "audio-volume-low-symbolic",
        p if p < 67.0 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

pub fn input_icon(muted: bool) -> &'static str {
    if muted {
        "microphone-disabled-symbolic"
    } else {
        "audio-input-microphone-symbolic"
    }
}

/// The description when there is a usable one, else the device's name. Descriptions come from
/// other processes: they are sanitised here.
pub fn device_label(description: Option<&str>, name: &str) -> String {
    description
        .map(|text| line(text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| line(name, NAME_CHARS))
}

/// One output or input device.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The sound server's name, to address it.
    pub name: String,
    pub label: String,
    pub percent: f64,
    pub muted: bool,
}

/// The device the module controls: the default one, else the first.
pub fn chosen(devices: &[Device], default: Option<&str>) -> Option<usize> {
    default
        .and_then(|name| devices.iter().position(|device| device.name == name))
        .or_else(|| (!devices.is_empty()).then_some(0))
}

/// What the audio module shows. Empty while the server is away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioState {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
    /// Counts the commands the sound server refused or could not take: the state is published
    /// again after each, so a reader that sees this change knows its last action did not
    /// complete. Beyond the plan's fields, so the bar keeps its "action did not complete" note.
    pub refused: u32,
    /// The server was reached and listed, or found away: the module shows what it will keep
    /// showing. Beyond the plan's fields, as in the other models.
    pub settled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioCommand {
    /// `percent` of the server's 100 %, from 0 to 100; the balance between channels is kept.
    Volume {
        sink: bool,
        name: String,
        percent: u32,
    },
    Mute {
        sink: bool,
        name: String,
        on: bool,
    },
    Default {
        sink: bool,
        name: String,
    },
}

pub use crate::runtime::backoff;

/// What wakes the audio thread.
enum Event {
    Command(AudioCommand),
    /// The context's state changed.
    Context,
    /// A sink, a source or the server changed, or a refresh was asked for while one ran.
    Changed,
    Quit,
}

/// What the four introspection calls of one refresh gather.
#[derive(Default)]
struct Gathering {
    active: bool,
    /// An event arrived during the refresh: refresh again once it ends.
    stale: bool,
    remaining: u8,
    outputs: Vec<Device>,
    inputs: Vec<Device>,
    default_output: Option<String>,
    default_input: Option<String>,
    volumes: HashMap<(bool, String), ChannelVolumes>,
}

struct Shared {
    state: watch::Sender<AudioState>,
    gathering: Mutex<Gathering>,
    /// The volumes of the last refresh, to scale for a command.
    volumes: Mutex<HashMap<(bool, String), ChannelVolumes>>,
    events: events::Sender<Event>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A callback cannot panic while holding it (`panic = "abort"`); the data stays valid.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn refuse(&self, what: &str) {
        tracing::warn!("the sound server refused an action: {what}");
        self.state.send_modify(|state| state.refused += 1);
    }

    /// The server is away: the module hides, and what was gathered is forgotten.
    fn publish_empty(&self) {
        *locked(&self.gathering) = Gathering::default();
        locked(&self.volumes).clear();
        self.state.send_if_modified(|state| {
            let next = AudioState {
                refused: state.refused,
                settled: true,
                ..AudioState::default()
            };
            let changed = *state != next;
            *state = next;
            changed
        });
    }

    /// One of the four calls ended; the last one publishes.
    fn finished_one(&self) {
        let mut gathering = locked(&self.gathering);
        gathering.remaining = gathering.remaining.saturating_sub(1);
        if gathering.remaining > 0 {
            return;
        }
        let done = std::mem::take(&mut *gathering);
        drop(gathering);
        *locked(&self.volumes) = done.volumes;
        self.state.send_if_modified(|state| {
            let next = AudioState {
                outputs: done.outputs,
                inputs: done.inputs,
                default_output: done.default_output,
                default_input: done.default_input,
                refused: state.refused,
                settled: true,
            };
            let changed = *state != next;
            *state = next;
            changed
        });
        if done.stale {
            self.events.send(Event::Changed).ok();
        }
    }

    fn completion(self: &Arc<Self>, what: &'static str) -> Box<dyn FnMut(bool)> {
        let shared = self.clone();
        Box::new(move |ok| {
            if !ok {
                shared.refuse(what);
            }
        })
    }
}

fn device(name: &str, description: Option<&str>, volume: &ChannelVolumes, muted: bool) -> Device {
    Device {
        name: name.to_owned(),
        label: device_label(description, name),
        percent: percent(volume.max().0),
        muted,
    }
}

/// Reads the default sink and source, the sinks and the sources; events during a refresh
/// coalesce into one more. The defaults come through the special names, whose callbacks end
/// with `ListResult::Error` when there is none. Call it on a ready context, under the lock.
fn refresh(shared: &Arc<Shared>, context: &Context) {
    {
        let mut gathering = locked(&shared.gathering);
        if gathering.active {
            gathering.stale = true;
            return;
        }
        *gathering = Gathering {
            active: true,
            remaining: 4,
            ..Gathering::default()
        };
    }
    let introspect = context.introspect();
    let at = shared.clone();
    introspect.get_sink_info_by_name("@DEFAULT_SINK@", move |result| match result {
        ListResult::Item(info) => {
            locked(&at.gathering).default_output = info.name.as_deref().map(str::to_owned);
        }
        ListResult::End | ListResult::Error => at.finished_one(),
    });
    let at = shared.clone();
    introspect.get_source_info_by_name("@DEFAULT_SOURCE@", move |result| match result {
        ListResult::Item(info) => {
            locked(&at.gathering).default_input = info.name.as_deref().map(str::to_owned);
        }
        ListResult::End | ListResult::Error => at.finished_one(),
    });
    let at = shared.clone();
    introspect.get_sink_info_list(move |result| match result {
        ListResult::Item(info) => {
            if let Some(name) = info.name.as_deref() {
                let mut gathering = locked(&at.gathering);
                if gathering.outputs.len() < MAX_DEVICES {
                    gathering.outputs.push(device(
                        name,
                        info.description.as_deref(),
                        &info.volume,
                        info.mute,
                    ));
                    gathering
                        .volumes
                        .insert((true, name.to_owned()), info.volume);
                }
            }
        }
        ListResult::End | ListResult::Error => at.finished_one(),
    });
    let at = shared.clone();
    introspect.get_source_info_list(move |result| match result {
        // A monitor is a sink's loopback, not a microphone.
        ListResult::Item(info) if info.monitor_of_sink.is_none() => {
            if let Some(name) = info.name.as_deref() {
                let mut gathering = locked(&at.gathering);
                if gathering.inputs.len() < MAX_DEVICES {
                    gathering.inputs.push(device(
                        name,
                        info.description.as_deref(),
                        &info.volume,
                        info.mute,
                    ));
                    gathering
                        .volumes
                        .insert((false, name.to_owned()), info.volume);
                }
            }
        }
        ListResult::Item(_) => {}
        ListResult::End | ListResult::Error => at.finished_one(),
    });
}

/// Carries out `command` on a ready context, under the lock. A refusal is counted, either now
/// or when the server answers.
fn execute(shared: &Arc<Shared>, context: &mut Context, command: AudioCommand) {
    match command {
        AudioCommand::Volume {
            sink,
            name,
            percent,
        } => {
            let known = locked(&shared.volumes).get(&(sink, name.clone())).copied();
            // `scale` keeps the balance between channels; an invalid volume would make libpulse
            // return a null operation.
            let Some(mut volume) = known.filter(ChannelVolumes::is_valid) else {
                return shared.refuse("a volume for a device that is not known");
            };
            if volume.scale(Volume(raw(f64::from(percent)))).is_none() {
                return shared.refuse("a volume libpulse cannot scale");
            }
            let mut introspect = context.introspect();
            let done = Some(shared.completion("volume"));
            if sink {
                introspect.set_sink_volume_by_name(&name, &volume, done);
            } else {
                introspect.set_source_volume_by_name(&name, &volume, done);
            }
        }
        AudioCommand::Mute { sink, name, on } => {
            let mut introspect = context.introspect();
            let done = Some(shared.completion("mute"));
            if sink {
                introspect.set_sink_mute_by_name(&name, on, done);
            } else {
                introspect.set_source_mute_by_name(&name, on, done);
            }
        }
        AudioCommand::Default { sink, name } => {
            let done = shared.completion("default device");
            if sink {
                context.set_default_sink(&name, done);
            } else {
                context.set_default_source(&name, done);
            }
        }
    }
}

/// How a connection ended.
enum Outcome {
    Quit,
    /// The server was never there, or went away; `ready` tells whether it had answered.
    Lost {
        ready: bool,
    },
}

/// One connection, from the first attempt to its loss.
fn session(inbox: &events::Receiver<Event>, shared: &Arc<Shared>) -> Outcome {
    let Some(mut mainloop) = Mainloop::new() else {
        tracing::error!("libpulse has no threaded main loop; the audio module is hidden");
        return Outcome::Lost { ready: false };
    };
    if let Err(err) = mainloop.start() {
        tracing::error!(error = %err, "libpulse's main loop does not start");
        return Outcome::Lost { ready: false };
    }
    mainloop.lock();
    let context = connect(&mainloop, shared);
    mainloop.unlock();
    let Some(mut context) = context else {
        mainloop.stop();
        return Outcome::Lost { ready: false };
    };
    let mut ready = false;
    let outcome = loop {
        let Ok(event) = inbox.recv() else {
            break Outcome::Quit;
        };
        mainloop.lock();
        let lost = match event {
            Event::Quit => {
                mainloop.unlock();
                break Outcome::Quit;
            }
            Event::Context => match context.get_state() {
                State::Ready if !ready => {
                    ready = true;
                    // No autospawn, and the session starts the server: nothing here
                    // waits on its answer.
                    context.subscribe(
                        InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
                        |_| {},
                    );
                    refresh(shared, &context);
                    false
                }
                State::Failed | State::Terminated => true,
                _ => false,
            },
            Event::Changed => {
                if ready {
                    refresh(shared, &context);
                }
                false
            }
            Event::Command(command) => {
                if ready {
                    execute(shared, &mut context, command);
                } else {
                    shared.refuse("the sound server is away");
                }
                false
            }
        };
        mainloop.unlock();
        if lost {
            break Outcome::Lost { ready };
        }
    };
    // The callbacks go first: they hold the shared state, and none may run on a context
    // that is being closed.
    mainloop.lock();
    context.set_state_callback(None);
    context.set_subscribe_callback(None);
    context.disconnect();
    mainloop.unlock();
    mainloop.stop();
    outcome
}

/// A context connecting to the server, with its callbacks set; under the lock.
fn connect(mainloop: &Mainloop, shared: &Arc<Shared>) -> Option<Context> {
    let mut proplist = Proplist::new()?;
    if proplist
        .set_str(properties::APPLICATION_NAME, "athanor-bar")
        .is_err()
        || proplist
            .set_str(properties::APPLICATION_ID, "os.athanor.Bar")
            .is_err()
    {
        tracing::error!("libpulse refused the bar's properties");
    }
    let mut context = Context::new_with_proplist(mainloop, "athanor-bar", &proplist)?;
    let events = shared.events.clone();
    context.set_state_callback(Some(Box::new(move || {
        events.send(Event::Context).ok();
    })));
    let events = shared.events.clone();
    context.set_subscribe_callback(Some(Box::new(move |facility, _, _| {
        if matches!(
            facility,
            Some(Facility::Sink | Facility::Source | Facility::Server)
        ) {
            events.send(Event::Changed).ok();
        }
    })));
    // No autospawn: the session starts the sound server, never the bar.
    if let Err(err) = context.connect(None, FlagSet::NOAUTOSPAWN, None) {
        tracing::info!(error = %err, "no sound server yet");
        return None;
    }
    Some(context)
}

/// Connects, serves and connects again, until told to quit.
fn run(inbox: events::Receiver<Event>, shared: Arc<Shared>) {
    let mut attempt = 0u32;
    loop {
        match session(&inbox, &shared) {
            Outcome::Quit => return,
            Outcome::Lost { ready } => {
                if ready {
                    tracing::info!(
                        "the sound server went away; the audio module is hidden until it returns"
                    );
                    attempt = 0;
                }
            }
        }
        shared.publish_empty();
        let until = Instant::now() + backoff(attempt);
        attempt = attempt.saturating_add(1);
        // The wait is no sleep: a command is answered, and a quit is heard, at once.
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match inbox.recv_timeout(left) {
                Ok(Event::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Event::Command(_)) => shared.refuse("the sound server is away"),
                Ok(Event::Context | Event::Changed) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

/// However the command forwarder ends, the audio thread is told to quit: a dropped task
/// (the runtime shutting down) included. No handle is kept, so nothing can look for an
/// ambient runtime.
struct ExitGuard(events::Sender<Event>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.0.send(Event::Quit).ok();
    }
}

/// Starts the model: a thread of libpulse's own for the sound server, and a task on `handle`'s
/// runtime that hands it the commands. The receiver ends when the model does; without a
/// sound server the state stays empty and settled, and the thread tries again.
pub fn spawn(
    handle: &Handle,
) -> (
    watch::Receiver<AudioState>,
    mpsc::UnboundedSender<AudioCommand>,
) {
    let (state, rx) = watch::channel(AudioState::default());
    let (commands, mut command_rx) = mpsc::unbounded_channel();
    let (tx, inbox) = events::channel();
    let shared = Arc::new(Shared {
        state,
        gathering: Mutex::new(Gathering::default()),
        volumes: Mutex::new(HashMap::new()),
        events: tx.clone(),
    });
    if let Err(err) = thread::Builder::new()
        .name("athanor-audio".to_owned())
        .spawn(move || run(inbox, shared))
    {
        tracing::error!(error = %err, "cannot start the audio thread; the audio module is hidden");
        return (rx, commands);
    }
    handle.spawn(async move {
        let guard = ExitGuard(tx);
        while let Some(command) = command_rx.recv().await {
            if guard.0.send(Event::Command(command)).is_err() {
                return;
            }
        }
    });
    (rx, commands)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::runtime::Handle;

    use super::*;

    #[test]
    fn volumes_round_trip_and_are_bounded() {
        assert_eq!(percent(NORMAL), 100.0);
        assert_eq!(percent(NORMAL / 2), 50.0);
        assert_eq!(percent(NORMAL * 3 / 2), 100.0, "amplified shows as 100");
        assert_eq!(raw(100.0), NORMAL);
        assert_eq!(raw(150.0), NORMAL);
        assert_eq!(raw(-3.0), 0);
        assert_eq!(raw(f64::NAN), 0);
        for p in [0.0, 1.0, 33.0, 50.0, 99.0] {
            assert!((percent(raw(p)) - p).abs() < 0.01, "{p}");
        }
    }

    #[test]
    fn icons_follow_the_level_and_the_mute() {
        assert_eq!(output_icon(80.0, true), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(10.0, false), "audio-volume-low-symbolic");
        assert_eq!(output_icon(50.0, false), "audio-volume-medium-symbolic");
        assert_eq!(output_icon(100.0, false), "audio-volume-high-symbolic");
        assert_eq!(input_icon(true), "microphone-disabled-symbolic");
    }

    #[test]
    fn the_default_device_is_chosen_else_the_first() {
        let device = |name: &str| Device {
            name: name.into(),
            label: name.into(),
            percent: 0.0,
            muted: false,
        };
        let devices = [device("a"), device("b")];
        assert_eq!(chosen(&devices, Some("b")), Some(1));
        assert_eq!(chosen(&devices, Some("gone")), Some(0));
        assert_eq!(chosen(&[], Some("b")), None);
        assert_eq!(
            device_label(Some("\u{202e}Speakers\n"), "sink.0"),
            "Speakers"
        );
        assert_eq!(device_label(Some(" "), "sink.0"), "sink.0");
    }

    #[test]
    fn the_reconnect_backoff_doubles_from_one_second_to_thirty() {
        let secs: Vec<u64> = (0..8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff(u32::MAX), Duration::from_secs(30), "no overflow");
    }

    /// With no server to reach, the model settles on the empty state, refuses what it is
    /// asked and keeps its thread: nothing here may panic or block.
    #[tokio::test(flavor = "current_thread")]
    async fn with_no_sound_server_the_state_is_empty_and_a_command_is_refused() {
        // Only this test reads it; a socket that does not exist fails the connection at once.
        std::env::set_var("PULSE_SERVER", "unix:/nonexistent/athanor-test-pulse");
        let (mut states, commands) = spawn(&Handle::current());
        let settled = tokio::time::timeout(
            Duration::from_secs(10),
            states.wait_for(|state| state.settled),
        )
        .await
        .expect("settles")
        .expect("model alive")
        .clone();
        assert_eq!(
            settled,
            AudioState {
                settled: true,
                ..AudioState::default()
            }
        );
        commands
            .send(AudioCommand::Mute {
                sink: true,
                name: "x".into(),
                on: true,
            })
            .expect("send");
        let refused = tokio::time::timeout(
            Duration::from_secs(10),
            states.wait_for(|state| state.refused == 1),
        )
        .await
        .expect("refused")
        .expect("model alive")
        .clone();
        assert!(refused.outputs.is_empty() && refused.inputs.is_empty());
    }
}
