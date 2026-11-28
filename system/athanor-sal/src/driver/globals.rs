//! A global the driver does not bind, bound for the application on the driver's connection
//! ([`super::Shell::bind_global`]): the one connection, and the one identity the compositor gave
//! it, serve a protocol the toolkit does not speak.

use std::ops::RangeInclusive;
use wayland_client::globals::{BindError, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Dispatch, Proxy, QueueHandle};

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
