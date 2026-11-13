//! Decision 33, wave 2a: the standard globals. The alpha modifier's test is with the marks it
//! must leave opaque (`marks.rs`).

use super::*;
use smithay::reexports::wayland_protocols_misc::server_decoration::server::__interfaces::{
    ORG_KDE_KWIN_SERVER_DECORATION_INTERFACE, ORG_KDE_KWIN_SERVER_DECORATION_MANAGER_INTERFACE,
};
use smithay::wayland::compositor::{get_children, with_states};
use crate::handlers::{MAX_HELD_COMMITS, MAX_RETAINED_COMMITS};
use crate::pacing::MAX_TIMED_WAIT;
use crate::toplevel_icon::{ToplevelIcon, MAX_ICON_BUFFERS};
use smithay::wayland::fifo::FifoBarrierCachedState;
use wayland_client::protocol::{wl_fixes, wl_subcompositor};
use wayland_protocols::wp::commit_timing::v1::client::{
    wp_commit_timer_v1, wp_commit_timing_manager_v1,
};
use wayland_protocols::wp::fifo::v1::client::{wp_fifo_manager_v1, wp_fifo_v1};
use wayland_protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1,
};

delegate_noop!(Client: wl_fixes::WlFixes);
delegate_noop!(Client: ignore xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1);
delegate_noop!(Client: xdg_toplevel_icon_v1::XdgToplevelIconV1);
delegate_noop!(Client: wp_fifo_manager_v1::WpFifoManagerV1);
delegate_noop!(Client: wp_fifo_v1::WpFifoV1);
delegate_noop!(Client: wp_commit_timing_manager_v1::WpCommitTimingManagerV1);
delegate_noop!(Client: wp_commit_timer_v1::WpCommitTimerV1);

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
        let mut icon = states.cached_state.get::<ToplevelIcon>();
        let icon = icon.current();
        (
            icon.name.clone(),
            icon.buffers.iter().map(|(_, scale, _)| *scale).collect(),
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

/// The width of the window of client `id` as the compositor has applied it, if it is mapped.
fn applied_width(state: &State, id: &ClientId) -> Option<i32> {
    state
        .space
        .elements()
        .find(|window| owned_by(window, id))
        .map(|window| window.geometry().size.w)
}

/// FIFO commits are applied a frame apart: in a burst of commits, each waiting on the barrier
/// the one before set, the last is applied, and not before a frame has passed for each.
#[test]
fn fifo_commits_are_applied_a_frame_apart() {
    const COMMITS: i32 = 6;
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_fifo_manager_v1::WpFifoManagerV1 = local.bind(1);
    let fifo = manager.get_fifo(&surface, &local.queue.handle(), ());
    let shm: wl_shm::WlShm = local.bind(1);
    let mut buffers = Vec::new();
    let started = Instant::now();
    for step in 1..=COMMITS {
        let (buffer, file) = local.buffer(&shm, WIDTH + step, HEIGHT, [0, 0, 255, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, WIDTH + step, HEIGHT);
        fifo.wait_barrier();
        fifo.set_barrier();
        surface.commit();
        buffers.push((buffer, file));
    }
    local.pump(&mut run, "the last FIFO commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + COMMITS)
    });
    // The first commit waits on no barrier; each of the others waits for one frame, of which
    // the first may be a frame already under way.
    let frame = Duration::from_secs(1) / 60;
    let frames = u32::try_from(COMMITS - 2).expect("a count");
    assert!(
        started.elapsed() >= frame * frames,
        "{COMMITS} commits applied in {:?}",
        started.elapsed()
    );
    run.stop();
}

/// A minimised window's FIFO commits are still applied, as no output shows it.
#[test]
fn a_minimised_window_s_fifo_commits_are_applied() {
    const COMMITS: i32 = 3;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_fifo_manager_v1::WpFifoManagerV1 = local.bind(1);
    let fifo = manager.get_fifo(&surface, &local.queue.handle(), ());
    let window = window_of(&run, &id);
    assert!(run.state.minimise(&window));
    let shm: wl_shm::WlShm = local.bind(1);
    let mut buffers = Vec::new();
    for step in 1..=COMMITS {
        let (buffer, file) = local.buffer(&shm, WIDTH + step, HEIGHT, [0, 0, 255, 255]);
        surface.attach(Some(&buffer), 0, 0);
        fifo.wait_barrier();
        fifo.set_barrier();
        surface.commit();
        buffers.push((buffer, file));
    }
    local.pump(
        &mut run,
        "the minimised window's last commit is applied",
        |_, _| window.geometry().size.w == WIDTH + COMMITS,
    );
    run.stop();
}

/// A commit with a timestamp is applied once that time has come, and not before.
#[test]
fn a_timed_commit_is_applied_at_its_timestamp() {
    let wait = Duration::from_millis(300);
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _file) = local.buffer(&shm, WIDTH + 7, HEIGHT, [0, 0, 255, 255]);
    let due = Duration::from(run.state.clock.now() + wait);
    let secs = due.as_secs();
    timer.set_timestamp(
        u32::try_from(secs >> 32).expect("seconds"),
        u32::try_from(secs & u64::from(u32::MAX)).expect("seconds"),
        due.subsec_nanos(),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.pump(&mut run, "the timed commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + 7)
    });
    assert!(
        Duration::from(run.state.clock.now()) >= due,
        "applied before its timestamp"
    );
    run.stop();
}

/// Sets the timestamp of the next commit through `timer` to `due` on the presentation clock.
fn time(timer: &wp_commit_timer_v1::WpCommitTimerV1, due: Duration) {
    let secs = due.as_secs();
    timer.set_timestamp(
        u32::try_from(secs >> 32).expect("seconds"),
        u32::try_from(secs & u64::from(u32::MAX)).expect("seconds"),
        due.subsec_nanos(),
    );
}

/// The protocol error the compositor closed `local` with.
fn error_of(
    run: &mut Running,
    local: &mut Local,
) -> wayland_client::backend::protocol::ProtocolError {
    local.connection.flush().expect("flush");
    let started = Instant::now();
    loop {
        if let Some(error) = local.connection.protocol_error() {
            return error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = local.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
}

/// The compositor's side of the window `local` mapped.
fn served_surface(run: &Running, local: &Local) -> WlSurface {
    let id = local.id.clone().expect("an id");
    let window = window_of(run, &id);
    window.toplevel().expect("a toplevel").wl_surface().clone()
}

/// A burst of FIFO commits, each waiting on the barrier the one before set, queues one commit a
/// frame clears: past `MAX_HELD_COMMITS` waiting the client is disconnected, with nothing held.
#[test]
fn a_client_queuing_too_many_fifo_commits_is_disconnected() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_fifo_manager_v1::WpFifoManagerV1 = local.bind(1);
    let fifo = manager.get_fifo(&surface, &local.queue.handle(), ());
    for _ in 0..MAX_HELD_COMMITS + 2 {
        fifo.wait_barrier();
        fifo.set_barrier();
        surface.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the client past the bound is disconnected", |state| {
        !served.is_alive() && state.held.is_empty()
    });
    run.stop();
}

/// Plain commits behind a commit timed far ahead wait on it, and count against the bound: past
/// `MAX_HELD_COMMITS` waiting the client is disconnected, and its timer goes with it.
#[test]
fn a_client_queuing_too_many_commits_behind_a_timed_one_is_disconnected() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    time(&timer, due);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(run.state.held.len(), 1, "the timed commit is held");
    for _ in 0..MAX_HELD_COMMITS + 1 {
        surface.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the client past the bound is disconnected", |state| {
        !served.is_alive() && state.held.is_empty()
    });
    run.stop();
}

/// Timed commits each take a place among the client's waiting commits: past `MAX_HELD_COMMITS`
/// the client is disconnected, and none of their timers is left.
#[test]
fn a_client_holding_too_many_timed_commits_is_disconnected() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    for _ in 0..MAX_HELD_COMMITS + 1 {
        time(&timer, due);
        surface.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the client past the bound is disconnected", |state| {
        !served.is_alive() && state.held.is_empty()
    });
    run.stop();
}

/// A client that disconnects with a timed commit held leaves no timer behind.
#[test]
fn a_timed_commit_is_released_when_its_client_disconnects() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    time(&timer, due);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(run.state.held.len(), 1, "the timed commit is held");
    drop(local);
    run.until("the held commit is released", |state| state.held.is_empty());
    run.stop();
}

/// A surface destroyed with a timed commit held frees its place and its timer at once, not when
/// the timer fires.
#[test]
fn a_timed_commit_is_released_when_its_surface_is_destroyed() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let surface = compositor.create_surface(&local.queue.handle(), ());
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    time(&timer, due);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(run.state.held.len(), 1, "the timed commit is held");
    let destroyed = Instant::now();
    timer.destroy();
    surface.destroy();
    local.roundtrip(&mut run);
    assert!(run.state.held.is_empty(), "released with its surface");
    assert!(
        destroyed.elapsed() < MAX_TIMED_WAIT,
        "released before its timer could fire"
    );
    run.stop();
}

/// A commit timed further ahead than `MAX_TIMED_WAIT`, even by more seconds than an instant can
/// hold, is applied once that wait has passed.
#[test]
fn a_commit_timed_far_ahead_is_applied_after_the_longest_wait() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let timer = manager.get_timer(&surface, &local.queue.handle(), ());
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _file) = local.buffer(&shm, WIDTH + 9, HEIGHT, [0, 0, 255, 255]);
    let started = Instant::now();
    timer.set_timestamp(0x7fff_ffff, 0, 0);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.pump(&mut run, "the far timed commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + 9)
    });
    assert!(
        started.elapsed() >= MAX_TIMED_WAIT,
        "applied in {:?}",
        started.elapsed()
    );
    run.stop();
}

/// A timestamp that is not a time, with nanoseconds of a second or more or seconds past what
/// the clock counts, is the timer's `invalid_timestamp` error, which disconnects its client and
/// leaves the compositor running.
#[test]
fn an_invalid_timestamp_is_the_timer_s_error() {
    let mut run = Running::start(None);
    for (tv_sec_hi, tv_nsec) in [(0, 1_000_000_000), (0x8000_0000, 0)] {
        let context = sandbox(&run);
        let mut local = Local::connect(&mut run, Some(context));
        local.toplevel(&mut run);
        let (surface, _) = local.window.clone().expect("a window");
        let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
        let timer = manager.get_timer(&surface, &local.queue.handle(), ());
        timer.set_timestamp(tv_sec_hi, 1, tv_nsec);
        surface.commit();
        let error = error_of(&mut run, &mut local);
        assert_eq!(error.object_interface, "wp_commit_timer_v1", "{tv_sec_hi}");
        assert_eq!(
            error.code,
            wp_commit_timer_v1::Error::InvalidTimestamp as u32,
            "{tv_sec_hi}"
        );
    }
    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    run.stop();
}

/// A surface has one commit timer at a time, and a commit one timestamp: a second is the
/// manager's `commit_timer_exists` error or the timer's `timestamp_exists`.
#[test]
fn a_second_timer_or_timestamp_is_an_error() {
    let mut run = Running::start(None);
    for case in ["timer", "timestamp"] {
        let context = sandbox(&run);
        let mut local = Local::connect(&mut run, Some(context));
        local.toplevel(&mut run);
        let (surface, _) = local.window.clone().expect("a window");
        let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
        let timer = manager.get_timer(&surface, &local.queue.handle(), ());
        let (interface, code) = match case {
            "timer" => {
                manager.get_timer(&surface, &local.queue.handle(), ());
                (
                    "wp_commit_timing_manager_v1",
                    wp_commit_timing_manager_v1::Error::CommitTimerExists as u32,
                )
            }
            _ => {
                let due = Duration::from(run.state.clock.now());
                time(&timer, due);
                time(&timer, due);
                (
                    "wp_commit_timer_v1",
                    wp_commit_timer_v1::Error::TimestampExists as u32,
                )
            }
        };
        let error = error_of(&mut run, &mut local);
        assert_eq!(error.object_interface, interface, "{case}");
        assert_eq!(error.code, code, "{case}");
    }
    run.stop();
}

/// A barrier set by a commit that waits on none is cleared at the next frame, which that commit
/// asks for: a later commit that waits on it is applied without a frame of its own, as an output
/// that draws only on request needs.
#[test]
fn a_fifo_barrier_set_alone_is_cleared_at_the_next_frame() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let manager: wp_fifo_manager_v1::WpFifoManagerV1 = local.bind(1);
    let fifo = manager.get_fifo(&surface, &local.queue.handle(), ());
    fifo.set_barrier();
    surface.commit();
    local.roundtrip(&mut run);
    let barrier = || {
        with_states(&served, |states| {
            states
                .cached_state
                .get::<FifoBarrierCachedState>()
                .current()
                .barrier
                .clone()
        })
    };
    assert!(barrier().is_some(), "the barrier is set");
    run.until("the barrier is cleared", |state| {
        barrier().is_none() && state.fifo_surfaces.is_empty()
    });
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _file) = local.buffer(&shm, WIDTH + 5, HEIGHT, [0, 0, 255, 255]);
    surface.attach(Some(&buffer), 0, 0);
    fifo.wait_barrier();
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(
        applied_width(&run.state, &id),
        Some(WIDTH + 5),
        "applied at once"
    );
    run.stop();
}

/// A request that changes an icon already set on a toplevel is the `immutable` error, which
/// disconnects the client and leaves the compositor running.
#[test]
fn changing_an_icon_after_set_icon_is_an_error() {
    let mut run = Running::start(None);
    for change in ["set_name", "add_buffer"] {
        let context = sandbox(&run);
        let mut local = Local::connect(&mut run, Some(context));
        let toplevel = local.toplevel(&mut run);
        let manager: xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1 = local.bind(1);
        let (buffer, icon) = set_icon(&mut local, &manager, &toplevel, "org.example.Set", 1);
        match change {
            "set_name" => icon.set_name("org.example.Changed".into()),
            _ => icon.add_buffer(&buffer, 2),
        }
        let error = error_of(&mut run, &mut local);
        assert_eq!(error.object_interface, "xdg_toplevel_icon_v1", "{change}");
        assert_eq!(
            error.code,
            xdg_toplevel_icon_v1::Error::Immutable as u32,
            "{change}"
        );
    }
    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    run.stop();
}

/// A buffer at a scale under 1, and one past `MAX_ICON_BUFFERS` on an icon, are the
/// `invalid_buffer` error, which disconnects the client and leaves the compositor running.
#[test]
fn an_icon_refuses_a_scale_under_one_and_too_many_buffers() {
    let mut run = Running::start(None);
    for case in ["scale", "count"] {
        let context = sandbox(&run);
        let mut local = Local::connect(&mut run, Some(context));
        let manager: xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1 = local.bind(1);
        let shm: wl_shm::WlShm = local.bind(1);
        let (buffer, _) = local.buffer(&shm, 16, 16, [0, 0, 255, 255]);
        let icon = manager.create_icon(&local.queue.handle(), ());
        if case == "scale" {
            icon.add_buffer(&buffer, 0);
        } else {
            for scale in 1..=MAX_ICON_BUFFERS {
                icon.add_buffer(&buffer, i32::try_from(scale).expect("a scale"));
            }
            local.roundtrip(&mut run);
            assert!(
                local.connection.protocol_error().is_none(),
                "{MAX_ICON_BUFFERS} buffers are taken"
            );
            icon.add_buffer(&buffer, 1);
        }
        let error = error_of(&mut run, &mut local);
        assert_eq!(error.object_interface, "xdg_toplevel_icon_v1", "{case}");
        assert_eq!(
            error.code,
            xdg_toplevel_icon_v1::Error::InvalidBuffer as u32,
            "{case}"
        );
    }
    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    run.stop();
}

/// A synchronised subsurface's commits wait for its parent's next commit, and count against the
/// bound: a client committing one past `MAX_HELD_COMMITS` times under a parent that never
/// commits is disconnected.
#[test]
fn a_client_committing_too_often_under_a_parent_that_never_commits_is_disconnected() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let qh = local.queue.handle();
    let child = compositor.create_surface(&qh, ());
    let _subsurface = subcompositor.get_subsurface(&child, &surface, &qh, ());
    for _ in 0..MAX_HELD_COMMITS + 2 {
        child.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the client past the bound is disconnected", |_| {
        !served.is_alive()
    });
    run.stop();
}

/// A synchronised subsurface destroyed while its parent's commit waits on the subsurface's
/// timestamp releases that commit at once, rather than leave the parent waiting.
#[test]
fn a_parent_waiting_on_a_destroyed_subsurface_s_timestamp_is_applied() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let (buffer, _file) = local.buffer(&shm, WIDTH + 3, HEIGHT, [0, 0, 255, 255]);
    let child = compositor.create_surface(&qh, ());
    let subsurface = subcompositor.get_subsurface(&child, &surface, &qh, ());
    let timer = manager.get_timer(&child, &qh, ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    time(&timer, due);
    child.commit();
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(&mut run);
    assert_eq!(run.state.held.len(), 1, "the subsurface's timed commit is held");
    assert_ne!(
        applied_width(&run.state, &id),
        Some(WIDTH + 3),
        "the parent waits on it"
    );
    let destroyed = Instant::now();
    timer.destroy();
    subsurface.destroy();
    child.destroy();
    local.pump(&mut run, "the parent's commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + 3)
    });
    assert!(
        destroyed.elapsed() < MAX_TIMED_WAIT,
        "applied in {:?}",
        destroyed.elapsed()
    );
    assert!(run.state.held.is_empty(), "nothing is left held");
    run.stop();
}

/// A surface destroyed with a FIFO barrier to clear is forgotten with it, not at the next frame,
/// which an output that is off or paused does not draw.
#[test]
fn a_destroyed_fifo_surface_is_forgotten_at_once() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let manager: wp_fifo_manager_v1::WpFifoManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    local.roundtrip(&mut run);
    for _ in 0..20 {
        let surface = compositor.create_surface(&qh, ());
        let fifo = manager.get_fifo(&surface, &qh, ());
        fifo.set_barrier();
        surface.commit();
        fifo.destroy();
        surface.destroy();
        local.connection.flush().expect("flush");
        run.event_loop
            .dispatch(Some(Duration::ZERO), &mut run.state)
            .expect("dispatch");
        assert!(
            run.state
                .fifo_surfaces
                .values()
                .all(|surface| surface.is_alive()),
            "a destroyed surface is remembered"
        );
    }
    run.stop();
}

/// The commits of a destroyed surface leave the bound with it: a client at `MAX_HELD_COMMITS`
/// that destroys the surface they wait on and commits another goes on.
#[test]
fn commits_of_a_destroyed_surface_leave_the_bound_with_it() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let (window, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let manager: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let (buffer, _file) = local.buffer(&shm, WIDTH + 11, HEIGHT, [0, 0, 255, 255]);
    let surface = compositor.create_surface(&qh, ());
    let timer = manager.get_timer(&surface, &qh, ());
    let due = Duration::from(run.state.clock.now()) + Duration::from_secs(3600);
    time(&timer, due);
    for _ in 0..MAX_HELD_COMMITS {
        surface.commit();
    }
    local.roundtrip(&mut run);
    assert_eq!(run.state.held.len(), 1, "the timed commit is held");
    timer.destroy();
    surface.destroy();
    window.attach(Some(&buffer), 0, 0);
    window.commit();
    local.pump(&mut run, "the window's commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + 11)
    });
    run.stop();
}

/// How many commits each subsurface of the next tests keeps: more than half the bound, so that two
/// subsurfaces counted together would pass it.
const KEPT: usize = MAX_HELD_COMMITS / 2 + 8;

/// The commits a subsurface kept while synchronised stay after `set_desync` until its own next
/// commit, and they are its own: another subsurface's do not add up with them, and once it is
/// synchronised again they still count, so its client is disconnected one past the bound.
#[test]
fn commits_kept_through_set_desync_count_for_their_subsurface_alone() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let qh = local.queue.handle();
    let first = compositor.create_surface(&qh, ());
    let first_subsurface = subcompositor.get_subsurface(&first, &surface, &qh, ());
    let second = compositor.create_surface(&qh, ());
    let _second_subsurface = subcompositor.get_subsurface(&second, &surface, &qh, ());
    for _ in 0..KEPT {
        first.commit();
    }
    first_subsurface.set_desync();
    surface.commit();
    for _ in 0..KEPT {
        second.commit();
    }
    local.roundtrip(&mut run);
    assert!(served.is_alive(), "two subsurfaces each under the bound");
    first_subsurface.set_sync();
    for _ in KEPT..=MAX_HELD_COMMITS {
        first.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the subsurface past the bound has its client disconnected", |_| {
        !served.is_alive()
    });
    run.stop();
}

/// A subsurface whose `wl_subsurface` is destroyed stays synchronised without a parent, and Smithay
/// keeps each of its commits: they count for that surface alone, and one past the bound its
/// client is disconnected.
#[test]
fn an_orphaned_subsurface_s_commits_count_for_it_alone() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let qh = local.queue.handle();
    let orphan = compositor.create_surface(&qh, ());
    let orphan_subsurface = subcompositor.get_subsurface(&orphan, &surface, &qh, ());
    let other = compositor.create_surface(&qh, ());
    let _other_subsurface = subcompositor.get_subsurface(&other, &surface, &qh, ());
    orphan_subsurface.destroy();
    for _ in 0..KEPT {
        orphan.commit();
        other.commit();
    }
    local.roundtrip(&mut run);
    assert!(served.is_alive(), "an orphan and a subsurface each under the bound");
    for _ in KEPT..=MAX_HELD_COMMITS {
        orphan.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the orphan past the bound has its client disconnected", |_| {
        !served.is_alive()
    });
    run.stop();
}

/// The commits a destroyed subsurface kept leave the bound with it, even while the compositor still
/// holds the surface: subsurfaces destroyed one after another, together past the client's bound
/// on kept commits, leave their client connected and its window's commits applied.
#[test]
fn commits_kept_by_destroyed_subsurfaces_leave_the_bound_with_them() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let destroyed = MAX_RETAINED_COMMITS / KEPT + 2;
    let mut held = Vec::new();
    for _ in 0..destroyed {
        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &surface, &qh, ());
        for _ in 0..KEPT {
            child.commit();
        }
        local.roundtrip(&mut run);
        held.extend(get_children(&served));
        subsurface.destroy();
        child.destroy();
    }
    local.roundtrip(&mut run);
    assert!(served.is_alive(), "destroyed subsurfaces past the bound together");
    assert!(held.len() >= destroyed, "the compositor holds each destroyed subsurface");
    let (buffer, _file) = local.buffer(&shm, WIDTH + 13, HEIGHT, [0, 0, 255, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.pump(&mut run, "the window's commit is applied", |_, state| {
        applied_width(state, &id) == Some(WIDTH + 13)
    });
    run.stop();
}

/// The commits all of a client's synchronised subsurfaces keep count together too: subsurfaces
/// each under `MAX_HELD_COMMITS`, together one bound's worth short of `MAX_RETAINED_COMMITS`,
/// leave their client connected, and one more past it has the client disconnected.
#[test]
fn many_subsurfaces_keeping_commits_together_past_the_client_bound_are_disconnected() {
    const EACH: usize = MAX_HELD_COMMITS - 4;
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let mut local = Local::connect(&mut run, Some(context));
    local.toplevel(&mut run);
    let served = served_surface(&run, &local);
    let (surface, _) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let qh = local.queue.handle();
    let mut children = Vec::new();
    for _ in 0..=MAX_RETAINED_COMMITS / EACH {
        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &surface, &qh, ());
        children.push((child, subsurface));
    }
    let (last, _) = children.pop().expect("a last subsurface");
    for (child, _) in &children {
        for _ in 0..EACH {
            child.commit();
        }
    }
    local.roundtrip(&mut run);
    assert!(served.is_alive(), "the subsurfaces' commits under the client's bound");
    for _ in 0..EACH {
        last.commit();
    }
    local.connection.flush().expect("flush");
    run.until("the client past its bound on kept commits is disconnected", |_| {
        !served.is_alive()
    });
    run.stop();
}
