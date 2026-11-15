//! xdg_toplevel_tag_manager_v1 (doc_own_stack.md decision 36, standard): each toplevel's tag and
//! description, kept in its surface's `ToplevelTag` and applied with the commit after the
//! request.
//!
//! A tag is the client's own word for one of its windows. It grants nothing: no privilege, no
//! class, no line of the allow-list, no border and no trusted-prompt mark reads it, since all of
//! them are decided on what the compositor read of the client at connect (`identity.rs`).
//! Smithay also keeps the latest tag at once in `XdgToplevelTagSurfaceData`, outside the commit;
//! nothing here reads that copy.

use crate::state::State;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::XdgToplevel;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::compositor::{with_states, Cacheable};
use smithay::wayland::xdg_toplevel_tag::{XdgToplevelTagHandler, XdgToplevelTagManager};

/// A toplevel's tag and description, double-buffered: those its last applied requests set.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ToplevelTag {
    pub tag: Option<String>,
    pub description: Option<String>,
}

impl Cacheable for ToplevelTag {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// Offers the manager to every client.
pub fn global(dh: &DisplayHandle) {
    XdgToplevelTagManager::new::<State>(dh);
}

impl State {
    /// Changes the pending tag of `toplevel`, which its surface's next commit applies.
    fn pending_tag(&self, toplevel: &XdgToplevel, change: impl FnOnce(&mut ToplevelTag)) {
        if let Some(toplevel) = self.xdg_shell_state.get_toplevel(toplevel) {
            with_states(toplevel.wl_surface(), |states| {
                change(states.cached_state.get::<ToplevelTag>().pending());
            });
        }
    }
}

impl XdgToplevelTagHandler for State {
    fn set_tag(&mut self, toplevel: XdgToplevel, tag: String) {
        self.pending_tag(&toplevel, |pending| pending.tag = Some(tag));
    }

    fn set_description(&mut self, toplevel: XdgToplevel, description: String) {
        self.pending_tag(&toplevel, |pending| pending.description = Some(description));
    }
}
