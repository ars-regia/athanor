//! The greeter: the first Athanor surface drawn on the Calmo tokens.
//!
//! It reads nothing from COSMIC: the accent is the factory accent, and the variant comes
//! from ATHANOR_GREETER_VARIANT and from the high-contrast toggle (doc_shell.md, SH5).
//! Every interactive widget carries an accessible name and every string goes through
//! gettext (SH13).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_trust_state::{ReadError, State};

use athanor_style::calmo::{self, Variant};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Application, ApplicationWindow, Box, Button, Entry, Image, Label, ListBox,
    ListBoxRow, Orientation, PasswordEntry, ScrolledWindow, Stack, ToggleButton,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use tokio::sync::oneshot;

use crate::accounts::{typed_name_is_valid, Account};
use crate::auth::{authenticate_interactive, Conversation, Outcome, Prompt};
use crate::i18n::{tr, tr_with};

/// The seal's state is asked again every hour: the badge ages ("checked within 14 days"
/// turns false without the state changing).
const SEAL_AGAIN_SECS: u32 = 3600;
/// After no answer, the next questions come this many seconds later, then hourly: the
/// greeter starts with the machine, and a service slow to start under the boot's load must
/// not leave the seal unverified for an hour. The bar's delays (doc_bar.md, BR6).
const SEAL_RETRY_SECS: [u32; 3] = [5, 30, 300];

/// The icons come from the theme the image ships (doc_shell.md, SH5: cosmic-icon-theme
/// stays in stage 1); the seal is ours, installed into hicolor, which every theme inherits.
const ICON_THEME: &str = "Cosmic";

fn initial_variant() -> Variant {
    std::env::var("ATHANOR_GREETER_VARIANT")
        .ok()
        .and_then(|name| Variant::from_name(&name))
        .unwrap_or(Variant::Light)
}

fn named<W: IsA<gtk4::Widget> + IsA<gtk4::Accessible>>(widget: &W, id: &str, label: &str) {
    widget.set_widget_name(id);
    widget.update_property(&[Property::Label(label)]);
}

fn icon_chip(id: &str, icon: &str, label: &str) -> Button {
    let button = Button::builder()
        .icon_name(icon)
        .css_classes(["greeter-chip"])
        .tooltip_text(label)
        .build();
    named(&button, id, label);
    button
}

/// The seal of SH12 (package 1d): the badge the trust state backs, its header as words and
/// accessible name, the reason as tooltip. The state is asked of the update service at
/// start, every hour and soon again after no answer.
fn seal_chip() -> Box {
    let seal = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .css_classes(["greeter-chip"])
        .build();
    seal.set_widget_name("greeter-seal");
    let icon = Image::builder()
        .css_classes(["athanor-seal"])
        .pixel_size(24)
        .build();
    let words = Label::new(None);
    seal.append(&icon);
    seal.append(&words);
    let show: Rc<ShowSeal> = {
        let seal = seal.clone();
        Rc::new(move |read| {
            let now = crate::seal::now();
            let badge = crate::seal::badge(read, now);
            let header = crate::seal::header(badge);
            icon.set_icon_name(Some(crate::seal::icon(badge)));
            icon.update_property(&[Property::Label(&header)]);
            words.set_label(&header);
            seal.set_tooltip_text(Some(&crate::seal::detail(read, now)));
        })
    };
    // Until the service answers, nothing backs the badge.
    show(&Err(ReadError::Io(std::io::ErrorKind::NotConnected)));
    let asked = Rc::new(Cell::new(0));
    ask_seal(&show, &asked, 0);
    glib::timeout_add_seconds_local(SEAL_AGAIN_SECS, move || {
        ask_seal(&show, &asked, 0);
        glib::ControlFlow::Continue
    });
    seal
}

/// Draws the seal for one answer of the update service.
type ShowSeal = dyn Fn(&Result<State, ReadError>);

/// Asks the state once and shows the answer, unless a newer question was asked meanwhile.
/// After no answer the next of `SEAL_RETRY_SECS` follows; `missed` counts them.
fn ask_seal(show: &Rc<ShowSeal>, asked: &Rc<Cell<u64>>, missed: usize) {
    let question = asked.get().wrapping_add(1);
    asked.set(question);
    let (show, asked) = (show.clone(), asked.clone());
    glib::MainContext::default().spawn_local(async move {
        let read = crate::seal::ask().await;
        if asked.get() != question {
            return;
        }
        show(&read);
        if let (Err(ReadError::Io(_)), Some(&delay)) = (&read, SEAL_RETRY_SECS.get(missed)) {
            glib::timeout_add_seconds_local_once(delay, move || {
                ask_seal(&show, &asked, missed + 1)
            });
        }
    });
}

fn now_text(format: &str) -> String {
    glib::DateTime::now_local()
        .and_then(|now| now.format(format))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

/// The avatar of a card: the initial of its name on the accent, or, for "Other user", a
/// generic person. No picture: the greeter decodes no image an account supplied and reads
/// no file under a home directory (doc_lock_and_prompts.md, LP19).
#[derive(Clone)]
struct Avatar {
    disc: Box,
    initial: Label,
    anyone: Image,
}

impl Avatar {
    fn new() -> Self {
        let disc = Box::builder()
            .css_classes(["greeter-avatar"])
            .halign(Align::Center)
            .valign(Align::Center)
            .build();
        let initial = Label::builder()
            .hexpand(true)
            .halign(Align::Center)
            .valign(Align::Center)
            .build();
        let anyone = Image::builder()
            .icon_name("avatar-default-symbolic")
            .pixel_size(28)
            .hexpand(true)
            .halign(Align::Center)
            .valign(Align::Center)
            .visible(false)
            .build();
        disc.append(&initial);
        disc.append(&anyone);
        Avatar {
            disc,
            initial,
            anyone,
        }
    }

    /// The initial of `account`, or the generic person when there is none.
    fn show(&self, account: Option<&Account>) {
        self.initial
            .set_label(&account.map(Account::initial).unwrap_or_default());
        self.initial.set_visible(account.is_some());
        self.anyone.set_visible(account.is_none());
    }
}

/// One entry of the list: the avatar, the full name and, when another account has the same
/// one, the user name; None is "Other user". A screen reader reads "Ada Lovelace, ada".
fn account_row(account: Option<&Account>) -> ListBoxRow {
    let avatar = Avatar::new();
    avatar.show(account);
    let (name, accessible) = match account {
        Some(account) => (account.full_name.clone(), account.accessible_name()),
        None => (tr("Other user"), tr("Other user")),
    };
    let lines = Box::builder()
        .orientation(Orientation::Vertical)
        .valign(Align::Center)
        .build();
    lines.append(
        &Label::builder()
            .label(&name)
            .css_classes(["greeter-name"])
            .halign(Align::Start)
            .build(),
    );
    if let Some(account) = account.filter(|account| account.shows_user) {
        lines.append(
            &Label::builder()
                .label(&account.user)
                .css_classes(["greeter-status"])
                .halign(Align::Start)
                .build(),
        );
    }
    let content = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(14)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    content.append(&avatar.disc);
    content.append(&lines);
    let row = ListBoxRow::builder().child(&content).build();
    row.update_property(&[Property::Label(&accessible)]);
    row
}

/// The entry a typed `letter` moves to: the next after `from` whose name starts with it,
/// round the end of the list; None when no name does.
fn next_match(names: &[String], from: usize, letter: char) -> Option<usize> {
    let letter: String = letter.to_lowercase().collect();
    (1..=names.len())
        .map(|step| (from + step) % names.len())
        .find(|&index| names[index].to_lowercase().starts_with(&letter))
}

/// The keyboard layout, read from the seat. Hidden when the seat names none: the chip
/// never shows a constant.
fn layout_chip(display: &gdk::Display) -> Label {
    let chip = Label::builder()
        .css_classes(["greeter-chip"])
        .visible(false)
        .build();
    chip.set_widget_name("greeter-layout");
    let Some(keyboard) = display.default_seat().and_then(|seat| seat.keyboard()) else {
        return chip;
    };
    let refresh = {
        let chip = chip.clone();
        move |keyboard: &gdk::Device| {
            let names = keyboard.layout_names();
            let active = usize::try_from(keyboard.active_layout_index())
                .ok()
                .and_then(|index| names.get(index));
            match active {
                Some(name) => {
                    chip.set_label(name);
                    chip.update_property(&[Property::Label(&tr_with(
                        "Keyboard layout: {layout}",
                        "layout",
                        name,
                    ))]);
                    chip.set_visible(true);
                }
                None => chip.set_visible(false),
            }
        }
    };
    refresh(&keyboard);
    keyboard.connect_active_layout_index_notify(refresh.clone());
    keyboard.connect_layout_names_notify(refresh);
    chip
}

#[derive(Clone, Copy)]
enum Power {
    Suspend,
    Restart,
    ShutDown,
}

fn power_chip(id: &str, icon: &str, label: &str, which: Power) -> Button {
    let button = icon_chip(id, icon, label);
    button.connect_clicked(move |_| {
        glib::MainContext::default().spawn_local(async move {
            let result = async {
                let connection = zbus::Connection::system().await?;
                let proxy = crate::power::LogindProxy::new(&connection).await?;
                match which {
                    Power::Suspend => proxy.suspend(true).await,
                    Power::Restart => proxy.reboot(true).await,
                    Power::ShutDown => proxy.power_off(true).await,
                }
            }
            .await;
            if let Err(err) = result {
                tracing::error!(error = %err, "logind refused the power request");
            }
        });
    });
    button
}

/// Names a text field for a screen reader: the entry and the text widget inside it, which
/// is the node a screen reader lands on.
fn label_field<W: IsA<gtk4::Editable> + IsA<gtk4::Accessible>>(field: &W, label: &str) {
    field.update_property(&[Property::Label(label)]);
    if let Some(delegate) = field.delegate() {
        delegate.update_property(&[Property::Label(label)]);
    }
}

/// What the card says of one sign-in besides its fields, kept free of GTK so that its rules
/// are tested: PAM's PAM_TEXT_INFO and PAM_ERROR_MSG lines, apart from the greeter's own
/// line, and whether a question of PAM's is open, which is when it can be cancelled.
#[derive(Debug, Default, PartialEq)]
struct Notices {
    info: Vec<String>,
    error: Vec<String>,
    cancellable: bool,
}

impl Notices {
    /// A new sign-in starts: nothing of the previous one is left on the card.
    fn start(&mut self) {
        *self = Notices::default();
    }

    /// PAM put a question: it can be cancelled until it is answered.
    fn ask(&mut self) {
        self.cancellable = true;
    }

    /// The question is answered, cancelled or gone with the sign-in; PAM's lines stay.
    fn close_question(&mut self) {
        self.cancellable = false;
    }
}

/// Which sign-in owns the card. The greeter never has two greetd sessions open: a sign-in
/// starts only when the last one has finished talking to greetd. And a sign-in the person
/// left, by going back to the list, never draws on the card of the next.
#[derive(Debug, Default)]
struct Turn {
    attempt: u64,
    conversing: bool,
}

impl Turn {
    /// A sign-in starts: its number, or None while the previous one is still talking to
    /// greetd (a cancelled one sends CancelSession on its way out).
    fn begin(&mut self) -> Option<u64> {
        if self.conversing {
            return None;
        }
        self.conversing = true;
        self.attempt += 1;
        Some(self.attempt)
    }

    /// The person left the card: what the running sign-in says next is not drawn.
    fn leave(&mut self) {
        self.attempt += 1;
    }

    /// Sign-in `attempt` is over: whether its outcome is still the card's to draw.
    fn end(&mut self, attempt: u64) -> bool {
        self.conversing = false;
        self.attempt == attempt
    }
}

/// The card's fields as one sign-in sees them: PAM's messages are shown on it, and its
/// further questions are put on it, each answered from the field its type calls for.
#[derive(Clone)]
struct SignInForm {
    password: PasswordEntry,
    password_label: Rc<RefCell<String>>,
    /// "Other user"'s field, shown only on its card and only until PAM asks.
    user_name: Entry,
    /// The card is "Other user"'s.
    other: Rc<Cell<bool>>,
    /// Back to the list, beside the field; hidden when there is no list.
    back: Button,
    turn: Rc<RefCell<Turn>>,
    answer: Entry,
    question: Label,
    submit: Button,
    cancel: Button,
    /// The greeter's own line of progress.
    status: Label,
    /// The greeter's line for a sign-in that failed.
    error: Label,
    /// PAM's PAM_TEXT_INFO and PAM_ERROR_MSG lines, drawn from `notices`.
    pam_info: Label,
    pam_error: Label,
    notices: Rc<RefCell<Notices>>,
    /// Where the answer to PAM's open question goes, while one is open.
    pending: Rc<RefCell<Option<oneshot::Sender<zeroize::Zeroizing<String>>>>>,
}

impl SignInForm {
    /// Nothing can be typed while greetd has the turn.
    fn wait(&self) {
        self.password.set_sensitive(false);
        self.user_name.set_sensitive(false);
        self.answer.set_sensitive(false);
        self.submit.set_sensitive(false);
        self.back.set_sensitive(false);
        self.notices.borrow_mut().close_question();
        self.show_notices();
    }

    /// A new sign-in starts: the messages of the previous one go.
    fn start(&self) {
        self.error.set_visible(false);
        self.notices.borrow_mut().start();
        self.show_notices();
    }

    /// Draws `notices` on the card.
    fn show_notices(&self) {
        let notices = self.notices.borrow();
        for (label, lines) in [
            (&self.pam_info, &notices.info),
            (&self.pam_error, &notices.error),
        ] {
            label.set_label(&lines.join("\n"));
            label.set_visible(!lines.is_empty());
        }
        self.cancel.set_visible(notices.cancellable);
    }

    /// The card as it was before the sign-in: the password field, ready, or on "Other
    /// user"'s card the name field, emptied, so that a password typed there by mistake
    /// stays neither on screen nor in the accessibility tree.
    fn reset(&self) {
        self.pending.take();
        self.notices.borrow_mut().close_question();
        self.show_notices();
        self.question.set_visible(false);
        self.answer.set_text("");
        self.answer.set_visible(false);
        self.password.set_text("");
        label_field(&self.password, &self.password_label.borrow());
        self.user_name.set_text("");
        let other = self.other.get();
        self.user_name.set_visible(other);
        self.user_name.set_sensitive(true);
        self.password.set_visible(!other);
        self.password.set_sensitive(true);
        self.submit.set_sensitive(true);
        self.back.set_sensitive(true);
        if other {
            self.user_name.grab_focus();
        } else {
            self.password.grab_focus();
        }
    }
}

impl Conversation for SignInForm {
    fn progress(&self, text: &str) {
        self.status.set_label(text);
        self.status.set_visible(true);
    }

    fn info(&self, text: &str) {
        self.notices.borrow_mut().info.push(text.to_string());
        self.show_notices();
    }

    fn error(&self, text: &str) {
        self.notices.borrow_mut().error.push(text.to_string());
        self.show_notices();
    }

    fn ask(
        &self,
        prompt: Prompt,
    ) -> impl std::future::Future<Output = Option<zeroize::Zeroizing<String>>> {
        let (reply, answer) = oneshot::channel();
        self.pending.replace(Some(reply));
        self.status.set_visible(false);
        let (text, secret) = match prompt {
            Prompt::Secret(text) => (text, true),
            Prompt::Visible(text) => (text, false),
        };
        self.question.set_label(&text);
        self.question.set_visible(true);
        self.user_name.set_visible(false);
        self.back.set_sensitive(true);
        self.password.set_visible(secret);
        self.answer.set_visible(!secret);
        let field: &gtk4::Widget = if secret {
            label_field(&self.password, &text);
            self.password.upcast_ref()
        } else {
            label_field(&self.answer, &text);
            self.answer.upcast_ref()
        };
        field.set_sensitive(true);
        self.submit.set_sensitive(true);
        self.notices.borrow_mut().ask();
        self.show_notices();
        field.grab_focus();
        async move { answer.await.ok() }
    }
}

pub fn build_ui(app: &Application, accounts: &[Account]) {
    // The direction comes from the language of the catalog in use, not from a process
    // locale, and has to be set before the first widget exists.
    if crate::i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Athanor")
        .build();
    window.add_css_class("athanor-surface");
    window.add_css_class("athanor-greeter");

    window.init_layer_shell();
    if let Err(reason) = crate::layer_guard::require_layer_surface(&window) {
        // No tracing subscriber may be listening this early in a failing start; stderr
        // reaches the journal through the compositor's systemd-cat.
        eprintln!("athanor-greeter-ui: not a layer surface: {reason}");
        std::process::exit(1);
    }
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace(Some("greeter"));
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }

    let display = gtk4::prelude::WidgetExt::display(&window);
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_icon_theme_name(Some(ICON_THEME));
    }
    let variant = initial_variant();
    calmo::load(&display, variant);

    // Top: the wordmark on the left, the seal on the right.
    let wordmark = Label::builder()
        .label("Athanor")
        .css_classes(["greeter-wordmark"])
        .build();
    let seal = seal_chip();
    let top = gtk4::CenterBox::builder()
        .margin_top(12)
        .margin_start(20)
        .margin_end(16)
        .build();
    top.set_start_widget(Some(&wordmark));
    top.set_end_widget(Some(&seal));

    // Centre: the clock, the date, the card.
    let clock = Label::builder().css_classes(["greeter-clock"]).build();
    let date = Label::builder()
        .css_classes(["greeter-date"])
        .margin_top(8)
        .margin_bottom(34)
        .build();
    let tick = {
        let (clock, date) = (clock.clone(), date.clone());
        move || {
            clock.set_label(&now_text("%H:%M"));
            date.set_label(&now_text(&tr("%A %-d %B")));
        }
    };
    tick();
    glib::timeout_add_seconds_local(1, move || {
        tick();
        glib::ControlFlow::Continue
    });

    // The cards (doc_lock_and_prompts.md, LP19): the list, when there is one, and the card
    // of the entry selected, with its password field. Without a list, "Other user" alone.
    let accounts: Rc<[Account]> = accounts.into();
    let listed = !accounts.is_empty();
    // The account whose card is open; None is "Other user".
    let selected: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    let avatar = Avatar::new();
    let name = Label::builder()
        .css_classes(["greeter-name"])
        .margin_top(10)
        .build();
    // The user name, under a full name that another listed account shares.
    let user_line = Label::builder()
        .css_classes(["greeter-status"])
        .visible(false)
        .build();
    let name_lines = Box::builder()
        .orientation(Orientation::Vertical)
        .margin_bottom(14)
        .build();
    name_lines.append(&name);
    name_lines.append(&user_line);

    let password = PasswordEntry::builder()
        .placeholder_text(tr("Password"))
        .show_peek_icon(true)
        .hexpand(true)
        .css_classes(["greeter-field"])
        .build();
    // Its accessible name, "Password for {name}", is the open card's.
    password.set_widget_name("greeter-password");
    // "Other user"'s name, sent to greetd as typed.
    let user_name = Entry::builder()
        .placeholder_text(tr("User name"))
        .hexpand(true)
        .css_classes(["greeter-field"])
        .visible(false)
        .build();
    named(&user_name, "greeter-user-name", &tr("User name"));
    label_field(&user_name, &tr("User name"));
    // The field for PAM's questions whose answer is shown as it is typed (a one-time code):
    // hidden until PAM asks one.
    let answer = Entry::builder()
        .hexpand(true)
        .css_classes(["greeter-field"])
        .visible(false)
        .build();
    answer.set_widget_name("greeter-answer");
    let back = Button::builder()
        .icon_name("go-previous-symbolic")
        .css_classes(["greeter-chip"])
        .valign(Align::Center)
        .tooltip_text(tr("Back to the list"))
        .visible(listed)
        .build();
    named(&back, "greeter-back", &tr("Back to the list"));
    let submit = Button::builder()
        .icon_name("go-next-symbolic")
        .css_classes(["greeter-submit"])
        .valign(Align::Center)
        .build();
    named(&submit, "greeter-submit", &tr("Sign in"));
    // Ends an open question of PAM's, and with it the sign-in: shown only while one is open.
    let cancel = Button::builder()
        .label(tr("Cancel"))
        .valign(Align::Center)
        .visible(false)
        .build();
    named(&cancel, "greeter-cancel", &tr("Cancel"));
    let field_row = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    field_row.append(&back);
    field_row.append(&user_name);
    field_row.append(&password);
    field_row.append(&answer);
    field_row.append(&submit);
    field_row.append(&cancel);

    // PAM's question after the password (a new password, a one-time code), in its words.
    let question = Label::builder()
        .css_classes(["greeter-status"])
        .margin_bottom(8)
        .wrap(true)
        .visible(false)
        .build();
    let status = Label::builder()
        .css_classes(["greeter-status"])
        .margin_top(12)
        .wrap(true)
        .visible(false)
        .build();
    let error = Label::builder()
        .css_classes(["greeter-error"])
        .margin_top(12)
        .wrap(true)
        .visible(false)
        .build();
    // A failed sign-in is announced, not only painted.
    error.set_accessible_role(gtk4::AccessibleRole::Alert);
    let pam_info = Label::builder()
        .css_classes(["greeter-status"])
        .margin_bottom(8)
        .wrap(true)
        .visible(false)
        .build();
    let pam_error = Label::builder()
        .css_classes(["greeter-error"])
        .margin_bottom(8)
        .wrap(true)
        .visible(false)
        .build();
    pam_error.set_accessible_role(gtk4::AccessibleRole::Alert);

    // Typing hides the greeter's own lines; PAM's stay until the next sign-in starts.
    for field in [
        password.upcast_ref::<gtk4::Editable>(),
        user_name.upcast_ref(),
        answer.upcast_ref(),
    ] {
        let (status, error) = (status.clone(), error.clone());
        field.connect_changed(move |_| {
            status.set_visible(false);
            error.set_visible(false);
        });
    }

    let form = SignInForm {
        password: password.clone(),
        password_label: Rc::new(RefCell::new(String::new())),
        user_name: user_name.clone(),
        other: Rc::new(Cell::new(false)),
        back: back.clone(),
        turn: Rc::new(RefCell::new(Turn::default())),
        answer: answer.clone(),
        question: question.clone(),
        submit: submit.clone(),
        cancel: cancel.clone(),
        status: status.clone(),
        error: error.clone(),
        pam_info: pam_info.clone(),
        pam_error: pam_error.clone(),
        notices: Rc::new(RefCell::new(Notices::default())),
        pending: Rc::new(RefCell::new(None)),
    };
    {
        let form = form.clone();
        cancel.connect_clicked(move |_| {
            // Dropping the sender leaves the question unanswered: the conversation tells
            // greetd to cancel the session, and the card goes back as it was.
            if form.pending.take().is_some() {
                form.wait();
            }
        });
    }

    let card = Box::builder()
        .orientation(Orientation::Vertical)
        .halign(Align::Center)
        .css_classes(["greeter-card"])
        .build();
    card.append(&avatar.disc);
    card.append(&name_lines);
    card.append(&pam_info);
    card.append(&pam_error);
    card.append(&question);
    card.append(&field_row);
    card.append(&status);
    card.append(&error);

    let pages = Stack::builder()
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    pages.add_named(&card, Some("card"));

    // Opens the card of `index`, or "Other user"'s for None, with its field ready.
    let open: Rc<dyn Fn(Option<usize>)> = Rc::new({
        let (accounts, selected, form, pages) = (
            accounts.clone(),
            selected.clone(),
            form.clone(),
            pages.clone(),
        );
        move |index| {
            let account = index.and_then(|index| accounts.get(index));
            selected.set(account.and(index));
            avatar.show(account);
            match account {
                Some(account) => {
                    name.set_label(&account.full_name);
                    user_line.set_label(&account.user);
                    user_line.set_visible(account.shows_user);
                    form.password_label.replace(tr_with(
                        "Password for {name}",
                        "name",
                        &account.full_name,
                    ));
                }
                None => {
                    name.set_label(&tr("Other user"));
                    user_line.set_visible(false);
                }
            }
            form.other.set(account.is_none());
            form.start();
            form.status.set_visible(false);
            pages.set_visible_child_name("card");
            form.reset();
        }
    });

    // The list: one entry per account, then "Other user". Never built when it is hidden,
    // so that no account name reaches the accessibility tree.
    let list = listed.then(|| {
        let list = ListBox::builder()
            .css_classes(["navigation-sidebar"])
            .build();
        list.update_property(&[Property::Label(&tr("Accounts"))]);
        for account in accounts.iter() {
            list.append(&account_row(Some(account)));
        }
        list.append(&account_row(None));
        list.connect_selected_rows_changed(|list| {
            let mut index = 0;
            while let Some(row) = list.row_at_index(index) {
                row.update_state(&[gtk4::accessible::State::Selected(Some(row.is_selected()))]);
                index += 1;
            }
        });
        {
            let open = open.clone();
            let count = accounts.len();
            list.connect_row_activated(move |_, row| {
                let index = usize::try_from(row.index()).ok();
                open(index.filter(|index| *index < count));
            });
        }
        // A typed letter moves to the next entry whose name starts with it.
        let names: Vec<String> = accounts
            .iter()
            .map(|account| account.full_name.clone())
            .chain([tr("Other user")])
            .collect();
        let typing = gtk4::EventControllerKey::new();
        {
            let list = list.clone();
            typing.connect_key_pressed(move |_, key, _, modifiers| {
                let chord = gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK;
                let letter = key.to_unicode().filter(|letter| letter.is_alphanumeric());
                let (Some(letter), false) = (letter, modifiers.intersects(chord)) else {
                    return glib::Propagation::Proceed;
                };
                let from = list
                    .selected_row()
                    .and_then(|row| usize::try_from(row.index()).ok())
                    .unwrap_or(names.len() - 1);
                let row = next_match(&names, from, letter)
                    .and_then(|index| i32::try_from(index).ok())
                    .and_then(|index| list.row_at_index(index));
                match row {
                    Some(row) => {
                        list.select_row(Some(&row));
                        row.grab_focus();
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                }
            });
        }
        list.add_controller(typing);
        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(360)
            .child(&list)
            .build();
        let list_card = Box::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .css_classes(["greeter-card"])
            .build();
        list_card.append(&scroller);
        pages.add_named(&list_card, Some("list"));
        list
    });

    // Back to the list: PAM's open question goes unanswered, so the conversation sends
    // CancelSession, and nothing the left sign-in says next is drawn.
    let go_back: Rc<dyn Fn()> = Rc::new({
        let (accounts, selected, form, pages) = (
            accounts.clone(),
            selected.clone(),
            form.clone(),
            pages.clone(),
        );
        move || {
            // No list, or greetd has the turn.
            let Some(list) = list.as_ref().filter(|_| form.back.is_sensitive()) else {
                return;
            };
            form.pending.take();
            form.turn.borrow_mut().leave();
            form.password.set_text("");
            form.answer.set_text("");
            form.user_name.set_text("");
            pages.set_visible_child_name("list");
            let entry = selected.get().unwrap_or(accounts.len());
            if let Some(row) = i32::try_from(entry)
                .ok()
                .and_then(|entry| list.row_at_index(entry))
            {
                list.select_row(Some(&row));
                row.grab_focus();
            }
        }
    });
    {
        let go_back = go_back.clone();
        back.connect_clicked(move |_| go_back());
    }

    // Escape goes back to the list from a card; without a list it cancels an open
    // question, as the button does; otherwise it goes on its way.
    let escape = gtk4::EventControllerKey::new();
    escape.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let (cancel, back, pages) = (cancel.clone(), back.clone(), pages.clone());
        escape.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            let on_card = pages.visible_child_name().as_deref() == Some("card");
            if on_card && back.is_visible() && back.is_sensitive() {
                go_back();
                glib::Propagation::Stop
            } else if cancel.is_visible() {
                cancel.emit_clicked();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    window.add_controller(escape);
    let sign_in = Rc::new({
        let app = app.clone();
        move || {
            // A question of PAM's is open: this is its answer, not a new sign-in.
            if let Some(reply) = form.pending.take() {
                let field: &gtk4::Editable = if WidgetExt::is_visible(&form.answer) {
                    form.answer.upcast_ref()
                } else {
                    form.password.upcast_ref()
                };
                let text = zeroize::Zeroizing::new(field.text().to_string());
                field.set_text("");
                form.wait();
                if reply.send(text).is_err() {
                    tracing::warn!("the sign-in ended before its question was answered");
                }
                return;
            }
            let other = form.other.get();
            let username = if other {
                form.user_name.text().to_string()
            } else {
                let Some(account) = selected.get().and_then(|index| accounts.get(index)) else {
                    return;
                };
                account.user.clone()
            };
            if other && !typed_name_is_valid(&username) {
                // Cleared before the text is shown: clearing a field hides the messages.
                form.user_name.set_text("");
                form.error.set_label(&tr(
                    "Enter a user name of at most 256 bytes, without control characters.",
                ));
                form.error.set_visible(true);
                return;
            }
            // The previous sign-in is still closing its session: never two at once.
            let Some(attempt) = form.turn.borrow_mut().begin() else {
                return;
            };
            // "Other user" has typed no password yet: PAM asks for it in its own words.
            let secret =
                (!other).then(|| zeroize::Zeroizing::new(form.password.text().to_string()));
            // Cleared before anything is shown: clearing a field hides the messages.
            form.password.set_text("");
            form.wait();
            form.start();
            form.status.set_label(&tr("Signing in…"));
            form.status.set_visible(true);
            let (app, form) = (app.clone(), form.clone());
            glib::MainContext::default().spawn_local(async move {
                let password = secret.as_ref().map(|secret| secret.as_str());
                let outcome = authenticate_interactive(&username, password, &form).await;
                // The person went back to the list meanwhile: the card is another's now.
                if !form.turn.borrow_mut().end(attempt) {
                    return;
                }
                match outcome {
                    Ok(Outcome::SignedIn) => app.quit(),
                    // The person's own choice, not a failure: said plainly, not alerted.
                    Ok(Outcome::Cancelled) => {
                        form.reset();
                        form.status.set_label(&tr("Sign-in was cancelled."));
                        form.status.set_visible(true);
                    }
                    Err(reason) => {
                        form.reset();
                        form.status.set_visible(false);
                        form.error.set_label(&tr_with(
                            "Sign-in failed: {reason}",
                            "reason",
                            &reason,
                        ));
                        form.error.set_visible(true);
                    }
                }
            });
        }
    });
    {
        let sign_in = sign_in.clone();
        password.connect_activate(move |_| sign_in());
    }
    for field in [&user_name, &answer] {
        let sign_in = sign_in.clone();
        field.connect_activate(move |_| sign_in());
    }
    submit.connect_clicked(move |_| sign_in());

    let centre = Box::builder()
        .orientation(Orientation::Vertical)
        .halign(Align::Center)
        .valign(Align::Center)
        .vexpand(true)
        .build();
    centre.append(&clock);
    centre.append(&date);
    centre.append(&pages);

    // Bottom right: keyboard layout, accessibility, power.
    let contrast = ToggleButton::builder()
        .icon_name("preferences-desktop-accessibility-symbolic")
        .css_classes(["greeter-chip"])
        .active(variant.is_high_contrast())
        .build();
    named(&contrast, "greeter-contrast", &tr("High contrast"));
    contrast.set_tooltip_text(Some(&tr("High contrast")));
    {
        let display = display.clone();
        contrast.connect_toggled(move |toggle| {
            calmo::load(&display, variant.with_high_contrast(toggle.is_active()))
        });
    }

    let bottom = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .margin_bottom(14)
        .margin_end(16)
        .build();
    bottom.append(&layout_chip(&display));
    bottom.append(&contrast);
    bottom.append(&power_chip(
        "greeter-suspend",
        "system-suspend-symbolic",
        &tr("Suspend"),
        Power::Suspend,
    ));
    bottom.append(&power_chip(
        "greeter-restart",
        "system-reboot-symbolic",
        &tr("Restart"),
        Power::Restart,
    ));
    bottom.append(&power_chip(
        "greeter-shutdown",
        "system-shutdown-symbolic",
        &tr("Shut down"),
        Power::ShutDown,
    ));

    let root = Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.append(&top);
    root.append(&centre);
    root.append(&bottom);
    window.set_child(Some(&root));

    // The default card is the first: the first key pressed is the first character of its
    // password. Without a list, of "Other user"'s name.
    open(listed.then_some(0));
    window.present();
}

#[cfg(test)]
mod tests {
    use super::{next_match, Notices, Turn};

    #[test]
    fn pams_lines_stay_through_the_sign_in_and_go_when_the_next_one_starts() {
        let mut notices = Notices::default();
        notices.info.push("Last login: yesterday".to_string());
        notices
            .error
            .push("There were 2 failed login attempts".to_string());
        assert!(!notices.cancellable, "no question, nothing to cancel");
        notices.ask();
        assert!(notices.cancellable, "an open question can be cancelled");
        notices.close_question();
        assert!(!notices.cancellable, "an answered question cannot");
        assert_eq!(notices.info, ["Last login: yesterday"], "PAM's lines stay");
        assert_eq!(notices.error, ["There were 2 failed login attempts"]);
        notices.start();
        assert_eq!(
            notices,
            Notices::default(),
            "the next sign-in starts with none of the previous one's lines"
        );
    }

    #[test]
    fn one_sign_in_at_a_time_and_a_left_one_draws_nothing() {
        let mut turn = Turn::default();
        let first = turn.begin().expect("the first sign-in starts");
        assert_eq!(
            turn.begin(),
            None,
            "no second session while the first talks to greetd"
        );
        // The person goes back to the list: the first sign-in cancels and ends unseen.
        turn.leave();
        assert!(!turn.end(first), "a left sign-in does not draw on the card");
        let second = turn
            .begin()
            .expect("the next sign-in starts once the first ended");
        assert!(turn.end(second), "a sign-in nobody left draws its outcome");
        let third = turn.begin().expect("and the next");
        assert_ne!(third, second);
    }

    #[test]
    fn a_typed_letter_moves_to_the_next_name_that_starts_with_it() {
        let names: Vec<String> = ["Ada", "anna", "Bea", "Émile", "Other user"]
            .map(str::to_string)
            .into();
        assert_eq!(next_match(&names, 4, 'a'), Some(0), "round the end");
        assert_eq!(next_match(&names, 0, 'A'), Some(1), "the next, in any case");
        assert_eq!(next_match(&names, 1, 'a'), Some(0));
        assert_eq!(next_match(&names, 0, 'é'), Some(3));
        assert_eq!(next_match(&names, 0, 'o'), Some(4));
        assert_eq!(next_match(&names, 0, 'z'), None);
        assert_eq!(next_match(&names, 2, 'b'), Some(2), "the only one stays");
    }
}
