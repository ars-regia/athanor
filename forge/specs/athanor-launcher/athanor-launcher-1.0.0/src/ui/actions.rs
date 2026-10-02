//! What a choice does (LA5, LA8). Every application starts through BR2 (Task 10); a failed
//! start fails closed: a notification names it and the error is logged at err priority
//! (LA10).

use std::collections::HashMap;
use std::rc::Rc;

use athanor_launcher::menu::{self, Choice};
use athanor_preview::Subject;
use athanor_search::calc::{Calc, Rates};
use athanor_search::item::{Action, Hit};
use athanor_search::providers;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::{Launcher, PREVIEW_DELAY};
use crate::i18n::{tr, tr_with};

impl Launcher {
    pub(super) fn run(self: &Rc<Self>, hit: Hit, choice: Choice) {
        if choice == Choice::Copy {
            // Ctrl+C copies and stays, as Spotlight does.
            self.copy(&hit);
            return;
        }
        if choice == Choice::Open && menu::primary(&hit.action) == Choice::Copy {
            // Enter on a calculation copies the result and closes (LA5).
            self.copy(&hit);
            self.hide();
            return;
        }
        if !menu::choices(&hit.action).contains(&choice) && !matches!(choice, Choice::OpenWith(_)) {
            return;
        }
        // Hiding clears the query: take it first. Usage counts what opened, not what was tried.
        let query = self.engine.current();
        self.hide();
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            match this.perform(&hit, &choice).await {
                Ok(()) => this.engine.record(&query, &hit.key),
                Err(err) => {
                    tracing::error!(title = %hit.title, error = %err, "the result could not be opened");
                    notify_failure(&hit.title).await;
                }
            }
        });
    }

    fn copy(&self, hit: &Hit) {
        if let Some(text) = menu::copy_text(&hit.action, &hit.title) {
            self.display.clipboard().set_text(&text);
        }
    }

    async fn perform(&self, hit: &Hit, choice: &Choice) -> Result<(), String> {
        let client = || self.client.as_ref().ok_or_else(|| "no compositor client".to_owned());
        match (choice, &hit.action) {
            (Choice::Open, Action::Launch { desktop_id }) => {
                let app = gio_unix::DesktopAppInfo::new(desktop_id).ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
                client()?.launch(&app).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::OpenWith(desktop_id), Action::Open { uri }) => {
                let app = gio_unix::DesktopAppInfo::new(desktop_id).ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
                client()?.launch_with(&app, Some(uri)).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Window { index }) => {
                let id = self.windows.borrow().get(*index).copied().ok_or("the window is gone")?;
                client()?.activate(id).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Open { uri } | Action::Web { url: uri }) => {
                client()?.open_uri(uri).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::ShowInFolder, Action::Open { uri }) => {
                let folder = gio::File::for_uri(uri).parent().ok_or("the file has no folder")?.uri();
                client()?.open_uri(&folder).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Provider { bus_name, object_path, result_id, terms }) => {
                providers::activate(bus_name, object_path, result_id, terms, 0).await.map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Command { argv }) => {
                client()?.launch_command(argv).await.map(drop).map_err(|err| err.to_string())
            }
            (choice, action) => Err(format!("{choice:?} does not apply to {action:?}")),
        }
    }

    /// Tab: the choices of the selected result in a popover on its row.
    pub(super) fn open_menu(self: &Rc<Self>) {
        let (Some(hit), Some(row)) = (self.selected_hit(), self.list.selected_row()) else { return };
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let mut choices = menu::choices(&hit.action);
            if let (true, Action::Open { uri }) = (menu::offers_open_with(&hit.action), &hit.action) {
                let info = gio::File::for_uri(uri)
                    .query_info_future("standard::content-type", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT)
                    .await;
                match info.ok().and_then(|info| info.content_type()) {
                    Some(kind) => {
                        choices.splice(
                            1..1,
                            gio::AppInfo::recommended_for_type(&kind).into_iter().filter_map(|app| app.id()).map(|id| Choice::OpenWith(id.to_string())),
                        );
                    }
                    None => tracing::warn!(uri, "no content type; Open with is not offered"),
                }
            }
            this.show_menu(&row, hit, choices);
        });
    }

    fn show_menu(self: &Rc<Self>, row: &gtk4::ListBoxRow, hit: Hit, choices: Vec<Choice>) {
        if self.shown.borrow().is_none() || self.list.selected_row().as_ref() != Some(row) {
            return;
        }
        let list = gtk4::ListBox::new();
        list.update_property(&[gtk4::accessible::Property::Label(&tr("Actions"))]);
        for choice in &choices {
            let label = gtk4::Label::builder().label(label(choice, &hit.action)).xalign(0.0).build();
            list.append(&label);
        }
        let popover = gtk4::Popover::builder().child(&list).position(gtk4::PositionType::Right).autohide(false).build();
        popover.set_parent(row);
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || {
                // fill() may have unparented it already.
                if popover.parent().is_some() {
                    popover.unparent();
                }
            });
        });
        let weak = Rc::downgrade(self);
        let target = popover.downgrade();
        list.connect_row_activated(move |_, picked| {
            if let Some(target) = target.upgrade() {
                target.popdown();
            }
            let (Some(launcher), Ok(index)) = (weak.upgrade(), usize::try_from(picked.index())) else { return };
            if let Some(choice) = choices.get(index) {
                launcher.run(hit.clone(), choice.clone());
            }
        });
        if let Some(previous) = self.menu.replace(Some(popover.clone())) {
            previous.popdown();
        }
        popover.popup();
        // The entry keeps the focus: the menu has no grab, and its selection is drawn.
        if let Some(first) = list.row_at_index(0) {
            list.select_row(Some(&first));
        }
    }

    fn menu_list(&self) -> Option<gtk4::ListBox> {
        self.menu.borrow().as_ref().filter(|menu| menu.is_visible()).and_then(|menu| menu.child()).and_downcast()
    }

    pub(super) fn menu_open(&self) -> bool {
        self.menu_list().is_some()
    }

    pub(super) fn close_menu(&self) {
        if let Some(menu) = self.menu.take() {
            menu.popdown();
        }
    }

    pub(super) fn move_menu(&self, delta: i32) {
        let Some(list) = self.menu_list() else { return };
        let at = list.selected_row().map_or(0, |row| row.index());
        // None past either end: the selection stays on the last choice.
        if let Some(row) = list.row_at_index(at + delta) {
            list.select_row(Some(&row));
        }
    }

    pub(super) fn run_menu(&self) {
        if let Some(row) = self.menu_list().and_then(|list| list.selected_row()) {
            row.activate();
        }
    }

    pub(super) fn schedule_preview(self: &Rc<Self>) {
        if let Some(source) = self.pending_preview.take() {
            source.remove();
        }
        self.selection.set(self.selection.get().wrapping_add(1));
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(PREVIEW_DELAY, move || {
            if let Some(launcher) = weak.upgrade() {
                launcher.pending_preview.replace(None);
                launcher.draw_preview();
            }
        });
        self.pending_preview.replace(Some(source));
    }

    fn draw_preview(self: &Rc<Self>) {
        let Some(hit) = self.selected_hit() else {
            self.preview.clear();
            return;
        };
        let note = || Subject::Note { title: hit.title.clone(), body: hit.subtitle.clone() };
        let subject = match &hit.action {
            Action::Launch { desktop_id } => gio_unix::DesktopAppInfo::new(desktop_id).map_or_else(note, |info| Subject::App { info }),
            Action::Open { uri } if uri.starts_with("file://") => Subject::File { uri: uri.clone() },
            Action::Copy { .. } if hit.group == athanor_search::item::Group::Calc => {
                let calc = Calc { expression: hit.subtitle.clone(), result: hit.title.clone() };
                let rates_date = Rates::read(&Rates::paths()).filter(|rates| rates.involves(&calc)).map(|rates| rates.date);
                Subject::Calc { calc, rates_date }
            }
            Action::Window { index } => {
                self.capture_window(*index, &hit);
                Subject::Window { title: hit.title.clone(), app: hit.subtitle.clone(), picture: None }
            }
            _ => note(),
        };
        self.preview.show(subject);
    }

    /// The thumbnail arrives after the text; it is dropped when the selection moved on.
    fn capture_window(self: &Rc<Self>, index: usize, hit: &Hit) {
        let (Some(id), Some(_)) = (self.windows.borrow().get(index).copied(), self.client.as_ref()) else { return };
        let selection = self.selection.get();
        let (this, hit) = (Rc::clone(self), hit.clone());
        glib::spawn_future_local(async move {
            let Some(client) = this.client.as_ref() else { return };
            match client.capture(id).await {
                Ok(texture) if this.selection.get() == selection => this.preview.show(Subject::Window {
                    title: hit.title,
                    app: hit.subtitle,
                    picture: Some(texture.upcast()),
                }),
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "no window thumbnail"),
            }
        });
    }
}

fn label(choice: &Choice, action: &Action) -> String {
    match (choice, action) {
        (Choice::Open, Action::Window { .. }) => tr("Switch to"),
        (Choice::Open, Action::Command { .. }) => tr("Run in terminal"),
        (Choice::Open, Action::Web { .. }) => tr("Search the web"),
        (Choice::Open, _) => tr("Open"),
        (Choice::OpenWith(id), _) => {
            let name = gio_unix::DesktopAppInfo::new(id).map_or_else(|| id.clone(), |info| info.name().to_string());
            tr_with("Open with {app}", "app", &athanor_unit::text::line(&name, athanor_unit::text::NAME_CHARS))
        }
        (Choice::ShowInFolder, _) => tr("Show in folder"),
        (Choice::Copy, Action::Open { .. }) => tr("Copy path"),
        (Choice::Copy, Action::Web { .. }) => tr("Copy link"),
        (Choice::Copy, Action::Launch { .. }) => tr("Copy name"),
        (Choice::Copy, Action::Window { .. }) => tr("Copy title"),
        (Choice::Copy, _) => tr("Copy"),
    }
}

/// LA10: a failed start fails closed and says so.
async fn notify_failure(title: &str) {
    let bus = match gio::bus_get_future(gio::BusType::Session).await {
        Ok(bus) => bus,
        Err(err) => {
            tracing::error!(error = %err, "no session bus; the failure is only in the journal");
            return;
        }
    };
    let summary = tr_with("{name} could not be opened", "name", title);
    let parameters = (
        "Athanor",
        0u32,
        "dialog-error",
        summary,
        String::new(),
        Vec::<String>::new(),
        HashMap::<String, glib::Variant>::new(),
        -1i32,
    )
        .to_variant();
    let sent = bus
        .call_future(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    if let Err(err) = sent {
        tracing::error!(error = %err, "the failure notification was not shown");
    }
}
