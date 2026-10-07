//! The trust shield and its sheet (doc_bar.md BR6). The shield is the seal with SH12's
//! badge; its accessible name and tooltip are the sheet's header sentence, and nothing else
//! is written in the bar. The sheet is a popover of the bar, so it stacks and dismisses as
//! every popover does (P4), and it adds no layer surface. Every string from the state file
//! is set as plain text; "verified" and "refused" are our own words.
//!
//! The state comes from `os.athanor.Update1.State`, not from the file: the unit's sandbox
//! puts the bar in a user namespace where root is the overflow uid, so only the root service
//! can check the file's owner. The bar trusts the reply only when the bus says root sent it.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_bar::order::Module;
use athanor_bar::shield::{self, Refusal, Request, Rows, Sheet, Unreadable};
use athanor_trust_state::{Badge, ErrorCode, ReadError, Reason, State, UpdateState, STATE_PATH};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::bus;
use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

/// The state is asked again every hour: the badge ages ("checked within 14 days" turns false
/// without the file changing), and a directory watch that failed or missed an event heals.
const ASK_AGAIN_SECS: u32 = 3600;
/// After no answer, the next questions come this many seconds later, then hourly: a service
/// slow to start under the login's load must not leave the shield unverified for hours.
const RETRY_SECS: [u32; 3] = [5, 30, 300];
/// Apply and GoBack wait for the person at the polkit prompt however long it takes: GIO's
/// `G_MAXINT` means no timeout. The bus still answers NoReply if the service leaves first.
const REQUEST_TIMEOUT_MS: i32 = i32::MAX;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// The state, asked of the update service at start, every hour, whenever the file's directory
/// changes (the system side renames a new file into place, UT7), and soon again after no answer.
pub struct Trust {
    bar: Weak<Bar>,
    read: RefCell<Result<State, ReadError>>,
    /// The number of the latest question: an older answer arriving late is dropped.
    asked: Cell<u64>,
    /// The questions left unanswered in a row, which pick the next retry's delay.
    unanswered: Cell<usize>,
    /// The scheduled retry: a burst of unanswered questions waits for it, not for more, and
    /// an answer cancels it.
    retrying: RefCell<Option<glib::SourceId>>,
    /// The last refusal of Apply or GoBack, shown in the sheet until the sheet closes.
    refusal: Cell<Option<Refusal>>,
    _monitor: Option<gio::FileMonitor>,
}

impl Trust {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Trust> {
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
                bar: bar.clone(),
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
                trust.changed();
                trust.retry(answered);
            }
        });
    }

    /// After no answer, asks again after the next of `RETRY_SECS`; the hourly question
    /// takes over once they are spent.
    fn retry(self: &Rc<Self>, answered: bool) {
        if answered {
            self.unanswered.set(0);
            if let Some(pending) = self.retrying.take() {
                pending.remove();
            }
            return;
        }
        let missed = self.unanswered.get();
        let Some(&delay) = RETRY_SECS.get(missed) else { return };
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

    fn changed(&self) {
        if let Some(bar) = self.bar.upgrade() {
            bar.refresh(Changed::Trust);
        }
    }

    pub fn badge(&self) -> Badge {
        shield::badge(&self.read.borrow(), now())
    }

    pub fn sheet(&self) -> Sheet {
        shield::sheet(&self.read.borrow())
    }

    pub fn restart_to_update_offered(&self) -> bool {
        shield::restart_to_update_offered(&self.read.borrow())
    }

    pub fn go_back_offered(&self) -> bool {
        shield::go_back_offered(&self.read.borrow())
    }

    pub fn withdrawn(&self, request: Request) -> Refusal {
        shield::withdrawn(&self.read.borrow(), request)
    }

    pub fn refused(&self, refusal: Refusal) {
        self.refusal.set(Some(refusal));
        self.changed();
    }

    pub fn refusal(&self) -> Option<Refusal> {
        self.refusal.get()
    }

    pub fn take_refusal(&self) -> Option<Refusal> {
        self.refusal.take()
    }
}

/// Asks `os.athanor.Update1.State` on the system bus (the call starts the service), then
/// asks the bus which user sent the reply: the reply's sender is set by the bus, so this is
/// the uid of the connection that answered, not of whoever owns the name by then.
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
    let json = reply
        .body()
        .and_then(|body| body.get::<(String,)>())
        .map(|(json,)| json);
    let answer = if reply.message_type() == gio::DBusMessageType::Error {
        Err(reply.error_name())
    } else {
        Ok(json)
    };
    let read = match &answer {
        Ok(json) => shield::state_reply(Ok(json.as_deref().unwrap_or_default()), sender_uid),
        Err(name) => shield::state_reply(Err(name.as_deref()), sender_uid),
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

/// Sends `request` to the update service on the system bus, with polkit allowed to ask.
/// A refusal is shown in the sheet, opened on the surface of `origin`, the button whose menu
/// made the request: the polkit agent takes the focus, which closes an open sheet.
pub fn request(bar: &Rc<Bar>, request: Request, origin: &gtk4::Widget) {
    let weak = Rc::downgrade(bar);
    let origin = origin.downgrade();
    glib::spawn_future_local(async move {
        let result = match gio::bus_get_future(gio::BusType::System).await {
            Ok(system) => bus::call(
                &system,
                shield::UPDATE_NAME,
                shield::UPDATE_PATH,
                shield::UPDATE_INTERFACE,
                request.method(),
                None,
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
        if let Some(bar) = weak.upgrade() {
            bar.trust().refused(refusal);
            bar.open_module_near(Module::Shield, origin.upgrade().as_ref());
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
            tr("You went back from the newest version; only a newer one is offered")
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
        Refusal::NoAnswer => tr("The update service did not answer"),
    }
}

/// A date of the file in the locale's format, in UTC: the build and check times are UTC.
fn date(epoch: i64) -> String {
    glib::DateTime::from_unix_utc(epoch)
        .and_then(|time| time.format("%x"))
        .map_or_else(|_| epoch.to_string(), |text| text.to_string())
}

fn note(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_wrap(true);
    label.set_max_width_chars(40);
    label.set_xalign(0.0);
    label.add_css_class("bar-popover-note");
    label
}

fn fill_sheet(content: &gtk4::Box, trust: &Trust, actions: &Actions) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    let badge = trust.badge();
    let heading = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let seal = gtk4::Image::builder()
        .icon_name(shield::icon(badge))
        .accessible_role(gtk4::AccessibleRole::Presentation)
        .build();
    seal.add_css_class("athanor-seal");
    let title = gtk4::Label::new(Some(&header(badge)));
    title.add_css_class("bar-popover-title");
    title.set_xalign(0.0);
    heading.append(&seal);
    heading.append(&title);
    content.append(&heading);
    match trust.sheet() {
        Sheet::Unreadable(why) => content.append(&note(&unreadable_words(why))),
        Sheet::Read(rows) => append_rows(content, &rows),
    }
    if let Some(refusal) = trust.refusal() {
        let refused = gtk4::Label::builder()
            .label(refusal_words(refusal))
            .wrap(true)
            .max_width_chars(40)
            .xalign(0.0)
            .accessible_role(gtk4::AccessibleRole::Alert)
            .build();
        refused.add_css_class("bar-popover-note");
        content.append(&refused);
    }
    actions
        .restart
        .set_visible(trust.restart_to_update_offered());
    actions.go_back.set_visible(trust.go_back_offered());
    content.append(&actions.row);
}

fn append_rows(content: &gtk4::Box, rows: &Rows) {
    content.append(&note(&tr_with(
        "Version {version}",
        "version",
        &rows.version,
    )));
    content.append(&note(&tr_with(
        "Built on {date}",
        "date",
        &date(rows.build_time),
    )));
    content.append(&note(&reason_words(rows.reason)));
    content.append(&note(&match rows.last_check {
        Some(at) => tr_with("Last checked on {date}", "date", &date(at)),
        None => tr("Never checked"),
    }));
    content.append(&note(&update_words(rows.update)));
    if let Some((code, host)) = &rows.error {
        content.append(&note(&error_words(*code)));
        if let Some(host) = host {
            content.append(&note(&tr_with("Host: {host}", "host", host)));
        }
    }
    content.append(&note(&if rows.policy_in_force {
        tr("Signature policy in force")
    } else {
        tr("Signature policy not in force")
    }));
    content.append(&note(&if rows.policy_shipped {
        tr("The policy is the one Athanor ships")
    } else {
        tr("The policy was changed on this machine")
    }));
    content.append(&note(&if rows.secure_boot_on {
        tr("Secure Boot on")
    } else {
        tr("Secure Boot off: this machine runs in the declared degraded mode")
    }));
}

/// The sheet's two actions; built once per shield and moved into each fill of the sheet.
struct Actions {
    row: gtk4::Box,
    restart: gtk4::Button,
    go_back: gtk4::Button,
}

struct ShieldUi {
    popup: Popup,
    seal: gtk4::Image,
    content: gtk4::Box,
    actions: Actions,
    /// What the sheet shows now. A rebuild drops the keyboard focus and announces the
    /// refusal again, so the sheet is rebuilt only when this changes.
    drawn: RefCell<Option<(Badge, Sheet, Option<Refusal>)>>,
}

impl ModuleUi for ShieldUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed == Changed::Trust {
            self.draw(bar);
        }
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

impl ShieldUi {
    fn draw(&self, bar: &Rc<Bar>) {
        let trust = bar.trust();
        let badge = trust.badge();
        let drawn = Some((badge, trust.sheet(), trust.refusal()));
        if *self.drawn.borrow() == drawn {
            return;
        }
        let name = header(badge);
        self.seal.set_icon_name(Some(shield::icon(badge)));
        self.popup.button.set_tooltip_text(Some(&name));
        self.popup
            .button
            .update_property(&[gtk4::accessible::Property::Label(&name)]);
        // Moving the action row out of the old fill before the new one appends it.
        if let Some(parent) = self.actions.row.parent().and_downcast::<gtk4::Box>() {
            parent.remove(&self.actions.row);
        }
        fill_sheet(&self.content, trust, &self.actions);
        self.drawn.replace(drawn);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    // The button carries the name; the seal inside it is decoration, as in the sheet.
    let seal = gtk4::Image::builder()
        .icon_name(shield::icon(bar.trust().badge()))
        .accessible_role(gtk4::AccessibleRole::Presentation)
        .build();
    seal.add_css_class("athanor-seal");
    let popup = Popup::new(bar, &seal, &header(bar.trust().badge()));

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let restart = gtk4::Button::with_label(&tr("Restart to update"));
    restart.add_css_class("bar-row");
    let go_back = gtk4::Button::with_label(&tr("Go back to the previous version"));
    go_back.add_css_class("bar-row");
    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    row.append(&restart);
    row.append(&go_back);

    // The confirmation page, as in the power menu: Cancel focused, so a stray Enter backs out.
    let question = gtk4::Label::new(None);
    question.add_css_class("bar-popover-title");
    question.set_wrap(true);
    question.set_max_width_chars(40);
    question.set_xalign(0.0);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    cancel.add_css_class("bar-row");
    let confirm = gtk4::Button::new();
    confirm.add_css_class("bar-confirm");
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&cancel);
    buttons.append(&confirm);
    let confirm_page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    confirm_page.append(&question);
    confirm_page.append(&buttons);
    let stack = gtk4::Stack::new();
    stack.add_named(&content, Some("sheet"));
    stack.add_named(&confirm_page, Some("confirm"));
    popup.popover.set_child(Some(&stack));

    let pending: Rc<Cell<Option<Request>>> = Rc::new(Cell::new(None));
    for (button, wanted, ask, label) in [
        (
            &restart,
            Request::Apply,
            tr("Restart and install the update now?"),
            tr("Restart to update"),
        ),
        (
            &go_back,
            Request::GoBack,
            tr("Go back to the previous version and restart? This asks for an administrator's password."),
            tr("Go back"),
        ),
    ] {
        let (pending, question, confirm, stack, cancel) = (
            pending.clone(),
            question.downgrade(),
            confirm.downgrade(),
            stack.downgrade(),
            cancel.downgrade(),
        );
        button.connect_clicked(move |_| {
            let (Some(question), Some(confirm), Some(stack), Some(cancel)) = (
                question.upgrade(),
                confirm.upgrade(),
                stack.upgrade(),
                cancel.upgrade(),
            ) else {
                return;
            };
            pending.set(Some(wanted));
            question.set_text(&ask);
            confirm.set_label(&label);
            stack.set_visible_child_name("confirm");
            cancel.grab_focus();
        });
    }
    let back = {
        let (pending, stack) = (pending.clone(), stack.downgrade());
        move || {
            pending.set(None);
            if let Some(stack) = stack.upgrade() {
                stack.set_visible_child_name("sheet");
            }
        }
    };
    {
        // Cancel gives the focus back to the action that asked.
        let (back, pending) = (back.clone(), pending.clone());
        let (restart, go_back) = (restart.downgrade(), go_back.downgrade());
        cancel.connect_clicked(move |_| {
            let opener = match pending.get() {
                Some(Request::Apply) => restart.upgrade(),
                Some(Request::GoBack) => go_back.upgrade(),
                None => None,
            };
            back();
            if let Some(opener) = opener {
                opener.grab_focus();
            }
        });
    }
    {
        let bar = Rc::downgrade(bar);
        let back = back.clone();
        popup.popover.connect_closed(move |_| {
            back();
            // A refusal is shown until the sheet closes.
            if let Some(bar) = bar.upgrade() {
                if bar.trust().take_refusal().is_some() {
                    bar.refresh(Changed::Trust);
                }
            }
        });
    }
    {
        let bar = Rc::downgrade(bar);
        let origin = popup.button.downgrade();
        confirm.connect_clicked(move |_| {
            let Some(wanted) = pending.take() else { return };
            back();
            let Some(bar) = bar.upgrade() else { return };
            // The file may have changed while the question was open (Review Focus 1).
            let still = match wanted {
                Request::Apply => bar.trust().restart_to_update_offered(),
                Request::GoBack => bar.trust().go_back_offered(),
            };
            if still {
                if let Some(origin) = origin.upgrade() {
                    request(&bar, wanted, origin.upcast_ref());
                }
            } else {
                bar.trust().refused(bar.trust().withdrawn(wanted));
            }
        });
    }

    let ui = ShieldUi {
        popup,
        seal,
        content,
        actions: Actions {
            row,
            restart,
            go_back,
        },
        drawn: RefCell::new(None),
    };
    ui.draw(bar);
    Some(Box::new(ui))
}
