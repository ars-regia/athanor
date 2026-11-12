//! Modal dialogs: a window the application opens on one of its windows, through
//! [`super::Shell::open_dialog`]. It is the parent's child (`xdg_toplevel.set_parent`), modal
//! through `xdg_wm_dialog_v1` when the compositor offers it, and the driver takes no input for
//! the parent while it is open, as GTK's modal windows: neither the pointer, nor the keyboard,
//! nor the input method, nor an assistive technology's action reaches it.

use super::{App, SurfaceId};
use smithay_client_toolkit::dispatch2::Dispatch2;
pub(super) use smithay_client_toolkit::reexports::protocols::xdg::dialog::v1::client::xdg_dialog_v1::XdgDialogV1;
use smithay_client_toolkit::reexports::protocols::xdg::dialog::v1::client::{
    xdg_dialog_v1, xdg_wm_dialog_v1::{self, XdgWmDialogV1},
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_client::globals::GlobalList;
use wayland_client::{Connection, QueueHandle};

/// The dialogs open, each with the window it was opened on, and the compositor's dialog global.
#[derive(Default)]
pub(super) struct Dialogs {
    manager: Option<XdgWmDialogV1>,
    open: Vec<(SurfaceId, SurfaceId)>,
}

impl Dialogs {
    /// Binds `xdg_wm_dialog_v1`, optional: without it, a dialog is still the parent's child and
    /// the driver still blocks the parent's input; only the compositor is not told it is modal.
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self {
            manager: globals.bind(qh, 1..=1, DialogData).ok(),
            open: Vec::new(),
        }
    }

    /// Makes `toplevel` modal to its parent, when the compositor offers the global.
    pub(super) fn set_modal(
        &self,
        toplevel: &XdgToplevel,
        qh: &QueueHandle<App>,
    ) -> Option<XdgDialogV1> {
        let dialog = self
            .manager
            .as_ref()?
            .get_xdg_dialog(toplevel, qh, DialogData);
        dialog.set_modal();
        Some(dialog)
    }

    /// Records `dialog`, opened on `parent`.
    pub(super) fn opened(&mut self, dialog: SurfaceId, parent: SurfaceId) {
        self.open.push((dialog, parent));
    }

    /// Whether `id` is a dialog.
    pub(super) fn is_dialog(&self, id: SurfaceId) -> bool {
        self.open.iter().any(|&(dialog, _)| dialog == id)
    }

    /// Whether a dialog open on `id` keeps its input from it.
    pub(super) fn blocked(&self, id: SurfaceId) -> bool {
        self.open.iter().any(|&(_, parent)| parent == id)
    }

    /// Forgets `id`, a dialog or a parent; returns the dialogs open on it, which close with it.
    pub(super) fn closed(&mut self, id: SurfaceId) -> Vec<SurfaceId> {
        let children = self
            .open
            .iter()
            .filter(|&&(_, parent)| parent == id)
            .map(|&(dialog, _)| dialog)
            .collect();
        self.open
            .retain(|&(dialog, parent)| dialog != id && parent != id);
        children
    }
}

/// The user data of `xdg_wm_dialog_v1` and `xdg_dialog_v1`, which send no events.
struct DialogData;

impl Dispatch2<XdgWmDialogV1, App> for DialogData {
    fn event(
        &self,
        _: &mut App,
        _: &XdgWmDialogV1,
        _: xdg_wm_dialog_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

impl Dispatch2<XdgDialogV1, App> for DialogData {
    fn event(
        &self,
        _: &mut App,
        _: &XdgDialogV1,
        _: xdg_dialog_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Event, Key, Kind, RootRole, Ui, WindowButton};
    use crate::Theme;

    #[test]
    fn a_dialog_closes_at_escape_and_is_not_minimized() {
        let mut ui = Ui::new("A dialog", Theme::default(), false);
        ui.set_dialog();
        assert_eq!(ui.root_role, RootRole::Dialog);
        let escape = ui.key(Key::Escape);
        assert_eq!(escape, Some(Event::Window(WindowButton::Close)));
        ui.add_title_bar();
        let buttons: Vec<WindowButton> = ui
            .widgets
            .iter()
            .filter_map(|w| match w.kind {
                Kind::WindowButton(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(buttons, [WindowButton::Close]);
        let mut window = Ui::new("A window", Theme::default(), false);
        assert_eq!(window.key(Key::Escape), Some(Event::Escape));
        let mut alert = Ui::new("Replace?", Theme::default(), false);
        alert.root_role = RootRole::AlertDialog;
        alert.set_dialog();
        assert_eq!(alert.root_role, RootRole::AlertDialog);
    }

    #[test]
    fn a_dialog_blocks_its_parent() {
        let (window, dialog, other) = (SurfaceId(0), SurfaceId(1), SurfaceId(2));
        let mut dialogs = Dialogs::default();
        assert!(!dialogs.blocked(window));
        dialogs.opened(dialog, window);
        assert!(dialogs.blocked(window));
        assert!(!dialogs.blocked(dialog) && !dialogs.blocked(other));
        assert!(dialogs.is_dialog(dialog) && !dialogs.is_dialog(window));
        // A dialog of the dialog blocks the dialog, and its parent stays blocked.
        let nested = SurfaceId(3);
        dialogs.opened(nested, dialog);
        assert!(dialogs.blocked(dialog) && dialogs.blocked(window));
        assert!(dialogs.closed(nested).is_empty());
        assert!(!dialogs.blocked(dialog));
        assert!(dialogs.closed(dialog).is_empty());
        assert!(!dialogs.blocked(window) && !dialogs.is_dialog(dialog));
    }

    #[test]
    fn closing_the_parent_closes_its_dialogs() {
        let (window, dialog) = (SurfaceId(0), SurfaceId(1));
        let mut dialogs = Dialogs::default();
        dialogs.opened(dialog, window);
        assert_eq!(dialogs.closed(window), vec![dialog]);
        assert!(!dialogs.is_dialog(dialog) && !dialogs.blocked(window));
    }
}
