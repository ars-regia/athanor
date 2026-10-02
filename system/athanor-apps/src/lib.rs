//! The applications row the bar (in `bar`) and the dock share (doc_bar.md, BR3, BR7): the
//! favourites and the running windows grouped by application, minimised windows included,
//! pinning and unpinning, launching behind a security context (BR2), and the opener
//! buttons. Generic over `Host`, which each program implements for its own state.

pub mod favorites;
pub mod i18n;
pub mod menu;
pub mod model;
pub mod openers;
pub mod row;

use std::rc::Rc;

use athanor_compositor_client::Client;

/// What the row needs from the program that shows it.
pub trait Host: 'static {
    /// The program's name: the application name of its notifications.
    const APP: &'static str;
    /// Whether dragging a pinned entry onto another reorders the favourites (the dock).
    const REORDER: bool;
    /// `None` when the privileged globals are absent: no row and no opener (SH1).
    fn client(&self) -> Option<&Client>;
    fn favorites(&self) -> &favorites::Store;
    /// The label of the pin row: to unpin when `pinned`, to pin otherwise.
    fn pin_label(&self, pinned: bool) -> String;
    /// A menu of a row is about to pop up: the host closes any other popover (BR6) and
    /// redraws whatever it stacks against its popovers.
    fn menu_opened(&self, menu: &gtk4::Popover);
    /// The favourites or the installed applications changed: every row refreshes.
    fn refresh_rows(self: &Rc<Self>);
    /// A menu or a drag started (`true`) or ended (`false`) on a row: an auto-hiding
    /// host stays shown meanwhile, and a host that stacks surfaces against its popovers
    /// redraws them on `false`.
    fn hold(&self, _held: bool) {}
}
