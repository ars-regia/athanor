//! The power menu of the toolkit bar (doc_bar.md, BR3), as `ui/power.rs` draws the GTK one:
//! Lock, Log Out, Suspend, Restart and Shut Down, each asked once more before it runs, on a
//! second page with Cancel, which takes the focus, and the action's own button. Suspend,
//! Restart and Shut Down show only while logind offers them, asked each time the menu opens.

use athanor_bar::i18n::tr;
use athanor_bar::power::Action;
use athanor_sal::{Kind, Theme, Ui, WidgetId, Wrap};

/// The page of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Actions,
    Confirm(Action),
}

/// What a button of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Asks before `Action` runs: the confirmation page.
    Ask(Action),
    /// Back to the actions.
    Cancel,
    Run(Action),
}

/// The label of `action`'s row and the question its confirmation asks.
fn texts(action: Action) -> (String, String) {
    match action {
        Action::Lock => (tr("Lock"), tr("Lock the screen?")),
        Action::LogOut => (tr("Log Out"), tr("Log out now?")),
        Action::Suspend => (tr("Suspend"), tr("Suspend now?")),
        Action::Reboot => (tr("Restart"), tr("Restart now?")),
        Action::PowerOff => (tr("Shut Down"), tr("Shut down now?")),
    }
}

/// Whether `action` is shown: always for those logind is not asked about, else as its last
/// answer said, `offered` in the order of [`Action::ALL`].
pub fn shown(action: Action, offered: &[bool; 5]) -> bool {
    action.can_method().is_none()
        || Action::ALL
            .iter()
            .position(|a| *a == action)
            .is_some_and(|at| offered[at])
}

/// The menu's buttons and what each does.
pub struct Menu {
    acts: Vec<(WidgetId, Act)>,
}

impl Menu {
    pub fn act(&self, widget: WidgetId) -> Option<Act> {
        self.acts
            .iter()
            .find(|(id, _)| *id == widget)
            .map(|(_, act)| *act)
    }
}

/// The menu's `page`, in `theme`.
pub fn build(theme: Theme, rtl: bool, page: Page, offered: &[bool; 5]) -> (Ui, Menu) {
    let mut ui = Ui::new(&tr("Power"), theme, rtl);
    let mut acts = Vec::new();
    match page {
        Page::Actions => {
            for action in Action::ALL {
                if shown(action, offered) {
                    let (label, _) = texts(action);
                    acts.push((ui.add(Ui::ROOT, Kind::Button(label)), Act::Ask(action)));
                }
            }
        }
        Page::Confirm(action) => {
            let (label, question) = texts(action);
            let text = ui.add(Ui::ROOT, Kind::Label(question));
            ui.set_wrap(text, Some(Wrap::default()));
            let buttons = ui.add(Ui::ROOT, Kind::Row);
            acts.push((ui.add(buttons, Kind::Button(tr("Cancel"))), Act::Cancel));
            acts.push((ui.add(buttons, Kind::Button(label)), Act::Run(action)));
        }
    }
    (ui, Menu { acts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_and_log_out_show_without_an_answer_from_logind() {
        let none = [false; 5];
        let shown: Vec<Action> = Action::ALL
            .into_iter()
            .filter(|a| shown(*a, &none))
            .collect();
        assert_eq!(shown, [Action::Lock, Action::LogOut]);
        let all = [true; 5];
        assert!(Action::ALL.into_iter().all(|a| super::shown(a, &all)));
    }

    #[test]
    fn the_confirmation_asks_with_cancel_first_and_runs_only_from_its_own_button() {
        let (ui, menu) = build(
            Theme::default(),
            false,
            Page::Confirm(Action::Reboot),
            &[true; 5],
        );
        let acts: Vec<Act> = menu.acts.iter().map(|(_, act)| *act).collect();
        assert_eq!(acts, [Act::Cancel, Act::Run(Action::Reboot)]);
        assert!(ui.len() > 3, "the question, the row and its two buttons");
    }

    #[test]
    fn a_row_asks_before_its_action_runs() {
        let (_, menu) = build(Theme::default(), false, Page::Actions, &[true; 5]);
        assert!(menu.acts.iter().all(|(_, act)| matches!(act, Act::Ask(_))));
        assert_eq!(menu.acts.len(), Action::ALL.len());
    }
}
