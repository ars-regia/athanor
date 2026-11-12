//! Decision 33, wave 2a: the standard globals. The alpha modifier's test is with the marks it
//! must leave opaque (`marks.rs`).

use super::*;
use smithay::reexports::wayland_protocols_misc::server_decoration::server::__interfaces::{
    ORG_KDE_KWIN_SERVER_DECORATION_INTERFACE, ORG_KDE_KWIN_SERVER_DECORATION_MANAGER_INTERFACE,
};
use smithay::wayland::compositor::with_states;
use smithay::wayland::xdg_toplevel_icon::ToplevelIconCachedState;
use wayland_client::protocol::wl_fixes;
use wayland_protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1,
};

delegate_noop!(Client: wl_fixes::WlFixes);
delegate_noop!(Client: ignore xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1);
delegate_noop!(Client: xdg_toplevel_icon_v1::XdgToplevelIconV1);

/// wl_fixes destroys a registry in the compositor, for a confined client as for any other.
#[test]
fn a_registry_destroyed_through_wl_fixes_is_gone_from_the_compositor() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    let fixes: wl_fixes::WlFixes = local.bind(1);
    let registry = local
        .connection
        .display()
        .get_registry(&local.queue.handle(), ());
    local.roundtrip(&mut run);
    let id = local.id.clone().expect("an id");
    let protocol_id = registry.id().protocol_id();
    let known = |run: &Running| {
        run.state
            .display_handle
            .backend_handle()
            .object_for_protocol_id(
                id.clone(),
                wl_registry::WlRegistry::interface(),
                protocol_id,
            )
            .is_ok()
    };
    assert!(known(&run), "the second registry is the compositor's");
    fixes.destroy_registry(&registry);
    local.roundtrip(&mut run);
    assert!(!known(&run), "the registry is destroyed");
    run.stop();
}

/// KDE's server decoration is client-side whatever the client asks: the default mode at bind,
/// the mode of a new decoration and the answer to every request, a server-side one included.
#[test]
fn a_kde_decoration_is_client_side_whatever_is_asked() {
    const CLIENT: &str = "1";
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let manager = Raw::bind(
        &mut local,
        &ORG_KDE_KWIN_SERVER_DECORATION_MANAGER_INTERFACE,
    );
    let (surface, _) = local.window.clone().expect("a window");
    let decoration = manager.create(
        "create",
        vec![
            Argument::NewId(ObjectId::null()),
            Argument::Object(surface.id()),
        ],
        &ORG_KDE_KWIN_SERVER_DECORATION_INTERFACE,
    );
    // Server, none, client and server again.
    let asked = [2, 0, 1, 2];
    for mode in asked {
        decoration.send("request_mode", vec![Argument::Uint(mode)]);
    }
    local.roundtrip(&mut run);
    assert_eq!(
        manager.events(),
        [("default_mode", vec![CLIENT.to_string()])]
    );
    assert_eq!(
        decoration.events(),
        vec![("mode", vec![CLIENT.to_string()]); asked.len() + 1]
    );
    run.stop();
}

/// The name and the scales of the buffers of the icon the window of client `id` shows now.
fn icon_of(run: &Running, id: &ClientId) -> (Option<String>, Vec<i32>) {
    let window = window_of(run, id);
    let toplevel = window.toplevel().expect("a toplevel");
    with_states(toplevel.wl_surface(), |states| {
        let mut icon = states.cached_state.get::<ToplevelIconCachedState>();
        let icon = icon.current();
        (
            icon.icon_name().map(str::to_owned),
            icon.buffers().iter().map(|(_, scale)| *scale).collect(),
        )
    })
}

/// Gives `toplevel` a new icon named `name` with a square buffer at `scale`; the buffer is
/// returned, as it must outlive the icon.
fn set_icon(
    local: &mut Local,
    manager: &xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1,
    toplevel: &xdg_toplevel::XdgToplevel,
    name: &str,
    scale: i32,
) -> (wl_buffer::WlBuffer, xdg_toplevel_icon_v1::XdgToplevelIconV1) {
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _) = local.buffer(&shm, 16, 16, [0, 0, 255, 255]);
    let icon = manager.create_icon(&local.queue.handle(), ());
    icon.set_name(name.into());
    icon.add_buffer(&buffer, scale);
    manager.set_icon(toplevel, Some(&icon));
    (buffer, icon)
}

/// A toplevel's icon is applied with the commit after `set_icon`, outlives its icon object, is
/// replaced by a later icon and cleared by none, and each toplevel keeps its own.
#[test]
fn a_toplevel_keeps_its_icon_until_it_is_replaced_or_cleared() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let manager: xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1 = local.bind(1);

    let first = set_icon(&mut local, &manager, &toplevel, "org.example.First", 1);
    local.roundtrip(&mut run);
    assert_eq!(icon_of(&run, &id), (None, vec![]), "before the commit");
    surface.commit();
    first.1.destroy();
    local.roundtrip(&mut run);
    let named = |name: &str, scale| (Some(name.to_string()), vec![scale]);
    assert_eq!(
        icon_of(&run, &id),
        named("org.example.First", 1),
        "applied and kept"
    );

    let _second = set_icon(&mut local, &manager, &toplevel, "org.example.Second", 2);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(
        icon_of(&run, &id),
        named("org.example.Second", 2),
        "replaced"
    );

    let mut other = Local::connect(&mut run, None);
    let other_toplevel = other.toplevel(&mut run);
    let other_id = other.id.clone().expect("an id");
    let (other_surface, _) = other.window.clone().expect("a window");
    let other_manager: xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1 = other.bind(1);
    let _third = set_icon(
        &mut other,
        &other_manager,
        &other_toplevel,
        "org.example.Other",
        1,
    );
    other_surface.commit();
    other.roundtrip(&mut run);
    assert_eq!(icon_of(&run, &other_id), named("org.example.Other", 1));
    assert_eq!(
        icon_of(&run, &id),
        named("org.example.Second", 2),
        "its own"
    );

    manager.set_icon(&toplevel, None);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(icon_of(&run, &id), (None, vec![]), "cleared");
    assert_eq!(
        icon_of(&run, &other_id),
        named("org.example.Other", 1),
        "the other's kept"
    );
    run.stop();
}
