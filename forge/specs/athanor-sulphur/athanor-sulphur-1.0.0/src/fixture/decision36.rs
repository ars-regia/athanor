//! Decision 36 of `doc_own_stack.md`: a surface's content type and a toplevel's tag are kept,
//! applied with the commit, and grant nothing.

use super::*;
use smithay::wayland::compositor::with_states;
use smithay::wayland::content_type::ContentTypeSurfaceCachedState;
use wayland_protocols::wp::content_type::v1::client::{
    wp_content_type_manager_v1, wp_content_type_v1,
};
use wayland_protocols::wp::content_type::v1::server::wp_content_type_v1::Type;

delegate_noop!(Client: ignore wp_content_type_manager_v1::WpContentTypeManagerV1);
delegate_noop!(Client: wp_content_type_v1::WpContentTypeV1);

/// The content type the compositor holds for the client's window, as of its last commit.
fn content_type_of(run: &Running, local: &Local) -> Type {
    let window = window_of(run, local.id.as_ref().expect("an id"));
    let toplevel = window.toplevel().expect("a toplevel");
    with_states(toplevel.wl_surface(), |states| {
        *states
            .cached_state
            .get::<ContentTypeSurfaceCachedState>()
            .current()
            .content_type()
    })
}

/// A confined client sees the content type manager, and the type it sets takes effect with the
/// surface's next commit, not before.
#[test]
fn a_content_type_is_applied_with_the_commit() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    assert!(local.sees("wp_content_type_manager_v1"));
    local.toplevel(&mut run);
    let manager: wp_content_type_manager_v1::WpContentTypeManagerV1 = local.bind(1);
    let (surface, _) = local.window.clone().expect("a window");
    let content_type = manager.get_surface_content_type(&surface, &local.queue.handle(), ());
    content_type.set_content_type(wp_content_type_v1::Type::Video);
    local.roundtrip(&mut run);
    assert_eq!(content_type_of(&run, &local), Type::None);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(content_type_of(&run, &local), Type::Video);
}
