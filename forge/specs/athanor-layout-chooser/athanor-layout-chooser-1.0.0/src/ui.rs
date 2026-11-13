//! The layout chooser window (doc_shell.md, SH6-SH8): three groups of toggle buttons. A
//! pick writes the user document; the translator, watching it, applies it.

use athanor_layout::document::{Document, DocumentError, Key};
use athanor_layout::loader::{self, Paths, Resolved, UserState};
use athanor_layout::preset::{DockKnob, PanelEdge, Preset};
use athanor_layout::user::{self, Change};
use athanor_sal::driver::Role;
use athanor_sal::{
    Application, Event, Kind, RootRole, Shell, SurfaceId, Ui, WidgetId, WindowButton, Wrap,
};

use crate::i18n::{tr, tr_with};
use crate::look::Look;

/// The application id, as the name of the chooser's `.desktop` file.
pub const APP_ID: &str = "os.athanor.Layout";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupState {
    Open,
    /// Marked mandatory by the policy layer: shown, insensitive, explained.
    Locked,
    /// No knob in the preset in force.
    Hidden,
}

pub fn group_state(resolved: &Resolved, key: Key) -> GroupState {
    if key == Key::Dock && !resolved.layout.preset().has_dock() {
        GroupState::Hidden
    } else if resolved.mandatory.contains(&key) {
        GroupState::Locked
    } else {
        GroupState::Open
    }
}

/// What the status line says about the user document, if anything (SH8).
pub fn status_line(resolved: &Resolved) -> Option<String> {
    let UserState::Rejected { error, .. } = &resolved.user else {
        return None;
    };
    Some(match error {
        DocumentError::NewerSchema(schema) => tr_with(
            "Your layout.toml was written by a newer Athanor (schema {schema}). The nearest layout is in use; choosing one here replaces it.",
            "schema",
            &schema.to_string(),
        ),
        _ => tr("Your layout.toml could not be read and was left unchanged. The nearest layout is in use; choosing one here replaces it."),
    })
}

/// One labelled group of choices.
struct Group {
    key: Key,
    row: WidgetId,
    note: WidgetId,
    /// Each button with the change it makes.
    buttons: Vec<(WidgetId, Change)>,
}

/// The question asked before a newer document is replaced, while its dialog is open.
struct Question {
    dialog: SurfaceId,
    replace: WidgetId,
    document: Document,
    schema: i64,
}

/// The chooser's window and what it acts on.
pub struct Chooser {
    paths: Paths,
    look: Look,
    /// The window's widgets until the driver opens them.
    ui: Option<Ui>,
    /// The window, once drawn.
    window: Option<SurfaceId>,
    groups: Vec<Group>,
    status: WidgetId,
    question: Option<Question>,
}

impl Chooser {
    /// The window for the layout in force, in the catalogue's direction.
    pub fn new(paths: Paths, look: Look, rtl: bool) -> Self {
        let mut ui = Ui::new(&tr("Layout"), look.theme.clone(), rtl);
        ui.min_width = 420.0;
        let groups = vec![
            group(
                &mut ui,
                &look,
                Key::Preset,
                &tr("Style"),
                &[
                    (tr("Island"), Change::Preset(Preset::Float)),
                    (tr("Bar"), Change::Preset(Preset::Bar)),
                    (tr("Essential"), Change::Preset(Preset::Minimal)),
                ],
            ),
            group(
                &mut ui,
                &look,
                Key::Panel,
                &tr("Panel"),
                &[
                    (tr("Top"), Change::Panel(PanelEdge::Top)),
                    (tr("Bottom"), Change::Panel(PanelEdge::Bottom)),
                ],
            ),
            group(
                &mut ui,
                &look,
                Key::Dock,
                &tr("Dock"),
                &[
                    (tr("Visible"), Change::Dock(DockKnob::Visible)),
                    (tr("Auto-hide"), Change::Dock(DockKnob::AutoHide)),
                    (tr("None"), Change::Dock(DockKnob::Off)),
                ],
            ),
        ];
        let status = wrapping_note(&mut ui, &look, Ui::ROOT);
        let mut chooser = Self {
            paths,
            look,
            ui: None,
            window: None,
            groups,
            status,
            question: None,
        };
        chooser.refresh(&mut ui);
        chooser.ui = Some(ui);
        chooser
    }

    /// The window's widgets before the driver opens them, for the tests.
    #[cfg(test)]
    fn ui(&self) -> Option<&Ui> {
        self.ui.as_ref()
    }

    /// Shows the layout in force, as the loader resolves it now.
    fn refresh(&self, ui: &mut Ui) {
        let resolved = loader::resolve(&self.paths);
        for group in &self.groups {
            let state = group_state(&resolved, group.key);
            ui.set_visible(group.row, state != GroupState::Hidden);
            ui.set_sensitive(group.row, state == GroupState::Open);
            let note = match state {
                GroupState::Locked => tr("Set by your administrator."),
                GroupState::Hidden => tr("The bar holds the running applications."),
                GroupState::Open => String::new(),
            };
            ui.set_text(group.note, &note);
            ui.set_visible(group.note, state != GroupState::Open);
            for (button, change) in &group.buttons {
                // Set without a pick; the active toggle releases the others of its row.
                let active = match change {
                    Change::Preset(preset) => resolved.layout.preset() == *preset,
                    Change::Panel(panel) => resolved.layout.panel() == *panel,
                    Change::Dock(dock) => resolved.layout.dock() == *dock,
                };
                if active {
                    ui.set_active(*button, true);
                }
            }
        }
        let line = status_line(&resolved);
        ui.set_text(self.status, line.as_deref().unwrap_or(""));
        ui.set_visible(self.status, line.is_some());
        ui.layout();
    }

    /// The change the toggle `id` makes, when it is one of the groups'.
    fn change(&self, id: WidgetId) -> Option<Change> {
        self.groups
            .iter()
            .flat_map(|group| &group.buttons)
            .find(|(button, _)| *button == id)
            .map(|(_, change)| *change)
    }

    fn pick(&mut self, shell: &mut Shell, window: SurfaceId, change: Change) {
        let pending = user::prepare(&self.paths, change);
        let Some(schema) = pending.replaces_newer else {
            self.save(shell, window, &pending.document, None);
            return;
        };
        let file = user::write_target(&self.paths.user_file)
            .map(|target| user::backup_path(&target, schema).display().to_string())
            .unwrap_or_else(|_| format!("layout.toml.{schema}"));
        let rtl = shell.ui(window).is_some_and(|ui| ui.rtl);
        let (ui, replace) = question(&self.look, rtl, &file);
        match shell.open_dialog(window, ui) {
            Ok(dialog) => {
                self.question = Some(Question {
                    dialog,
                    replace,
                    document: pending.document,
                    schema,
                });
            }
            Err(error) => {
                // Nothing is saved without an answer: the window goes back to the layout in force.
                tracing::error!(%error, "cannot ask before replacing the newer layout document");
                if let Some(ui) = shell.ui_mut(window) {
                    self.refresh(ui);
                }
            }
        }
    }

    /// The answer to the question: Replace saves and keeps the newer file; Cancel, Escape and
    /// closing the dialog keep the layout in force.
    fn answer(&mut self, shell: &mut Shell, window: SurfaceId, event: Event) {
        let replace = match event {
            Event::Clicked(id) => self.question.as_ref().is_some_and(|q| q.replace == id),
            Event::Window(WindowButton::Close) => false,
            _ => return,
        };
        let Some(question) = self.question.take() else {
            return;
        };
        shell.close(question.dialog);
        if replace {
            self.save(shell, window, &question.document, Some(question.schema));
        } else if let Some(ui) = shell.ui_mut(window) {
            self.refresh(ui);
        }
    }

    fn save(&self, shell: &mut Shell, window: SurfaceId, document: &Document, keep: Option<i64>) {
        let saved = user::save(&self.paths.user_file, document, keep);
        let Some(ui) = shell.ui_mut(window) else {
            return;
        };
        // The translator follows the file; the window shows what was just saved.
        self.refresh(ui);
        if let Err(err) = saved {
            tracing::error!(error = %err, "cannot save the layout document");
            let line = tr_with(
                "The layout could not be saved: {error}",
                "error",
                &err.to_string(),
            );
            ui.set_text(self.status, &line);
            ui.set_visible(self.status, true);
            ui.layout();
        }
    }
}

impl Application for Chooser {
    fn start(&mut self) -> Vec<(Ui, Role)> {
        let role = Role::Window {
            app_id: APP_ID.into(),
        };
        self.ui.take().map(|ui| (ui, role)).into_iter().collect()
    }

    fn drawn(&mut self, _: &mut Shell, surface: SurfaceId) {
        self.window.get_or_insert(surface);
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        let Some(window) = self.window else {
            return;
        };
        if self.question.as_ref().is_some_and(|q| q.dialog == surface) {
            self.answer(shell, window, event);
        } else if let Event::Selected(id) = event {
            if let Some(change) = self.change(id) {
                self.pick(shell, window, change);
            }
        }
    }
}

/// One labelled group: a heading, a row of toggle buttons, a note under it.
fn group(ui: &mut Ui, look: &Look, key: Key, title: &str, choices: &[(String, Change)]) -> Group {
    let container = ui.add(Ui::ROOT, Kind::Group(title.into()));
    let heading = ui.add(container, Kind::Label(title.into()));
    look.heading(ui, heading);
    let row = ui.add(container, Kind::Row);
    // One choice per row, as radio buttons behave: pressing the active one keeps it.
    let buttons = choices
        .iter()
        .enumerate()
        .map(|(i, (label, change))| (ui.toggle(row, label, i == 0), *change))
        .collect();
    let note = wrapping_note(ui, look, container);
    Group {
        key,
        row,
        note,
        buttons,
    }
}

/// A hidden label under `parent` that wraps at the width it is given, as GTK's wrapping label.
fn wrapping_note(ui: &mut Ui, look: &Look, parent: WidgetId) -> WidgetId {
    let note = ui.add(parent, Kind::Label(String::new()));
    ui.set_wrap(note, Some(Wrap::default()));
    look.note(ui, note);
    ui.set_visible(note, false);
    note
}

/// The question before a newer document is replaced, as GTK's alert dialog asks it: the
/// message, the file kept, then Cancel, which has the focus, and Replace. Returns the dialog's
/// widgets and its Replace button.
fn question(look: &Look, rtl: bool, file: &str) -> (Ui, WidgetId) {
    let message = tr("Replace the newer layout?");
    let mut ui = Ui::new(&message, look.theme.clone(), rtl);
    ui.root_role = RootRole::AlertDialog;
    ui.min_width = 300.0;
    let heading = ui.add(Ui::ROOT, Kind::Label(message));
    look.message(&mut ui, heading);
    let detail = tr_with("Athanor keeps the current file as {file}.", "file", file);
    let detail = ui.add(Ui::ROOT, Kind::Label(detail));
    ui.set_wrap(detail, Some(Wrap::default()));
    let row = ui.add(Ui::ROOT, Kind::Row);
    let cancel = ui.add(row, Kind::Button(tr("Cancel")));
    let replace = ui.add(row, Kind::Button(tr("Replace")));
    ui.set_focus(Some(cancel));
    ui.layout();
    (ui, replace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_layout::document::{Document, DocumentError};
    use athanor_layout::loader::UserState;
    use athanor_layout::preset::{DockKnob, Layout, PanelEdge, Preset};
    use athanor_style::appearance::Appearance;
    use std::collections::BTreeSet;

    fn resolved(layout: Layout, mandatory: &[Key], user: UserState) -> Resolved {
        Resolved {
            layout,
            mandatory: mandatory.iter().copied().collect::<BTreeSet<_>>(),
            user,
            policy_names_preset: false,
        }
    }

    fn float() -> Layout {
        Layout::new(Preset::Float, PanelEdge::Top, DockKnob::Visible)
    }

    #[test]
    fn a_mandatory_key_greys_its_group_and_says_why() {
        let state = resolved(float(), &[Key::Panel], UserState::Absent);
        assert_eq!(group_state(&state, Key::Panel), GroupState::Locked);
        assert_eq!(group_state(&state, Key::Preset), GroupState::Open);
    }

    #[test]
    fn the_dock_group_is_hidden_under_the_bar() {
        let bar = resolved(
            Layout::new(Preset::Bar, PanelEdge::Bottom, DockKnob::Off),
            &[],
            UserState::Absent,
        );
        assert_eq!(group_state(&bar, Key::Dock), GroupState::Hidden);
        let locked_bar = resolved(bar.layout, &[Key::Dock], UserState::Absent);
        assert_eq!(
            group_state(&locked_bar, Key::Dock),
            GroupState::Hidden,
            "hidden wins: there is no knob to grey"
        );
    }

    #[test]
    fn a_degraded_document_is_explained_and_a_valid_one_is_not() {
        assert_eq!(
            status_line(&resolved(float(), &[], UserState::Absent)),
            None
        );
        assert_eq!(
            status_line(&resolved(
                float(),
                &[],
                UserState::Valid(Document::default())
            )),
            None
        );
        let unknown = UserState::Rejected {
            error: DocumentError::UnknownKey("colour".into()),
            nearest: Some(Preset::Float),
        };
        assert!(status_line(&resolved(float(), &[], unknown))
            .expect("a line")
            .contains("layout.toml"));
        let newer = UserState::Rejected {
            error: DocumentError::NewerSchema(3),
            nearest: None,
        };
        assert!(status_line(&resolved(float(), &[], newer))
            .expect("a line")
            .contains('3'));
    }

    /// Paths under a fresh directory, holding `user` as the user document when given.
    fn paths(name: &str, user: Option<&str>) -> Paths {
        let root = std::env::temp_dir().join(format!(
            "athanor-layout-chooser-{name}-{}",
            std::process::id()
        ));
        // A directory left by an earlier run of this test would carry its document.
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove the old test directory");
        }
        let paths = Paths {
            vendor_dir: root.join("vendor"),
            policy_dir: root.join("policy"),
            user_file: root.join("athanor/layout.toml"),
        };
        std::fs::create_dir_all(root.join("athanor")).expect("create the config directory");
        if let Some(text) = user {
            std::fs::write(&paths.user_file, text).expect("write the user document");
        }
        paths
    }

    fn chooser(paths: Paths) -> Chooser {
        Chooser::new(paths, crate::look::look(&Appearance::default()), false)
    }

    #[test]
    fn the_window_shows_the_layout_in_force() {
        let chooser = chooser(paths(
            "in-force",
            Some("schema = 1\n[output.\"*\"]\npreset = \"bar\"\npanel = \"bottom\"\n"),
        ));
        let ui = chooser.ui().expect("the window's widgets");
        let active: Vec<_> = chooser
            .groups
            .iter()
            .flat_map(|group| &group.buttons)
            .filter(|(button, _)| ui.is_active(*button))
            .map(|(_, change)| *change)
            .collect();
        assert_eq!(
            active,
            [
                Change::Preset(Preset::Bar),
                Change::Panel(PanelEdge::Bottom),
                Change::Dock(DockKnob::Off),
            ]
        );
        let dock = &chooser.groups[2];
        assert!(!ui.is_visible(dock.row), "the bar has no dock knob");
        assert!(ui.is_visible(dock.note));
        assert!(!ui.is_visible(chooser.status));
    }

    #[test]
    fn a_newer_document_is_explained_on_the_status_line() {
        let chooser = chooser(paths("newer", Some("schema = 99\n")));
        let ui = chooser.ui().expect("the window's widgets");
        assert!(ui.is_visible(chooser.status));
    }

    #[test]
    fn the_question_has_cancel_focused_and_replace_second() {
        let (ui, replace) = question(
            &crate::look::look(&Appearance::default()),
            false,
            "layout.toml.99",
        );
        assert_eq!(ui.root_role, RootRole::AlertDialog);
        let cancel = ui.focus().expect("a focused button");
        assert_ne!(cancel, replace);
        assert!(matches!(ui.widget(cancel).kind, Kind::Button(ref t) if t == "Cancel"));
        assert!(matches!(ui.widget(replace).kind, Kind::Button(ref t) if t == "Replace"));
    }
}
