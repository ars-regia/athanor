//! xdg_toplevel_icon_manager_v1 (doc_own_stack.md decision 33, standard): each toplevel's icon,
//! kept in its surface's `ToplevelIcon` and applied with the commit after `set_icon`. It is not
//! forwarded to the shell yet: that comes with `athanor_toplevel_state_v1`.
//!
//! The compositor answers the manager itself. Smithay's icon object, at 62ec78d, posts
//! `immutable` for a request on an icon already set and then goes on into a debug assertion,
//! which ends a debug build. Each icon here wraps Smithay's: a request on an icon already set
//! is refused and goes no further; any other goes on to Smithay's, which checks the buffer and
//! raises `no_buffer` when it is destroyed before the icon. A buffer at a scale under 1, or one
//! past `MAX_ICON_BUFFERS` on the same icon, is refused with `invalid_buffer` before Smithay's
//! sees it: Smithay keeps every buffer it is given, with a destruction hook on each, and scans
//! them all at each one added.

use crate::state::State;
use smithay::reexports::wayland_protocols::xdg::toplevel_icon::v1::server::{
    xdg_toplevel_icon_manager_v1::{self, XdgToplevelIconManagerV1},
    xdg_toplevel_icon_v1::{self, XdgToplevelIconV1},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::compositor::{with_states, Cacheable};
use smithay::wayland::shm::with_buffer_contents;
use smithay::wayland::xdg_toplevel_icon::XdgToplevelIconUserData;
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// How many buffers one icon may be given. An icon has a buffer per scale and size the client
/// draws it at, a handful.
pub const MAX_ICON_BUFFERS: usize = 16;

/// The user data of the manager's global and of every manager.
pub struct IconManagers;

/// A toplevel's icon, double-buffered: the name and the buffers of the icon its last applied
/// `set_icon` gave, each buffer with its scale and its edge in pixels.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ToplevelIcon {
    pub name: Option<String>,
    pub buffers: Vec<(WlBuffer, i32, i32)>,
}

impl Cacheable for ToplevelIcon {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// The user data of an icon: Smithay's, which checks each request, and what the icon holds,
/// until `set_icon` makes it immutable.
#[derive(Default)]
pub struct Icon {
    checks: XdgToplevelIconUserData,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    set: bool,
    added: usize,
    icon: ToplevelIcon,
}

impl Icon {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Offers the manager to every client. It names no preferred size.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, XdgToplevelIconManagerV1, _>(1, IconManagers);
}

impl GlobalDispatch2<XdgToplevelIconManagerV1, State> for IconManagers {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<XdgToplevelIconManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, IconManagers).done();
    }
}

impl Dispatch2<XdgToplevelIconManagerV1, State> for IconManagers {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _manager: &XdgToplevelIconManagerV1,
        request: xdg_toplevel_icon_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use xdg_toplevel_icon_manager_v1::Request;
        match request {
            Request::CreateIcon { id } => {
                data_init.init(id, Icon::default());
            }
            Request::SetIcon { toplevel, icon } => {
                // An icon the compositor did not make cannot be: its user data is its own.
                let icon = icon
                    .as_ref()
                    .and_then(|icon| icon.data::<Icon>())
                    .map(|icon| {
                        let mut held = icon.held();
                        held.set = true;
                        held.icon.clone()
                    })
                    .unwrap_or_default();
                if let Some(toplevel) = state.xdg_shell_state.get_toplevel(&toplevel) {
                    with_states(toplevel.wl_surface(), |states| {
                        *states.cached_state.get::<ToplevelIcon>().pending() = icon;
                    });
                }
            }
            Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch2<XdgToplevelIconV1, State> for Icon {
    fn request(
        &self,
        state: &mut State,
        client: &Client,
        resource: &XdgToplevelIconV1,
        request: xdg_toplevel_icon_v1::Request,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use xdg_toplevel_icon_v1::Request;
        {
            let mut held = self.held();
            let changes = matches!(request, Request::SetName { .. } | Request::AddBuffer { .. });
            if changes && held.set {
                resource.post_error(
                    xdg_toplevel_icon_v1::Error::Immutable,
                    "the icon was set on a toplevel and cannot change",
                );
                return;
            }
            if let Request::AddBuffer { scale, .. } = &request {
                if *scale < 1 || held.added >= MAX_ICON_BUFFERS {
                    resource.post_error(
                        xdg_toplevel_icon_v1::Error::InvalidBuffer,
                        format!(
                            "a buffer needs a scale of 1 or more, and an icon takes at most \
                             {MAX_ICON_BUFFERS}"
                        ),
                    );
                    return;
                }
                held.added += 1;
            }
            match &request {
                Request::SetName { icon_name } => held.icon.name = Some(icon_name.clone()),
                Request::AddBuffer { buffer, scale } => {
                    // A buffer that is not a square of wl_shm is Smithay's to refuse.
                    let edge = with_buffer_contents(buffer, |_, _, data| {
                        (data.width == data.height).then_some(data.width)
                    });
                    if let Ok(Some(edge)) = edge {
                        let buffers = &mut held.icon.buffers;
                        buffers.retain(|(_, kept_scale, kept_edge)| {
                            (*kept_scale, *kept_edge) != (*scale, edge)
                        });
                        buffers.push((buffer.clone(), *scale, edge));
                    }
                }
                _ => {}
            }
        }
        Dispatch2::<XdgToplevelIconV1, State>::request(
            &self.checks,
            state,
            client,
            resource,
            request,
            dh,
            data_init,
        );
    }

    fn destroyed(&self, state: &mut State, client: ClientId, resource: &XdgToplevelIconV1) {
        Dispatch2::<XdgToplevelIconV1, State>::destroyed(&self.checks, state, client, resource);
    }
}
