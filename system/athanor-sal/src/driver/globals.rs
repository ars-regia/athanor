//! A global the driver does not bind, bound for the application on the driver's connection
//! ([`super::Shell::bind_global`]): the one connection, and the one identity the compositor gave
//! it, serve a protocol the toolkit does not speak.

use std::ops::RangeInclusive;
use wayland_client::globals::{BindError, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Dispatch, EventQueue, Proxy, QueueHandle};

use super::Shell;

impl Shell {
    /// A queue of the application's own on the driver's connection, for a global bound through
    /// [`Shell::bind_global`] and the objects made from it, whose events go to the application's
    /// own `Dispatch` implementations on `D`. The driver reads and flushes the connection; the
    /// application calls [`EventQueue::dispatch_pending`] in [`super::Application::turn`], which
    /// follows each read, and never `roundtrip` nor `blocking_dispatch`, which would read the
    /// connection behind the driver's back.
    // ponytail: with the `gpu` feature wgpu may read the connection outside the loop, and an
    // event it reads for this queue waits for the next turn; no gpu client binds a global today.
    pub fn event_queue<D>(&self) -> EventQueue<D> {
        self.conn.new_event_queue()
    }

    /// Binds a global the driver does not, found by the name of `I`'s interface, at the highest
    /// version of `version` the compositor offers, on a `queue` of [`Shell::event_queue`]: a
    /// protocol of the application's own, on the driver's one connection, which a compositor
    /// that admits clients by their connection, as athanor-sulphur does, admits as it admits the
    /// driver. A global the compositor does not announce to this client is
    /// [`BindError::NotPresent`]; one below `version`, or a `version` past what `I`'s description
    /// knows, is [`BindError::UnsupportedVersion`]. Of a global announced more than once, the
    /// first.
    pub fn bind_global<I, D, U>(
        &self,
        queue: &QueueHandle<D>,
        version: RangeInclusive<u32>,
        data: U,
    ) -> Result<I, BindError>
    where
        I: Proxy + 'static,
        D: Dispatch<I, U> + 'static,
        U: Send + Sync + 'static,
    {
        bind(&self.registry, queue, version, data)
    }
}

/// Binds the global of interface `I` that `registry` announced, at the highest version of
/// `version` it offers, on `queue`. Refused, never a panic: a global not announced is
/// [`BindError::NotPresent`]; one announced below `version`, an empty `version`, or one that asks
/// for more than `I`'s description knows, is [`BindError::UnsupportedVersion`].
pub(super) fn bind<I, D, U>(
    registry: &WlRegistry,
    queue: &QueueHandle<D>,
    version: RangeInclusive<u32>,
    data: U,
) -> Result<I, BindError>
where
    I: Proxy + 'static,
    D: Dispatch<I, U> + 'static,
    U: Send + Sync + 'static,
{
    let interface = I::interface();
    // wayland-client's GlobalList::bind panics on the last; an application gets an error.
    if version.is_empty() || *version.end() > interface.version {
        return Err(BindError::UnsupportedVersion);
    }
    let (name, offered) = registry
        .data::<GlobalListContents>()
        .and_then(|globals| {
            globals.with_list(|list| {
                list.iter()
                    .find(|global| global.interface == interface.name)
                    .map(|global| (global.name, global.version))
            })
        })
        .ok_or(BindError::NotPresent)?;
    if offered < *version.start() {
        return Err(BindError::UnsupportedVersion);
    }
    Ok(registry.bind(name, offered.min(*version.end()), queue, data))
}
