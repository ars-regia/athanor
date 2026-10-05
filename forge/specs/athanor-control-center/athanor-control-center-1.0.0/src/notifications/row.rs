//! One notification as a row of the list (NC11).

use std::rc::{Rc, Weak};

use athanor_services::notifications::wire::WireNotification;
use athanor_services::notifications::NotificationsCommand;
use athanor_unit::text;
use gtk4::accessible::{Property, State};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};

use super::group::{app_title, clickable};
use super::header::drawer;
use super::text::{accessible_name, relative_time};
use super::{desktop_name, watch_reveal, Panel};
use crate::i18n::{tr, tr_with};

const PICTURE_PX: i32 = 32;
const TEXT_CHARS: i32 = 36;
const FALLBACK_ICON: &str = "dialog-information-symbolic";
/// A body this long may need more than two lines.
const LONG_BODY: usize = 60;
const VISIBLE_ACTIONS: usize = 3;
/// A touch swipe faster than this (px/s), and mostly horizontal, closes the row.
const SWIPE_SPEED: f64 = 500.0;
/// KDE's inline reply action key (NC9): its label names the button that sends the reply.
const REPLY_ACTION: &str = "inline-reply";
/// The reply's strings are short; the daemon bounded them, and this is the second bound.
const REPLY_LABEL_CHARS: usize = 64;
/// What the daemon passes on to the application.
const MAX_REPLY_CHARS: i32 = 4096;

/// The widget name of the row of notification `id`, which keeps the keyboard focus across a
/// rebuild of the list.
pub fn name_of(id: u32) -> String {
    format!("n{id}")
}

fn label(text: &str, lines: i32) -> gtk4::Label {
    // A plain label: `use-markup` stays false, so markup in the text shows as text (SH12);
    // only `show_spans` turns it on, for the body.
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);
    label.set_max_width_chars(TEXT_CHARS);
    label.set_lines(lines);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label
}

/// The body as the daemon's spans, rendered to Pango only by `pango_markup`: the label never
/// sees the sender's text as markup. A link opens on a click or Enter (`activate-link`), never on
/// hover or focus; the tooltip shows its address.
fn show_spans(label: &gtk4::Label, spans: &[(String, u32, String)]) {
    if spans.is_empty() {
        return;
    }
    label.set_use_markup(true);
    label.set_markup(&athanor_services::notifications::wire::pango_markup(spans));
    label.connect_activate_link(|_, uri| {
        // No parent window: GTK would export a layer surface through the compositor, which
        // refuses it and disconnects the client.
        gtk4::UriLauncher::new(uri).launch(
            None::<&gtk4::Window>,
            gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    tracing::warn!(error = %err, "cannot open a link of a notification");
                }
            },
        );
        glib::Propagation::Stop
    });
}

/// A texture from straight RGBA; `None` for a zero side or a length that does not match,
/// which `MemoryTexture::new` would abort on.
fn texture(width: u32, height: u32, rgba: &[u8]) -> Option<gdk::Texture> {
    let (w, h) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let expected = stride.checked_mul(usize::try_from(height).ok()?)?;
    if w <= 0 || h <= 0 || rgba.len() != expected {
        return None;
    }
    let bytes = glib::Bytes::from(rgba);
    Some(gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &bytes, stride).upcast())
}

/// The image data, else the file, else the icon name, else the application's icon, else a generic one.
fn picture(n: &WireNotification) -> gtk4::Image {
    let themed = |name: &str| {
        gdk::Display::default()
            .is_some_and(|display| gtk4::IconTheme::for_display(&display).has_icon(name))
    };
    let image = texture(n.image_width, n.image_height, &n.image_rgba)
        .map(|texture| gtk4::Image::from_paintable(Some(&texture)))
        .or_else(|| {
            // The bar's guards: an absolute clean path to a small regular PNG, or nothing.
            athanor_unit::icon::read_icon_file(&n.icon_file)
                .and_then(|bytes| gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok())
                .map(|texture| gtk4::Image::from_paintable(Some(&texture)))
        })
        .or_else(|| {
            (!n.icon_name.is_empty() && themed(&n.icon_name))
                .then(|| gtk4::Image::from_icon_name(&n.icon_name))
        })
        .or_else(|| {
            gio_unix::DesktopAppInfo::new(&format!("{}.desktop", n.app_id))
                .and_then(|info| info.icon())
                .map(|icon| gtk4::Image::from_gicon(&icon))
        })
        .unwrap_or_else(|| gtk4::Image::from_icon_name(FALLBACK_ICON));
    image.set_pixel_size(PICTURE_PX);
    image.set_valign(gtk4::Align::Start);
    image
}

/// The application's name: its desktop entry's or its id when its identity is proven, and
/// only otherwise what the sender calls itself.
fn app_label(n: &WireNotification) -> String {
    let desktop = desktop_name(&n.app_id).filter(|_| !n.app_id.is_empty());
    app_title(&n.app_id, &n.app_name, desktop.as_deref())
        .unwrap_or_else(|| tr("Unknown application"))
}

/// A click on an action, or on the row for `default`: the daemon calls the sender back while
/// it is there, and once it is gone the application itself opens (NC3).
fn activate(panel: &Panel, n: &WireNotification, key: &str) {
    if n.actions_available {
        panel.send(NotificationsCommand::Invoke {
            id: n.id,
            key: key.to_owned(),
            token: panel.token(),
        });
    } else if !n.app_id.is_empty() {
        panel.open_app(&format!("{}.desktop", n.app_id));
    }
}

/// `show_app` names the application on the row, for the group of applications with no proven
/// identity, whose heading cannot.
pub fn build(panel: &Rc<Panel>, n: &WireNotification, now: i64, show_app: bool) -> gtk4::Box {
    let weak: Weak<Panel> = Rc::downgrade(panel);
    let id = n.id;
    let time = relative_time(now, n.time);
    let title = if n.summary.is_empty() { app_label(n) } else { n.summary.clone() };

    let card = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(6)
        .accessible_role(gtk4::AccessibleRole::Group)
        .focusable(true)
        .build();
    card.add_css_class("notification-card");
    if n.read {
        card.add_css_class("nc-read");
    }
    card.set_widget_name(&name_of(id));
    card.update_property(&[Property::Label(&accessible_name(
        &app_label(n),
        &n.summary,
        &n.body,
        &time,
    ))]);

    let summary = label(&title, 2);
    summary.add_css_class("notification-title");
    summary.set_hexpand(true);
    let when = gtk4::Label::new(Some(&time));
    when.add_css_class("nc-dim");
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    head.append(&summary);
    head.append(&when);
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_hexpand(true);
    if show_app {
        let app = label(&app_label(n), 1);
        app.add_css_class("nc-dim");
        text.append(&app);
    }
    text.append(&head);

    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_valign(gtk4::Align::Start);
    close.set_widget_name(&format!("{}:close", name_of(id)));
    let close_name = tr_with("Close {title}", "title", &title);
    close.set_tooltip_text(Some(&close_name));
    close.update_property(&[Property::Label(&close_name)]);
    let panel_close = weak.clone();
    close.connect_clicked(move |_| {
        if let Some(panel) = panel_close.upgrade() {
            panel.send(NotificationsCommand::Close(id));
        }
    });

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    top.append(&picture(n));
    top.append(&text);

    let open = panel.body_open(id);
    if !n.body.is_empty() {
        let body = label(&n.body, if open { -1 } else { 2 });
        show_spans(&body, &n.body_spans);
        body.set_ellipsize(if open { pango::EllipsizeMode::None } else { pango::EllipsizeMode::End });
        body.add_css_class("notification-body");
        text.append(&body);
        if n.body.chars().count() > LONG_BODY || n.body.contains('\n') {
            let more = gtk4::ToggleButton::new();
            more.set_active(open);
            more.set_icon_name(if open { "pan-up-symbolic" } else { "pan-down-symbolic" });
            more.add_css_class("flat");
            more.set_valign(gtk4::Align::Start);
            more.set_widget_name(&format!("{}:body", name_of(id)));
            let name = tr("Show the whole text");
            more.set_tooltip_text(Some(&name));
            more.update_property(&[Property::Label(&name)]);
            more.update_state(&[State::Expanded(Some(open))]);
            let (body, panel_more) = (body.downgrade(), weak.clone());
            more.connect_toggled(move |more| {
                let open = more.is_active();
                more.set_icon_name(if open { "pan-up-symbolic" } else { "pan-down-symbolic" });
                more.update_state(&[State::Expanded(Some(open))]);
                if let (Some(body), Some(panel)) = (body.upgrade(), panel_more.upgrade()) {
                    body.set_lines(if open { -1 } else { 2 });
                    body.set_ellipsize(if open {
                        pango::EllipsizeMode::None
                    } else {
                        pango::EllipsizeMode::End
                    });
                    panel.set_body_open(id, open);
                }
            });
            top.append(&more);
        }
    }
    top.append(&close);
    card.append(&top);

    let actions: Vec<&(String, String)> = n
        .actions
        .iter()
        .filter(|(key, _)| key != "default" && key != REPLY_ACTION)
        .collect();
    if !actions.is_empty() {
        let button = |key: &str, text: &str| {
            let button = gtk4::Button::with_label(text);
            button.set_widget_name(&format!("{}:a:{key}", name_of(id)));
            if !n.actions_available {
                // Shown insensitive, yet a click opens the application: a button that is
                // really insensitive would take the click and do nothing.
                button.add_css_class("nc-unavailable");
                button.update_state(&[State::Disabled(true)]);
            }
            let (panel, n, key) = (weak.clone(), n.clone(), key.to_owned());
            button.connect_clicked(move |_| {
                if let Some(panel) = panel.upgrade() {
                    activate(&panel, &n, &key);
                }
            });
            button
        };
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        row.set_halign(gtk4::Align::End);
        for (key, text) in actions.iter().take(VISIBLE_ACTIONS) {
            row.append(&button(key, text));
        }
        card.append(&row);
        if actions.len() > VISIBLE_ACTIONS {
            let (toggle, revealer, rest) = drawer(&tr("More"));
            toggle.set_label(&tr("More"));
            toggle.set_widget_name(&format!("{}:more", name_of(id)));
            panel.track(&toggle, format!("{}:more", name_of(id)));
            watch_reveal(&revealer, &weak);
            for (key, text) in actions.iter().skip(VISIBLE_ACTIONS) {
                rest.append(&button(key, text));
            }
            row.append(&toggle);
            card.append(&revealer);
        }
    }

    if progress_shown(n) {
        card.append(&progress(n));
    }
    if n.reply && n.actions_available {
        card.append(&reply_row(panel, n));
    }

    // A click on the row is the `default` action, when that does anything.
    let has_default = n.actions.iter().any(|(key, _)| key == "default");
    let can_activate = clickable(n.actions_available, has_default, &n.app_id);
    if can_activate {
        let click = gtk4::GestureClick::new();
        let (panel_click, n_click) = (weak.clone(), n.clone());
        click.connect_released(move |_, _, _, _| {
            if let Some(panel) = panel_click.upgrade() {
                activate(&panel, &n_click, "default");
            }
        });
        card.add_controller(click);
    }
    // Enter does the same, Delete closes the row.
    let keys = gtk4::EventControllerKey::new();
    let (panel_keys, n_keys) = (weak.clone(), n.clone());
    keys.connect_key_pressed(move |controller, key, _, _| {
        let Some(panel) = panel_keys.upgrade() else {
            return glib::Propagation::Proceed;
        };
        // The reply entry has its own Delete and Enter.
        if typing(controller) {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::Return | gdk::Key::KP_Enter if can_activate => {
                activate(&panel, &n_keys, "default");
            }
            gdk::Key::Delete | gdk::Key::KP_Delete => panel.send(NotificationsCommand::Close(id)),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    card.add_controller(keys);
    let swipe = gtk4::GestureSwipe::new();
    swipe.set_touch_only(true);
    let panel_swipe = weak;
    swipe.connect_swipe(move |_, vx, vy| {
        if vx.abs() > SWIPE_SPEED && vx.abs() > 2.0 * vy.abs() {
            if let Some(panel) = panel_swipe.upgrade() {
                panel.send(NotificationsCommand::Close(id));
            }
        }
    });
    card.add_controller(swipe);
    card
}

/// The keyboard is in a text entry of the row.
fn typing(controller: &gtk4::EventControllerKey) -> bool {
    controller
        .widget()
        .and_then(|widget| widget.root())
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus.is::<gtk4::Text>())
}

fn progress_shown(n: &WireNotification) -> bool {
    (0..=100).contains(&n.value)
}

/// The progress of a notification; its accessible value is the percentage (NC9).
fn progress(n: &WireNotification) -> gtk4::ProgressBar {
    let bar = gtk4::ProgressBar::new();
    bar.set_fraction(f64::from(n.value) / 100.0);
    bar.update_property(&[
        Property::Label(&tr("Progress")),
        Property::ValueMin(0.0),
        Property::ValueMax(100.0),
        Property::ValueNow(f64::from(n.value)),
        Property::ValueText(&format!("{}%", n.value)),
    ]);
    bar
}

/// The reply entry and its button (NC9). Every string is plain text: `set_text` and
/// `set_placeholder_text`, never markup. The text is sent to the daemon and kept nowhere else
/// than in the entry, and in the panel's draft while the list is drawn again.
fn reply_row(panel: &Rc<Panel>, n: &WireNotification) -> gtk4::Box {
    let id = n.id;
    let name = format!("{}:reply", name_of(id));
    let entry = gtk4::Entry::new();
    entry.set_hexpand(true);
    entry.set_max_length(MAX_REPLY_CHARS);
    let placeholder = match text::line(&n.reply_placeholder, REPLY_LABEL_CHARS) {
        words if words.is_empty() => tr("Write a reply"),
        words => words,
    };
    entry.set_placeholder_text(Some(&placeholder));
    entry.update_property(&[Property::Label(&placeholder)]);
    entry.set_widget_name(&name);
    // The focus lands on the text inside the entry: it carries the name too, so a redraw of
    // the list finds it again.
    if let Some(inner) = entry.delegate().and_then(|inner| inner.dynamic_cast::<gtk4::Widget>().ok()) {
        inner.set_widget_name(&name);
    }
    if let Some(draft) = panel.draft(id) {
        entry.set_text(&draft);
    }
    if panel.reply_failed(id) {
        // The application did not take the reply: the text is back, marked.
        entry.add_css_class("error");
    }
    let weak = Rc::downgrade(panel);
    entry.connect_changed({
        let weak = weak.clone();
        move |entry| {
            if let Some(panel) = weak.upgrade() {
                panel.set_draft(id, &entry.text());
            }
        }
    });

    let label = [n.reply_submit.as_str(), reply_label(n)]
        .into_iter()
        .map(|words| text::line(words, REPLY_LABEL_CHARS))
        .find(|words| !words.is_empty())
        .unwrap_or_else(|| tr("Send"));
    let themed = !n.reply_icon.is_empty()
        && gdk::Display::default()
            .is_some_and(|display| gtk4::IconTheme::for_display(&display).has_icon(&n.reply_icon));
    let send = if themed {
        let button = gtk4::Button::from_icon_name(&n.reply_icon);
        button.set_tooltip_text(Some(&label));
        button.update_property(&[Property::Label(&label)]);
        button
    } else {
        gtk4::Button::with_label(&label)
    };
    send.set_widget_name(&format!("{}:send", name_of(id)));
    let submit = {
        let entry = entry.downgrade();
        move || {
            let (Some(panel), Some(entry)) = (weak.upgrade(), entry.upgrade()) else {
                return;
            };
            let reply = entry.text().to_string();
            if reply.trim().is_empty() {
                return;
            }
            entry.set_text("");
            panel.send_reply(id, reply);
        }
    };
    let on_enter = submit.clone();
    entry.connect_activate(move |_| on_enter());
    send.connect_clicked(move |_| submit());

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.append(&entry);
    row.append(&send);
    row
}

/// The label of the sender's `inline-reply` action, `""` when it gave none.
fn reply_label(n: &WireNotification) -> &str {
    n.actions
        .iter()
        .find(|(key, _)| key == REPLY_ACTION)
        .map_or("", |(_, label)| label.as_str())
}
