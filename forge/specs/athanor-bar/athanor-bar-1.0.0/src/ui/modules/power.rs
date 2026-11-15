//! The power menu of the toolkit bar (doc_bar.md, BR3), as `ui/power.rs` draws the GTK one:
//! Lock, Log Out, Suspend, Restart and Shut Down, each asked once more before it runs, on a
//! second page with Cancel, which takes the focus, and the action's own button. Suspend,
//! Restart and Shut Down show only while logind offers them, asked each time the menu opens.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::i18n::tr;
use athanor_bar::logind;
use athanor_bar::power::Action;
use athanor_sal::{Event, Kind, Theme, Ui, WidgetId, Wrap};

use super::{Cx, Module, Reply, Start, Wake, Wakeup, ICON};

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

/// The menu's `page`, in `theme`, in a popover below the bar when `top`.
pub fn build(theme: Theme, rtl: bool, top: bool, page: Page, offered: &[bool; 5]) -> (Ui, Menu) {
    let (mut ui, frame) = crate::ui::look::popover(&tr("Power"), theme, rtl, top);
    let mut acts = Vec::new();
    match page {
        Page::Actions => {
            // A menu's rows, flat as the GTK popover's `bar-row` buttons.
            let list = ui.add(frame, Kind::Menu);
            for action in Action::ALL {
                if shown(action, offered) {
                    let (label, _) = texts(action);
                    acts.push((ui.add(list, Kind::Button(label)), Act::Ask(action)));
                }
            }
        }
        Page::Confirm(action) => {
            let (label, question) = texts(action);
            let text = ui.add(frame, Kind::Label(question));
            ui.set_wrap(text, Some(Wrap::default()));
            let buttons = ui.add(frame, Kind::Row);
            acts.push((ui.add(buttons, Kind::Button(tr("Cancel"))), Act::Cancel));
            acts.push((ui.add(buttons, Kind::Button(label)), Act::Run(action)));
        }
    }
    (ui, Menu { acts })
}

/// logind's last answers, in the order of [`Action::ALL`].
type Offered = Rc<Cell<[bool; 5]>>;

struct Power {
    offered: Offered,
    wake: Wake,
    page: Page,
    /// The open menu's buttons, and the answers it was drawn with.
    menu: Option<(Menu, [bool; 5])>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let power = Power {
        offered: Offered::default(),
        wake: start.wake.clone(),
        page: Page::Actions,
        menu: None,
    };
    power.ask_logind();
    Some(Box::new(power))
}

impl Power {
    /// Asks logind whether each action it decides is offered; an open menu follows each
    /// answer.
    fn ask_logind(&self) {
        for (at, action) in Action::ALL.into_iter().enumerate() {
            if action.can_method().is_none() {
                continue;
            }
            let (offered, wake) = (self.offered.clone(), self.wake.clone());
            glib::spawn_future_local(async move {
                let answer = logind::offered(action).await;
                let mut now = offered.get();
                now[at] = answer;
                offered.set(now);
                wake(Wakeup::Changed);
            });
        }
    }
}

impl Module for Power {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let icon = "system-shutdown-symbolic";
        Some(cx.icons.button(ui, group, icon, ICON, cx.scale, &tr("Power"), None))
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        if self.page == Page::Actions {
            self.ask_logind();
        }
        let offered = self.offered.get();
        let (ui, menu) = build(cx.theme.clone(), cx.rtl, cx.top, self.page, &offered);
        self.menu = Some((menu, offered));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, _ui: &mut Ui, event: Event) -> Reply {
        let Event::Clicked(widget) = event else {
            return Reply::Stay;
        };
        match self.menu.as_ref().and_then(|(menu, _)| menu.act(widget)) {
            Some(Act::Ask(action)) => {
                self.page = Page::Confirm(action);
                Reply::Reopen
            }
            Some(Act::Cancel) => {
                self.page = Page::Actions;
                Reply::Reopen
            }
            Some(Act::Run(action)) => {
                glib::spawn_future_local(async move {
                    if let Err(err) = logind::run(action).await {
                        tracing::error!(error = %err, action = action.id(), "the power action failed");
                    }
                });
                Reply::Close
            }
            None => Reply::Stay,
        }
    }

    fn refresh(&mut self, _cx: &mut Cx, _ui: &mut Ui) -> Reply {
        // A row comes or goes with logind's answer.
        match &self.menu {
            Some((_, drawn)) if self.page == Page::Actions && *drawn != self.offered.get() => {
                Reply::Reopen
            }
            _ => Reply::Stay,
        }
    }

    fn closed(&mut self) {
        self.page = Page::Actions;
        self.menu = None;
    }
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
            true,
            Page::Confirm(Action::Reboot),
            &[true; 5],
        );
        let acts: Vec<Act> = menu.acts.iter().map(|(_, act)| *act).collect();
        assert_eq!(acts, [Act::Cancel, Act::Run(Action::Reboot)]);
        assert!(ui.len() > 3, "the question, the row and its two buttons");
    }

    #[test]
    fn a_row_asks_before_its_action_runs() {
        let (_, menu) = build(Theme::default(), false, true, Page::Actions, &[true; 5]);
        assert!(menu.acts.iter().all(|(_, act)| matches!(act, Act::Ask(_))));
        assert_eq!(menu.acts.len(), Action::ALL.len());
    }
}
