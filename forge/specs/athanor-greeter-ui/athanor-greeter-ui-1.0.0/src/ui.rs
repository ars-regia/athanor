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
    gdk, Align, Application, ApplicationWindow, Box, Button, Entry, Image, Label, Orientation,
    PasswordEntry, ToggleButton,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use tokio::sync::oneshot;

use crate::auth::{authenticate_interactive, discover_target_user, Conversation, Prompt, UserInfo};
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

/// The avatar: the account's picture when there is one, the initial on the accent otherwise.
fn avatar(user: &UserInfo) -> gtk4::Widget {
    if let Some(path) = &user.avatar_path {
        let picture = gtk4::Picture::for_filename(path);
        picture.set_size_request(60, 60);
        picture.set_content_fit(gtk4::ContentFit::Cover);
        picture.set_overflow(gtk4::Overflow::Hidden);
        picture.add_css_class("greeter-avatar");
        picture.set_halign(Align::Center);
        picture.update_property(&[Property::Label(&user.real_name)]);
        return picture.upcast();
    }
    let initial: String = user
        .real_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    Label::builder()
        .label(initial)
        .css_classes(["greeter-avatar"])
        .halign(Align::Center)
        .build()
        .upcast()
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

/// The card's fields as one sign-in sees them: PAM's messages are shown on it, and its
/// further questions are put on it, each answered from the field its type calls for.
#[derive(Clone)]
struct SignInForm {
    password: PasswordEntry,
    password_label: String,
    answer: Entry,
    question: Label,
    submit: Button,
    status: Label,
    error: Label,
    /// Where the answer to PAM's open question goes, while one is open.
    pending: Rc<RefCell<Option<oneshot::Sender<zeroize::Zeroizing<String>>>>>,
}

impl SignInForm {
    /// Nothing can be typed while greetd has the turn.
    fn wait(&self) {
        self.password.set_sensitive(false);
        self.answer.set_sensitive(false);
        self.submit.set_sensitive(false);
    }

    /// The card as it was before the sign-in: the password field, ready.
    fn reset(&self) {
        self.pending.take();
        self.question.set_visible(false);
        self.answer.set_text("");
        self.answer.set_visible(false);
        self.password.set_text("");
        label_field(&self.password, &self.password_label);
        self.password.set_visible(true);
        self.password.set_sensitive(true);
        self.submit.set_sensitive(true);
        self.password.grab_focus();
    }
}

impl Conversation for SignInForm {
    fn status(&self, text: &str) {
        self.status.set_label(text);
        self.status.set_visible(true);
    }

    fn error(&self, text: &str) {
        let shown = if self.error.is_visible() {
            format!("{}\n{text}", self.error.label())
        } else {
            text.to_string()
        };
        self.error.set_label(&shown);
        self.error.set_visible(true);
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
        field.grab_focus();
        async move { answer.await.ok() }
    }
}

pub fn build_ui(app: &Application) {
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

    let user = discover_target_user();
    let name = Label::builder()
        .label(&user.real_name)
        .css_classes(["greeter-name"])
        .margin_top(10)
        .margin_bottom(14)
        .build();

    let password = PasswordEntry::builder()
        .placeholder_text(tr("Password"))
        .show_peek_icon(true)
        .hexpand(true)
        .css_classes(["greeter-field"])
        .build();
    let password_label = tr_with("Password for {name}", "name", &user.real_name);
    named(&password, "greeter-password", &password_label);
    label_field(&password, &password_label);
    // The field for PAM's questions whose answer is shown as it is typed (a one-time code):
    // hidden until PAM asks one.
    let answer = Entry::builder()
        .hexpand(true)
        .css_classes(["greeter-field"])
        .visible(false)
        .build();
    answer.set_widget_name("greeter-answer");
    let submit = Button::builder()
        .icon_name("go-next-symbolic")
        .css_classes(["greeter-submit"])
        .valign(Align::Center)
        .build();
    named(&submit, "greeter-submit", &tr("Sign in"));
    let field_row = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    field_row.append(&password);
    field_row.append(&answer);
    field_row.append(&submit);

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

    for field in [password.upcast_ref::<gtk4::Editable>(), answer.upcast_ref()] {
        let (status, error) = (status.clone(), error.clone());
        field.connect_changed(move |_| {
            status.set_visible(false);
            error.set_visible(false);
        });
    }

    let form = SignInForm {
        password: password.clone(),
        password_label,
        answer: answer.clone(),
        question: question.clone(),
        submit: submit.clone(),
        status: status.clone(),
        error: error.clone(),
        pending: Rc::new(RefCell::new(None)),
    };
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
            let secret = zeroize::Zeroizing::new(form.password.text().to_string());
            // Cleared before anything is shown: clearing a field hides the messages.
            form.password.set_text("");
            form.wait();
            form.error.set_visible(false);
            form.status.set_label(&tr("Signing in…"));
            form.status.set_visible(true);
            let (app, form) = (app.clone(), form.clone());
            glib::MainContext::default().spawn_local(async move {
                match authenticate_interactive(&secret, &form).await {
                    Ok(()) => app.quit(),
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
    {
        let sign_in = sign_in.clone();
        answer.connect_activate(move |_| sign_in());
    }
    submit.connect_clicked(move |_| sign_in());

    let card = Box::builder()
        .orientation(Orientation::Vertical)
        .halign(Align::Center)
        .css_classes(["greeter-card"])
        .build();
    card.append(&avatar(&user));
    card.append(&name);
    card.append(&question);
    card.append(&field_row);
    card.append(&status);
    card.append(&error);

    let centre = Box::builder()
        .orientation(Orientation::Vertical)
        .halign(Align::Center)
        .valign(Align::Center)
        .vexpand(true)
        .build();
    centre.append(&clock);
    centre.append(&date);
    centre.append(&card);

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

    // The first key pressed is the first character of the password.
    password.grab_focus();
    window.present();
}
