//! What a choice does (LA5, LA8). Every application starts through BR2 (Task 10); a failed
//! start fails closed: a notification names it and the error is logged at err priority
//! (LA10).

use std::collections::HashMap;
use std::rc::Rc;

use athanor_launcher::menu::{self, Choice};
use athanor_launcher::thumbnail;
use athanor_preview::Subject;
use athanor_sal::driver::{Shell, SurfaceId};
use athanor_sal::{Kind, Side, Ui, WidgetId};
use athanor_search::calc::{Calc, Rates};
use athanor_search::item::{Action, Group, Hit};
use athanor_search::providers;
use athanor_sulphur_client::{Client, WindowId};
use gio::glib;
use gio::prelude::*;

use super::{Change, Launcher, Menu, PREVIEW_DELAY};
use crate::i18n::{tr, tr_with};

impl Launcher {
    /// Runs `choice` on `hit`, from a key or a click on `from`, whose `Ui` a copy goes through.
    pub(super) fn run(&mut self, shell: &mut Shell, from: SurfaceId, hit: Hit, choice: Choice) {
        if choice == Choice::Copy {
            // Ctrl+C copies and stays, as Spotlight does.
            copy(shell, from, &hit);
            return;
        }
        if choice == Choice::Open && menu::primary(&hit.action) == Choice::Copy {
            // Enter on a calculation copies the result and closes (LA5). The toolkit serves a
            // copy from the surface's `Ui` once the handler returns: the hide waits for the turn.
            copy(shell, from, &hit);
            self.queue.borrow_mut().push(Change::Hide);
            return;
        }
        if !menu::choices(&hit.action).contains(&choice) && !matches!(choice, Choice::OpenWith(_)) {
            return;
        }
        // Hiding clears the query: take it first. Usage counts what opened, not what was tried.
        let query = self.engine.current();
        self.hide(shell);
        let (engine, client, windows) = (
            self.engine.clone(),
            self.client.clone(),
            self.windows.clone(),
        );
        glib::spawn_future_local(async move {
            match perform(client.as_deref(), &windows, &hit, &choice).await {
                Ok(()) => engine.record(&query, &hit.key),
                Err(err) => {
                    tracing::error!(title = %hit.title, error = %err, "the result could not be opened");
                    notify_failure(&hit.title).await;
                }
            }
        });
    }

    /// Tab: the choices of the selected result, in a menu beside its row. Open with asks GIO
    /// for the file's type first, so the menu opens at a later turn.
    pub(super) fn open_menu(&mut self, shell: &Shell) {
        let (Some(hit), Some(row)) = (self.selected_hit(shell), self.selected_row(shell)) else {
            return;
        };
        let queue = self.queue.clone();
        glib::spawn_future_local(async move {
            let mut choices = menu::choices(&hit.action);
            if let (true, Action::Open { uri }) = (menu::offers_open_with(&hit.action), &hit.action)
            {
                let info = gio::File::for_uri(uri)
                    .query_info_future(
                        "standard::content-type",
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await;
                match info.ok().and_then(|info| info.content_type()) {
                    Some(kind) => {
                        choices.splice(
                            1..1,
                            gio::AppInfo::recommended_for_type(&kind)
                                .into_iter()
                                .filter_map(|app| app.id())
                                .map(|id| Choice::OpenWith(id.to_string())),
                        );
                    }
                    None => tracing::warn!(uri, "no content type; Open with is not offered"),
                }
            }
            queue.borrow_mut().push(Change::Menu { row, hit, choices });
        });
    }

    /// Opens the menu of `choices` on `row`, unless the launcher was hidden or the selection
    /// moved on while the choices were gathered.
    pub(super) fn show_menu(
        &mut self,
        shell: &mut Shell,
        row: WidgetId,
        hit: Hit,
        choices: Vec<Choice>,
    ) {
        let Some(surface) = self.shown else { return };
        if self.selected_row(shell) != Some(row) {
            return;
        }
        let mut ui = Ui::new(&tr("Actions"), self.look.menu.clone(), self.rtl);
        let list = ui.add(Ui::ROOT, Kind::Menu);
        let items = choices
            .into_iter()
            .map(|choice| {
                (
                    ui.add(list, Kind::MenuItem(label(&choice, &hit.action))),
                    choice,
                )
            })
            .collect();
        // Beside the row, at its end edge, as the GTK popover.
        let side = if self.rtl { Side::Left } else { Side::Right };
        match shell.open_popup(surface, row, ui, side) {
            Ok(popup) => self.menu = Some(Menu { popup, hit, items }),
            Err(err) => tracing::error!(error = %err, "cannot open the result's menu"),
        }
    }

    /// Closes the menu, if one is open; the focus goes back to the query.
    pub(super) fn close_menu(&mut self, shell: &mut Shell) {
        if self.menu.take().is_some() {
            shell.close_popup();
            self.focus_query(shell);
        }
    }

    /// The selection settles for `PREVIEW_DELAY` before the preview draws it (LA6).
    pub(super) fn schedule_preview(&mut self) {
        self.selection.set(self.selection.get().wrapping_add(1));
        self.preview_timer
            .start(PREVIEW_DELAY, &self.queue, || Change::DrawPreview);
    }

    pub(super) fn draw_preview(&mut self, shell: &Shell) {
        let Some(hit) = self.selected_hit(shell) else {
            self.preview.clear();
            return;
        };
        let note = || Subject::Note {
            title: hit.title.clone(),
            body: hit.subtitle.clone(),
        };
        let subject = match &hit.action {
            Action::Launch { desktop_id } => gio_unix::DesktopAppInfo::new(desktop_id)
                .map_or_else(note, |info| Subject::App { info }),
            Action::Open { uri } if uri.starts_with("file://") => {
                Subject::File { uri: uri.clone() }
            }
            Action::Copy { .. } if hit.group == Group::Calc => {
                let calc = Calc {
                    expression: hit.subtitle.clone(),
                    result: hit.title.clone(),
                };
                let rates_date = Rates::read(&Rates::paths())
                    .filter(|rates| rates.involves(&calc))
                    .map(|rates| rates.date);
                Subject::Calc { calc, rates_date }
            }
            Action::Window { index } => {
                self.capture_window(*index, &hit);
                Subject::Window {
                    title: hit.title.clone(),
                    app: hit.subtitle.clone(),
                    picture: None,
                }
            }
            _ => note(),
        };
        self.preview.show(subject);
    }

    /// The thumbnail arrives after the text; it is dropped when the selection moved on.
    fn capture_window(&self, index: usize, hit: &Hit) {
        let (Some(id), Some(client)) = (self.windows.get(index).copied(), self.client.clone())
        else {
            return;
        };
        let (selection, at) = (Rc::clone(&self.selection), self.selection.get());
        let (preview, hit) = (Rc::clone(&self.preview), hit.clone());
        glib::spawn_future_local(async move {
            match client.capture_pixels(id).await {
                Ok(capture) if selection.get() == at => {
                    let picture = thumbnail::picture(&capture, preview.picture_side());
                    if picture.is_none() {
                        tracing::warn!(
                            width = capture.width,
                            height = capture.height,
                            stride = capture.stride,
                            bytes = capture.bytes.len(),
                            "the window capture holds no picture; the preview shows none"
                        );
                    }
                    preview.show(Subject::Window {
                        title: hit.title,
                        app: hit.subtitle,
                        picture,
                    });
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "no window thumbnail"),
            }
        });
    }
}

/// Puts what `hit` copies on the clipboard, through the `Ui` of `from`, whose key or click
/// asked for it.
fn copy(shell: &mut Shell, from: SurfaceId, hit: &Hit) {
    if let (Some(text), Some(ui)) = (menu::copy_text(&hit.action, &hit.title), shell.ui_mut(from)) {
        ui.copy(&text);
    }
}

async fn perform(
    client: Option<&Client>,
    windows: &[WindowId],
    hit: &Hit,
    choice: &Choice,
) -> Result<(), String> {
    let client = || client.ok_or_else(|| "no compositor client".to_owned());
    match (choice, &hit.action) {
        (Choice::Open, Action::Launch { desktop_id }) => {
            let app = gio_unix::DesktopAppInfo::new(desktop_id)
                .ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
            client()?
                .launch(&app)
                .await
                .map(drop)
                .map_err(|err| err.to_string())
        }
        (Choice::OpenWith(desktop_id), Action::Open { uri }) => {
            let app = gio_unix::DesktopAppInfo::new(desktop_id)
                .ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
            client()?
                .launch_with(&app, Some(uri))
                .await
                .map(drop)
                .map_err(|err| err.to_string())
        }
        (Choice::Open, Action::Window { index }) => {
            let id = windows.get(*index).copied().ok_or("the window is gone")?;
            client()?
                .window_actions()
                .activate(id)
                .map_err(|err| err.to_string())
        }
        (Choice::Open, Action::Open { uri } | Action::Web { url: uri }) => client()?
            .open_uri(uri)
            .await
            .map(drop)
            .map_err(|err| err.to_string()),
        (Choice::ShowInFolder, Action::Open { uri }) => {
            let folder = gio::File::for_uri(uri)
                .parent()
                .ok_or("the file has no folder")?
                .uri();
            client()?
                .open_uri(&folder)
                .await
                .map(drop)
                .map_err(|err| err.to_string())
        }
        (
            Choice::Open,
            Action::Provider {
                bus_name,
                object_path,
                result_id,
                terms,
            },
        ) => providers::activate(bus_name, object_path, result_id, terms, 0)
            .await
            .map_err(|err| err.to_string()),
        (Choice::Open, Action::Command { argv }) => client()?
            .launch_command(argv)
            .await
            .map(drop)
            .map_err(|err| err.to_string()),
        (choice, action) => Err(format!("{choice:?} does not apply to {action:?}")),
    }
}

fn label(choice: &Choice, action: &Action) -> String {
    match (choice, action) {
        (Choice::Open, Action::Window { .. }) => tr("Switch to"),
        (Choice::Open, Action::Command { .. }) => tr("Run in terminal"),
        (Choice::Open, Action::Web { .. }) => tr("Search the web"),
        (Choice::Open, _) => tr("Open"),
        (Choice::OpenWith(id), _) => {
            let name = gio_unix::DesktopAppInfo::new(id)
                .map_or_else(|| id.clone(), |info| info.name().to_string());
            tr_with(
                "Open with {app}",
                "app",
                &athanor_unit::text::line(&name, athanor_unit::text::NAME_CHARS),
            )
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
