//! The notification panel (doc_notification_center.md, NC11): the header, the history
//! grouped by application, and the calendar. It draws what the model of athanor-services
//! holds and sends it commands; it keeps no notification of its own.

mod calendar;
mod group;
mod header;
mod row;
mod text;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use athanor_compositor_client::Client;
use athanor_control_center::NOTIFICATIONS_WIDTH;
use athanor_services::notifications::{self, NotificationsCommand, NotificationsState};
use athanor_services::Buses;
use gtk4::accessible::{Property, Relation, State};
use gtk4::prelude::*;
use gtk4::{glib, pango};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use crate::i18n::{tr, tr_with};
use group::{app_title, groups, next_focus, row_of, shown_rows, unread_to_mark, Group};
use header::{drawer, item, Header, SETTINGS};

const BLANK: &str = "blank";
const LIST: &str = "list";
const EMPTY: &str = "empty";
const UNAVAILABLE: &str = "unavailable";
/// The scrolling part never shrinks below this.
const LIST_MIN_HEIGHT: i32 = 80;

pub struct Panel {
    pub root: gtk4::Box,
    commands: Option<UnboundedSender<NotificationsCommand>>,
    client: Option<Rc<Client>>,
    state: RefCell<NotificationsState>,
    header: Rc<Header>,
    stack: gtk4::Stack,
    scroller: gtk4::ScrolledWindow,
    list: gtk4::Box,
    calendar: gtk4::Calendar,
    /// The groups whose rows are all shown, and the rows whose whole text is.
    expanded: RefCell<HashSet<String>>,
    open_bodies: RefCell<HashSet<u32>>,
    /// The drawers that are open, by widget name: the group menus, their Mute question and
    /// the "More" of a row.
    open_drawers: RefCell<HashSet<String>>,
    /// The rows shown by the last drawing, in display order.
    listed: RefCell<Vec<u32>>,
    /// The ids already asked to be marked read since the panel opened.
    asked: RefCell<HashSet<u32>>,
    shown: Cell<bool>,
    resized: RefCell<Option<Box<dyn Fn()>>>,
}

/// Starts the notifications model on `runtime` and the panel that follows it. Without a
/// runtime the panel says the notifications are unavailable.
pub fn new(runtime: Option<(Handle, Buses)>, client: Option<Rc<Client>>) -> Rc<Panel> {
    let (rx, commands) = match runtime {
        Some((handle, buses)) => {
            let (rx, commands) = notifications::spawn(&handle, buses);
            (Some(rx), Some(commands))
        }
        None => (None, None),
    };
    let panel = Rc::new_cyclic(|me: &Weak<Panel>| Panel::build(me, commands, client));
    match rx {
        Some(mut rx) => {
            let weak = Rc::downgrade(&panel);
            glib::spawn_future_local(async move {
                loop {
                    // Cloned so no borrow of the channel is held while the panel draws.
                    let state = rx.borrow_and_update().clone();
                    match weak.upgrade() {
                        Some(panel) => panel.apply(state),
                        None => return,
                    }
                    if rx.changed().await.is_err() {
                        return;
                    }
                }
            });
        }
        None => panel.apply(NotificationsState {
            settled: true,
            ..NotificationsState::default()
        }),
    }
    panel
}

impl Panel {
    fn build(me: &Weak<Panel>, commands: Option<UnboundedSender<NotificationsCommand>>, client: Option<Rc<Client>>) -> Panel {
        let header = Header::new(me);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        let message = |text: &str| {
            let label = gtk4::Label::new(Some(text));
            label.add_css_class("nc-dim");
            label.set_margin_top(24);
            label.set_margin_bottom(24);
            label
        };
        let stack = gtk4::Stack::new();
        stack.set_vhomogeneous(false);
        stack.add_named(&gtk4::Box::new(gtk4::Orientation::Vertical, 0), Some(BLANK));
        stack.add_named(&list, Some(LIST));
        stack.add_named(&message(&tr("No notifications")), Some(EMPTY));
        stack.add_named(&message(&tr("Notifications unavailable")), Some(UNAVAILABLE));
        let calendar = calendar::new();
        let root = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(10)
            .accessible_role(gtk4::AccessibleRole::Group)
            .build();
        // The calendar is below the list and scrolls with it, so a short output keeps room
        // for both.
        let body = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        body.append(&stack);
        body.append(&calendar);
        let scroller = gtk4::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .propagate_natural_height(true)
            .min_content_height(LIST_MIN_HEIGHT)
            .vexpand(true)
            .build();
        root.append(&header.root);
        root.append(&scroller);
        let resized = me.clone();
        calendar.connect_prev_month(move |_| notify_resized(&resized));
        let resized = me.clone();
        calendar.connect_next_month(move |_| notify_resized(&resized));
        Panel {
            root,
            commands,
            client,
            state: RefCell::default(),
            header,
            stack,
            scroller,
            list,
            calendar,
            expanded: RefCell::default(),
            open_bodies: RefCell::default(),
            open_drawers: RefCell::default(),
            listed: RefCell::default(),
            asked: RefCell::default(),
            shown: Cell::new(false),
            resized: RefCell::default(),
        }
    }

    /// Calls `on_resize` whenever the content's height may have changed while the panel is
    /// shown, so that the surface can size itself to it (NC11).
    pub fn connect_resized(&self, on_resize: impl Fn() + 'static) {
        self.resized.replace(Some(Box::new(on_resize)));
    }

    /// The panel is shown: today on the calendar, the list drawn and read, and the unread
    /// count announced (ST7).
    pub fn opened(self: &Rc<Self>) {
        self.shown.set(true);
        self.asked.borrow_mut().clear();
        calendar::refresh(&self.calendar);
        calendar::show_today(&self.calendar);
        self.render();
        let unread = self.unread();
        let words = text::announcement(unread);
        self.root.update_property(&[Property::Label(&words)]);
        let root = self.root.clone();
        // Once the surface is mapped, or the announcement has no one to go to.
        glib::idle_add_local_once(move || {
            root.announce(&words, gtk4::AccessibleAnnouncementPriority::Medium);
        });
    }

    /// Takes `excess` pixels from the scrolling part, or nothing for 0: the header keeps its
    /// size and the list and the calendar scroll.
    pub fn cap_list(&self, excess: i32) {
        self.scroller.set_max_content_height(-1);
        if excess > 0 {
            let width = match self.root.width() {
                0 => NOTIFICATIONS_WIDTH,
                width => width,
            };
            let wanted = self.scroller.measure(gtk4::Orientation::Vertical, width).1;
            self.scroller
                .set_max_content_height((wanted - excess).max(LIST_MIN_HEIGHT));
        }
    }

    pub fn closed(&self) {
        self.shown.set(false);
    }

    fn unread(&self) -> usize {
        self.state.borrow().entries.iter().filter(|n| !n.read && !n.transient).count()
    }

    fn apply(self: &Rc<Self>, state: NotificationsState) {
        self.header.update(
            &state.dnd,
            state.available,
            state.entries.iter().any(|n| !n.transient),
        );
        self.state.replace(state);
        if self.shown.get() {
            self.render();
        }
    }

    pub(super) fn send(&self, command: NotificationsCommand) {
        let sent = self.commands.as_ref().map(|commands| commands.send(command));
        if !matches!(sent, Some(Ok(()))) {
            tracing::warn!("the notifications model is not running; the command is dropped");
        }
    }

    /// The activation token that lets the application raise its window; without one the
    /// call still goes.
    pub(super) fn token(&self) -> String {
        self.client
            .as_ref()
            .and_then(|client| client.activation_token(None))
            .unwrap_or_default()
    }

    /// Opens an application through its desktop entry, behind a security context (BR2).
    pub(super) fn open_app(&self, desktop_id: &str) {
        let Some(client) = self.client.clone() else {
            tracing::warn!(app = desktop_id, "no compositor client; the application is not opened");
            return;
        };
        let Some(info) = gio_unix::DesktopAppInfo::new(desktop_id) else {
            tracing::warn!(app = desktop_id, "there is no desktop entry to open");
            return;
        };
        let desktop_id = desktop_id.to_owned();
        glib::spawn_future_local(async move {
            if let Err(err) = client.launch(&info).await {
                tracing::error!(error = %err, app = desktop_id, "the application was not started");
            }
        });
    }

    pub(super) fn body_open(&self, id: u32) -> bool {
        self.open_bodies.borrow().contains(&id)
    }

    /// A row's text opened or closed in place: the list is not rebuilt, so the focus stays.
    pub(super) fn set_body_open(&self, id: u32, open: bool) {
        if open {
            self.open_bodies.borrow_mut().insert(id);
        } else {
            self.open_bodies.borrow_mut().remove(&id);
        }
        self.notify_resized();
    }

    fn notify_resized(&self) {
        if let Some(on_resize) = self.resized.borrow().as_ref() {
            on_resize();
        }
    }

    /// Draws the list again from the model. The control that had the keyboard has it again
    /// afterwards, or the next row when its row is gone, and the list keeps its scroll.
    fn render(self: &Rc<Self>) {
        // Only a name set on purpose is a name to find again: GTK names the rest by class.
        let focus = self
            .root
            .root()
            .and_then(|root| root.focus())
            .filter(|widget| widget.is_ancestor(&self.list))
            .filter(|widget| widget.widget_name() != widget.type_().name())
            .map(|widget| widget.widget_name().to_string());
        let scroll = self.scroller.vadjustment().value();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let state = self.state.borrow();
        let now = glib::real_time() / 1_000_000;
        let all = groups(&state.entries);
        let expanded = self.expanded.borrow().clone();
        for group in &all {
            self.list.append(&self.group_widget(group, expanded.contains(&group.app_id), &state, now));
        }
        let page = match (state.settled, state.available, all.is_empty()) {
            (false, _, _) => BLANK,
            (true, false, _) => UNAVAILABLE,
            (true, true, true) => EMPTY,
            (true, true, false) => LIST,
        };
        self.stack.set_visible_child_name(page);
        let order: Vec<u32> = all
            .iter()
            .flat_map(|group| shown_rows(group, expanded.contains(&group.app_id)).iter().copied())
            .collect();
        let before = self.listed.replace(order.clone());
        if let Some(name) = focus {
            let mut wanted = vec![name.clone()];
            if let Some(row) = row_of(&name) {
                match next_focus(&before, &order, row) {
                    // The row stays: its control, else the row itself.
                    Some(kept) if kept == row => wanted.push(row::name_of(row)),
                    Some(next) => wanted = vec![row::name_of(next)],
                    None => wanted.clear(),
                }
            }
            if let Some(widget) = wanted
                .iter()
                .find_map(|name| find_named(self.list.upcast_ref(), name))
            {
                widget.grab_focus();
            }
        }
        let scroller = self.scroller.clone();
        glib::idle_add_local_once(move || scroller.vadjustment().set_value(scroll));
        let marked = unread_to_mark(&state.entries, &all, &expanded, &self.asked.borrow());
        drop(state);
        let words = text::announcement(self.unread());
        self.root.update_property(&[Property::Label(&words)]);
        if !marked.is_empty() {
            self.asked.borrow_mut().extend(marked.iter().copied());
            self.send(NotificationsCommand::MarkRead(marked));
        }
        self.notify_resized();
    }

    fn group_widget(
        self: &Rc<Self>,
        group: &Group,
        expanded: bool,
        state: &NotificationsState,
        now: i64,
    ) -> gtk4::Box {
        let weak = Rc::downgrade(self);
        // What the sender says about itself names a group only when it has no proven
        // identity; a proven one is named by its desktop entry, or by the id (NC2).
        let name = if group.app_id.is_empty() {
            tr("Other applications")
        } else {
            let declared = group
                .rows
                .first()
                .and_then(|id| state.entries.iter().find(|n| n.id == *id))
                .map_or("", |n| n.app_name.as_str());
            app_title(&group.app_id, declared, desktop_name(&group.app_id).as_deref())
                .unwrap_or_else(|| group.app_id.clone())
        };
        let key = |part: &str| format!("g{}:{part}", group.app_id);
        let heading = gtk4::Label::new(Some(&name));
        heading.add_css_class("nc-heading");
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        heading.set_ellipsize(pango::EllipsizeMode::End);
        let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        bar.append(&heading);
        let (menu_toggle, menu, entries) = drawer(&tr_with("Options for {app}", "app", &name));
        watch_reveal(&menu, &weak);
        menu_toggle.set_widget_name(&key("menu"));
        self.track(&menu_toggle, key("menu"));
        let collapsible = group.rows.len() > shown_rows(group, false).len();
        if collapsible {
            let words = if expanded {
                tr("Show less")
            } else {
                tr_with("Show {n} more", "n", &(group.rows.len() - 1).to_string())
            };
            let toggle = gtk4::Button::with_label(&words);
            toggle.add_css_class("flat");
            toggle.set_widget_name(&key("toggle"));
            toggle.update_state(&[State::Expanded(Some(expanded))]);
            let (panel, app) = (weak.clone(), group.app_id.clone());
            toggle.connect_clicked(move |_| {
                if let Some(panel) = panel.upgrade() {
                    {
                        let mut open = panel.expanded.borrow_mut();
                        if !open.remove(&app) {
                            open.insert(app.clone());
                        }
                    }
                    panel.render();
                }
            });
            bar.append(&toggle);
        }
        bar.append(&menu_toggle);

        // "Mute this application" asks first, in the drawer, with the choices swapped for
        // the question. Both the drawer and the question stay as they are across a rebuild.
        let choices = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let confirm = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        let pages = gtk4::Stack::new();
        pages.add_named(&choices, Some("choices"));
        pages.add_named(&confirm, Some("confirm"));
        if self.is_open(&key("mute")) {
            pages.set_visible_child_name("confirm");
        }
        let question = gtk4::Label::new(Some(&tr_with(
            "Mute {app}? It will send no notification at all.",
            "app",
            &name,
        )));
        question.set_wrap(true);
        question.set_xalign(0.0);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.set_widget_name(&key("cancel"));
        cancel.update_relation(&[Relation::DescribedBy(&[question.upcast_ref()])]);
        let mute = gtk4::Button::with_label(&tr("Mute"));
        mute.set_widget_name(&key("confirm"));
        mute.add_css_class("destructive-action");
        mute.update_relation(&[Relation::DescribedBy(&[question.upcast_ref()])]);
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&mute);
        confirm.append(&question);
        confirm.append(&buttons);
        entries.append(&pages);
        let (pages_ask, cancel_focus, panel) = (pages.downgrade(), cancel.downgrade(), weak.clone());
        let mute_key = key("mute");
        let ask = item(&tr("Mute this application"), move || {
            if let (Some(pages), Some(cancel), Some(panel)) =
                (pages_ask.upgrade(), cancel_focus.upgrade(), panel.upgrade())
            {
                panel.remember(&mute_key, true);
                pages.set_visible_child_name("confirm");
                cancel.grab_focus();
            }
        });
        ask.set_widget_name(&key("mute"));
        choices.append(&ask);
        let (panel, app, toggle) = (weak.clone(), group.app_id.clone(), menu_toggle.downgrade());
        let list_only = item(&tr("Show in the list only"), move || {
            if let Some(panel) = panel.upgrade() {
                panel.send(NotificationsCommand::ListOnly(app.clone()));
            }
            if let Some(toggle) = toggle.upgrade() {
                toggle.set_active(false);
            }
        });
        list_only.set_widget_name(&key("listonly"));
        choices.append(&list_only);
        let panel = weak.clone();
        let settings = item(&tr("Notification settings"), move || {
            if let Some(panel) = panel.upgrade() {
                panel.open_app(SETTINGS);
            }
        });
        settings.set_widget_name(&key("settings"));
        choices.append(&settings);
        let (pages_cancel, panel, mute_key) = (pages.downgrade(), weak.clone(), key("mute"));
        cancel.connect_clicked(move |_| {
            if let (Some(pages), Some(panel)) = (pages_cancel.upgrade(), panel.upgrade()) {
                panel.remember(&mute_key, false);
                pages.set_visible_child_name("choices");
            }
        });
        let (panel, app, toggle) = (weak.clone(), group.app_id.clone(), menu_toggle.downgrade());
        mute.connect_clicked(move |_| {
            if let Some(panel) = panel.upgrade() {
                panel.send(NotificationsCommand::Mute(app.clone()));
            }
            if let Some(toggle) = toggle.upgrade() {
                toggle.set_active(false);
            }
        });
        let (reset, panel, mute_key) = (pages.downgrade(), weak.clone(), key("mute"));
        menu_toggle.connect_toggled(move |toggle| {
            if let (false, Some(pages), Some(panel)) = (toggle.is_active(), reset.upgrade(), panel.upgrade()) {
                panel.remember(&mute_key, false);
                pages.set_visible_child_name("choices");
            }
        });

        let widget = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(6)
            .accessible_role(gtk4::AccessibleRole::Group)
            .build();
        widget.update_relation(&[Relation::LabelledBy(&[heading.upcast_ref()])]);
        widget.append(&bar);
        widget.append(&menu);
        let show_app = group.app_id.is_empty();
        for id in shown_rows(group, expanded) {
            if let Some(n) = state.entries.iter().find(|n| n.id == *id) {
                widget.append(&row::build(self, n, now, show_app));
            }
        }
        widget
    }

    /// The drawer opened by `toggle` is kept open, or closed, when the list is drawn again.
    pub(super) fn track(self: &Rc<Self>, toggle: &gtk4::ToggleButton, key: String) {
        toggle.set_active(self.is_open(&key));
        let panel = Rc::downgrade(self);
        toggle.connect_toggled(move |toggle| {
            if let Some(panel) = panel.upgrade() {
                panel.remember(&key, toggle.is_active());
            }
        });
    }

    fn is_open(&self, key: &str) -> bool {
        self.open_drawers.borrow().contains(key)
    }

    fn remember(&self, key: &str, open: bool) {
        if open {
            self.open_drawers.borrow_mut().insert(key.to_owned());
        } else {
            self.open_drawers.borrow_mut().remove(key);
        }
    }
}

/// Sizes the surface again when a drawer opens or closes, since the panel's height follows
/// its content (NC11).
pub(super) fn watch_reveal(revealer: &gtk4::Revealer, panel: &Weak<Panel>) {
    for property in ["reveal-child", "child-revealed"] {
        let panel = panel.clone();
        revealer.connect_notify_local(Some(property), move |_, _| notify_resized(&panel));
    }
}

/// The display name of the desktop entry of a proven application.
fn desktop_name(app_id: &str) -> Option<String> {
    gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop"))
        .map(|info| info.display_name().to_string())
}

fn notify_resized(panel: &Weak<Panel>) {
    if let Some(panel) = panel.upgrade() {
        panel.notify_resized();
    }
}

/// The descendant of `widget` named `name`.
fn find_named(widget: &gtk4::Widget, name: &str) -> Option<gtk4::Widget> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if current.widget_name() == name {
            return Some(current);
        }
        if let Some(found) = find_named(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
