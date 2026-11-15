//! The bar's trust shield and its sheet (doc_bar.md BR6), as the GTK bar's `ui/shield.rs` drew
//! them. The shield is the seal with SH12's badge; its accessible name and tooltip are the
//! sheet's header sentence, and nothing else is written in the bar. The sheet is the shield's
//! popover. Every string from the state file is set as plain text; "verified" and "refused" are
//! our own words.
//!
//! The state comes from `os.athanor.Update1.State`, not from the file: the unit's sandbox puts
//! the bar in a user namespace where root is the overflow uid, so only the root service can
//! check the file's owner. The bar trusts the reply only when the bus says root sent it. What
//! is admitted or refused is the service's and `athanor_trust_state`'s; this file shows it and
//! sends the person's requests.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_bar::bus;
use athanor_bar::i18n::{tr, tr_with};
use athanor_bar::order;
use athanor_bar::shield::{self, Refusal, Request, Rows, Sheet, Unreadable};
use athanor_sal::{
    Event, IconTheme, Image, Kind, Priority, Rgba, Symbolic, TextStyle, Ui, WidgetId, Wrap,
};
use athanor_style::appearance::Appearance;
use athanor_style::sal;
use athanor_trust_state::{Badge, ErrorCode, ReadError, Reason, State, UpdateState, STATE_PATH};
use gio::prelude::*;

use super::{Cx, Module, Reply, Start, Wake, Wakeup};
use crate::ui::look;

/// The state is asked again every hour: the badge ages ("checked within 14 days" turns false
/// without the file changing), and a directory watch that failed or missed an event heals.
const ASK_AGAIN_SECS: u32 = 3600;
/// After no answer, the next questions come this many seconds later, then hourly: a service
/// slow to start under the login's load must not leave the shield unverified for hours.
const RETRY_SECS: [u32; 3] = [5, 30, 300];
/// Apply and GoBack wait for the person at the polkit prompt however long it takes: GIO's
/// `G_MAXINT` means no timeout. The bus still answers NoReply if the service leaves first.
const REQUEST_TIMEOUT_MS: i32 = i32::MAX;
/// The seal, in the bar and in the sheet, as calmo's `.athanor-seal`.
const SEAL: u16 = 22;
/// The sheet's lines, as GTK's `max-width-chars` of 40.
const NOTE_CHARS: u32 = 40;
/// The seals ship in `hicolor`, which every theme inherits.
const SEAL_THEME: &str = "hicolor";

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// The state, asked of the update service at start, every hour, whenever the file's directory
/// changes (the system side renames a new file into place, UT7), and soon again after no answer.
struct Trust {
    wake: Wake,
    read: RefCell<Result<State, ReadError>>,
    /// The number of the latest question: an older answer arriving late is dropped.
    asked: Cell<u64>,
    /// The questions left unanswered in a row, which pick the next retry's delay.
    unanswered: Cell<usize>,
    /// The scheduled retry: a burst of unanswered questions waits for it, not for more, and
    /// an answer cancels it.
    retrying: RefCell<Option<glib::SourceId>>,
    /// The last refusal of a request, shown in the sheet until the sheet closes.
    refusal: Cell<Option<Refusal>>,
    _monitor: Option<gio::FileMonitor>,
}

impl Trust {
    fn start(wake: Wake) -> Rc<Trust> {
        let trust = Rc::new_cyclic(|me: &Weak<Trust>| {
            let monitor = Path::new(STATE_PATH).parent().and_then(|dir| {
                match gio::File::for_path(dir)
                    .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                {
                    Ok(monitor) => Some(monitor),
                    Err(err) => {
                        tracing::error!(error = %err, "cannot watch the trust state; the shield shows it as it was at start");
                        None
                    }
                }
            });
            if let Some(monitor) = &monitor {
                let me = me.clone();
                monitor.connect_changed(move |_, _, _, _| {
                    if let Some(trust) = me.upgrade() {
                        trust.reload();
                    }
                });
            }
            let hourly = me.clone();
            glib::timeout_add_seconds_local(ASK_AGAIN_SECS, move || match hourly.upgrade() {
                Some(trust) => {
                    trust.reload();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            Trust {
                wake,
                // Until the service answers, nothing backs the badge.
                read: RefCell::new(Err(ReadError::Io(std::io::ErrorKind::NotConnected))),
                asked: Cell::new(0),
                unanswered: Cell::new(0),
                retrying: RefCell::new(None),
                refusal: Cell::new(None),
                _monitor: monitor,
            }
        });
        trust.reload();
        trust
    }

    fn reload(self: &Rc<Self>) {
        let question = self.asked.get().wrapping_add(1);
        self.asked.set(question);
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let read = ask_state().await;
            let Some(trust) = me.upgrade() else { return };
            if trust.asked.get() == question {
                let answered = !matches!(read, Err(ReadError::Io(_)));
                *trust.read.borrow_mut() = read;
                (trust.wake)(Wakeup::Changed);
                trust.retry(answered);
            }
        });
    }

    /// After no answer, asks again after the next of `RETRY_SECS`; the hourly question takes
    /// over once they are spent.
    fn retry(self: &Rc<Self>, answered: bool) {
        if answered {
            self.unanswered.set(0);
            if let Some(pending) = self.retrying.take() {
                pending.remove();
            }
            return;
        }
        let missed = self.unanswered.get();
        let Some(&delay) = RETRY_SECS.get(missed) else {
            return;
        };
        if self.retrying.borrow().is_some() {
            return;
        }
        self.unanswered.set(missed + 1);
        let me = Rc::downgrade(self);
        let pending = glib::timeout_add_seconds_local_once(delay, move || {
            if let Some(trust) = me.upgrade() {
                // Fired: the source is gone, so its id is forgotten, not removed.
                trust.retrying.replace(None);
                trust.reload();
            }
        });
        self.retrying.replace(Some(pending));
    }

    fn badge(&self) -> Badge {
        shield::badge(&self.read.borrow(), now())
    }

    /// Whether `request` is offered now. Turning the switch is offered in every readable
    /// state; the service has the last word.
    fn offered(&self, request: Request) -> bool {
        let read = self.read.borrow();
        match request {
            Request::Apply => shield::restart_to_update_offered(&read),
            Request::GoBack => shield::go_back_offered(&read),
            Request::Postpone => shield::postpone_offered(&read, now()),
            Request::SetAutomatic(_) => true,
        }
    }

    /// What the sheet shows now.
    fn shown(&self) -> Shown {
        let (badge, sheet) = {
            let read = self.read.borrow();
            (shield::badge(&read, now()), shield::sheet(&read, now()))
        };
        Shown {
            badge,
            sheet,
            refusal: self.refusal.get(),
            offered: [Request::Postpone, Request::Apply, Request::GoBack]
                .map(|request| self.offered(request)),
        }
    }

    /// Shows `refusal` in the sheet, which opens: the polkit agent took the focus, which closed
    /// an open sheet.
    fn refused(&self, refusal: Refusal) {
        self.refusal.set(Some(refusal));
        (self.wake)(Wakeup::Open(order::Module::Shield));
    }

    /// Sends `request` if the state still offers it, as it may have stopped while a question
    /// was open (Review Focus 1); refuses it in the sheet otherwise.
    fn send(self: &Rc<Self>, request: Request) {
        if self.offered(request) {
            send(self, request);
        } else {
            let withdrawn = shield::withdrawn(&self.read.borrow(), request);
            self.refused(withdrawn);
        }
    }
}

/// Asks `os.athanor.Update1.State` on the system bus (the call starts the service), then asks
/// the bus which user sent the reply: the reply's sender is set by the bus, so this is the uid
/// of the connection that answered, not of whoever owns the name by then.
async fn ask_state() -> Result<State, ReadError> {
    let no_answer = shield::state_reply(Err(None), None);
    let system = match gio::bus_get_future(gio::BusType::System).await {
        Ok(system) => system,
        Err(err) => {
            tracing::warn!(error = %err, "no system bus; the trust state cannot be asked");
            return no_answer;
        }
    };
    let question = gio::DBusMessage::new_method_call(
        Some(shield::UPDATE_NAME),
        shield::UPDATE_PATH,
        Some(shield::UPDATE_INTERFACE),
        shield::STATE_METHOD,
    );
    let reply = match system
        .send_message_with_reply_future(&question, gio::DBusSendMessageFlags::NONE, bus::TIMEOUT_MS)
        .await
    {
        Ok(reply) => reply,
        Err(err) => {
            tracing::warn!(error = %err, "the update service did not answer State");
            return no_answer;
        }
    };
    let sender_uid = match reply.sender() {
        Some(sender) => uid_of(&system, &sender).await,
        None => None,
    };
    let read = if reply.message_type() == gio::DBusMessageType::Error {
        shield::state_reply(Err(reply.error_name().as_deref()), sender_uid)
    } else {
        let json = reply
            .body()
            .and_then(|body| body.get::<(String,)>())
            .map(|(json,)| json);
        shield::state_reply(Ok(json.as_deref().unwrap_or_default()), sender_uid)
    };
    if let Err(err) = &read {
        tracing::warn!(?err, ?sender_uid, "the trust state is not trusted");
    }
    read
}

/// The uid the bus daemon recorded for `name` when it connected: outside any namespace.
async fn uid_of(system: &gio::DBusConnection, name: &str) -> Option<u32> {
    let reply = bus::call(
        system,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixUser",
        Some(&(name,).to_variant()),
        bus::TIMEOUT_MS,
    )
    .await;
    match reply {
        Ok(reply) => reply.get::<(u32,)>().map(|(uid,)| uid),
        Err(err) => {
            tracing::warn!(error = %err, name, "the bus did not say who answered State");
            None
        }
    }
}

/// Sends `request` to the update service on the system bus, with polkit allowed to ask. A
/// refusal is shown in the sheet.
fn send(trust: &Rc<Trust>, request: Request) {
    let weak = Rc::downgrade(trust);
    // Only the switch carries an argument.
    let body = match request {
        Request::SetAutomatic(enabled) => Some((enabled,).to_variant()),
        _ => None,
    };
    glib::spawn_future_local(async move {
        let result = match gio::bus_get_future(gio::BusType::System).await {
            Ok(system) => bus::call(
                &system,
                shield::UPDATE_NAME,
                shield::UPDATE_PATH,
                shield::UPDATE_INTERFACE,
                request.method(),
                body.as_ref(),
                REQUEST_TIMEOUT_MS,
            )
            .await
            .map(|_| ()),
            Err(err) => Err(err),
        };
        let Err(err) = result else { return };
        let remote = gio::DBusError::remote_error(&err);
        let refusal = shield::refusal(remote.as_ref().map(glib::GString::as_str));
        tracing::warn!(error = %err, method = request.method(), ?refusal, "the update service refused");
        if let Some(trust) = weak.upgrade() {
            trust.refused(refusal);
        }
    });
}

fn header(badge: Badge) -> String {
    match badge {
        Badge::Check => tr("System image verified"),
        Badge::Attention => tr("Not verified yet"),
        Badge::Cross => tr("Update refused"),
    }
}

fn reason_words(reason: Reason) -> String {
    match reason {
        Reason::Signature => tr("Signed with a key of the policy in force"),
        Reason::Media => tr("Not verified: installed from media"),
        Reason::NoSignature => tr("Not verified: no signature"),
        Reason::KeyNotInPolicy => tr("Not verified: its key is not in the policy"),
        Reason::PolicyNotInForce => tr("Not verified: fetched under a permissive policy"),
        Reason::ReferenceOutOfScope => tr("Not verified: the image is outside the policy's scope"),
        Reason::LocalChanges => tr("Not verified: the system was changed on this machine"),
        Reason::ChannelAbsent => tr("Not verified: waiting for the stable channel to be published"),
        Reason::OriginNotEnforcing => tr("Not verified: this machine does not verify its updates"),
        Reason::PinnedBuild => tr("Signed, but pinned to one build: it receives no updates"),
    }
}

fn update_words(update: UpdateState) -> String {
    match update {
        UpdateState::None => tr("Up to date"),
        UpdateState::Available => tr("An update is available; it downloads at the next check"),
        UpdateState::Downloaded => tr("An update is ready; it installs when you restart"),
        UpdateState::WillApplyAtNextShutdown => tr("The update installs at the next restart"),
        UpdateState::Refused => tr("The last update was refused by the policy"),
        UpdateState::Held => {
            tr("The offered version is held back: you went back from it, or it did not start. Only a newer one is offered")
        }
        UpdateState::OlderThanBooted => tr("This version is older than one this machine has run"),
    }
}

fn error_words(error: ErrorCode) -> String {
    match error {
        ErrorCode::None => String::new(),
        ErrorCode::Network => tr("The last check failed: network error"),
        ErrorCode::Registry => tr("The last check failed: registry error"),
        ErrorCode::Policy => tr("The last check failed: refused by the policy"),
        ErrorCode::Storage => tr("The last check failed: storage error"),
        ErrorCode::Internal => tr("The last check failed: internal error"),
    }
}

fn unreadable_words(why: Unreadable) -> String {
    match why {
        Unreadable::Missing => tr("No trust state yet: the first check has not run"),
        Unreadable::Untrusted => {
            tr("The trust state file is not owned by the system and was ignored")
        }
        Unreadable::Malformed => tr("The trust state file could not be read"),
        Unreadable::NoAnswer => tr("The update service did not answer"),
    }
}

fn refusal_words(refusal: Refusal) -> String {
    match refusal {
        Refusal::NotAuthorized => tr("Not authorised"),
        Refusal::Busy => tr("Another update request is running"),
        Refusal::NothingDownloaded => {
            tr("Nothing is downloaded yet; the update downloads at the next check")
        }
        Refusal::NoPreviousVersion => tr("There is no previous version to go back to"),
        Refusal::Blocked => tr("A program is blocking the restart; close it and try again"),
        Refusal::Failed => tr("The request failed; the system journal says why"),
        Refusal::AlreadyPostponed => {
            tr("This security update was already postponed once, or can no longer be")
        }
        Refusal::NotSecurityClass => tr("Only security updates can be postponed"),
        Refusal::NoAnswer => tr("The update service did not answer"),
    }
}

/// A date of the file in the locale's format, in UTC: the build and check times are UTC.
fn date(epoch: i64) -> String {
    glib::DateTime::from_unix_utc(epoch)
        .and_then(|time| time.format("%x"))
        .map_or_else(|_| epoch.to_string(), |text| text.to_string())
}

/// The month's name from the bar's own catalog, so that it speaks the language of the sentence
/// around it, not GLib's `LC_TIME`.
fn month_name(month: i32) -> Option<String> {
    Some(match month {
        1 => tr("January"),
        2 => tr("February"),
        3 => tr("March"),
        4 => tr("April"),
        5 => tr("May"),
        6 => tr("June"),
        7 => tr("July"),
        8 => tr("August"),
        9 => tr("September"),
        10 => tr("October"),
        11 => tr("November"),
        12 => tr("December"),
        _ => return None,
    })
}

/// The expiry as UT13 words it: local day, month and hour, "28 October, 14:30", the month from
/// the catalog and the rest numeric.
fn local_time(epoch: i64) -> String {
    let words = glib::DateTime::from_unix_local(epoch)
        .ok()
        .and_then(|time| {
            Some((
                time.day_of_month(),
                month_name(time.month())?,
                time.hour(),
                time.minute(),
            ))
        });
    match words {
        Some((day, month, hour, minute)) => format!("{day} {month}, {hour:02}:{minute:02}"),
        None => {
            tracing::warn!(
                epoch,
                "cannot format the postponed-until time; showing the raw epoch"
            );
            epoch.to_string()
        }
    }
}

/// The sheet's lines for a readable state, in BR6's order.
fn lines(rows: &Rows) -> Vec<String> {
    let mut lines = vec![
        tr_with("Version {version}", "version", &rows.version),
        tr_with("Built on {date}", "date", &date(rows.build_time)),
        reason_words(rows.reason),
        match rows.last_check {
            Some(at) => tr_with("Last checked on {date}", "date", &date(at)),
            None => tr("Never checked"),
        },
        update_words(rows.update),
    ];
    if let Some((code, host)) = &rows.error {
        lines.push(error_words(*code));
        if let Some(host) = host {
            lines.push(tr_with("Host: {host}", "host", host));
        }
    }
    lines.push(if rows.policy_in_force {
        tr("Signature policy in force")
    } else {
        tr("Signature policy not in force")
    });
    lines.push(if rows.policy_shipped {
        tr("The policy is the one Athanor ships")
    } else {
        tr("The policy was changed on this machine")
    });
    lines.push(if rows.secure_boot_on {
        tr("Secure Boot on")
    } else {
        tr("Secure Boot off: this machine runs in the declared degraded mode")
    });
    if !rows.automatic_security_updates {
        lines.push(tr(
            "Automatic security updates are off: security fixes wait until someone installs them.",
        ));
    }
    if let Some(until) = rows.postponed_until {
        lines.push(tr_with(
            "The security update is postponed, at most until {time}; then it installs at the next shutdown or restart.",
            "time",
            &local_time(until),
        ));
    }
    lines
}

/// The question asked before `request` is sent, and its button's label; `None` for a request
/// sent at once: postponing needs no password for the active user, and turning automatic
/// security updates on is no risk.
fn question(request: Request) -> Option<(String, String)> {
    Some(match request {
        Request::Apply => (tr("Restart and install the update now?"), tr("Restart to update")),
        Request::GoBack => (
            tr("Go back to the previous version and restart? This asks for an administrator's password."),
            tr("Go back"),
        ),
        Request::SetAutomatic(false) => (
            tr("Turn off automatic security updates? Security fixes will then wait until someone installs them, and this computer stays exposed until then. This applies to every user of this computer."),
            tr("Turn Off"),
        ),
        Request::Postpone | Request::SetAutomatic(true) => return None,
    })
}

/// What the sheet shows: a change of the state draws it again only when this changes, since a
/// new drawing moves the focus and announces the refusal again.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    badge: Badge,
    sheet: Sheet,
    refusal: Option<Refusal>,
    /// Whether Postpone, Restart to update and Go back are offered.
    offered: [bool; 3],
}

/// The sheet's page: its lines and actions, or the question before a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Sheet,
    Confirm(Request),
}

impl Page {
    /// The request asked on a confirmation page, with its question and button.
    fn confirm(self) -> Option<(Request, (String, String))> {
        match self {
            Page::Sheet => None,
            Page::Confirm(request) => Some((request, question(request)?)),
        }
    }
}

/// What a widget of the sheet does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// The confirmation page, before the request.
    Ask(Request),
    /// Sends the request, if the state still offers it.
    Send(Request),
    /// Back to the sheet.
    Cancel,
}

/// The sheet's widgets that act.
struct Widgets {
    acts: Vec<(WidgetId, Act)>,
    /// The automatic security updates' switch, on the sheet of a readable state.
    switch: Option<WidgetId>,
}

impl Widgets {
    fn act(&self, widget: WidgetId) -> Option<Act> {
        self.acts
            .iter()
            .find(|(id, _)| *id == widget)
            .map(|(_, act)| *act)
    }
}

/// The type and ink of the sheet's lines and title, as calmo's `bar-popover-note` and
/// `bar-popover-title`, and the seal's colours, as the icon palette of `.athanor-seal`.
struct Style {
    note: TextStyle,
    secondary: Rgba,
    title: TextStyle,
    seal: Symbolic,
}

impl Style {
    fn of(appearance: &Appearance) -> Style {
        let palette = sal::palette(appearance);
        Style {
            note: TextStyle {
                scale: sal::size::CAPTION.scale,
                weight: sal::size::CAPTION.weight,
                ..TextStyle::default()
            },
            secondary: Rgba(palette.ink2),
            title: TextStyle {
                weight: 600,
                ..TextStyle::default()
            },
            seal: Symbolic {
                ink: Rgba(palette.ink),
                success: Rgba(palette.ok),
                warning: Rgba(palette.warn),
                error: Rgba(palette.bad),
            },
        }
    }
}

/// Wraps `label` at GTK's 40 characters.
fn wrap(ui: &mut Ui, label: WidgetId) {
    let mut wrap = Wrap::default();
    wrap.max_width_chars = NonZeroU32::new(NOTE_CHARS);
    ui.set_wrap(label, Some(wrap));
}

/// A line of the sheet.
fn note(ui: &mut Ui, parent: WidgetId, text: &str, style: &Style) -> WidgetId {
    let label = ui.add(parent, Kind::Label(text.to_owned()));
    wrap(ui, label);
    ui.set_text_style(label, style.note.clone());
    ui.set_ink(label, Some(style.secondary));
    label
}

/// The sheet's `page` of `shown`, in the popover of `cx`. BR6's order: the header with the
/// seal, the lines, a refusal, the switch, Postpone, then Restart to update and Go back. The
/// confirmation page asks as the power menu does, Cancel first, which takes the focus, so a
/// stray Enter backs out. `back_to`, the request whose question was cancelled, takes the focus
/// back on the sheet.
fn build(
    cx: &Cx,
    shown: &Shown,
    page: Page,
    back_to: Option<Request>,
    style: &Style,
    seal: Option<Image>,
) -> (Ui, Widgets) {
    let (mut ui, frame) = look::popover(&header(shown.badge), cx.theme.clone(), cx.rtl, cx.top);
    let mut acts = Vec::new();
    let mut switch = None;
    match page.confirm() {
        None => {
            let heading = ui.inline(frame, [0.0, 0.0]);
            // The seal is decoration: the title says it.
            if let Some(seal) = seal {
                ui.image(heading, seal, f32::from(SEAL), "");
            }
            let title = ui.add(heading, Kind::Label(header(shown.badge)));
            ui.set_text_style(title, style.title.clone());
            match &shown.sheet {
                Sheet::Unreadable(why) => {
                    note(&mut ui, frame, &unreadable_words(*why), style);
                }
                Sheet::Read(rows) => {
                    for line in lines(rows) {
                        note(&mut ui, frame, &line, style);
                    }
                }
            }
            if let Some(refusal) = shown.refusal {
                let words = refusal_words(refusal);
                let alert = ui.alert(frame, &words);
                note(&mut ui, alert, &words, style);
                ui.announce(&words, Priority::Assertive);
            }
            if let Sheet::Read(rows) = &shown.sheet {
                let on = rows.automatic_security_updates;
                switch = Some(ui.switch(frame, &tr("Automatic security updates"), on));
            }
            let [postpone, restart, go_back] = shown.offered;
            let offered = [
                (postpone, tr("Postpone"), Act::Send(Request::Postpone)),
                (restart, tr("Restart to update"), Act::Ask(Request::Apply)),
                (
                    go_back,
                    tr("Go back to the previous version"),
                    Act::Ask(Request::GoBack),
                ),
            ];
            if offered.iter().any(|(shown, _, _)| *shown) {
                let list = ui.add(frame, Kind::Menu);
                for (_, label, act) in offered.into_iter().filter(|(shown, _, _)| *shown) {
                    acts.push((ui.add(list, Kind::Button(label)), act));
                }
            }
            focus_before(&mut ui, switch, &acts, back_to);
        }
        Some((request, (asked, label))) => {
            let text = ui.add(frame, Kind::Label(asked));
            wrap(&mut ui, text);
            ui.set_text_style(text, style.title.clone());
            let buttons = ui.add(frame, Kind::Row);
            acts.push((ui.add(buttons, Kind::Button(tr("Cancel"))), Act::Cancel));
            acts.push((ui.add(buttons, Kind::Button(label)), Act::Send(request)));
        }
    }
    (ui, Widgets { acts, switch })
}

/// Gives the focus back to the widget whose question was cancelled, as GTK's Cancel did. A menu
/// popup puts the focus on the widget after the focused one when it opens (from none, on the
/// first), so the focus goes on the one before it: the sheet's focusable widgets are the switch
/// and the actions, in that order.
fn focus_before(
    ui: &mut Ui,
    switch: Option<WidgetId>,
    acts: &[(WidgetId, Act)],
    back_to: Option<Request>,
) {
    let Some(back_to) = back_to else { return };
    let chain: Vec<(WidgetId, Option<Request>)> = switch
        .map(|switch| (switch, Some(Request::SetAutomatic(false))))
        .into_iter()
        .chain(acts.iter().map(|(id, act)| match act {
            Act::Ask(request) | Act::Send(request) => (*id, Some(*request)),
            Act::Cancel => (*id, None),
        }))
        .collect();
    let at = chain
        .iter()
        .position(|(_, request)| *request == Some(back_to));
    if let Some(before) = at.and_then(|at| at.checked_sub(1)) {
        ui.set_focus(Some(chain[before].0));
    }
}

struct Shield {
    trust: Rc<Trust>,
    seals: IconTheme,
    /// The badge the buttons show.
    drawn: Option<Badge>,
    page: Page,
    /// The request whose question was cancelled, until the sheet is drawn again.
    back_to: Option<Request>,
    /// The open sheet's widgets, and what it shows.
    open: Option<(Widgets, Shown)>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    Some(Box::new(Shield {
        trust: Trust::start(start.wake.clone()),
        seals: IconTheme::system(SEAL_THEME),
        drawn: None,
        page: Page::Sheet,
        back_to: None,
        open: None,
    }))
}

impl Shield {
    fn seal(&self, badge: Badge, scale: u16, style: &Style) -> Option<Image> {
        let icon = shield::icon(badge);
        match self.seals.symbolic(icon, SEAL, scale, &style.seal) {
            Ok(image) => Some(image),
            Err(err) => {
                tracing::error!(error = %err, icon, "cannot draw the seal; the shield shows its name");
                None
            }
        }
    }
}

impl Module for Shield {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let badge = self.trust.badge();
        self.drawn = Some(badge);
        let name = header(badge);
        let style = Style::of(&Appearance::read());
        let button = match self.seal(badge, cx.scale, &style) {
            Some(seal) => ui.icon_button(group, &name, seal, f32::from(SEAL), None),
            None => ui.add(group, Kind::Button(name.clone())),
        };
        ui.set_tooltip(button, Some(&name));
        Some(button)
    }

    /// The badge ages without an event, and follows each answer of the service.
    fn stale(&self, _cx: &Cx) -> bool {
        self.drawn != Some(self.trust.badge())
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let shown = self.trust.shown();
        let style = Style::of(&Appearance::read());
        let seal = self.seal(shown.badge, cx.scale, &style);
        let back_to = self.back_to.take();
        let (ui, widgets) = build(cx, &shown, self.page, back_to, &style, seal);
        self.open = Some((widgets, shown));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        let Some((widgets, _)) = &self.open else {
            return Reply::Stay;
        };
        let act = match event {
            Event::Toggled(widget) if Some(widget) == widgets.switch => {
                // The switch only follows the state, which comes back when the service changes
                // it: turning on goes straight through, turning off asks first.
                let wanted = ui.is_active(widget);
                ui.set_active(widget, !wanted);
                if wanted {
                    Act::Send(Request::SetAutomatic(true))
                } else {
                    Act::Ask(Request::SetAutomatic(false))
                }
            }
            Event::Clicked(widget) => match widgets.act(widget) {
                Some(act) => act,
                None => return Reply::Stay,
            },
            _ => return Reply::Stay,
        };
        match act {
            Act::Ask(request) => {
                self.page = Page::Confirm(request);
                Reply::Reopen
            }
            Act::Cancel => {
                if let Page::Confirm(request) = self.page {
                    self.back_to = Some(request);
                }
                self.page = Page::Sheet;
                Reply::Reopen
            }
            Act::Send(request) => {
                self.trust.send(request);
                if self.page == Page::Sheet {
                    Reply::Stay
                } else {
                    self.page = Page::Sheet;
                    Reply::Reopen
                }
            }
        }
    }

    /// The sheet follows the state, unless a question is open.
    fn refresh(&mut self, _cx: &mut Cx, _ui: &mut Ui) -> Reply {
        match &self.open {
            Some((_, drawn)) if self.page == Page::Sheet && *drawn != self.trust.shown() => {
                Reply::Reopen
            }
            _ => Reply::Stay,
        }
    }

    /// A refusal is shown until the sheet closes.
    fn closed(&mut self) {
        self.page = Page::Sheet;
        self.back_to = None;
        self.open = None;
        self.trust.refusal.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_apps::sal::Icons;
    use athanor_sal::Theme;

    fn shown(offered: [bool; 3]) -> Shown {
        Shown {
            badge: Badge::Attention,
            sheet: Sheet::Unreadable(Unreadable::NoAnswer),
            refusal: None,
            offered,
        }
    }

    fn built(shown: &Shown, page: Page, back_to: Option<Request>) -> (Ui, Widgets) {
        let mut icons = Icons::new("Adwaita", Theme::default().text);
        let theme = Theme::default();
        let cx = Cx {
            icons: &mut icons,
            theme: &theme,
            rtl: false,
            top: true,
            scale: 1,
            connector: None,
        };
        let style = Style::of(&Appearance::default());
        build(&cx, shown, page, back_to, &style, None)
    }

    fn acts(widgets: &Widgets) -> Vec<Act> {
        widgets.acts.iter().map(|(_, act)| *act).collect()
    }

    #[test]
    fn only_the_requests_that_risk_something_are_asked_first() {
        assert!(question(Request::Apply).is_some());
        assert!(question(Request::GoBack).is_some());
        assert!(question(Request::SetAutomatic(false)).is_some());
        assert!(question(Request::SetAutomatic(true)).is_none());
        assert!(question(Request::Postpone).is_none());
    }

    #[test]
    fn an_unreadable_state_has_no_switch_and_only_the_actions_offered() {
        let (_, widgets) = built(&shown([false; 3]), Page::Sheet, None);
        assert!(widgets.switch.is_none() && widgets.acts.is_empty());
        let (_, widgets) = built(&shown([false, true, false]), Page::Sheet, None);
        assert_eq!(acts(&widgets), [Act::Ask(Request::Apply)]);
        let (_, widgets) = built(&shown([true; 3]), Page::Sheet, None);
        assert_eq!(
            acts(&widgets),
            [
                Act::Send(Request::Postpone),
                Act::Ask(Request::Apply),
                Act::Ask(Request::GoBack)
            ]
        );
    }

    #[test]
    fn the_question_puts_cancel_first_and_sends_only_from_its_own_button() {
        let (_, widgets) = built(&shown([true; 3]), Page::Confirm(Request::GoBack), None);
        assert_eq!(acts(&widgets), [Act::Cancel, Act::Send(Request::GoBack)]);
    }

    #[test]
    fn a_cancelled_question_puts_the_focus_before_the_action_that_asked() {
        let (ui, widgets) = built(&shown([true; 3]), Page::Sheet, Some(Request::GoBack));
        let restart = widgets
            .acts
            .iter()
            .find(|(_, act)| *act == Act::Ask(Request::Apply))
            .map(|(id, _)| *id);
        assert_eq!(ui.focus(), restart);
        // The first focusable widget needs nothing: the popup starts there.
        let (ui, _) = built(&shown([true; 3]), Page::Sheet, Some(Request::Postpone));
        assert_eq!(ui.focus(), None);
    }
}
