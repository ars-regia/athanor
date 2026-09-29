//! The layout chooser window (doc_shell.md, SH6-SH8): three groups of toggle buttons. A
//! pick writes the user document; the translator, watching it, applies it.

use std::rc::Rc;

use athanor_layout::document::{DocumentError, Key};
use athanor_layout::loader::{self, Paths, Resolved, UserState};
use athanor_layout::preset::{DockKnob, PanelEdge, Preset};
use athanor_layout::user::{self, Change};
use athanor_style::{calmo, cosmic_theme};
use gtk4::accessible::Relation;
use gtk4::prelude::*;
use gtk4::{
    AccessibleRole, AlertDialog, Application, ApplicationWindow, Box as GtkBox, Label, Orientation,
    ToggleButton,
};

use crate::i18n::{tr, tr_with};

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
    row: GtkBox,
    note: Label,
    /// Each button with the change it makes.
    buttons: Vec<(ToggleButton, Change)>,
}

struct Chooser {
    paths: Paths,
    window: ApplicationWindow,
    groups: Vec<Group>,
    status: Label,
}

pub fn build_ui(app: &Application, paths: Paths) {
    if crate::i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let window = ApplicationWindow::builder()
        .application(app)
        .title(tr("Layout"))
        .default_width(420)
        .resizable(false)
        .build();
    window.add_css_class("athanor-surface");
    window.add_css_class("athanor-layout");

    let display = gtk4::prelude::WidgetExt::display(&window);
    let theme = cosmic_theme::read();
    calmo::load(&display, theme.variant());
    cosmic_theme::load_accent(&display, &theme);

    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    let groups = vec![
        group(
            &content,
            Key::Preset,
            &tr("Style"),
            &[
                (tr("Island"), Change::Preset(Preset::Float)),
                (tr("Bar"), Change::Preset(Preset::Bar)),
                (tr("Essential"), Change::Preset(Preset::Minimal)),
            ],
        ),
        group(
            &content,
            Key::Panel,
            &tr("Panel"),
            &[
                (tr("Top"), Change::Panel(PanelEdge::Top)),
                (tr("Bottom"), Change::Panel(PanelEdge::Bottom)),
            ],
        ),
        group(
            &content,
            Key::Dock,
            &tr("Dock"),
            &[
                (tr("Visible"), Change::Dock(DockKnob::Visible)),
                (tr("Auto-hide"), Change::Dock(DockKnob::AutoHide)),
                (tr("None"), Change::Dock(DockKnob::Off)),
            ],
        ),
    ];
    let status = Label::builder()
        .wrap(true)
        .xalign(0.0)
        .css_classes(["layout-status"])
        .visible(false)
        .build();
    content.append(&status);
    window.set_child(Some(&content));

    let chooser = Rc::new(Chooser {
        paths,
        window,
        groups,
        status,
    });
    for group in &chooser.groups {
        for (button, change) in &group.buttons {
            // A strong reference: nothing else outlives build_ui, so a weak one never
            // upgrades. GTK drops the handler, and the cycle with it, when the window is
            // disposed.
            let (chooser, change) = (Rc::clone(&chooser), *change);
            button.connect_clicked(move |_| chooser.pick(change));
        }
    }
    chooser.refresh();
    chooser.window.present();
}

/// One labelled group: a title, a row of toggle buttons, a note under it.
fn group(content: &GtkBox, key: Key, title: &str, choices: &[(String, Change)]) -> Group {
    let container = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .accessible_role(AccessibleRole::Group)
        .build();
    let heading = Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["layout-group-title"])
        .build();
    container.update_relation(&[Relation::LabelledBy(&[heading.upcast_ref()])]);
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .homogeneous(true)
        .build();
    let buttons: Vec<(ToggleButton, Change)> = choices
        .iter()
        .map(|(label, change)| {
            let button = ToggleButton::builder()
                .label(label)
                .css_classes(["layout-choice"])
                .build();
            row.append(&button);
            (button, *change)
        })
        .collect();
    // One choice per group, as radio buttons behave: pressing the active one keeps it.
    if let Some((first, _)) = buttons.first() {
        for (button, _) in &buttons[1..] {
            button.set_group(Some(first));
        }
    }
    let note = Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["layout-note"])
        .visible(false)
        .build();
    container.append(&heading);
    container.append(&row);
    container.append(&note);
    content.append(&container);
    Group {
        key,
        row,
        note,
        buttons,
    }
}

impl Chooser {
    /// Shows the layout in force, as the loader resolves it now.
    fn refresh(&self) {
        let resolved = loader::resolve(&self.paths);
        for group in &self.groups {
            let state = group_state(&resolved, group.key);
            group.row.set_visible(state != GroupState::Hidden);
            group.row.set_sensitive(state == GroupState::Open);
            match state {
                GroupState::Locked => group.note.set_label(&tr("Set by your administrator.")),
                GroupState::Hidden => group
                    .note
                    .set_label(&tr("The bar holds the running applications.")),
                GroupState::Open => group.note.set_label(""),
            }
            group.note.set_visible(state != GroupState::Open);
            for (button, change) in &group.buttons {
                // Set without emitting `clicked`: a refresh is not a pick.
                let active = match change {
                    Change::Preset(preset) => resolved.layout.preset() == *preset,
                    Change::Panel(panel) => resolved.layout.panel() == *panel,
                    Change::Dock(dock) => resolved.layout.dock() == *dock,
                };
                button.set_active(active);
            }
        }
        let line = status_line(&resolved);
        self.status.set_visible(line.is_some());
        self.status.set_label(line.as_deref().unwrap_or(""));
    }

    fn pick(self: &Rc<Self>, change: Change) {
        let pending = user::prepare(&self.paths, change);
        let Some(schema) = pending.replaces_newer else {
            self.save(&pending.document, None);
            return;
        };
        let file = user::write_target(&self.paths.user_file)
            .map(|target| user::backup_path(&target, schema).display().to_string())
            .unwrap_or_else(|_| format!("layout.toml.{schema}"));
        let dialog = AlertDialog::builder()
            .modal(true)
            .message(tr("Replace the newer layout?"))
            .detail(tr_with(
                "Athanor keeps the current file as {file}.",
                "file",
                &file,
            ))
            .buttons([tr("Cancel"), tr("Replace")])
            .cancel_button(0)
            .default_button(0)
            .build();
        let chooser = Rc::clone(self);
        dialog.choose(
            Some(&self.window),
            gtk4::gio::Cancellable::NONE,
            move |answer| {
                if matches!(answer, Ok(1)) {
                    chooser.save(&pending.document, Some(schema));
                } else {
                    chooser.refresh();
                }
            },
        );
    }

    fn save(&self, document: &athanor_layout::document::Document, keep_newer: Option<i64>) {
        if let Err(err) = user::save(&self.paths.user_file, document, keep_newer) {
            tracing::error!(error = %err, "cannot save the layout document");
            self.refresh();
            self.status.set_label(&tr_with(
                "The layout could not be saved: {error}",
                "error",
                &err.to_string(),
            ));
            self.status.set_visible(true);
            return;
        }
        // The translator follows the file; the window shows what was just saved.
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_layout::document::{Document, DocumentError};
    use athanor_layout::loader::UserState;
    use athanor_layout::preset::{DockKnob, Layout, PanelEdge, Preset};
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
}
