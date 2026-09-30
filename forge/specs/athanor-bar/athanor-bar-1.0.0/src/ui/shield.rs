//! The trust shield and its sheet (doc_bar.md BR6). The shield is the seal with SH12's
//! badge; its accessible name and tooltip are the sheet's header sentence, and nothing else
//! is written in the bar. The sheet is a popover of the bar, so it stacks and dismisses as
//! every popover does (P4), and it adds no layer surface. Every string from the state file
//! is set as plain text; "verified" and "refused" are our own words.

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

/// The badge ages: "checked within 14 days" turns false without the file changing.
const AGE_CHECK_SECS: u32 = 3600;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// The state file, read again whenever its directory changes (the system side renames a
/// new file into place, UT7) and once an hour for the badge's age.
pub struct Trust {
    bar: Weak<Bar>,
    read: RefCell<Result<State, ReadError>>,
    /// The last refusal of Apply or GoBack, shown in the sheet until the sheet closes.
    refusal: Cell<Option<Refusal>>,
    _monitor: Option<gio::FileMonitor>,
}

impl Trust {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Trust> {
        Rc::new_cyclic(|me: &Weak<Trust>| {
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
            let aging = me.clone();
            glib::timeout_add_seconds_local(AGE_CHECK_SECS, move || match aging.upgrade() {
                Some(trust) => {
                    trust.changed();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            Trust {
                bar: bar.clone(),
                read: RefCell::new(athanor_trust_state::read()),
                refusal: Cell::new(None),
                _monitor: monitor,
            }
        })
    }

    fn reload(&self) {
        *self.read.borrow_mut() = athanor_trust_state::read();
        self.changed();
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

/// Sends `request` to the update service on the system bus, with polkit allowed to ask.
/// A refusal is shown in the sheet: when the request came from the power menu, the sheet
/// opens on the first surface to show it.
pub fn request(bar: &Rc<Bar>, request: Request, from_sheet: bool) {
    let weak = Rc::downgrade(bar);
    glib::spawn_future_local(async move {
        let result = match gio::bus_get_future(gio::BusType::System).await {
            Ok(system) => bus::call(
                &system,
                shield::UPDATE_NAME,
                shield::UPDATE_PATH,
                shield::UPDATE_INTERFACE,
                request.method(),
                None,
                bus::INTERACTIVE_TIMEOUT_MS,
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
            if !from_sheet {
                bar.open_module(Module::Shield);
            }
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
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let seal = gtk4::Image::from_icon_name(shield::icon(bar.trust().badge()));
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
    let cancel_back = back.clone();
    cancel.connect_clicked(move |_| cancel_back());
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
                request(&bar, wanted, true);
            } else {
                bar.trust().refused(match wanted {
                    Request::Apply => Refusal::NothingDownloaded,
                    Request::GoBack => Refusal::NoPreviousVersion,
                });
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
    };
    ui.draw(bar);
    Some(Box::new(ui))
}
