//! What a key does and what the overlay shows (doc_osd.md OD2): the steps and their bounds
//! (OD6), the pending target that quick presses build on, the show rule and the timeout (OD5,
//! OD10), the coalesced announcement (OD11) and the rate of the bus (OD4). No Sal, Wayland or
//! D-Bus type: every rule here is tested without a display. The volume keys act through the
//! audio model, the brightness keys through the battery model (OD17 step 2).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use athanor_services::audio::{self, AudioCommand, AudioState, Device};
use athanor_services::battery::{Backlight, BatteryCommand, BatteryState};

use crate::bus;
use crate::i18n::{tr, tr_with};

/// A volume key moves the volume by this much (OD6).
pub const STEP: f64 = 5.0;
/// The highest volume a key sets without CC5's opt-in.
pub const CEILING: f64 = 100.0;
/// The highest volume a key sets with CC5's opt-in on (doc_control_center.md CC5).
pub const AMPLIFIED_CEILING: f64 = audio::MAX_PERCENT;
/// The overlay hides this long after the last key (OD10).
pub const TIMEOUT: Duration = Duration::from_secs(2);
/// A run of keys is announced once, this long after the last (OD11).
pub const ANNOUNCE_AFTER: Duration = Duration::from_millis(300);
/// How long a command's target stands for the model's state, which has not caught up with it
/// yet (OD6, "each key acts on the state it sees").
pub const PENDING_FOR: Duration = Duration::from_secs(1);
/// How long a key waits for the audio model's first settled state, or a brightness key for the
/// battery model's first state, before it acts on the state as it is (OD13). The bound runs from the first key of a batch of held keys, not per restart.
/// The unit restarts after 100 ms and the next key within 1 s of a kill shows its overlay
/// (ST5), so the wait leaves about 400 ms for the start itself.
pub const SETTLE_WAIT: Duration = Duration::from_millis(500);
/// More calls than this within [`RATE_WINDOW`] from one sender are dropped (OD4).
pub const RATE_LIMIT: u32 = 30;
pub const RATE_WINDOW: Duration = Duration::from_secs(1);

/// The screen's backlight moves by this share of its maximum, never less than one raw step
/// (OD6).
pub const BRIGHTNESS_STEP: u32 = 5;
/// The icons of the two backlights, as the control center's brightness slider uses the first.
pub const SCREEN_ICON: &str = "display-brightness-symbolic";
pub const KEYBOARD_ICON: &str = "keyboard-brightness-symbolic";

/// A method of `os.athanor.Osd1` that steps 1 and 2 serve (OD4, OD17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleMicMute,
    BrightnessUp,
    BrightnessDown,
    KeyboardBrightnessUp,
    KeyboardBrightnessDown,
    CycleKeyboardBrightness,
}

/// The device a level shows: the default sink, the default source, the screen's backlight or
/// the keyboard's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Output,
    Input,
    Screen,
    Keyboard,
}

/// A level on screen (OD9): what it shows, from the state the model confirmed.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub source: Source,
    /// From 0 to [`Level::max`].
    pub percent: f64,
    pub muted: bool,
    /// The end of the bar: 150 while CC5's opt-in is on or the volume is above 100, else 100.
    /// The part beyond 100 is the warning segment.
    pub max: f64,
    /// The bar's and the control center's icon for the same state (`audio::output_icon`,
    /// `audio::input_icon`).
    pub icon: &'static str,
}

impl Level {
    /// The number the overlay shows.
    pub fn number(&self) -> u32 {
        // At most `audio::MAX_PERCENT`, so the cast cannot truncate.
        self.percent.round().clamp(0.0, audio::MAX_PERCENT) as u32
    }

    /// The content in words (OD11): the announcement and the surface's name.
    pub fn text(&self) -> String {
        let number = self.number().to_string();
        match (self.source, self.muted) {
            (Source::Output, true) => tr("Muted"),
            (Source::Output, false) => tr_with("Volume {percent} percent", "percent", &number),
            (Source::Input, true) => tr("Microphone muted"),
            (Source::Input, false) => tr_with("Microphone {percent} percent", "percent", &number),
            (Source::Screen, _) => tr_with("Brightness {percent} percent", "percent", &number),
            (Source::Keyboard, _) => {
                tr_with("Keyboard brightness {percent} percent", "percent", &number)
            }
        }
    }
}

/// What the overlay's surface is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    /// Show this level, or update the one on screen in place (OD5).
    Show(Level),
    /// Leave, after the timeout.
    Hide,
    /// Announce this text at polite priority.
    Announce(String),
}

/// What [`Core`] asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Command(AudioCommand),
    Battery(BatteryCommand),
    View(View),
}

/// The value a command will set on a device, while the model has not reported it yet.
#[derive(Clone, Debug, PartialEq)]
struct Target {
    name: String,
    percent: f64,
    muted: bool,
    at: Instant,
}

/// The volume one step up from `percent`, never past `ceiling` and never lower than
/// `percent`: a key at or above the ceiling leaves the volume where it is (OD5 still shows it).
pub fn step_up(percent: f64, ceiling: f64) -> f64 {
    if percent >= ceiling {
        percent
    } else {
        (percent.round() + STEP).min(ceiling)
    }
}

/// The volume one step down from `percent`, never below 0.
pub fn step_down(percent: f64) -> f64 {
    (percent.round() - STEP).max(0.0)
}

/// The level a backlight command will set, while the model has not reported it yet.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    name: String,
    level: u32,
    at: Instant,
}

/// The screen's raw step: [`BRIGHTNESS_STEP`] % of `max`, rounded, and at least 1.
fn brightness_step(max: u32) -> u32 {
    let step = u64::from(max)
        .saturating_mul(u64::from(BRIGHTNESS_STEP))
        .saturating_add(50)
        / 100;
    u32::try_from(step).unwrap_or(u32::MAX).max(1)
}

/// The screen's level one step up from `level`, never past `max`.
pub fn brightness_up(level: u32, max: u32) -> u32 {
    level.saturating_add(brightness_step(max)).min(max)
}

/// The screen's level one step down from `level`, never below one raw step: the key never
/// turns the panel black, as the slider never does (`Backlight::raw`).
pub fn brightness_down(level: u32, max: u32) -> u32 {
    level.saturating_sub(brightness_step(max)).max(1).min(level)
}

/// The keyboard's level for `key` from `level`: one hardware level, and the cycle wraps from
/// the highest to off (OD6).
pub fn keyboard_level(key: Key, level: u32, max: u32) -> u32 {
    match key {
        Key::KeyboardBrightnessUp => level.saturating_add(1).min(max),
        Key::KeyboardBrightnessDown => level.saturating_sub(1),
        _ if level >= max => 0,
        _ => level.saturating_add(1),
    }
}

/// The keys' logic and the overlay's state, driven by the bus, the audio model and the clock.
#[derive(Debug)]
pub struct Core {
    /// CC5's opt-in, which lets the keys raise the volume to 150 %.
    amplify: bool,
    output: Option<Target>,
    input: Option<Target>,
    /// The model's count of refused commands when the targets were set.
    refused: u32,
    /// The battery model's last state, which the brightness keys act on.
    battery: BatteryState,
    screen: Option<Pending>,
    keyboard: Option<Pending>,
    /// The battery model's count of refused commands when the backlights' targets were set.
    battery_refused: u32,
    /// What is on screen, until the timeout.
    shown: Option<Source>,
    hide_at: Option<Instant>,
    announce_at: Option<Instant>,
    /// Keys that came before the model settled, replayed in order once it does.
    held: Vec<Key>,
    /// When the first held key stops waiting for the model.
    hold_until: Option<Instant>,
    /// The wait ended with the model still unsettled: no key waits again until it settles.
    gave_up: bool,
    /// The battery model published a state, or the wait for its first ended: the brightness
    /// keys no longer wait.
    battery_known: bool,
    /// Brightness keys that came before the battery model's first state, replayed in order
    /// once it comes.
    held_lights: Vec<Key>,
    /// When the first held brightness key stops waiting for the battery model.
    lights_until: Option<Instant>,
}

impl Core {
    pub fn new(amplify: bool) -> Self {
        Core {
            amplify,
            output: None,
            input: None,
            refused: 0,
            battery: BatteryState::default(),
            screen: None,
            keyboard: None,
            battery_refused: 0,
            shown: None,
            hide_at: None,
            announce_at: None,
            held: Vec::new(),
            hold_until: None,
            gave_up: false,
            battery_known: false,
            held_lights: Vec::new(),
            lights_until: None,
        }
    }

    fn ceiling(&self) -> f64 {
        if self.amplify {
            AMPLIFIED_CEILING
        } else {
            CEILING
        }
    }

    /// The pending volume of an audio source; `None` for a backlight.
    fn target(&mut self, source: Source) -> Option<&mut Option<Target>> {
        match source {
            Source::Output => Some(&mut self.output),
            Source::Input => Some(&mut self.input),
            Source::Screen | Source::Keyboard => None,
        }
    }

    /// The device `source` names in `state`, and its volume and mute as the next key sees
    /// them: the pending target, while it stands, else the model's.
    fn seen<'a>(
        &mut self,
        source: Source,
        state: &'a AudioState,
        now: Instant,
    ) -> Option<(&'a Device, f64, bool)> {
        let device = device(state, source)?;
        let target = self.target(source)?;
        if target
            .as_ref()
            .is_some_and(|t| t.name != device.name || now.duration_since(t.at) >= PENDING_FOR)
        {
            *target = None;
        }
        Some(match target {
            Some(t) => (device, t.percent, t.muted),
            None => (device, device.percent, device.muted),
        })
    }

    /// Whether keys wait for the model: it has not settled and [`SETTLE_WAIT`], counted from
    /// the first key held, has not ended. Once it has, no key waits until the model settles.
    fn holding(&mut self, state: &AudioState, now: Instant) -> bool {
        if state.settled {
            self.gave_up = false;
            return false;
        }
        if !self.gave_up && now < *self.hold_until.get_or_insert(now + SETTLE_WAIT) {
            return true;
        }
        self.gave_up = true;
        false
    }

    /// Replays the held keys, in order, against `state`.
    fn release(&mut self, state: &AudioState, now: Instant) -> Vec<Effect> {
        self.hold_until = None;
        std::mem::take(&mut self.held)
            .into_iter()
            .flat_map(|key| self.act(key, state, now))
            .collect()
    }

    /// A key: a brightness key held until the battery model's first state, a volume key
    /// while the audio model has not settled, each for at most [`SETTLE_WAIT`] (OD13), else
    /// carried out at once.
    pub fn key(&mut self, key: Key, state: &AudioState, now: Instant) -> Vec<Effect> {
        if matches!(
            key,
            Key::BrightnessUp
                | Key::BrightnessDown
                | Key::KeyboardBrightnessUp
                | Key::KeyboardBrightnessDown
                | Key::CycleKeyboardBrightness
        ) {
            // An instant past the clock's end waits for nothing.
            let until = now.checked_add(SETTLE_WAIT).unwrap_or(now);
            if !self.battery_known && now < *self.lights_until.get_or_insert(until) {
                if self.held_lights.len() < bus::KEYS {
                    self.held_lights.push(key);
                } else {
                    tracing::warn!(
                        ?key,
                        "too many keys wait for the battery state; one is dropped"
                    );
                }
                return Vec::new();
            }
            let mut effects = self.release_lights(state, now);
            effects.extend(self.light(key, state, now));
            return effects;
        }
        if self.holding(state, now) {
            if self.held.len() < bus::KEYS {
                self.held.push(key);
            } else {
                tracing::warn!(
                    ?key,
                    "too many keys wait for the audio state; one is dropped"
                );
            }
            return Vec::new();
        }
        let mut effects = self.release(state, now);
        effects.extend(self.act(key, state, now));
        effects
    }

    /// Replays the held brightness keys, in order, against the battery model's last state;
    /// from now on no brightness key waits.
    fn release_lights(&mut self, state: &AudioState, now: Instant) -> Vec<Effect> {
        self.battery_known = true;
        self.lights_until = None;
        std::mem::take(&mut self.held_lights)
            .into_iter()
            .flat_map(|key| self.light(key, state, now))
            .collect()
    }

    /// A key against a state: the commands it sends and the overlay it shows. No device to
    /// act on, as while the sound server is away, does nothing and shows nothing.
    fn act(&mut self, key: Key, state: &AudioState, now: Instant) -> Vec<Effect> {
        let source = match key {
            Key::ToggleMicMute => Source::Input,
            _ => Source::Output,
        };
        let ceiling = self.ceiling();
        let Some((device, percent, muted)) = self.seen(source, state, now) else {
            tracing::info!(?key, "no default device for the key; nothing is done");
            return Vec::new();
        };
        let sink = source == Source::Output;
        let (new_percent, new_muted) = match key {
            Key::VolumeUp => (step_up(percent, ceiling), false),
            Key::VolumeDown => (step_down(percent), muted),
            Key::ToggleMute | Key::ToggleMicMute => (percent, !muted),
            // `key` sends the brightness keys to `light` and never holds them.
            Key::BrightnessUp
            | Key::BrightnessDown
            | Key::KeyboardBrightnessUp
            | Key::KeyboardBrightnessDown
            | Key::CycleKeyboardBrightness => return Vec::new(),
        };
        let mut effects = Vec::new();
        if new_percent != percent {
            effects.push(Effect::Command(AudioCommand::Volume {
                sink,
                name: device.name.clone(),
                // At most `audio::MAX_PERCENT`, so the cast cannot truncate.
                percent: new_percent.round().clamp(0.0, audio::MAX_PERCENT) as u32,
            }));
        }
        if new_muted != muted {
            effects.push(Effect::Command(AudioCommand::Mute {
                sink,
                name: device.name.clone(),
                on: new_muted,
            }));
        }
        let name = device.name.clone();
        if !effects.is_empty() {
            self.refused = state.refused;
            if let Some(target) = self.target(source) {
                *target = Some(Target {
                    name,
                    percent: new_percent,
                    muted: new_muted,
                    at: now,
                });
            }
        }
        // A key always shows, at the ceiling too (OD5), the value the model confirmed.
        effects.extend(self.show(source, state, now));
        effects
    }

    /// Shows `source` until the timeout and announces it after the run of keys.
    fn show(&mut self, source: Source, state: &AudioState, now: Instant) -> Option<Effect> {
        self.shown = Some(source);
        self.hide_at = Some(now + TIMEOUT);
        self.announce_at = Some(now + ANNOUNCE_AFTER);
        self.level(state)
            .map(|level| Effect::View(View::Show(level)))
    }

    /// A brightness key against the battery model's last state. No backlight, as on a desktop
    /// without a keyboard backlight, does nothing and shows nothing, and logs nothing per key:
    /// the start said it once (doc_osd.md, section 9).
    fn light(&mut self, key: Key, state: &AudioState, now: Instant) -> Vec<Effect> {
        let screen = matches!(key, Key::BrightnessUp | Key::BrightnessDown);
        let (source, device, pending) = if screen {
            (
                Source::Screen,
                self.battery.backlight.as_ref(),
                &mut self.screen,
            )
        } else {
            (
                Source::Keyboard,
                self.battery.keyboard.as_ref(),
                &mut self.keyboard,
            )
        };
        let Some(Backlight { name, max, level }) = device.cloned() else {
            return Vec::new();
        };
        if pending
            .as_ref()
            .is_some_and(|p| p.name != name || now.duration_since(p.at) >= PENDING_FOR)
        {
            *pending = None;
        }
        let seen = pending.as_ref().map_or(level, |p| p.level);
        let next = match key {
            Key::BrightnessUp => brightness_up(seen, max),
            Key::BrightnessDown => brightness_down(seen, max),
            _ => keyboard_level(key, seen, max),
        };
        let mut effects = Vec::new();
        if next != seen {
            effects.push(Effect::Battery(if screen {
                BatteryCommand::SetBrightness {
                    device: name.clone(),
                    raw: next,
                }
            } else {
                BatteryCommand::SetKeyboardBrightness {
                    device: name.clone(),
                    level: next,
                }
            }));
            *pending = Some(Pending {
                name,
                level: next,
                at: now,
            });
            self.battery_refused = self.battery.refused;
        }
        // A key always shows, at either end too (OD5).
        effects.extend(self.show(source, state, now));
        effects
    }

    /// A new state of the battery model: the backlights' targets it reached or refused are
    /// dropped, and a change the model did not command shows (OD5, OD6). `audio` is the audio
    /// model's state, for a level of the volume that stays on screen.
    pub fn observe_battery(
        &mut self,
        battery: &BatteryState,
        audio: &AudioState,
        now: Instant,
    ) -> Vec<Effect> {
        let before = std::mem::replace(&mut self.battery, battery.clone());
        if battery.refused != self.battery_refused {
            self.battery_refused = battery.refused;
            self.screen = None;
            self.keyboard = None;
        }
        let mut effects = self.release_lights(audio, now);
        let reached = |pending: &Option<Pending>, device: &Option<Backlight>| {
            pending.as_ref().is_some_and(|p| {
                device
                    .as_ref()
                    .is_some_and(|d| d.name == p.name && d.level == p.level)
            })
        };
        if reached(&self.screen, &battery.backlight) {
            self.screen = None;
        }
        if reached(&self.keyboard, &battery.keyboard) {
            self.keyboard = None;
        }
        let mut changed = None;
        if battery.backlight_changes != before.backlight_changes && battery.backlight.is_some() {
            changed = Some(Source::Screen);
        }
        if battery.keyboard_changes != before.keyboard_changes && battery.keyboard.is_some() {
            changed = Some(Source::Keyboard);
        }
        if let Some(source) = changed {
            effects.extend(self.show(source, audio, now));
        } else if matches!(self.shown, Some(Source::Screen | Source::Keyboard)) {
            effects.extend(
                self.level(audio)
                    .map(|level| Effect::View(View::Show(level))),
            );
        }
        effects
    }

    /// A new state of the model: the targets it reached or refused are dropped, and the level
    /// on screen follows it. A change nobody pressed a key for shows nothing (OD5).
    pub fn observe(&mut self, state: &AudioState, now: Instant) -> Vec<Effect> {
        if state.refused != self.refused {
            self.refused = state.refused;
            self.output = None;
            self.input = None;
        }
        if state.settled {
            self.gave_up = false;
        }
        let mut effects = if state.settled {
            self.release(state, now)
        } else {
            Vec::new()
        };
        for source in [Source::Output, Source::Input] {
            let device = device(state, source);
            if let Some(target) = self.target(source) {
                let reached = device.is_some_and(|device| {
                    target.as_ref().is_some_and(|t| {
                        t.name == device.name
                            && t.percent.round() == device.percent.round()
                            && t.muted == device.muted
                    })
                });
                if reached {
                    *target = None;
                }
            }
        }
        effects.extend(
            self.level(state)
                .map(|level| Effect::View(View::Show(level))),
        );
        effects
    }

    /// When [`Core::tick`] has something to do next; `None` while hidden and announced, so
    /// an idle overlay holds no timer (OD13).
    pub fn deadline(&self) -> Option<Instant> {
        let hold = self.hold_until.filter(|_| !self.held.is_empty());
        let lights = self.lights_until.filter(|_| !self.held_lights.is_empty());
        [self.hide_at, self.announce_at, hold, lights]
            .into_iter()
            .flatten()
            .min()
    }

    /// The clock reached `now`: the coalesced announcement, then the timeout.
    pub fn tick(&mut self, state: &AudioState, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if !self.held.is_empty() && self.hold_until.is_some_and(|at| at <= now) {
            tracing::warn!(
                keys = self.held.len(),
                "the audio state did not settle in time; the held keys act on it as it is"
            );
            self.gave_up = true;
            effects = self.release(state, now);
        }
        if !self.held_lights.is_empty() && self.lights_until.is_some_and(|at| at <= now) {
            tracing::warn!(
                keys = self.held_lights.len(),
                "the battery state did not come in time; the held brightness keys act on none"
            );
            effects.extend(self.release_lights(state, now));
        }
        if self.announce_at.is_some_and(|at| at <= now) {
            self.announce_at = None;
            if let Some(level) = self.level(state) {
                effects.push(Effect::View(View::Announce(level.text())));
            }
        }
        if self.hide_at.is_some_and(|at| at <= now) {
            self.hide_at = None;
            self.shown = None;
            effects.push(Effect::View(View::Hide));
        }
        effects
    }

    /// The level on screen, from `state` or the battery model's last state; `None` while
    /// hidden or once its device left.
    fn level(&self, state: &AudioState) -> Option<Level> {
        let source = self.shown?;
        let light = match source {
            Source::Screen => Some((self.battery.backlight.as_ref(), SCREEN_ICON)),
            Source::Keyboard => Some((self.battery.keyboard.as_ref(), KEYBOARD_ICON)),
            Source::Output | Source::Input => None,
        };
        if let Some((light, icon)) = light {
            let light = light?;
            return Some(Level {
                source,
                percent: light.percent().clamp(0.0, CEILING),
                muted: false,
                max: CEILING,
                icon,
            });
        }
        let device = device(state, source)?;
        let max = if self.amplify || device.percent > CEILING {
            AMPLIFIED_CEILING
        } else {
            CEILING
        };
        let icon = match source {
            Source::Output => audio::output_icon(device.percent, device.muted),
            _ => audio::input_icon(device.muted),
        };
        Some(Level {
            source,
            percent: device.percent.clamp(0.0, max),
            muted: device.muted,
            max,
            icon,
        })
    }
}

/// The default device of `source`, as the bar and the control center choose it.
fn device(state: &AudioState, source: Source) -> Option<&Device> {
    let (devices, default) = match source {
        Source::Output => (&state.outputs, state.default_output.as_deref()),
        Source::Input => (&state.inputs, state.default_input.as_deref()),
        Source::Screen | Source::Keyboard => return None,
    };
    audio::chosen(devices, default).map(|i| &devices[i])
}

/// Whether a call is taken (OD4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Yes,
    /// Dropped; `first` for the first drop of the window, the one that is logged.
    Dropped {
        first: bool,
    },
}

/// The bus's rate: at most [`RATE_LIMIT`] calls per sender in each window of [`RATE_WINDOW`]
/// that starts at the sender's first call (OD4).
#[derive(Debug, Default)]
pub struct Rate {
    senders: HashMap<String, (Instant, u32)>,
}

impl Rate {
    pub fn admit(&mut self, sender: &str, now: Instant) -> Admit {
        // Windows that ended are forgotten, so the map holds only the senders of the last
        // second.
        self.senders
            .retain(|_, (start, _)| now.duration_since(*start) < RATE_WINDOW);
        let (_, count) = self.senders.entry(sender.to_owned()).or_insert((now, 0));
        *count = count.saturating_add(1);
        match *count {
            n if n <= RATE_LIMIT => Admit::Yes,
            n => Admit::Dropped {
                first: n == RATE_LIMIT + 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, percent: f64, muted: bool) -> Device {
        Device {
            name: name.into(),
            label: name.into(),
            percent,
            muted,
        }
    }

    fn state(output: f64, muted: bool) -> AudioState {
        AudioState {
            outputs: vec![device("speakers", output, muted)],
            inputs: vec![device("mic", 80.0, false)],
            default_output: Some("speakers".into()),
            default_input: Some("mic".into()),
            settled: true,
            ..AudioState::default()
        }
    }

    fn commands(effects: &[Effect]) -> Vec<AudioCommand> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Command(c) => Some(c.clone()),
                Effect::Battery(_) | Effect::View(_) => None,
            })
            .collect()
    }

    fn volume(percent: u32) -> AudioCommand {
        AudioCommand::Volume {
            sink: true,
            name: "speakers".into(),
            percent,
        }
    }

    fn mute(on: bool) -> AudioCommand {
        AudioCommand::Mute {
            sink: true,
            name: "speakers".into(),
            on,
        }
    }

    #[test]
    fn steps_stop_at_the_ceiling_and_at_zero() {
        assert_eq!(step_up(45.0, CEILING), 50.0);
        assert_eq!(step_up(97.0, CEILING), 100.0);
        assert_eq!(step_up(100.0, CEILING), 100.0);
        assert_eq!(step_up(120.0, CEILING), 120.0, "never lowered by a key up");
        assert_eq!(step_up(100.0, AMPLIFIED_CEILING), 105.0);
        assert_eq!(step_up(148.0, AMPLIFIED_CEILING), 150.0);
        assert_eq!(step_up(150.0, AMPLIFIED_CEILING), 150.0);
        assert_eq!(step_up(44.999, CEILING), 50.0, "the model's rounding noise");
        assert_eq!(step_down(3.0), 0.0);
        assert_eq!(step_down(0.0), 0.0);
        assert_eq!(step_down(130.0), 125.0);
    }

    #[test]
    fn five_quick_presses_move_the_volume_by_25() {
        let mut core = Core::new(false);
        let now = Instant::now();
        let seen = state(40.0, false);
        let mut sent = Vec::new();
        for i in 0..5 {
            sent.extend(commands(&core.key(
                Key::VolumeUp,
                &seen,
                now + Duration::from_millis(50 * i),
            )));
        }
        assert_eq!(
            sent,
            [45, 50, 55, 60, 65].map(volume).to_vec(),
            "each press acts on the last target"
        );
    }

    #[test]
    fn a_target_the_model_reached_refused_or_outlived_is_dropped() {
        let now = Instant::now();
        // Reached, then changed by a mixer: the next key starts from the mixer's value.
        let mut core = Core::new(false);
        core.key(Key::VolumeUp, &state(40.0, false), now);
        core.observe(&state(45.0, false), Instant::now());
        core.observe(&state(20.0, false), Instant::now());
        let effects = core.key(Key::VolumeUp, &state(20.0, false), now);
        assert_eq!(commands(&effects), vec![volume(25)]);
        // Refused: the next key starts from what the model holds.
        let mut core = Core::new(false);
        core.key(Key::VolumeUp, &state(40.0, false), now);
        let refused = AudioState {
            refused: 1,
            ..state(40.0, false)
        };
        core.observe(&refused, Instant::now());
        assert_eq!(
            commands(&core.key(Key::VolumeUp, &refused, now)),
            vec![volume(45)]
        );
        // Outlived: a target older than PENDING_FOR no longer stands.
        let mut core = Core::new(false);
        core.key(Key::VolumeUp, &state(40.0, false), now);
        let later = now + PENDING_FOR;
        let effects = core.key(Key::VolumeUp, &state(40.0, false), later);
        assert_eq!(commands(&effects), vec![volume(45)]);
    }

    #[test]
    fn raising_a_muted_sink_unmutes_it_and_lowering_keeps_it_muted() {
        let now = Instant::now();
        let mut core = Core::new(false);
        assert_eq!(
            commands(&core.key(Key::VolumeUp, &state(40.0, true), now)),
            vec![volume(45), mute(false)]
        );
        let mut core = Core::new(false);
        assert_eq!(
            commands(&core.key(Key::VolumeDown, &state(40.0, true), now)),
            vec![volume(35)]
        );
        let mut core = Core::new(false);
        assert_eq!(
            commands(&core.key(Key::VolumeUp, &state(100.0, true), now)),
            vec![mute(false)],
            "at the ceiling only the mute changes"
        );
    }

    #[test]
    fn the_mute_keys_toggle_their_own_device_and_build_on_the_target() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let seen = state(40.0, false);
        assert_eq!(
            commands(&core.key(Key::ToggleMute, &seen, now)),
            vec![mute(true)]
        );
        assert_eq!(
            commands(&core.key(Key::ToggleMute, &seen, now)),
            vec![mute(false)]
        );
        assert_eq!(
            commands(&core.key(Key::ToggleMicMute, &seen, now)),
            vec![AudioCommand::Mute {
                sink: false,
                name: "mic".into(),
                on: true,
            }]
        );
    }

    #[test]
    fn a_key_always_shows_and_the_ceiling_follows_the_opt_in() {
        let now = Instant::now();
        for amplify in [false, true] {
            let mut core = Core::new(amplify);
            let seen = state(if amplify { 150.0 } else { 100.0 }, false);
            let effects = core.key(Key::VolumeUp, &seen, now);
            assert!(commands(&effects).is_empty(), "amplify {amplify}");
            let shown = effects.iter().find_map(|e| match e {
                Effect::View(View::Show(level)) => Some(level.clone()),
                _ => None,
            });
            let max = if amplify { 150.0 } else { 100.0 };
            assert_eq!(
                shown.map(|level| (level.percent, level.max)),
                Some((max, max)),
                "amplify {amplify}: the full bar at the ceiling"
            );
        }
        let mut core = Core::new(false);
        let effects = core.key(Key::VolumeDown, &state(130.0, false), now);
        assert!(
            effects.contains(&Effect::View(View::Show(Level {
                source: Source::Output,
                percent: 130.0,
                muted: false,
                max: 150.0,
                icon: "audio-volume-high-symbolic",
            }))),
            "a volume above 100 set elsewhere keeps its warning segment"
        );
    }

    #[test]
    fn without_a_device_a_key_does_nothing() {
        let mut core = Core::new(false);
        let empty = AudioState {
            settled: true,
            ..AudioState::default()
        };
        assert!(core.key(Key::VolumeUp, &empty, Instant::now()).is_empty());
        assert_eq!(core.deadline(), None);
    }

    #[test]
    fn a_change_shows_only_while_a_key_shows_the_overlay() {
        let now = Instant::now();
        let mut core = Core::new(false);
        assert!(
            core.observe(&state(30.0, false), Instant::now()).is_empty(),
            "a mixer's change"
        );
        core.key(Key::VolumeUp, &state(30.0, false), now);
        let effects = core.observe(&state(35.0, false), Instant::now());
        assert!(matches!(
            effects.as_slice(),
            [Effect::View(View::Show(Level { percent, .. }))] if *percent == 35.0
        ));
    }

    #[test]
    fn five_presses_are_announced_once_and_the_overlay_hides_after_the_last() {
        let start = Instant::now();
        let mut core = Core::new(false);
        let mut seen = state(40.0, false);
        let mut announced = Vec::new();
        let mut hidden = 0;
        for i in 0..5u64 {
            let at = start + Duration::from_millis(100 * i);
            core.key(Key::VolumeUp, &seen, at);
            seen = state(45.0 + 5.0 * i as f64, false);
            core.observe(&seen, Instant::now());
        }
        let last = start + Duration::from_millis(400);
        assert_eq!(core.deadline(), Some(last + ANNOUNCE_AFTER));
        let mut now = last;
        while let Some(at) = core.deadline() {
            assert!(at > now, "the deadline moves forward");
            now = at;
            for effect in core.tick(&seen, now) {
                match effect {
                    Effect::View(View::Announce(text)) => announced.push((text, now)),
                    Effect::View(View::Hide) => hidden += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
        assert_eq!(
            announced,
            vec![("Volume 65 percent".to_owned(), last + ANNOUNCE_AFTER)]
        );
        assert_eq!(hidden, 1);
        assert_eq!(now, last + TIMEOUT, "2 s after the last key");
        assert_eq!(core.deadline(), None, "hidden, no timer");
    }

    #[test]
    fn the_text_names_the_state() {
        let level = |source, percent, muted| Level {
            source,
            percent,
            muted,
            max: CEILING,
            icon: "",
        };
        assert_eq!(
            level(Source::Output, 44.6, false).text(),
            "Volume 45 percent"
        );
        assert_eq!(level(Source::Output, 44.6, true).text(), "Muted");
        assert_eq!(
            level(Source::Input, 80.0, false).text(),
            "Microphone 80 percent"
        );
        assert_eq!(level(Source::Input, 80.0, true).text(), "Microphone muted");
    }

    #[test]
    fn more_than_30_calls_a_second_are_dropped_with_one_line() {
        let start = Instant::now();
        let mut rate = Rate::default();
        let outcomes: Vec<Admit> = (0..32).map(|_| rate.admit(":1.5", start)).collect();
        assert!(outcomes[..30].iter().all(|a| *a == Admit::Yes));
        assert_eq!(outcomes[30], Admit::Dropped { first: true });
        assert_eq!(outcomes[31], Admit::Dropped { first: false });
        assert_eq!(rate.admit(":1.6", start), Admit::Yes, "per sender");
        assert_eq!(
            rate.admit(":1.5", start + RATE_WINDOW),
            Admit::Yes,
            "a new window"
        );
    }

    #[test]
    fn keys_before_the_model_settles_wait_and_replay_in_order() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let unsettled = AudioState::default();
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        assert!(core.deadline().is_some(), "the wait has a bound");
        let effects = core.observe(&state(40.0, false), now + Duration::from_millis(50));
        assert_eq!(commands(&effects), [45, 50].map(volume).to_vec());
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::View(View::Show(_)))));
    }

    #[test]
    fn held_keys_act_on_the_state_as_it_is_after_the_wait() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let unsettled = AudioState::default();
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        assert!(core.tick(&unsettled, now).is_empty(), "not yet");
        assert!(
            core.tick(&unsettled, now + SETTLE_WAIT).is_empty(),
            "no device: nothing"
        );
        assert!(core.held.is_empty());
        // The wait is over: a key that comes while the state is still unsettled does not
        // start a new one, until the model settles.
        let later = now + SETTLE_WAIT;
        assert!(core.key(Key::VolumeUp, &unsettled, later).is_empty());
        assert!(core.held.is_empty());
        assert_eq!(core.deadline(), None);
    }

    #[test]
    fn later_keys_do_not_extend_the_wait_from_the_first_key_held() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let unsettled = AudioState::default();
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        let mid = now + SETTLE_WAIT / 2;
        assert!(core.key(Key::VolumeUp, &unsettled, mid).is_empty());
        assert_eq!(core.deadline(), Some(now + SETTLE_WAIT));
        // The first key's bound passed: this key acts on the state as it is, and holds none.
        let end = now + SETTLE_WAIT;
        assert!(core.key(Key::VolumeUp, &unsettled, end).is_empty());
        assert!(core.held.is_empty());
        // Once the model settles, a later gap waits again.
        core.observe(&state(40.0, false), end);
        assert!(core.key(Key::VolumeUp, &unsettled, end).is_empty());
        assert_eq!(core.held, [Key::VolumeUp]);
    }

    #[test]
    fn no_more_than_the_bus_queue_waits() {
        let now = Instant::now();
        let mut core = Core::new(false);
        for _ in 0..bus::KEYS + 4 {
            core.key(Key::VolumeUp, &AudioState::default(), now);
        }
        assert_eq!(core.held.len(), bus::KEYS);
    }

    #[test]
    fn held_keys_act_on_an_unsettled_state_that_has_a_device() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let unsettled = AudioState {
            settled: false,
            ..state(40.0, false)
        };
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        assert!(core.key(Key::VolumeUp, &unsettled, now).is_empty());
        assert!(core.tick(&unsettled, now).is_empty(), "not yet");
        let effects = core.tick(&unsettled, now + SETTLE_WAIT);
        assert_eq!(commands(&effects), [45, 50].map(volume).to_vec());
        assert!(core.held.is_empty());
        assert_eq!(core.deadline(), core.hide_at.min(core.announce_at));
    }

    #[test]
    fn a_key_after_the_state_settled_goes_after_the_held_ones() {
        let now = Instant::now();
        let mut core = Core::new(false);
        assert!(core
            .key(Key::VolumeUp, &AudioState::default(), now)
            .is_empty());
        assert!(core
            .key(Key::VolumeUp, &AudioState::default(), now)
            .is_empty());
        // The state settled, but `observe` has not run: the new key replays the held ones first.
        let effects = core.key(Key::VolumeDown, &state(40.0, false), now);
        assert_eq!(
            commands(&effects),
            [45, 50, 45].map(volume).to_vec(),
            "two held ups, then the new down"
        );
        assert!(core.held.is_empty());
    }

    /// A laptop's battery state: the screen at `screen` of 100, the keyboard at `keyboard`
    /// of 2.
    fn lights(screen: u32, keyboard: u32) -> BatteryState {
        BatteryState {
            backlight: Some(Backlight {
                name: "intel_backlight".into(),
                max: 100,
                level: screen,
            }),
            keyboard: Some(Backlight {
                name: "tpacpi::kbd_backlight".into(),
                max: 2,
                level: keyboard,
            }),
            ..BatteryState::default()
        }
    }

    fn battery_commands(effects: &[Effect]) -> Vec<BatteryCommand> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Battery(c) => Some(c.clone()),
                Effect::Command(_) | Effect::View(_) => None,
            })
            .collect()
    }

    fn shown(effects: &[Effect]) -> Option<(Source, u32)> {
        effects.iter().find_map(|e| match e {
            Effect::View(View::Show(level)) => Some((level.source, level.number())),
            _ => None,
        })
    }

    fn screen(raw: u32) -> BatteryCommand {
        BatteryCommand::SetBrightness {
            device: "intel_backlight".into(),
            raw,
        }
    }

    #[test]
    fn brightness_steps_never_reach_zero_and_keyboard_levels_wrap() {
        assert_eq!(brightness_up(50, 100), 55);
        assert_eq!(brightness_up(98, 100), 100);
        assert_eq!(brightness_up(100, 100), 100);
        assert_eq!(brightness_down(50, 100), 45);
        assert_eq!(brightness_down(3, 100), 1, "never below one raw step");
        assert_eq!(brightness_down(1, 100), 1);
        assert_eq!(
            brightness_down(0, 100),
            0,
            "a key down never raises the level"
        );
        assert_eq!(brightness_up(5, 10), 6, "at least one raw step");
        assert_eq!(brightness_down(5, 10), 4);
        assert_eq!(brightness_up(0, 120_000), 6000);
        assert_eq!(brightness_up(u32::MAX - 1, u32::MAX), u32::MAX);
        assert_eq!(keyboard_level(Key::KeyboardBrightnessUp, 1, 2), 2);
        assert_eq!(keyboard_level(Key::KeyboardBrightnessUp, 2, 2), 2);
        assert_eq!(keyboard_level(Key::KeyboardBrightnessDown, 0, 2), 0);
        assert_eq!(keyboard_level(Key::CycleKeyboardBrightness, 1, 2), 2);
        assert_eq!(keyboard_level(Key::CycleKeyboardBrightness, 2, 2), 0);
    }

    #[test]
    fn five_quick_brightness_presses_build_on_the_target_and_show() {
        let mut core = Core::new(false);
        let now = Instant::now();
        core.observe_battery(&lights(40, 0), &state(50.0, false), now);
        let mut sent = Vec::new();
        for i in 0..5 {
            let effects = core.key(
                Key::BrightnessUp,
                &state(50.0, false),
                now + Duration::from_millis(50 * i),
            );
            assert_eq!(
                shown(&effects),
                Some((Source::Screen, 40)),
                "the confirmed level"
            );
            sent.extend(battery_commands(&effects));
        }
        assert_eq!(sent, [45, 50, 55, 60, 65].map(screen).to_vec());
        let effects = core.observe_battery(&lights(65, 0), &state(50.0, false), now);
        assert_eq!(shown(&effects), Some((Source::Screen, 65)));
    }

    #[test]
    fn a_brightness_key_at_either_end_commands_nothing_and_shows() {
        let now = Instant::now();
        let mut core = Core::new(false);
        core.observe_battery(&lights(100, 2), &state(50.0, false), now);
        let effects = core.key(Key::BrightnessUp, &state(50.0, false), now);
        assert!(battery_commands(&effects).is_empty());
        assert_eq!(shown(&effects), Some((Source::Screen, 100)));
        let effects = core.key(Key::KeyboardBrightnessUp, &state(50.0, false), now);
        assert!(battery_commands(&effects).is_empty());
        assert_eq!(shown(&effects), Some((Source::Keyboard, 100)));
        let effects = core.key(Key::CycleKeyboardBrightness, &state(50.0, false), now);
        assert_eq!(
            battery_commands(&effects),
            [BatteryCommand::SetKeyboardBrightness {
                device: "tpacpi::kbd_backlight".into(),
                level: 0
            }]
        );
    }

    #[test]
    fn without_a_backlight_its_keys_do_nothing() {
        let now = Instant::now();
        let mut core = Core::new(false);
        let desktop = BatteryState {
            keyboard: None,
            ..lights(50, 0)
        };
        core.observe_battery(&desktop, &state(50.0, false), now);
        for key in [
            Key::KeyboardBrightnessUp,
            Key::KeyboardBrightnessDown,
            Key::CycleKeyboardBrightness,
        ] {
            assert!(core.key(key, &state(50.0, false), now).is_empty());
        }
        assert_eq!(core.deadline(), None);
        assert!(!core
            .key(Key::BrightnessDown, &state(50.0, false), now)
            .is_empty());
    }

    #[test]
    fn a_brightness_key_before_the_first_battery_state_waits_for_it_and_shows() {
        let mut core = Core::new(false);
        let now = Instant::now();
        assert!(core.key(Key::BrightnessUp, &state(50.0, false), now).is_empty());
        assert_eq!(core.deadline(), Some(now + SETTLE_WAIT));
        let effects = core.observe_battery(
            &lights(40, 0),
            &state(50.0, false),
            now + Duration::from_millis(200),
        );
        assert_eq!(battery_commands(&effects), [screen(45)]);
        assert_eq!(shown(&effects), Some((Source::Screen, 40)));
        assert!(core.held_lights.is_empty());

        // Without a state in time, the held key acts on none, and no later key waits.
        let mut core = Core::new(false);
        assert!(core
            .key(Key::BrightnessUp, &state(50.0, false), now)
            .is_empty());
        assert!(core
            .tick(&state(50.0, false), now + SETTLE_WAIT)
            .is_empty());
        assert!(core.held_lights.is_empty());
        assert_eq!(core.deadline(), None);
        core.observe_battery(&lights(40, 0), &state(50.0, false), now + SETTLE_WAIT);
        let effects = core.key(Key::BrightnessUp, &state(50.0, false), now + SETTLE_WAIT);
        assert_eq!(battery_commands(&effects), [screen(45)]);
    }

    #[test]
    fn brightness_keys_do_not_wait_for_the_audio_model() {
        let mut core = Core::new(false);
        let now = Instant::now();
        core.observe_battery(&lights(40, 0), &state(50.0, false), now);
        let unsettled = AudioState::default();
        let effects = core.key(Key::BrightnessDown, &unsettled, now);
        assert_eq!(battery_commands(&effects), [screen(35)]);
    }

    #[test]
    fn a_backlight_change_the_model_did_not_command_shows() {
        let now = Instant::now();
        let audio = state(50.0, false);
        let mut core = Core::new(false);
        assert!(core.observe_battery(&lights(40, 1), &audio, now).is_empty());
        assert!(
            core.observe_battery(&lights(45, 1), &audio, now).is_empty(),
            "a change the model commanded, or read on its own write, shows nothing"
        );
        let firmware = BatteryState {
            backlight_changes: 1,
            ..lights(60, 1)
        };
        let effects = core.observe_battery(&firmware, &audio, now);
        assert_eq!(shown(&effects), Some((Source::Screen, 60)));
        assert_eq!(core.deadline(), Some(now + ANNOUNCE_AFTER));
        let controller = BatteryState {
            keyboard_changes: 1,
            ..firmware
        };
        let effects = core.observe_battery(&controller, &audio, now);
        assert_eq!(shown(&effects), Some((Source::Keyboard, 50)));
    }

    #[test]
    fn a_refused_brightness_drops_its_target() {
        let now = Instant::now();
        let audio = state(50.0, false);
        let mut core = Core::new(false);
        core.observe_battery(&lights(40, 0), &audio, now);
        assert_eq!(
            battery_commands(&core.key(Key::BrightnessUp, &audio, now)),
            [screen(45)]
        );
        let refused = BatteryState {
            refused: 1,
            ..lights(40, 0)
        };
        core.observe_battery(&refused, &audio, now);
        assert_eq!(
            battery_commands(&core.key(Key::BrightnessUp, &audio, now)),
            [screen(45)],
            "the next key acts on the level that stands"
        );
    }

    #[test]
    fn the_text_names_the_backlight() {
        let level = |source| Level {
            source,
            percent: 40.0,
            muted: false,
            max: CEILING,
            icon: "",
        };
        assert_eq!(level(Source::Screen).text(), "Brightness 40 percent");
        assert_eq!(
            level(Source::Keyboard).text(),
            "Keyboard brightness 40 percent"
        );
    }
}
