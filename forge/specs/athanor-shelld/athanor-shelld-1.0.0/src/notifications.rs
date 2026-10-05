//! The two notification interfaces (doc_bar.md BR1, BR4; doc_notification_center.md NC8): the
//! specification's, open to every application, and the private one, which answers
//! athanor-bar.service and athanor-control-center.service method by method. Both live on one
//! connection and share one state: the store, the rules, the do-not-disturb state and the
//! destinations of the private signals.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use futures_util::{FutureExt, StreamExt};
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use tokio::sync::{watch, Notify};
use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::{BusName, OwnedUniqueName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};
use zvariant::{Signature, Type};

use crate::battery::Level;
use crate::clock;
use crate::dnd::{self, Dnd, Effective, Observed, Trigger};
use crate::hints::{is_desktop_id, Hints};
use crate::history::{self, Coalescer};
use crate::i18n::{tr, tr_n};
use crate::icon;
use crate::identity::{self, Identity};
use crate::policy::{decide, Facts};
use crate::rate::RateLimit;
use crate::rules::{RuleError, Rules};
use crate::sender::{admits, Admitted, Caller};
use crate::server::{NOTIFICATIONS_PATH, PRIVATE_PATH};
use crate::sound::Player;
use crate::store::{self, Content, Outcome, Reason, Reply, Store, Urgency, Visual};
use crate::wire::{from_notification, WireNotification};
use athanor_unit::text;

pub const CAPABILITIES: [&str; 8] = [
    "actions",
    "body",
    "body-hyperlinks",
    "body-markup",
    "icon-static",
    "inline-reply",
    "persistence",
    "sound",
];
pub const MAX_ACTIONS: usize = 8;
pub const ACTION_KEY_BYTES: usize = 64;
pub const TOKEN_CHARS: usize = 256;
/// The action key of KDE's inline reply (NC9).
pub const REPLY_ACTION: &str = "inline-reply";
/// A reply is a message, not a document.
pub const REPLY_CHARS: usize = 4096;

const HISTORY_FILE: &str = "notifications.json";
const CONTROL_CENTER_NAME: &str = "os.athanor.ControlCenter1";
const CONTROL_CENTER_PATH: &str = "/os/athanor/ControlCenter1";
/// The control center's `Show` page of the notification panel.
const CONTROL_CENTER_PAGE: &str = "notifications";
/// The action of the low-battery warning, and the control center's page for it (CC5).
const BATTERY_ACTION: &str = "battery";
const BATTERY_PAGE: &str = "battery";

pub struct State {
    pub store: Store,
    started: Instant,
    state_dir: PathBuf,
    proc_root: PathBuf,
    rules: Rules,
    dnd: Dnd,
    /// What the do-not-disturb file holds, to save only a state that changed.
    dnd_saved: String,
    effective: Effective,
    fullscreen: Trigger,
    /// The triggers the journal has already said are unavailable (NC6): once, not on every
    /// evaluation.
    unavailable_logged: BTreeSet<dnd::Reason>,
    /// The unique name each admitted unit last called from: where its signals go.
    destinations: Vec<(Caller, OwnedUniqueName)>,
    coalescer: Coalescer,
    dirty: Arc<Notify>,
    wake: watch::Sender<u64>,
    bus_id: String,
    /// Finds and plays the sound of a notification (NC7).
    player: Player,
    /// NC12: who sends too many.
    rate: RateLimit,
}

/// What an evaluation of the do-not-disturb state leaves to announce.
#[derive(Debug, Default)]
pub struct Settled {
    /// (on, reason, until) when the state the units see changed.
    changed: Option<(bool, String, i64)>,
    /// How many popups the state that just ended hid.
    summary: Option<u32>,
}

/// Whether `new` is `held` with another progress and nothing else changed.
fn progress_only(held: &Content, new: &Content) -> bool {
    held.value != new.value
        && *held
            == Content {
                value: held.value,
                ..new.clone()
            }
}

/// What the hints of a `Notify` say about its sound.
struct Heard {
    name: Option<String>,
    suppress: bool,
}

/// What `Notify` came to.
enum Arrival {
    /// The rule mutes the application: nothing is kept; the application gets this id.
    Refused(u32),
    Kept(Box<(Outcome, WireNotification)>),
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl State {
    pub fn new(
        state_dir: PathBuf,
        config_dir: PathBuf,
        proc_root: PathBuf,
        dnd: Dnd,
        wake: watch::Sender<u64>,
        dirty: Arc<Notify>,
        player: Player,
    ) -> State {
        let mut state = State {
            store: Store::new(),
            started: Instant::now(),
            state_dir,
            proc_root,
            rules: Rules::new(config_dir),
            dnd_saved: dnd.render(),
            dnd,
            effective: Effective {
                on: false,
                reason: None,
                until: None,
                unavailable: Vec::new(),
            },
            fullscreen: Trigger::Unavailable,
            unavailable_logged: BTreeSet::new(),
            destinations: Vec::new(),
            coalescer: Coalescer::new(),
            dirty,
            wake,
            bus_id: String::new(),
            player,
            rate: RateLimit::new(),
        };
        state.evaluate();
        state
    }

    /// Plays the sound for `urgency`, the hinted name first.
    fn sound_of(&self, urgency: Urgency, hint: Option<&str>) {
        if let Some(file) = self.player.sound_for(urgency, hint) {
            self.player.play(&file, urgency == Urgency::Critical);
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn history_path(&self) -> PathBuf {
        self.state_dir.join(HISTORY_FILE)
    }

    fn observed(&self) -> Observed {
        let (now, local) = clock::local_now();
        Observed {
            now,
            local,
            fullscreen: self.fullscreen,
            // ponytail: no source for screen sharing until doc_portal.md delivers its signal
            // (NC6): published as unavailable, never as active.
            sharing: Trigger::Unavailable,
        }
    }

    /// The do-not-disturb state now, from the clock, the settings and the triggers. Keeps what
    /// changed on disk (an expiry or a cleared override reaches the file only here), arms the
    /// clock's next wake, and says what the units must hear.
    pub fn evaluate(&mut self) -> Settled {
        let settings = self.rules.settings();
        let observed = self.observed();
        let (effective, summary) = self.dnd.evaluate(&observed, &settings);
        for reason in &effective.unavailable {
            if self.unavailable_logged.insert(*reason) {
                tracing::warn!(
                    trigger = reason.as_str(),
                    "this do-not-disturb trigger cannot be observed and stays off"
                );
            }
        }
        self.unavailable_logged
            .retain(|reason| effective.unavailable.contains(reason));
        let text = self.dnd.render();
        if text != self.dnd_saved {
            match self.dnd.save(&self.state_dir) {
                Ok(()) => self.dnd_saved = text,
                Err(err) => tracing::warn!(%err, "cannot keep the do-not-disturb state"),
            }
        }
        self.wake
            .send_replace(dnd::next_wake(&observed, &settings, effective.until));
        let seen = |e: &Effective| (e.on, e.reason, e.until);
        let changed = (seen(&effective) != seen(&self.effective)).then(|| {
            (
                effective.on,
                effective.reason.map_or("", dnd::Reason::as_str).to_owned(),
                effective.until.unwrap_or(0),
            )
        });
        self.effective = effective;
        Settled { changed, summary }
    }

    /// The switch by hand, evaluated at once so that a popup missed right after it is counted
    /// in the state that has begun, not wiped by it.
    pub fn set_manual(&mut self, on: bool, until: Option<i64>) -> Settled {
        let settings = self.rules.settings();
        let observed = self.observed();
        self.dnd.set_manual(on, until, &observed, &settings);
        self.evaluate()
    }

    pub fn report_fullscreen(&mut self, available: bool, active: bool) -> Settled {
        self.fullscreen = match (available, active) {
            (false, _) => Trigger::Unavailable,
            (true, true) => Trigger::Active,
            (true, false) => Trigger::Inactive,
        };
        self.evaluate()
    }

    fn record(&mut self, caller: Caller, name: OwnedUniqueName) {
        self.destinations.retain(|(known, _)| *known != caller);
        self.destinations.push((caller, name));
    }

    /// A bus name lost its owner: the senders it carried and the units that called from it.
    fn name_lost(&mut self, name: &str) -> Settled {
        if !self.store.sender_gone(name).is_empty() {
            self.dirty.notify_one();
        }
        let bar_gone = self
            .destinations
            .iter()
            .any(|(caller, known)| *caller == Caller::Bar && known.as_str() == name);
        self.destinations
            .retain(|(_, known)| known.as_str() != name);
        if bar_gone {
            // Nobody observes fullscreen any more (NC6).
            return self.report_fullscreen(false, false);
        }
        Settled::default()
    }

    /// `Notify`, after the sender has been identified: the rule, the settings, do not
    /// disturb and the policy decide what happens to it.
    fn arrive(
        &mut self,
        mut content: Content,
        replaces_id: u32,
        expire_timeout: i32,
        identity: Identity,
        sender: String,
        sound: &Heard,
    ) -> Arrival {
        let rule = self.rules.rule(&identity);
        let settings = self.rules.settings();
        let now = self.now_ms();
        // A replace that changes only the progress (NC9) updates the notification in place:
        // the popup keeps what it had and nothing sounds. It is no new notification, so it
        // does not count against the rate limit either.
        let in_place = self
            .store
            .replaceable(replaces_id, &identity, &sender)
            .filter(|held| progress_only(&held.content, &content))
            .is_some();
        // The key is the application; without a proven one the sender's connection, which
        // the sender cannot rotate the way it can `app_name`.
        let rate_limited = !in_place
            && !match &identity {
                Identity::App(id) => self.rate.admit(id, now),
                Identity::Other => self.rate.admit(&sender, now),
            };
        let decision = decide(&Facts {
            identity: &identity,
            rule: &rule,
            settings: &settings,
            dnd_on: self.effective.on,
            urgency: content.urgency,
            transient: content.transient,
            expire_timeout,
            suppress_sound: sound.suppress,
            rate_limited,
        });
        if !decision.list {
            return Arrival::Refused(self.store.fresh_id());
        }
        content.timeout_ms = decision.timeout_ms;
        let urgency = content.urgency;
        let popup = decision.popup;
        if !in_place && self.effective.on && rule.popups && !decision.popup {
            self.dnd.missed_one();
        }
        let outcome = if in_place {
            // Nothing but the progress changed: the row, its time, read state and popup stay.
            let notification = self.store.set_value(replaces_id, content.value, sender.clone());
            notification.map(|notification| Outcome {
                notification,
                replaced: true,
                evicted: Vec::new(),
            })
        } else {
            None
        };
        let outcome = outcome.unwrap_or_else(|| {
            self.store
                .notify(content, replaces_id, now, unix_now(), identity, sender, popup)
        });
        if decision.sound && !in_place {
            self.sound_of(urgency, sound.name.as_deref());
        }
        self.dirty.notify_one();
        let mut wire = from_notification(&outcome.notification, self.now_ms());
        if in_place {
            // This arrival's decision: no new popup, whatever the held one is.
            wire.popup = false;
        }
        Arrival::Kept(Box::new((outcome, wire)))
    }
}

pub type Shared = Arc<Mutex<State>>;

/// The caller's unique name and its application, from its cgroup: the pid is the bus's word
/// for it.
async fn caller_of(conn: &Connection, state: &Shared, header: &Header<'_>) -> (String, Identity) {
    let sender = header.sender().map(ToString::to_string).unwrap_or_default();
    let identity = match header.sender() {
        Some(name) => match credentials_pid(conn, name).await {
            Ok(pid) => {
                let proc_root = lock(state).proc_root.clone();
                identity::of_pid(&proc_root, pid)
            }
            Err(err) => {
                tracing::warn!(error = %err, "cannot identify the sender");
                Identity::Other
            }
        },
        None => Identity::Other,
    };
    (sender, identity)
}

/// A poisoned lock means a panic, and panic = "abort" means there is none: take the guard.
pub(crate) fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Keeps the history on disk: the file as the state holds it now.
pub fn write_history(state: &Shared) {
    let mut state = lock(state);
    let result = history::save(&state.history_path(), &state.store, &state.bus_id);
    if state.coalescer.written(Instant::now(), result.is_ok()) {
        if let Err(err) = &result {
            tracing::warn!(%err, "cannot write the notification history; keeping it in memory");
        }
    }
}

/// Writes the history after each change, at most one write every two seconds.
pub async fn history_writer(state: Shared, dirty: Arc<Notify>) {
    loop {
        dirty.notified().await;
        let delay = lock(&state).coalescer.changed(Instant::now());
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        // What changed during the wait is part of this write.
        dirty.notified().now_or_never();
        write_history(&state);
    }
}

/// The arguments of `Notify`, made safe (BR4). The picture is, in the specification's order:
/// image-data, image-path, app_icon.
#[must_use]
pub fn content(
    app_name: &str,
    app_icon: &str,
    summary: &str,
    body: &str,
    actions: &[&str],
    hints: Hints,
    expire_timeout: i32,
) -> Content {
    let urgency = Urgency::from_hint(hints.urgency);
    let visual = match (
        hints.image,
        hints.image_path.or_else(|| icon::parse(app_icon)),
    ) {
        (Some(image), _) => Visual::Pixels(image),
        (None, Some(named)) => Visual::Icon(named),
        (None, None) => Visual::None,
    };
    let actions: Vec<(String, String)> = actions
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| is_action_key(pair[0]))
        .take(MAX_ACTIONS)
        .map(|pair| (pair[0].to_owned(), text::line(pair[1], text::NAME_CHARS)))
        .collect();
    // The hints are plain text, bounded like the other strings; they matter only when the
    // sender declared the action that opens the reply (NC9).
    let reply = actions
        .iter()
        .any(|(key, _)| key == REPLY_ACTION)
        .then(|| Reply {
            placeholder: text::line(
                hints.reply_placeholder.as_deref().unwrap_or_default(),
                text::NAME_CHARS,
            ),
            submit_text: text::line(
                hints.reply_submit_text.as_deref().unwrap_or_default(),
                text::NAME_CHARS,
            ),
            submit_icon: hints.reply_submit_icon.unwrap_or_default(),
        });
    Content {
        app_name: text::line(app_name, text::NAME_CHARS),
        summary: text::line(summary, text::SUMMARY_CHARS),
        body: text::lines(body, text::BODY_CHARS),
        actions,
        urgency,
        transient: hints.transient,
        resident: hints.resident,
        desktop_entry: hints.desktop_entry,
        visual,
        timeout_ms: store::timeout_ms(expire_timeout, urgency),
        value: hints.value,
        reply,
    }
}

/// A key returns to the application unchanged, so a bad one is refused, not cleaned.
fn is_action_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= ACTION_KEY_BYTES && !key.chars().any(text::is_hidden)
}

/// `Notify`'s `actions`, bounded while decoding: a plain `Vec<&str>` would grow to hold every
/// element of whatever the caller sent before `content` ever gets to trim it to `MAX_ACTIONS`
/// pairs. At most `2 * MAX_ACTIONS` strings — a whole pair per key and value — are kept; the
/// rest are walked past, not stored.
struct Actions<'a>(Vec<&'a str>);

impl Type for Actions<'_> {
    const SIGNATURE: &'static Signature = <Vec<&str> as Type>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Actions<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Actions<'de>, D::Error> {
        deserializer.deserialize_seq(ActionsVisitor)
    }
}

struct ActionsVisitor;

impl<'de> Visitor<'de> for ActionsVisitor {
    type Value = Actions<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of strings")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Actions<'de>, A::Error> {
        let cap = 2 * MAX_ACTIONS;
        let mut kept = Vec::new();
        while kept.len() < cap {
            match seq.next_element::<&str>()? {
                Some(item) => kept.push(item),
                None => return Ok(Actions(kept)),
            }
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Actions(kept))
    }
}

/// The pid the bus reports for `sender`, from `GetConnectionCredentials`: never a hint the
/// sender wrote, and never a pid kept from before (pids are reused).
async fn credentials_pid(conn: &Connection, sender: &UniqueName<'_>) -> fdo::Result<u32> {
    let credentials = DBusProxy::new(conn)
        .await?
        .get_connection_credentials(sender.clone().into())
        .await?;
    credentials
        .process_id()
        .ok_or_else(|| fdo::Error::AccessDenied("the bus gave no process id for the caller".into()))
}

/// The signal emitters of the private interface, one per unit that has called.
fn emitters(state: &Shared, conn: &Connection) -> Vec<SignalEmitter<'static>> {
    let names: Vec<OwnedUniqueName> = lock(state)
        .destinations
        .iter()
        .map(|(_, name)| name.clone())
        .collect();
    names
        .into_iter()
        .filter_map(|name| match SignalEmitter::new(conn, PRIVATE_PATH) {
            Ok(emitter) => Some(emitter.set_destination(BusName::from(name))),
            Err(err) => {
                tracing::warn!(error = %err, "cannot address a private signal");
                None
            }
        })
        .collect()
}

/// Sends one private signal to each unit that has called, unicast, never broadcast. Each
/// send is independent: a failure towards one is logged and never skips another.
async fn each_destination<F, Fut>(conn: &Connection, state: &Shared, what: &str, send: F)
where
    F: Fn(SignalEmitter<'static>) -> Fut,
    Fut: Future<Output = zbus::Result<()>>,
{
    for emitter in emitters(state, conn) {
        if let Err(err) = send(emitter).await {
            tracing::warn!(error = %err, "cannot tell a unit about {what}");
        }
    }
}

/// Tells both sides a notification closed: applications listen on the specification's
/// object, the units on the private one, unicast. The two are sent independently — a failure
/// sending one must never skip the other — and each failure is logged on its own; the store
/// has already changed regardless.
async fn emit_closed(conn: &Connection, state: &Shared, id: u32, reason: Reason) {
    let public = async {
        Notifications::notification_closed(
            &SignalEmitter::new(conn, NOTIFICATIONS_PATH)?,
            id,
            reason as u32,
        )
        .await
    }
    .await;
    if let Err(err) = public {
        tracing::warn!(id, error = %err, "cannot tell applications a notification closed");
    }
    each_destination(
        conn,
        state,
        "a notification that closed",
        |emitter| async move { Private::closed(&emitter, id, reason as u32).await },
    )
    .await;
}

/// Announces an evaluation of do not disturb: a change to the units, and the summary of a
/// state that ended with popups missed, as a notification of the daemon's own.
pub async fn settle(conn: &Connection, state: &Shared, settled: Settled) {
    if let Some((on, reason, until)) = settled.changed {
        let reason = reason.as_str();
        each_destination(conn, state, "do not disturb", |emitter| async move {
            Private::do_not_disturb_changed(&emitter, on, reason, until).await
        })
        .await;
    }
    if let Some(missed) = settled.summary {
        post_summary(conn, state, missed).await;
    }
}

/// What a stored notification sets off: a `Closed` for each one it pushed out of a full store
/// (the notification itself is already held), then `Added` or `Replaced` to the units.
async fn announce(conn: &Connection, state: &Shared, outcome: &Outcome, wire: &WireNotification) {
    for id in &outcome.evicted {
        emit_closed(conn, state, *id, Reason::Expired).await;
    }
    let replaced = outcome.replaced;
    each_destination(conn, state, "a notification", |emitter| async move {
        if replaced {
            Private::replaced(&emitter, wire).await
        } else {
            Private::added(&emitter, wire).await
        }
    })
    .await;
}

/// Keeps a notification the daemon sends itself, and sounds it when it is critical: the one
/// place both of its senders share, since `arrive` is only for what applications send.
fn keep_own(
    state: &Shared,
    own: String,
    content: impl FnOnce(u32) -> Content,
) -> (Outcome, WireNotification) {
    let mut state = lock(state);
    let content = content(state.rules.settings().timeout_normal_s);
    // Only a critical one sounds: the do-not-disturb summary is a popup that opens the center
    // when the user comes back, and NC7 gives no sound to it. Critical passes do not disturb,
    // and the global switch still silences it.
    if content.urgency == Urgency::Critical && state.rules.settings().sound {
        state.sound_of(content.urgency, None);
    }
    let now = state.now_ms();
    let outcome = state
        .store
        .notify(content, 0, now, unix_now(), Identity::Other, own, true);
    state.dirty.notify_one();
    let wire = from_notification(&outcome.notification, now);
    (outcome, wire)
}

/// Stores a notification the daemon sends itself and announces it to the units. Sent through
/// the daemon's own connection: it is the sender, so its actions are unavailable only when
/// the daemon is gone (`invoke_action` carries them out).
async fn post_own(conn: &Connection, state: &Shared, content: impl FnOnce(u32) -> Content) {
    let Some(own) = conn.unique_name().map(ToString::to_string) else {
        return;
    };
    let (outcome, wire) = keep_own(state, own, content);
    announce(conn, state, &outcome, &wire).await;
}

fn own_content(
    summary: String,
    body: String,
    action: &str,
    urgency: Urgency,
    timeout_ms: u32,
) -> Content {
    Content {
        app_name: "Athanor".to_owned(),
        summary,
        body,
        actions: vec![(action.to_owned(), tr("Show"))],
        urgency,
        transient: urgency != Urgency::Critical,
        resident: false,
        desktop_entry: None,
        visual: Visual::None,
        timeout_ms,
        value: None,
        reply: None,
    }
}

/// "{n} notifications while do not disturb was on", transient, whose `default` action opens
/// the control center on the notifications (NC6).
async fn post_summary(conn: &Connection, state: &Shared, missed: u32) {
    let summary = tr_n(
        "{n} notification while do not disturb was on",
        "{n} notifications while do not disturb was on",
        u64::from(missed),
    )
    .replace("{n}", &missed.to_string());
    post_own(conn, state, |seconds| {
        own_content(
            summary,
            String::new(),
            "default",
            Urgency::Normal,
            seconds.saturating_mul(1000),
        )
    })
    .await;
}

/// The low-battery warning: critical, so it stays until dismissed, with the time UPower
/// gives (only when it gives one) and a `battery` action that opens the control center there.
pub async fn post_low_battery(
    conn: &Connection,
    state: &Shared,
    level: Level,
    seconds: Option<u64>,
) {
    let summary = match level {
        Level::Low => tr("Battery low"),
        Level::Critical => tr("Battery critically low"),
    };
    let body = seconds.filter(|&s| s > 0).map_or_else(String::new, |s| {
        let (hours, minutes) = athanor_services::battery::hours_minutes(s);
        tr("{hours} h {minutes} min remaining")
            .replace("{hours}", &hours.to_string())
            .replace("{minutes}", &minutes.to_string())
    });
    post_own(conn, state, |_| {
        own_content(summary, body, BATTERY_ACTION, Urgency::Critical, 0)
    })
    .await;
}

static ABSENT_LOGGED: AtomicBool = AtomicBool::new(false);

/// Whether the call failed because no process owns the name and the bus cannot start one.
fn control_center_absent(err: &zbus::Error) -> bool {
    matches!(
        err,
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
            )
    ) || matches!(
        err,
        zbus::Error::FDO(inner)
            if matches!(**inner, fdo::Error::ServiceUnknown(_) | fdo::Error::NameHasNoOwner(_))
    )
}

pub struct Notifications {
    pub state: Shared,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&'static str> {
        CAPABILITIES.to_vec()
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        (
            "athanor-shelld",
            "Athanor",
            env!("CARGO_PKG_VERSION"),
            "1.2",
        )
    }

    #[allow(clippy::too_many_arguments)] // the specification's signature
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Actions<'_>,
        hints: Hints,
        expire_timeout: i32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> u32 {
        let sound = Heard {
            name: hints.sound_name.clone(),
            suppress: hints.suppress_sound,
        };
        let content = content(
            app_name,
            app_icon,
            summary,
            body,
            &actions.0,
            hints,
            expire_timeout,
        );
        let (sender, identity) = caller_of(conn, &self.state, &header).await;
        let arrival = lock(&self.state).arrive(
            content,
            replaces_id,
            expire_timeout,
            identity,
            sender,
            &sound,
        );
        let (outcome, wire) = match arrival {
            Arrival::Refused(id) => return id,
            Arrival::Kept(kept) => *kept,
        };
        let id = wire.id;
        announce(conn, &self.state, &outcome, &wire).await;
        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        // Only the sender's own: a row that is another's answers as an unknown id.
        let (sender, identity) = caller_of(conn, &self.state, &header).await;
        let closed = {
            let mut state = lock(&self.state);
            let closed = state.store.close_owned(id, &identity, &sender);
            if closed.is_some() {
                state.dirty.notify_one();
            }
            closed
        };
        if closed.is_none() {
            return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
        }
        emit_closed(conn, &self.state, id, Reason::Closed).await;
        Ok(())
    }

    #[zbus(signal)]
    pub async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    /// KDE's inline reply: sent to the notification's sender only (a targeted signal).
    #[zbus(signal)]
    pub async fn notification_replied(
        emitter: &SignalEmitter<'_>,
        id: u32,
        text: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        activation_token: &str,
    ) -> zbus::Result<()>;
}

pub struct Private {
    pub state: Shared,
    pub admitted: Admitted,
}

fn rule_error(err: RuleError) -> fdo::Error {
    match err {
        RuleError::BadApp => fdo::Error::InvalidArgs("not an application id".into()),
        RuleError::BadKey => fdo::Error::InvalidArgs("no such key".into()),
        RuleError::BadValue => fdo::Error::InvalidArgs("not a value of that key".into()),
        RuleError::Io(err) => fdo::Error::IOError(format!("cannot write the file: {err}")),
    }
}

fn identity_of(app: &str) -> fdo::Result<Identity> {
    if app.is_empty() {
        Ok(Identity::Other)
    } else if is_desktop_id(app) {
        Ok(Identity::App(app.to_owned()))
    } else {
        Err(fdo::Error::InvalidArgs(format!(
            "{app:?} is not an application id"
        )))
    }
}

impl Private {
    /// Which unit is calling `method`, and whether the table admits it. Admitting a call is
    /// also what records the caller's unique name as its unit's destination: the bar lists on
    /// start and on restart, so this is where its name is (re)recorded. The bus's
    /// `NameOwnerChanged` forgets it.
    async fn admit(
        &self,
        header: &Header<'_>,
        conn: &Connection,
        method: &str,
    ) -> fdo::Result<Caller> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("a call with no sender".into()))?;
        let pid = credentials_pid(conn, sender).await?;
        let caller = self
            .admitted
            .caller(sender.as_str(), pid)
            .filter(|caller| admits(*caller, method))
            .ok_or_else(|| fdo::Error::AccessDenied(format!("{method} is not for this caller")))?;
        lock(&self.state).record(caller, sender.to_owned().into());
        Ok(caller)
    }
}

#[interface(name = "os.athanor.Notifications1")]
impl Private {
    /// The unread notifications, oldest first.
    async fn list(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<Vec<WireNotification>> {
        self.admit(&header, conn, "List").await?;
        let state = lock(&self.state);
        let now = state.now_ms();
        Ok(state
            .store
            .iter()
            .filter(|held| !held.read)
            .map(|held| from_notification(held, now))
            .collect())
    }

    /// Every notification held, oldest first.
    async fn history(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<Vec<WireNotification>> {
        self.admit(&header, conn, "History").await?;
        let state = lock(&self.state);
        let now = state.now_ms();
        Ok(state
            .store
            .iter()
            .map(|held| from_notification(held, now))
            .collect())
    }

    /// `reason` is 1 (the popup of a transient notification ended) or 2 (the user closed it).
    async fn close(
        &self,
        id: u32,
        reason: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "Close").await?;
        let reason = match reason {
            1 => Reason::Expired,
            2 => Reason::Dismissed,
            other => {
                return Err(fdo::Error::InvalidArgs(format!(
                    "reason {other} is neither 1 nor 2"
                )))
            }
        };
        {
            let mut state = lock(&self.state);
            if state.store.close(id).is_none() {
                return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
            }
            state.dirty.notify_one();
        }
        emit_closed(conn, &self.state, id, reason).await;
        Ok(())
    }

    /// The token comes from the caller's surface and the click's serial (BR4). It is sent
    /// before the action, so the application can raise its window with it. An action of the
    /// daemon's own notification is the daemon's to carry out.
    async fn invoke_action(
        &self,
        id: u32,
        action_key: &str,
        activation_token: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "InvokeAction").await?;
        let own = conn.unique_name().map(ToString::to_string);
        let (resident, internal) = {
            let state = lock(&self.state);
            let held = state
                .store
                .get(id)
                .ok_or_else(|| fdo::Error::InvalidArgs(format!("no notification {id}")))?;
            if !held
                .content
                .actions
                .iter()
                .any(|(key, _)| key == action_key)
            {
                return Err(fdo::Error::InvalidArgs(format!(
                    "notification {id} has no action {action_key:?}"
                )));
            }
            if held.sender.is_empty() {
                return Err(fdo::Error::InvalidArgs(format!(
                    "the sender of notification {id} is gone"
                )));
            }
            (held.content.resident, Some(&held.sender) == own.as_ref())
        };
        if internal {
            let page = match action_key {
                "default" => Some(CONTROL_CENTER_PAGE),
                BATTERY_ACTION => Some(BATTERY_PAGE),
                _ => None,
            };
            if let Some(page) = page {
                let shown = conn
                    .call_method(
                        Some(CONTROL_CENTER_NAME),
                        CONTROL_CENTER_PATH,
                        Some(CONTROL_CENTER_NAME),
                        "Show",
                        &(page,),
                    )
                    .await;
                match shown {
                    Err(err) if action_key == BATTERY_ACTION && control_center_absent(&err) => {
                        // Not installed on most machines yet: said once, nothing more to do.
                        if !ABSENT_LOGGED.swap(true, Ordering::Relaxed) {
                            tracing::info!("the control center is not installed; the battery action does nothing");
                        }
                    }
                    Err(err) => {
                        return Err(fdo::Error::Failed(format!(
                            "cannot open the control center: {err}"
                        )));
                    }
                    Ok(_) => {}
                }
            }
        } else {
            let public = SignalEmitter::new(conn, NOTIFICATIONS_PATH)?;
            let token = text::line(activation_token, TOKEN_CHARS);
            if !token.is_empty() {
                Notifications::activation_token(&public, id, &token).await?;
            }
            Notifications::action_invoked(&public, id, action_key).await?;
        }
        let closed = {
            let mut state = lock(&self.state);
            let closed = !resident && state.store.close(id).is_some();
            if closed {
                state.dirty.notify_one();
            }
            closed
        };
        if closed {
            emit_closed(conn, &self.state, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    /// An inline reply (NC9): the text goes to the sending application, as a signal addressed
    /// to it alone, and nowhere else: not the store, not the history, not the log.
    async fn reply(
        &self,
        id: u32,
        text: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "Reply").await?;
        let (sender, resident) = {
            let state = lock(&self.state);
            let held = state
                .store
                .get(id)
                .ok_or_else(|| fdo::Error::InvalidArgs(format!("no notification {id}")))?;
            if held.content.reply.is_none() {
                return Err(fdo::Error::InvalidArgs(format!(
                    "notification {id} declared no {REPLY_ACTION} action"
                )));
            }
            let sender = OwnedUniqueName::try_from(held.sender.as_str()).map_err(|_| {
                fdo::Error::InvalidArgs(format!("the sender of notification {id} is gone"))
            })?;
            (sender, held.content.resident)
        };
        let public = SignalEmitter::new(conn, NOTIFICATIONS_PATH)?
            .set_destination(BusName::from(sender));
        Notifications::notification_replied(&public, id, &text::lines(text, REPLY_CHARS)).await?;
        let closed = {
            let mut state = lock(&self.state);
            let closed = !resident && state.store.close(id).is_some();
            if closed {
                state.dirty.notify_one();
            }
            closed
        };
        if closed {
            emit_closed(conn, &self.state, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    async fn mark_read(
        &self,
        ids: Vec<u32>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "MarkRead").await?;
        let changed = {
            let mut state = lock(&self.state);
            let changed = state.store.mark_read(&ids);
            if !changed.is_empty() {
                state.dirty.notify_one();
            }
            changed
        };
        if !changed.is_empty() {
            let changed = &changed;
            each_destination(
                conn,
                &self.state,
                "read notifications",
                |emitter| async move { Private::read(&emitter, changed).await },
            )
            .await;
        }
        Ok(())
    }

    async fn clear_all(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "ClearAll").await?;
        let cleared = {
            let mut state = lock(&self.state);
            let cleared = state.store.clear_all();
            state.dirty.notify_one();
            cleared
        };
        for id in cleared {
            emit_closed(conn, &self.state, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    /// `app` is the application id, `""` for Other.
    async fn clear_group(
        &self,
        app: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "ClearGroup").await?;
        let identity = identity_of(app)?;
        let cleared = {
            let mut state = lock(&self.state);
            let cleared = state.store.clear_group(&identity);
            state.dirty.notify_one();
            cleared
        };
        for id in cleared {
            emit_closed(conn, &self.state, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    /// `(on, reason, until, unavailable)`: `reason` is `""` when off, `until` 0 when there is
    /// none, `unavailable` the triggers the session cannot observe.
    async fn do_not_disturb(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<(bool, String, i64, Vec<String>)> {
        self.admit(&header, conn, "DoNotDisturb").await?;
        let state = lock(&self.state);
        let effective = &state.effective;
        Ok((
            effective.on,
            effective.reason.map_or("", dnd::Reason::as_str).to_owned(),
            effective.until.unwrap_or(0),
            effective
                .unavailable
                .iter()
                .map(|reason| reason.as_str().to_owned())
                .collect(),
        ))
    }

    async fn set_do_not_disturb(
        &self,
        on: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "SetDoNotDisturb").await?;
        let settled = lock(&self.state).set_manual(on, None);
        settle(conn, &self.state, settled).await;
        Ok(())
    }

    /// `until` is unix seconds; 0 means no end.
    async fn set_do_not_disturb_until(
        &self,
        on: bool,
        until: i64,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "SetDoNotDisturbUntil").await?;
        let settled = lock(&self.state).set_manual(on, (until > 0).then_some(until));
        settle(conn, &self.state, settled).await;
        Ok(())
    }

    /// Whether the bar can observe fullscreen windows, and whether one is fullscreen and
    /// activated (NC6).
    async fn report_fullscreen(
        &self,
        available: bool,
        active: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "ReportFullscreen").await?;
        let settled = lock(&self.state).report_fullscreen(available, active);
        settle(conn, &self.state, settled).await;
        Ok(())
    }

    async fn rules(
        &self,
        app: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<std::collections::HashMap<String, String>> {
        self.admit(&header, conn, "Rules").await?;
        let identity = identity_of(app)?;
        Ok(lock(&self.state).rules.rule(&identity).pairs())
    }

    /// The control center sets `allowed` and `popups` only: a mute is its to give, a bypass
    /// of do not disturb is not.
    async fn set_rule(
        &self,
        app: &str,
        key: &str,
        value: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        let caller = self.admit(&header, conn, "SetRule").await?;
        if caller == Caller::ControlCenter && !matches!(key, "allowed" | "popups") {
            return Err(fdo::Error::AccessDenied(format!(
                "the control center may set allowed and popups, not {key}"
            )));
        }
        // The change reaches the units through the watch on the files, as an edit by hand does.
        lock(&self.state)
            .rules
            .set_rule(app, key, value)
            .map_err(rule_error)
    }

    async fn settings(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<std::collections::HashMap<String, String>> {
        self.admit(&header, conn, "Settings").await?;
        Ok(lock(&self.state).rules.settings().pairs())
    }

    /// Nobody is admitted yet: the Settings application comes with its specification.
    async fn set_setting(
        &self,
        key: &str,
        value: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn, "SetSetting").await?;
        lock(&self.state)
            .rules
            .set_setting(key, value)
            .map_err(rule_error)
    }

    #[zbus(signal)]
    pub async fn added(
        emitter: &SignalEmitter<'_>,
        notification: &WireNotification,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn replaced(
        emitter: &SignalEmitter<'_>,
        notification: &WireNotification,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn read(emitter: &SignalEmitter<'_>, ids: &[u32]) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn do_not_disturb_changed(
        emitter: &SignalEmitter<'_>,
        on: bool,
        reason: &str,
        until: i64,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn rules_changed(emitter: &SignalEmitter<'_>, app: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn settings_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// What a file of the rules or the settings did, once the watch saw it change: the units
/// hear it, and the settings may change do not disturb.
pub async fn rules_changed(conn: &Connection, state: &Shared, relative: &Path) {
    let changed = lock(state).rules.invalidate(relative);
    match changed {
        Some(crate::rules::Changed::Rule(app)) => {
            let app = app.as_str();
            each_destination(conn, state, "a changed rule", |emitter| async move {
                Private::rules_changed(&emitter, app).await
            })
            .await;
        }
        Some(crate::rules::Changed::Settings) => {
            each_destination(conn, state, "changed settings", |emitter| async move {
                Private::settings_changed(&emitter).await
            })
            .await;
            let settled = lock(state).evaluate();
            settle(conn, state, settled).await;
        }
        None => {}
    }
}

/// A bus name lost its owner: what it carried goes, and so may a trigger.
pub async fn name_lost(conn: &Connection, state: &Shared, name: &str) {
    let settled = lock(state).name_lost(name);
    settle(conn, state, settled).await;
}

/// The history from the file, pruned by the retention, on this bus: a sender kept from the
/// last run that no longer has an owner is emptied, because no `NameOwnerChanged` fired
/// while the daemon was down.
pub async fn restore(conn: &Connection, state: &Shared) -> zbus::Result<()> {
    let bus = DBusProxy::new(conn).await?;
    let bus_id = bus.get_id().await?.to_string();
    let senders: BTreeSet<String> = {
        let mut state = lock(state);
        state.bus_id = bus_id.clone();
        let restored = history::load(&state.history_path(), &bus_id);
        state.store.restore(restored);
        let retention = state.rules.settings().retention;
        if !state.store.prune(unix_now(), retention).is_empty() {
            state.dirty.notify_one();
        }
        state
            .store
            .iter()
            .filter(|held| !held.sender.is_empty())
            .map(|held| held.sender.clone())
            .collect()
    };
    for sender in senders {
        let owned = match BusName::try_from(sender.as_str()) {
            Ok(name) => bus.name_has_owner(name).await?,
            Err(_) => false,
        };
        if !owned {
            let mut state = lock(state);
            state.store.sender_gone(&sender);
            state.dirty.notify_one();
        }
    }
    Ok(())
}

/// Follows the owners of the bus names: a name that vanishes takes its notifications' actions
/// and its unit's destination with it.
pub async fn follow_owners(conn: &Connection, state: Shared) -> zbus::Result<()> {
    let mut changes = DBusProxy::new(conn)
        .await?
        .receive_name_owner_changed()
        .await?;
    let conn = conn.clone();
    tokio::spawn(async move {
        while let Some(change) = changes.next().await {
            let Ok(args) = change.args() else {
                tracing::warn!("cannot parse a NameOwnerChanged signal");
                continue;
            };
            if args.new_owner().is_none() {
                name_lost(&conn, &state, args.name().as_str()).await;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::Icon;
    use std::time::Duration;

    #[test]
    fn bad_action_keys_are_dropped_and_labels_cleaned() {
        let long = "k".repeat(ACTION_KEY_BYTES + 1);
        let actions = [
            "default",
            "Open",
            "bad\u{202E}",
            "x",
            long.as_str(),
            "y",
            "ok",
            "La\u{0007}bel",
            "odd",
        ];
        let made = content("app", "", "s", "b", &actions, Hints::default(), -1);
        assert_eq!(
            made.actions,
            [
                ("default".into(), "Open".into()),
                ("ok".into(), "Label".into())
            ]
        );
    }

    #[test]
    fn the_picture_follows_the_specification_order() {
        let hints = Hints {
            image_path: Some(Icon::Name("from-hint".into())),
            ..Hints::default()
        };
        assert_eq!(
            content("a", "app-icon", "s", "b", &[], hints, -1).visual,
            Visual::Icon(Icon::Name("from-hint".into()))
        );
        assert_eq!(
            content("a", "app-icon", "s", "b", &[], Hints::default(), -1).visual,
            Visual::Icon(Icon::Name("app-icon".into()))
        );
        assert_eq!(
            content("a", "https://x/y.png", "s", "b", &[], Hints::default(), -1).visual,
            Visual::None
        );
    }

    #[test]
    fn a_hundred_thousand_actions_yield_at_most_max_actions_pairs() {
        use zvariant::serialized::Context;
        use zvariant::{to_bytes, LE};

        let many: Vec<String> = (0..100_000usize)
            .map(|n| {
                if n % 2 == 0 {
                    format!("k{n}")
                } else {
                    "v".to_owned()
                }
            })
            .collect();
        let encoded = to_bytes(Context::new_dbus(LE, 0), &many).expect("encode");
        let actions: Actions<'_> = encoded.deserialize().expect("decode").0;
        assert_eq!(actions.0.len(), 2 * MAX_ACTIONS);
        assert_eq!(&actions.0[..2], ["k0", "v"]);
        let made = content("app", "", "s", "b", &actions.0, Hints::default(), -1);
        assert_eq!(made.actions.len(), MAX_ACTIONS);
    }

    fn silent_state(dir: &std::path::Path, player: Player) -> Shared {
        let dnd = crate::dnd::Dnd::load(dir).expect("dnd");
        let (wake, _keep) = tokio::sync::watch::channel(0);
        Arc::new(Mutex::new(State::new(
            dir.join("state"),
            dir.join("config"),
            dir.join("proc"),
            dnd,
            wake,
            Arc::new(Notify::new()),
            player,
        )))
    }

    #[tokio::test]
    async fn the_daemons_own_critical_notification_sounds_once_and_a_normal_one_does_not() {
        let dir = std::env::temp_dir().join(format!("athanor-own-sound-{}", std::process::id()));
        let stereo = dir.join("data/sounds/freedesktop/stereo");
        std::fs::create_dir_all(&stereo).expect("theme");
        for name in ["message-new-instant", "dialog-warning"] {
            std::fs::write(stereo.join(format!("{name}.oga")), "").expect("sound");
        }
        let log = dir.join("played");
        let program = dir.join("fake-pw-play");
        std::fs::write(
            &program,
            format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display()),
        )
        .expect("program");
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .expect("chmod");
        let state = silent_state(&dir, Player::with_program(vec![dir.join("data")], program));
        let normal = |_| {
            own_content(
                "Summary".into(),
                String::new(),
                "default",
                Urgency::Normal,
                0,
            )
        };
        keep_own(&state, "own".into(), normal);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!log.exists(), "the do-not-disturb summary stays silent");
        keep_own(&state, "own".into(), |_| {
            own_content(
                "Battery low".into(),
                String::new(),
                BATTERY_ACTION,
                Urgency::Critical,
                0,
            )
        });
        for _ in 0..30 {
            if log.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let played = std::fs::read_to_string(&log).expect("one play");
        assert_eq!(played.lines().count(), 1);
        assert!(played.contains("dialog-warning.oga"), "{played}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
