//! Decision 36 of `doc_own_stack.md`: a surface's content type and a toplevel's tag are kept,
//! applied with the commit, and grant nothing.

use super::marks::{at, clear_the_fixture, confined, frame};
use super::*;
use crate::identity::{self, Privilege};
use crate::state::{granted, unconfined};
use crate::toplevel_tag::ToplevelTag;
use smithay::wayland::compositor::with_states;
use smithay::wayland::content_type::ContentTypeSurfaceCachedState;
use wayland_protocols::wp::content_type::v1::client::{
    wp_content_type_manager_v1, wp_content_type_v1,
};
use wayland_protocols::wp::content_type::v1::server::wp_content_type_v1::Type;
use wayland_protocols::xdg::toplevel_tag::v1::client::xdg_toplevel_tag_manager_v1;

delegate_noop!(Client: ignore wp_content_type_manager_v1::WpContentTypeManagerV1);
delegate_noop!(Client: wp_content_type_v1::WpContentTypeV1);
delegate_noop!(Client: ignore xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1);

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

/// The tag the compositor holds for the client's window, as of its last commit.
fn tag_of(run: &Running, local: &Local) -> ToplevelTag {
    let window = window_of(run, local.id.as_ref().expect("an id"));
    let toplevel = window.toplevel().expect("a toplevel");
    with_states(toplevel.wl_surface(), |states| {
        states.cached_state.get::<ToplevelTag>().current().clone()
    })
}

/// A confined client sees the tag manager, and the tag and description it sets take effect with
/// the window's next commit, not before.
#[test]
fn a_toplevel_tag_is_applied_with_the_commit() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    assert!(local.sees("xdg_toplevel_tag_manager_v1"));
    let toplevel = local.toplevel(&mut run);
    let manager: xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1 = local.bind(1);
    manager.set_toplevel_tag(&toplevel, "e-mail composer".into());
    manager.set_toplevel_description(&toplevel, "A new message".into());
    local.roundtrip(&mut run);
    assert_eq!(tag_of(&run, &local), ToplevelTag::default());
    let (surface, _) = local.window.clone().expect("a window");
    surface.commit();
    local.roundtrip(&mut run);
    let expected = ToplevelTag {
        tag: Some("e-mail composer".into()),
        description: Some("A new message".into()),
    };
    assert_eq!(tag_of(&run, &local), expected);
}

/// A confined window tagged with a privileged client's executable and unit, and described as a
/// trusted prompt, gains nothing: the client is still confined, granted nothing, sees no
/// privileged global on a new registry, and the window keeps the confinement border of OS4.
#[test]
fn a_tag_naming_a_privileged_client_grants_nothing() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel) = confined(&mut run);
    let manager: xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1 = local.bind(1);
    manager.set_toplevel_tag(&toplevel, "/usr/bin/athanor-bar athanor-bar.service".into());
    manager.set_toplevel_description(&toplevel, identity::MARK.into());
    let (surface, _) = local.window.clone().expect("a window");
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(
        tag_of(&run, &local).tag.as_deref(),
        Some("/usr/bin/athanor-bar athanor-bar.service")
    );

    let window = window_of(&run, local.id.as_ref().expect("an id"));
    let toplevel_surface = window.toplevel().expect("a toplevel");
    let client = toplevel_surface.wl_surface().client().expect("the client");
    assert!(!unconfined(&client));
    assert!(!granted(&client, Privilege::TrustedPrompt));
    assert!(!granted(&client, Privilege::TopLayers));
    local.client.globals.clear();
    let qh = local.queue.handle();
    let _registry = local.connection.display().get_registry(&qh, ());
    local.roundtrip(&mut run);
    assert!(local.sees("xdg_toplevel_tag_manager_v1"));
    assert!(!local.sees("wp_security_context_manager_v1"));
    for privileged in identity::names() {
        assert!(!local.sees(privileged), "{privileged} is listed");
    }

    // The border's white line inside, and black outside, at the window's left edge.
    let (x, y) = (400, 300);
    let middle = y + HEIGHT / 2;
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x - 2, middle), [255, 255, 255, 255]);
    assert_eq!(at(&shown, x - 3, middle), [0, 0, 0, 255]);
}
