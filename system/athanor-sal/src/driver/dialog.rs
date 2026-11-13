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

/// What the compositor's request to close a window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloseRequest {
    /// Nothing: a dialog open on the window keeps it from closing, as GTK ignores a delete
    /// event on any window but the root of its group's grab, which a shown modal window holds
    /// (gtk/gtkmain.c, GDK_DELETE).
    Ignored,
    /// The dialog's application is asked, as at its Close button.
    Dialog,
    /// The run ends.
    Exit,
}

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

    /// What the compositor's request to close the window `id` does.
    pub(super) fn close_request(&self, id: SurfaceId) -> CloseRequest {
        if self.blocked(id) {
            CloseRequest::Ignored
        } else if self.is_dialog(id) {
            CloseRequest::Dialog
        } else {
            CloseRequest::Exit
        }
    }

    /// `id` and the dialogs open on it, and theirs, each dialog before the surface it was opened
    /// on: the order in which they close when `id` is lost.
    pub(super) fn closing_order(&self, id: SurfaceId) -> Vec<SurfaceId> {
        let mut order: Vec<SurfaceId> = self
            .open
            .iter()
            .filter(|&&(_, parent)| parent == id)
            .flat_map(|&(dialog, _)| self.closing_order(dialog))
            .collect();
        order.push(id);
        order
    }

    /// Forgets `id`, a dialog or a parent, and the dialogs open on it, and theirs, whether or not
    /// their surfaces are still open; returns those of them `open` holds, each once, each dialog
    /// before the surface it was opened on: the order in which they close.
    pub(super) fn close(&mut self, id: SurfaceId, open: &[SurfaceId]) -> Vec<SurfaceId> {
        let order = self.closing_order(id);
        for &surface in &order {
            self.open
                .retain(|&(dialog, parent)| dialog != surface && parent != surface);
        }
        order.into_iter().filter(|s| open.contains(s)).collect()
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
        // A field composing with the input method keeps Escape for it.
        let mut form = Ui::new("A form", Theme::default(), false);
        form.set_dialog();
        let field = form.field(Ui::ROOT, "Name", "");
        form.layout();
        form.set_focus(Some(field));
        form.compose("ka", None);
        assert_eq!(form.key(Key::Escape), None);
        form.commit("か");
        assert_eq!(
            form.key(Key::Escape),
            Some(Event::Window(WindowButton::Close))
        );
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
        let all = [window, dialog, other, nested];
        assert_eq!(dialogs.close(nested, &all), [nested]);
        assert!(!dialogs.blocked(dialog));
        assert_eq!(dialogs.close(dialog, &all), [dialog]);
        assert!(!dialogs.blocked(window) && !dialogs.is_dialog(dialog));
    }

    /// Every way a dialog closes leaves its parent taking input again: its own close (the
    /// application's, or its loss), and its parent's.
    #[test]
    fn every_close_unblocks_the_parent() {
        let (window, dialog) = (SurfaceId(0), SurfaceId(1));
        let mut dialogs = Dialogs::default();
        let all = [window, dialog];
        // Shell::close of the dialog, and the loss of the dialog alone, forget the dialog.
        dialogs.opened(dialog, window);
        assert_eq!(dialogs.closing_order(dialog), [dialog]);
        assert_eq!(dialogs.close(dialog, &all), [dialog]);
        assert!(!dialogs.blocked(window));
        // The parent's close or loss closes the dialog with it, first.
        dialogs.opened(dialog, window);
        assert_eq!(dialogs.close(window, &all), [dialog, window]);
        assert!(!dialogs.blocked(window) && !dialogs.is_dialog(dialog));
    }

    #[test]
    fn a_lost_window_closes_its_dialogs_first_innermost_first() {
        let (window, first, nested, second) =
            (SurfaceId(0), SurfaceId(1), SurfaceId(2), SurfaceId(3));
        let other = SurfaceId(4);
        let mut dialogs = Dialogs::default();
        dialogs.opened(first, window);
        dialogs.opened(nested, first);
        dialogs.opened(second, window);
        dialogs.opened(SurfaceId(5), other);
        assert_eq!(
            dialogs.closing_order(window),
            [nested, first, second, window]
        );
        assert_eq!(dialogs.closing_order(first), [nested, first]);
        assert_eq!(dialogs.closing_order(other), [SurfaceId(5), other]);
    }

    /// Guards what Shell::remove closes, in the order Dialogs::close gives: each dialog before
    /// the surface it was opened on, each surface once, and a recorded dialog that is no longer
    /// open skipped and forgotten, with the dialogs under it.
    #[test]
    fn a_window_closes_after_its_dialogs_each_once() {
        let (window, first, nested, second) =
            (SurfaceId(0), SurfaceId(1), SurfaceId(2), SurfaceId(3));
        let under_second = SurfaceId(4);
        let mut dialogs = Dialogs::default();
        dialogs.opened(first, window);
        dialogs.opened(nested, first);
        dialogs.opened(second, window);
        dialogs.opened(under_second, second);
        // `second` closed already, its surface gone, though still recorded; the dialog under it
        // is still open.
        let open = [window, first, nested, under_second];
        assert_eq!(
            dialogs.close(window, &open),
            [nested, first, under_second, window]
        );
        assert!(!dialogs.blocked(window) && !dialogs.blocked(first));
        assert!(!dialogs.is_dialog(first) && !dialogs.is_dialog(nested));
        assert!(
            !dialogs.is_dialog(second) && !dialogs.blocked(second),
            "a dialog not open is forgotten too"
        );
        assert!(!dialogs.is_dialog(under_second), "and the dialogs under it");
        // Closed again, it reports no dialog a second time.
        assert_eq!(dialogs.close(window, &open), [window]);
    }

    /// The compositor's close of a window a dialog blocks is ignored, as GTK's; a dialog's
    /// asks its application, unless a dialog of its own blocks it; any other window's ends the
    /// run.
    #[test]
    fn a_close_request_on_a_blocked_window_is_ignored() {
        let (window, dialog, nested) = (SurfaceId(0), SurfaceId(1), SurfaceId(2));
        let mut dialogs = Dialogs::default();
        assert_eq!(dialogs.close_request(window), CloseRequest::Exit);
        dialogs.opened(dialog, window);
        assert_eq!(dialogs.close_request(window), CloseRequest::Ignored);
        assert_eq!(dialogs.close_request(dialog), CloseRequest::Dialog);
        dialogs.opened(nested, dialog);
        assert_eq!(dialogs.close_request(dialog), CloseRequest::Ignored);
        assert_eq!(dialogs.close_request(nested), CloseRequest::Dialog);
        dialogs.close(dialog, &[window, dialog, nested]);
        assert_eq!(dialogs.close_request(window), CloseRequest::Exit);
    }

    #[test]
    fn closing_the_parent_closes_its_dialogs() {
        let (window, dialog) = (SurfaceId(0), SurfaceId(1));
        let mut dialogs = Dialogs::default();
        dialogs.opened(dialog, window);
        assert_eq!(dialogs.close(window, &[window, dialog]), [dialog, window]);
        assert!(!dialogs.is_dialog(dialog) && !dialogs.blocked(window));
    }
}
