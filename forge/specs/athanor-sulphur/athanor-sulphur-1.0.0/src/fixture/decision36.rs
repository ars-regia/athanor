//! Decision 36 of `doc_own_stack.md`: a surface's content type and a toplevel's tag are kept,
//! applied with the commit, and grant nothing; a lock request ends every DRM lease and refuses
//! new ones until the unlock, `locked` waits until every lease has ended, and the lease device
//! is hidden from a confined client.

use super::marks::{at, clear_the_fixture, confined, frame};
use super::*;
use crate::drm_lease::{Held, Lease, Probe};
use crate::identity::{self, Privilege};
use crate::state::{granted, unconfined};
use crate::toplevel_tag::ToplevelTag;
use smithay::backend::drm::DrmNode;
use smithay::wayland::compositor::with_states;
use smithay::wayland::content_type::ContentTypeSurfaceCachedState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

/// A probe lease of the fixture's state, and the flag its revocation sets.
fn probe_lease(run: &mut Running, id: u32) -> Arc<AtomicBool> {
    let revoked = Arc::new(AtomicBool::new(false));
    run.state.lease_granted(Held {
        device: 1,
        id,
        connectors: Vec::new(),
        crtcs: Vec::new(),
        lease: Lease::Probe(Probe(revoked.clone())),
    });
    revoked
}

/// Why the headless backend fails to end a lease, as a GPU whose master the compositor is
/// not; `None` lets every lease end.
fn refuse_leases(run: &mut Running, cause: Option<&str>) {
    headless(run).lease_refused = cause.map(str::to_owned);
}

/// The fixture's headless backend.
fn headless(run: &mut Running) -> &mut crate::headless::Headless {
    let crate::state::Backend::Headless(headless) = &mut run.state.backend else {
        panic!("the fixture runs headless");
    };
    headless
}

/// Requests a lock, and runs until the compositor holds it.
fn request_lock(run: &mut Running, locker: &mut Local) -> ext_session_lock_v1::ExtSessionLockV1 {
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let lock = locks.lock(&locker.queue.handle(), ());
    locker.pump(run, "the lock is requested", |_, state| {
        state.lock.is_some()
    });
    lock
}

/// Runs for `LEASE_RETRY` and a half, frames and one retry of the leases included, and checks
/// that `locked` was not sent.
fn locked_withheld(run: &mut Running, locker: &mut Local, why: &str) {
    let started = Instant::now();
    locker.pump(run, why, |_, _| {
        started.elapsed() > Duration::from_millis(1500)
    });
    assert!(!locker.client.locked, "{why}: locked withheld");
}

/// Through the real `ext_session_lock_v1`: the lock request ends the lease held and refuses
/// new ones; one granted before `locked` ends at once; after the unlock a lease may be granted
/// and is kept.
#[test]
fn a_lock_request_ends_every_lease_until_the_unlock() {
    let mut run = Running::start(None);
    let held = probe_lease(&mut run, 7);
    assert!(run.state.may_lease());
    let mut locker = Local::connect(&mut run, None);
    let lock = request_lock(&mut run, &mut locker);
    assert!(held.load(Ordering::SeqCst), "ended at the lock request");
    assert!(!run.state.may_lease(), "refused from the lock request");
    let late = probe_lease(&mut run, 8);
    assert!(
        late.load(Ordering::SeqCst),
        "a lease granted while locked ends at once"
    );
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    assert!(!run.state.may_lease(), "refused while locked");
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    assert!(run.state.lock.is_none());
    assert!(run.state.may_lease(), "granted again after the unlock");
    let after = probe_lease(&mut run, 9);
    assert!(
        !after.load(Ordering::SeqCst),
        "a lease after the unlock is kept"
    );
    run.stop();
}

/// While a lease's revocation fails, `locked` waits, though the outputs show the lock, and no
/// lease is granted, the unlock past; once a second the lease is tried again, and `locked` is
/// sent once it ends.
#[test]
fn locked_waits_for_a_lease_whose_revocation_fails() {
    let mut run = Running::start(None);
    let held = probe_lease(&mut run, 7);
    refuse_leases(&mut run, Some("EACCES"));
    let mut locker = Local::connect(&mut run, None);
    let _lock = request_lock(&mut run, &mut locker);
    assert!(
        held.load(Ordering::SeqCst),
        "its client is told at the lock request"
    );
    locked_withheld(&mut run, &mut locker, "the revocation fails");
    refuse_leases(&mut run, None);
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    run.stop();
}

/// `locked` waits for every output to show the lock now: an output kept from being drawn, as
/// one plugged in while a revocation failed, holds it back after the leases have ended, though
/// every output had shown the lock before.
#[test]
fn locked_waits_for_an_output_that_has_not_shown_the_lock() {
    let mut run = Running::start(None);
    let _held = probe_lease(&mut run, 7);
    refuse_leases(&mut run, Some("EACCES"));
    let mut locker = Local::connect(&mut run, None);
    let _lock = request_lock(&mut run, &mut locker);
    locked_withheld(&mut run, &mut locker, "the revocation fails");
    let second = output_at(&run, 1);
    headless(&mut run).undrawn = Some(second);
    refuse_leases(&mut run, None);
    locked_withheld(&mut run, &mut locker, "an output has not shown the lock");
    headless(&mut run).undrawn = None;
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    run.stop();
}

/// Through the functions the udev backend's pause and resume call: while the session is paused
/// the device is not touched, the lease is not even dropped, and `locked` waits; the resume ends
/// the lease at once, and `locked` is sent.
#[test]
fn locked_waits_for_the_resume_of_a_paused_session() {
    let mut run = Running::start(None);
    let held = probe_lease(&mut run, 7);
    run.state.pause_leases();
    assert!(!run.state.may_lease(), "refused while paused");
    let mut locker = Local::connect(&mut run, None);
    let _lock = request_lock(&mut run, &mut locker);
    locked_withheld(&mut run, &mut locker, "the session is paused");
    assert!(
        !held.load(Ordering::SeqCst),
        "the device is not touched while paused"
    );
    run.state.resume_leases();
    assert!(held.load(Ordering::SeqCst), "ended at the resume");
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    run.stop();
}

/// A lease held through a pause, with no lock, ends at the resume, before any output is drawn,
/// and grants are made again.
#[test]
fn a_resume_ends_the_leases_held_through_a_pause() {
    let mut run = Running::start(None);
    let held = probe_lease(&mut run, 7);
    run.state.pause_leases();
    assert!(!held.load(Ordering::SeqCst), "kept while paused");
    run.state.resume_leases();
    assert!(held.load(Ordering::SeqCst), "ended at the resume");
    assert!(run.state.may_lease(), "granted again after the resume");
    run.stop();
}

/// Without a render node: the lease device's row in `data/globals` is `unconfined`, and the
/// filter its global is made with lists it to a client outside a security context only.
#[test]
fn the_lease_device_is_unconfined() {
    assert!(
        classified().contains(&("wp_drm_lease_device_v1", Class::Unconfined)),
        "its row in data/globals"
    );
    let mut run = Running::start(None);
    let mut open = Local::connect(&mut run, None);
    open.toplevel(&mut run);
    let (inside, _) = confined(&mut run);
    let client = |local: &Local| {
        window_of(&run, local.id.as_ref().expect("an id"))
            .toplevel()
            .expect("a toplevel")
            .wl_surface()
            .client()
            .expect("the client")
    };
    assert!(
        crate::drm_lease::shown_to(&client(&open)),
        "outside a context"
    );
    assert!(
        !crate::drm_lease::shown_to(&client(&inside)),
        "inside a context"
    );
    run.stop();
}

/// The lease device of the GPU `ATHANOR_RENDER_NODE` names is listed to a client outside a
/// security context and hidden from one inside it. Nothing binds it: the fixture has no udev
/// backend to lease from. `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn the_lease_device_is_hidden_from_a_confined_client() {
    let node = DrmNode::from_path(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    )
    .expect("a DRM node");
    let mut run = Running::start(None);
    let _leasing = crate::drm_lease::global(&run.state.display_handle, &node)
        .expect("the GPU's primary node opens");
    let unconfined = Local::connect(&mut run, None);
    assert!(
        unconfined.sees("wp_drm_lease_device_v1"),
        "outside a context"
    );
    let context = sandbox(&run);
    let confined = Local::connect(&mut run, Some(context));
    assert!(!confined.sees("wp_drm_lease_device_v1"), "inside a context");
    assert!(confined.sees("wp_content_type_manager_v1"));
    run.stop();
}
