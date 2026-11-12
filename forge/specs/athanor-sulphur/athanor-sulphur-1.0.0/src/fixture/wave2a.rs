//! Decision 33, wave 2a: the standard globals. The alpha modifier's test is with the marks it
//! must leave opaque (`marks.rs`).

use super::*;
use wayland_client::protocol::wl_fixes;

delegate_noop!(Client: wl_fixes::WlFixes);

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
