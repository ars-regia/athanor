//! Primary selection and data control (doc_own_stack.md OS17, decisions 23 and 30). Primary
//! selection is standard: the text a user selects pastes with the middle button, and it follows
//! the keyboard focus as the regular selection does (`State::focus_changed`). Data control, in
//! both its protocols, is privileged: a client bound to it reads and sets the clipboard and the
//! primary selection without holding the focus, so only the clients the allow-list grants it
//! see it, never one inside a security context (`granted`).
//!
//! No client is granted data control yet, and none may be before the session lock is handled:
//! Smithay updates the data control devices with no focus check and `new_selection` cannot veto,
//! so a granted client reads and overwrites both selections while the session is locked. A grant
//! first needs a lock-aware mechanism (a lock flag in the filter, and dropping the bound data
//! control clients at the lock) with its test.
//!
//! Smithay serves all three protocols; this module only creates their globals and names the
//! state. Nothing is ported from niri or anvil.

use crate::identity::Privilege;
use crate::state::{granted, State};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::selection::ext_data_control::{
    DataControlHandler as ExtDataControlHandler, DataControlState as ExtDataControlState,
};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState,
};
use smithay::wayland::selection::wlr_data_control::{
    DataControlHandler as WlrDataControlHandler, DataControlState as WlrDataControlState,
};

/// The globals of the selection protocols beyond `wl_data_device_manager`.
pub struct Clipboard {
    primary: PrimarySelectionState,
    ext_data_control: ExtDataControlState,
    wlr_data_control: WlrDataControlState,
}

impl Clipboard {
    pub fn new(dh: &DisplayHandle) -> Self {
        let primary = PrimarySelectionState::new::<State>(dh);
        // The data control globals carry the primary selection too: the clients that bind them
        // are the ones the allow-list grants, and primary selection itself is every client's.
        // `zwlr_data_control_manager_v1` is what wl-clipboard 2.2 and so cliphist speak.
        let ext_data_control = ExtDataControlState::new::<State, _>(dh, Some(&primary), |c| {
            granted(c, Privilege::ExtDataControl)
        });
        let wlr_data_control = WlrDataControlState::new::<State, _>(dh, Some(&primary), |c| {
            granted(c, Privilege::WlrDataControl)
        });
        Self {
            primary,
            ext_data_control,
            wlr_data_control,
        }
    }
}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.clipboard.primary
    }
}

impl ExtDataControlHandler for State {
    fn data_control_state(&mut self) -> &mut ExtDataControlState {
        &mut self.clipboard.ext_data_control
    }
}

impl WlrDataControlHandler for State {
    fn data_control_state(&mut self) -> &mut WlrDataControlState {
        &mut self.clipboard.wlr_data_control
    }
}
