//! Decision 30 (`constraints.rs`): pointer locks, relative motion and keyboard-shortcuts
//! inhibitors, with Ctrl+Alt+Tab's suspension.

use super::*;
use crate::bindings::Action;
use wayland_client::protocol::{wl_region, wl_subcompositor, wl_subsurface};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1},
};
use wayland_protocols::wp::pointer_constraints::zv1::client::{
    zwp_confined_pointer_v1::{self, ZwpConfinedPointerV1},
    zwp_locked_pointer_v1::{self, ZwpLockedPointerV1},
    zwp_pointer_constraints_v1::{Lifetime, ZwpPointerConstraintsV1},
};
use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};

/// The events an object was sent, in order.
type Log = Arc<Mutex<Vec<&'static str>>>;

fn events(log: &Log) -> Vec<&'static str> {
    log.lock().expect("the log").clone()
}

delegate_noop!(Client: ZwpPointerConstraintsV1);
delegate_noop!(Client: ZwpRelativePointerManagerV1);
delegate_noop!(Client: ZwpKeyboardShortcutsInhibitManagerV1);
delegate_noop!(Client: wl_region::WlRegion);
delegate_noop!(Client: wl_subcompositor::WlSubcompositor);
delegate_noop!(Client: wl_subsurface::WlSubsurface);

impl Dispatch<ZwpConfinedPointerV1, Log> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpConfinedPointerV1,
        event: zwp_confined_pointer_v1::Event,
        log: &Log,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let name = match event {
            zwp_confined_pointer_v1::Event::Confined => "confined",
            zwp_confined_pointer_v1::Event::Unconfined => "unconfined",
            _ => return,
        };
        log.lock().expect("the log").push(name);
    }
}

impl Dispatch<ZwpLockedPointerV1, Log> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        log: &Log,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let name = match event {
            zwp_locked_pointer_v1::Event::Locked => "locked",
            zwp_locked_pointer_v1::Event::Unlocked => "unlocked",
            _ => return,
        };
        log.lock().expect("the log").push(name);
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, Log> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        log: &Log,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let name = match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => "active",
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => "inactive",
            _ => return,
        };
        log.lock().expect("the log").push(name);
    }
}

/// Each relative motion: the delta, then the unaccelerated delta.
type Motions = Arc<Mutex<Vec<[f64; 4]>>>;

impl Dispatch<ZwpRelativePointerV1, Motions> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        motions: &Motions,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion {
            dx,
            dy,
            dx_unaccel,
            dy_unaccel,
            ..
        } = event
        {
            motions
                .lock()
                .expect("the motions")
                .push([dx, dy, dx_unaccel, dy_unaccel]);
        }
    }
}

/// A client that captures the input, as a game or a VM viewer does: a window with the
/// keyboard and the pointer on it.
struct Game {
    local: Local,
    /// The window's surface, the client's and the compositor's.
    surface: wl_surface::WlSurface,
    server: WlSurface,
    window: smithay::desktop::Window,
    /// Where the pointer was put on the window.
    at: Point<f64, Logical>,
    seat: wl_seat::WlSeat,
    pointer: wl_pointer::WlPointer,
    constraints: ZwpPointerConstraintsV1,
    inhibit: ZwpKeyboardShortcutsInhibitManagerV1,
}

impl Game {
    fn start(run: &mut Running) -> Self {
        let mut local = Local::connect(run, None);
        // A keyboard of its own, to read the keys the window is sent.
        let seat: wl_seat::WlSeat = local.bind(1);
        let _keyboard = seat.get_keyboard(&local.queue.handle(), ());
        local.toplevel(run);
        let (surface, _) = local.window.clone().expect("a window");
        let id = local.id.clone().expect("an id");
        let window = window_of(run, &id);
        let server = window.toplevel().expect("a toplevel").wl_surface().clone();
        let at = location_of(run, &id).to_f64() + Point::from((WIDTH / 2, HEIGHT / 2)).to_f64();
        run.state.pointer_to(at, InputTime::now());
        local.roundtrip(run);
        assert_eq!(
            run.state.surface_under(at).map(|(surface, _)| surface),
            Some(server.clone()),
            "the pointer is on the window"
        );
        assert_eq!(
            keyboard_focus(run),
            Some(server.clone()),
            "it has the keyboard"
        );
        let pointer = local.pointer.clone().expect("a pointer");
        let constraints: ZwpPointerConstraintsV1 = local.bind(1);
        let inhibit: ZwpKeyboardShortcutsInhibitManagerV1 = local.bind(1);
        Self {
            local,
            surface,
            server,
            window,
            at,
            seat,
            pointer,
            constraints,
            inhibit,
        }
    }

    fn lock(&mut self, lifetime: Lifetime) -> (ZwpLockedPointerV1, Log) {
        let log = Log::default();
        let lock = self.constraints.lock_pointer(
            &self.surface,
            &self.pointer,
            None,
            lifetime,
            &self.local.queue.handle(),
            log.clone(),
        );
        (lock, log)
    }

    fn inhibitor(
        &mut self,
        surface: &wl_surface::WlSurface,
    ) -> (ZwpKeyboardShortcutsInhibitorV1, Log) {
        let log = Log::default();
        let inhibitor = self.inhibit.inhibit_shortcuts(
            surface,
            &self.seat,
            &self.local.queue.handle(),
            log.clone(),
        );
        (inhibitor, log)
    }

    fn confine(
        &mut self,
        region: Option<&wl_region::WlRegion>,
        lifetime: Lifetime,
    ) -> (ZwpConfinedPointerV1, Log) {
        let log = Log::default();
        let confined = self.constraints.confine_pointer(
            &self.surface,
            &self.pointer,
            region,
            lifetime,
            &self.local.queue.handle(),
            log.clone(),
        );
        (confined, log)
    }

    /// A 40 by 40 pixel subsurface in the window's middle, and its surface in the compositor.
    fn subsurface(&mut self, run: &mut Running) -> (wl_subsurface::WlSubsurface, WlSurface) {
        let compositor: wl_compositor::WlCompositor = self.local.bind(4);
        let subcompositor: wl_subcompositor::WlSubcompositor = self.local.bind(1);
        let shm: wl_shm::WlShm = self.local.bind(1);
        let qh = self.local.queue.handle();
        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &self.surface, &qh, ());
        subsurface.set_position(WIDTH / 2 - 20, HEIGHT / 2 - 20);
        let (buffer, _) = self.local.buffer(&shm, 40, 40, [0, 0, 255, 255]);
        child.attach(Some(&buffer), 0, 0);
        child.commit();
        self.surface.commit();
        self.settle(run);
        let middle = run
            .state
            .space
            .element_location(&self.window)
            .expect("a location")
            .to_f64()
            + Point::from((WIDTH / 2, HEIGHT / 2)).to_f64();
        let on = run.state.surface_under(middle).map(|(surface, _)| surface);
        let server = on
            .filter(|on| *on != self.server && on.client().map(|c| c.id()) == self.local.id)
            .expect("the subsurface is in the window's middle");
        (subsurface, server)
    }

    /// The pointer's location in the compositor.
    fn location(run: &Running) -> Point<f64, Logical> {
        run.state
            .seat
            .get_pointer()
            .expect("a pointer")
            .current_location()
    }

    /// A relative motion by `delta`, accelerated and not.
    fn nudge(run: &mut Running, delta: (f64, f64)) {
        run.state
            .pointer_moved_by(delta.into(), delta.into(), InputTime::now());
    }

    /// Runs until `log` holds `expected`.
    fn wait(&mut self, run: &mut Running, log: &Log, expected: &[&str], what: &str) {
        let log = log.clone();
        self.local
            .pump(run, what, move |_, _| events(&log) == expected);
    }

    /// Two roundtrips: whatever the compositor sends meanwhile has arrived.
    fn settle(&mut self, run: &mut Running) {
        self.local.roundtrip(run);
        self.local.roundtrip(run);
    }

    /// Puts the pointer on the window's middle, wherever the window is now.
    fn aim(&mut self, run: &mut Running) {
        let location = run
            .state
            .space
            .element_location(&self.window)
            .expect("a location");
        self.at = location.to_f64() + Point::from((WIDTH / 2, HEIGHT / 2)).to_f64();
        run.state.pointer_to(self.at, InputTime::now());
    }

    /// A click on the window.
    fn click(&mut self, run: &mut Running) {
        press(run, self.at);
        release(run);
    }
}

const CYCLE: [u32; 3] = [KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB];

/// A lock is active only while its surface has the keyboard: asked for without it, it waits;
/// given the keyboard, it locks; the keyboard gone, it unlocks, and a persistent lock locks
/// again when the keyboard comes back.
#[test]
fn a_lock_holds_only_while_its_surface_has_the_keyboard() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    run.state.set_keyboard_focus(None);
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.settle(&mut run);
    assert_eq!(
        events(&log),
        [] as [&str; 0],
        "no lock without the keyboard"
    );
    run.state.focus(&game.window.clone());
    game.wait(&mut run, &log, &["locked"], "locked with the keyboard");
    run.state.set_keyboard_focus(None);
    game.wait(
        &mut run,
        &log,
        &["locked", "unlocked"],
        "unlocked without it",
    );
    run.state.focus(&game.window.clone());
    game.wait(
        &mut run,
        &log,
        &["locked", "unlocked", "locked"],
        "locked again",
    );
    run.stop();
}

/// While the pointer is locked a relative motion moves it not, and the window is sent the
/// motion, accelerated and not.
#[test]
fn a_locked_pointer_is_sent_relative_motion() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let manager: ZwpRelativePointerManagerV1 = game.local.bind(1);
    let motions = Motions::default();
    let _relative =
        manager.get_relative_pointer(&game.pointer, &game.local.queue.handle(), motions.clone());
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    run.state.pointer_moved_by(
        Point::from((7.0, -3.0)),
        Point::from((5.0, -2.0)),
        InputTime::now(),
    );
    let sent = motions.clone();
    game.local
        .pump(&mut run, "the relative motion", move |_, _| {
            !sent.lock().expect("the motions").is_empty()
        });
    assert_eq!(
        *motions.lock().expect("the motions"),
        [[7.0, -3.0, 5.0, -2.0]]
    );
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert_eq!(pointer.current_location(), game.at, "the pointer stays");
    run.stop();
}

/// Under an active inhibitor the user's way out stays the compositor's: Super+Alt+T,
/// Super+Escape, Super+Alt+Escape, the terminal switch, Super+Alt+S and Ctrl+Alt+Tab act; any
/// other binding, Super alone among them, reaches the window.
#[test]
fn an_inhibitor_keeps_the_way_out() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let surface = game.surface.clone();
    let (_inhibitor, log) = game.inhibitor(&surface);
    game.wait(&mut run, &log, &["active"], "the inhibitor is active");
    let window = &mut game.local;
    reaches(window, &mut run, &[KEY_LEFTMETA, KEY_C], "Super+C");
    reaches(window, &mut run, &[KEY_LEFTMETA, KEY_1], "Super+1");
    reaches(window, &mut run, &[KEY_LEFTMETA], "Super alone");
    for (codes, action, what) in [
        (
            &[KEY_LEFTMETA, KEY_LEFTALT, KEY_T][..],
            Action::Reveal,
            "Super+Alt+T",
        ),
        (&[KEY_LEFTMETA, KEY_ESC], Action::Lock, "Super+Escape"),
        (
            &[KEY_LEFTMETA, KEY_LEFTALT, KEY_ESC],
            Action::Terminate,
            "Super+Alt+Escape",
        ),
        (
            &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_F1],
            Action::Vt(1),
            "Ctrl+Alt+F1",
        ),
        (
            &[KEY_LEFTMETA, KEY_LEFTALT, KEY_S],
            Action::Call(&crate::bindings::cosmic::SCREEN_READER),
            "Super+Alt+S",
        ),
        (&CYCLE, Action::FocusCycle, "Ctrl+Alt+Tab"),
    ] {
        acts(window, &mut run, codes, Some(action), what);
    }
    run.stop();
}

/// Ctrl+Alt+Tab suspends the window's inhibitor and lock when the cycle has nowhere to go: the
/// window keeps the keyboard, and the bindings are the compositor's again.
#[test]
fn ctrl_alt_tab_suspends_with_nothing_to_cycle_to() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let surface = game.surface.clone();
    let (_inhibitor, inhibitor) = game.inhibitor(&surface);
    let (_lock, lock) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &inhibitor, &["active"], "the inhibitor is active");
    game.wait(&mut run, &lock, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    game.wait(
        &mut run,
        &inhibitor,
        &["active", "inactive"],
        "inhibitor suspended",
    );
    game.wait(&mut run, &lock, &["locked", "unlocked"], "lock suspended");
    assert_eq!(
        keyboard_focus(&run),
        Some(game.server.clone()),
        "the window keeps the keyboard"
    );
    acts(
        &mut game.local,
        &mut run,
        &[KEY_LEFTMETA, KEY_C],
        Some(Action::Call(&crate::session_calls::CONTROL_CENTER)),
        "Super+C",
    );
    run.stop();
}

/// Ctrl+Alt+Tab to the bar suspends the window's inhibitor and lock as well.
#[test]
fn ctrl_alt_tab_suspends_on_the_way_to_the_bar() {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity::OnDemand};
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_bar, bar) = top_surface(
        &mut run,
        "bar",
        Anchor::Top | Anchor::Left | Anchor::Right,
        OnDemand,
    );
    run.state.focus(&game.window.clone());
    game.aim(&mut run);
    let surface = game.surface.clone();
    let (_inhibitor, inhibitor) = game.inhibitor(&surface);
    let (_lock, lock) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &inhibitor, &["active"], "the inhibitor is active");
    game.wait(&mut run, &lock, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    assert_eq!(keyboard_focus(&run), Some(bar), "the bar has the keyboard");
    game.wait(
        &mut run,
        &inhibitor,
        &["active", "inactive"],
        "inhibitor suspended",
    );
    game.wait(&mut run, &lock, &["locked", "unlocked"], "lock suspended");
    run.stop();
}

/// A suspension outlasts a new window of the client and a focus by xdg-activation, and an
/// inhibitor asked for meanwhile stays inactive; a click on the window lifts it.
#[test]
fn only_a_click_or_a_key_lifts_a_suspension() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let first = game.surface.clone();
    let (_inhibitor, inhibitor) = game.inhibitor(&first);
    let (_lock, lock) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &lock, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    game.wait(&mut run, &lock, &["locked", "unlocked"], "suspended");
    let id = game.local.id.clone().expect("an id");

    // A new window takes the keyboard; its inhibitor stays inactive.
    game.local.toplevel(&mut run);
    let (second, _) = game.local.window.clone().expect("the second window");
    let server = game.server.clone();
    game.local.pump(
        &mut run,
        "the second window has the keyboard",
        |_, state| {
            let focus = state.seat.get_keyboard().and_then(|k| k.current_focus());
            focus.is_some_and(|focus| {
                focus != server && focus.client().map(|c| c.id()) == Some(id.clone())
            })
        },
    );
    let (_late, late) = game.inhibitor(&second);
    game.settle(&mut run);
    assert_eq!(
        events(&late),
        [] as [&str; 0],
        "no inhibitor while suspended"
    );
    assert!(
        run.state.input_hold.suspended.contains(&id),
        "still suspended"
    );

    // xdg-activation gives the first window the keyboard back.
    let serial = run
        .state
        .seat
        .get_keyboard()
        .and_then(|keyboard| keyboard.last_enter())
        .expect("an enter");
    let token = run.activation_token(&mut game.local, serial.into());
    let activation: xdg_activation_v1::XdgActivationV1 = game.local.bind(1);
    activation.activate(token, &first);
    game.settle(&mut run);
    assert_eq!(keyboard_focus(&run), Some(game.server.clone()), "activated");
    game.aim(&mut run);
    game.settle(&mut run);
    assert_eq!(
        events(&lock),
        ["locked", "unlocked"],
        "the lock stays suspended"
    );
    assert_eq!(
        events(&inhibitor),
        ["active", "inactive"],
        "and the inhibitor"
    );
    assert!(
        run.state.input_hold.suspended.contains(&id),
        "still suspended"
    );

    game.click(&mut run);
    game.wait(
        &mut run,
        &lock,
        &["locked", "unlocked", "locked"],
        "the click lifts it",
    );
    game.wait(
        &mut run,
        &inhibitor,
        &["active", "inactive", "active"],
        "the inhibitor too",
    );
    game.wait(
        &mut run,
        &late,
        &["active"],
        "and the one asked for meanwhile",
    );
    run.stop();
}

/// Ctrl+Alt+Tab from the bar back to a suspended window is a key that moves the keyboard to
/// it: the suspension ends.
#[test]
fn a_key_back_to_the_window_lifts_a_suspension() {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity::OnDemand};
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_bar, bar) = top_surface(
        &mut run,
        "bar",
        Anchor::Top | Anchor::Left | Anchor::Right,
        OnDemand,
    );
    run.state.focus(&game.window.clone());
    game.aim(&mut run);
    let (_lock, lock) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &lock, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    assert_eq!(keyboard_focus(&run), Some(bar), "to the bar");
    game.wait(&mut run, &lock, &["locked", "unlocked"], "suspended");
    chord(&mut run.state, &CYCLE);
    assert_eq!(
        keyboard_focus(&run),
        Some(game.server.clone()),
        "back to the window"
    );
    game.wait(&mut run, &lock, &["locked", "unlocked", "locked"], "lifted");
    run.stop();
}

/// On the lift a persistent lock locks again; a oneshot lock that was active when suspended
/// has ended, and one asked for meanwhile locks.
#[test]
fn a_persistent_lock_returns_and_an_active_oneshot_does_not() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (persistent, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    game.wait(&mut run, &log, &["locked", "unlocked"], "suspended");
    game.click(&mut run);
    game.wait(
        &mut run,
        &log,
        &["locked", "unlocked", "locked"],
        "the persistent lock returns",
    );
    persistent.destroy();

    let (_oneshot, once) = game.lock(Lifetime::Oneshot);
    game.wait(&mut run, &once, &["locked"], "the oneshot lock");
    chord(&mut run.state, &CYCLE);
    game.wait(&mut run, &once, &["locked", "unlocked"], "suspended");
    let (_meanwhile, meanwhile) = game.lock(Lifetime::Persistent);
    game.settle(&mut run);
    assert_eq!(
        events(&meanwhile),
        [] as [&str; 0],
        "no lock while suspended"
    );
    game.click(&mut run);
    game.wait(
        &mut run,
        &meanwhile,
        &["locked"],
        "the lock asked for meanwhile",
    );
    game.settle(&mut run);
    assert_eq!(
        events(&once),
        ["locked", "unlocked"],
        "the oneshot lock has ended"
    );
    run.stop();
}

/// A session lock takes the pointer and the keyboard from a window that holds a lock and an
/// inhibitor: the lock unlocks, no binding is inhibited, the pointer moves and the window is
/// sent no relative motion; the lock surface holds none of the window's state.
#[test]
fn a_session_lock_releases_what_a_window_holds() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let manager: ZwpRelativePointerManagerV1 = game.local.bind(1);
    let motions = Motions::default();
    let _relative =
        manager.get_relative_pointer(&game.pointer, &game.local.queue.handle(), motions.clone());
    let surface = game.surface.clone();
    let (_inhibitor, inhibitor) = game.inhibitor(&surface);
    let (_lock, lock) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &inhibitor, &["active"], "the inhibitor is active");
    game.wait(&mut run, &lock, &["locked"], "locked");
    assert!(
        run.state.shortcuts_inhibited(),
        "the window inhibits the bindings"
    );

    let (_locker, _session) = lock_session(&mut run);
    game.wait(
        &mut run,
        &lock,
        &["locked", "unlocked"],
        "the session lock unlocks it",
    );
    assert!(!run.state.shortcuts_inhibited(), "no binding is inhibited");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert_ne!(
        pointer.current_focus(),
        Some(game.server.clone()),
        "the pointer left the window"
    );
    run.state.pointer_moved_by(
        Point::from((7.0, -3.0)),
        Point::from((5.0, -2.0)),
        InputTime::now(),
    );
    assert_eq!(
        pointer.current_location(),
        game.at + Point::from((7.0, -3.0)),
        "the pointer moves"
    );
    game.settle(&mut run);
    assert_eq!(
        *motions.lock().expect("the motions"),
        [] as [[f64; 4]; 0],
        "the window is sent no relative motion"
    );
    assert_eq!(events(&lock), ["locked", "unlocked"], "nor locked again");
    run.stop();
}

/// The window's middle, a 20 by 20 pixel square: the region of the confinements below.
fn middle(local: &mut Local) -> wl_region::WlRegion {
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let region = compositor.create_region(&local.queue.handle(), ());
    region.add(WIDTH / 2 - 10, HEIGHT / 2 - 10, 20, 20);
    region
}

/// A confined pointer moves inside its region and never out of it.
#[test]
fn a_confined_pointer_stays_in_its_region() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let region = middle(&mut game.local);
    let (_confined, log) = game.confine(Some(&region), Lifetime::Persistent);
    game.wait(&mut run, &log, &["confined"], "confined");
    Game::nudge(&mut run, (5.0, 5.0));
    let inside = game.at + Point::from((5.0, 5.0));
    assert_eq!(Game::location(&run), inside, "it moves inside the region");
    Game::nudge(&mut run, (20.0, 0.0));
    assert_eq!(Game::location(&run), inside, "and not out of it");
    Game::nudge(&mut run, (0.0, -40.0));
    assert_eq!(Game::location(&run), inside, "on no side");
    run.stop();
}

/// A confined pointer on a window that crosses the outputs' edge stops at the edge: its
/// motion is kept on the outputs before the confinement is checked, as any motion is.
#[test]
fn a_confined_pointer_stays_on_the_outputs() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let area = run
        .state
        .output_areas()
        .into_iter()
        .find(|area| area.to_f64().contains(game.at))
        .expect("the window's output");
    let bottom = area.loc.y + area.size.h;
    let window = game.window.clone();
    run.state
        .space
        .relocate_element(&window, (area.loc.x, bottom - HEIGHT / 2));
    game.at = Point::from((area.loc.x + WIDTH / 2, bottom - 10)).to_f64();
    run.state.pointer_to(game.at, InputTime::now());
    assert_eq!(
        run.state.surface_under(game.at).map(|(surface, _)| surface),
        Some(game.server.clone()),
        "the pointer is on the window, near the outputs' edge"
    );
    let (_confined, log) = game.confine(None, Lifetime::Persistent);
    game.wait(&mut run, &log, &["confined"], "confined");
    Game::nudge(&mut run, (0.0, 30.0));
    let edge = Point::from((game.at.x, f64::from(bottom - 1)));
    assert_eq!(Game::location(&run), edge, "it stops at the edge");
    run.stop();
}

/// A lock on a window holds while the pointer is on one of its subsurfaces.
#[test]
fn a_lock_holds_over_a_subsurface() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_subsurface, child) = game.subsurface(&mut run);
    run.state.pointer_to(game.at, InputTime::now());
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert_eq!(
        pointer.current_focus(),
        Some(child),
        "the pointer is on the subsurface"
    );
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked over the subsurface");
    Game::nudge(&mut run, (7.0, 0.0));
    assert_eq!(Game::location(&run), game.at, "the pointer stays");
    run.stop();
}

/// Known limit, pinned (`State::refresh_constraint`): a confined pointer that moves from its
/// window onto the window's subsurface stays on it, but Smithay's leave of the window ends the
/// confinement first: a persistent one is confined again, a oneshot one has ended.
#[test]
fn a_move_onto_a_subsurface_reconfines() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_subsurface, child) = game.subsurface(&mut run);
    let middle = game.at;
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    for (lifetime, expected) in [
        (
            Lifetime::Persistent,
            &["confined", "unconfined", "confined"][..],
        ),
        (Lifetime::Oneshot, &["confined", "unconfined"][..]),
    ] {
        game.at = middle - Point::from((WIDTH / 2 - 10, HEIGHT / 2 - 10)).to_f64();
        run.state.pointer_to(game.at, InputTime::now());
        assert_eq!(
            pointer.current_focus(),
            Some(game.server.clone()),
            "{lifetime:?}: the pointer is on the window"
        );
        let (confined, log) = game.confine(None, lifetime);
        game.wait(&mut run, &log, &["confined"], "confined");
        let delta = middle - game.at;
        Game::nudge(&mut run, (delta.x, delta.y));
        assert_eq!(
            pointer.current_focus(),
            Some(child.clone()),
            "{lifetime:?}: onto the subsurface"
        );
        game.settle(&mut run);
        assert_eq!(events(&log), expected, "{lifetime:?}");
        confined.destroy();
        game.settle(&mut run);
    }
    run.stop();
}

/// Under a lock, a virtual pointer's absolute motion moves the pointer not, and the window is
/// sent the move from where the same device last was: nothing at a device's first sample, and
/// nothing at the first sample of a pointer made after one is gone. Under a confinement it
/// does not take the pointer out of the region.
#[test]
fn an_absolute_motion_keeps_a_lock_and_a_confinement() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let relative: ZwpRelativePointerManagerV1 = game.local.bind(1);
    let motions = Motions::default();
    let _relative =
        relative.get_relative_pointer(&game.pointer, &game.local.queue.handle(), motions.clone());
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = game.local.bind(2);
    let area = run
        .state
        .output_areas()
        .into_iter()
        .reduce(|all, area| all.merge(area))
        .expect("the outputs");
    let (w, h) = (area.size.w as u32, area.size.h as u32);
    let point = |game: &mut Game,
                 run: &mut Running,
                 device: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
                 at: Point<f64, Logical>| {
        let local = at - area.loc.to_f64();
        device.motion_absolute(0, local.x as u32, local.y as u32, w, h);
        device.frame();
        game.settle(run);
    };
    let new_device = |game: &mut Game| {
        manager.create_virtual_pointer(Some(&game.seat), &game.local.queue.handle(), ())
    };
    let sent = |motions: &Motions| std::mem::take(&mut *motions.lock().expect("the motions"));
    let none: [[f64; 4]; 0] = [];

    let (lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    let at = game.at;
    let by = |x: f64| at + Point::from((x, 0.0));
    let first = new_device(&mut game);
    let second = new_device(&mut game);
    point(&mut game, &mut run, &first, by(30.0));
    assert_eq!(sent(&motions), none, "a first sample sends nothing");
    point(&mut game, &mut run, &first, by(40.0));
    assert_eq!(
        sent(&motions),
        [[10.0, 0.0, 10.0, 0.0]],
        "the device's next one, its move"
    );
    point(&mut game, &mut run, &second, by(-50.0));
    assert_eq!(sent(&motions), none, "another device's first, nothing");
    point(&mut game, &mut run, &second, by(-45.0));
    assert_eq!(
        sent(&motions),
        [[5.0, 0.0, 5.0, 0.0]],
        "and its next one, its move"
    );
    first.destroy();
    game.settle(&mut run);
    assert_eq!(
        run.state.input_hold.last_absolute.len(),
        1,
        "the device that went is forgotten"
    );
    let third = new_device(&mut game);
    point(&mut game, &mut run, &third, by(60.0));
    assert_eq!(
        sent(&motions),
        none,
        "a pointer made after one is gone starts afresh"
    );
    assert_eq!(Game::location(&run), at, "a locked pointer stays");
    lock.destroy();
    game.settle(&mut run);

    let region = middle(&mut game.local);
    let (_confined, confined) = game.confine(Some(&region), Lifetime::Persistent);
    game.wait(&mut run, &confined, &["confined"], "confined");
    let inside = at + Point::from((5.0, 5.0));
    point(&mut game, &mut run, &second, inside);
    assert_eq!(Game::location(&run), inside, "it moves inside the region");
    point(&mut game, &mut run, &second, at + Point::from((60.0, 5.0)));
    assert_eq!(Game::location(&run), inside, "and not out of it");
    run.stop();
}

/// A key held while an inhibitor becomes active: the window saw only what it was sent at the
/// press, a bound key's release is kept as its press was, and Super alone released then
/// fires nothing.
#[test]
fn a_key_held_across_an_inhibitor_activating() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let surface = game.surface.clone();
    let (pressed, released) = (
        game.local.client.keys.len(),
        game.local.client.released.len(),
    );
    let fired = run.state.fired.len();
    key(&mut run.state, KEY_LEFTMETA, true);
    key(&mut run.state, KEY_C, true);
    let (inhibitor, log) = game.inhibitor(&surface);
    game.wait(&mut run, &log, &["active"], "the inhibitor is active");
    key(&mut run.state, KEY_C, false);
    key(&mut run.state, KEY_LEFTMETA, false);
    game.settle(&mut run);
    assert_eq!(
        game.local.client.keys[pressed..],
        [KEY_LEFTMETA],
        "Super reaches it"
    );
    assert_eq!(
        game.local.client.released[released..],
        [KEY_LEFTMETA],
        "and C's release does not"
    );
    assert_eq!(
        run.state.fired[fired..],
        [Action::Call(&crate::session_calls::CONTROL_CENTER)],
        "Super+C acted at its press"
    );
    inhibitor.destroy();
    game.settle(&mut run);

    let fired = run.state.fired.len();
    key(&mut run.state, KEY_LEFTMETA, true);
    let (_inhibitor, log) = game.inhibitor(&surface);
    game.wait(&mut run, &log, &["active"], "a second inhibitor is active");
    key(&mut run.state, KEY_LEFTMETA, false);
    game.settle(&mut run);
    assert_eq!(
        run.state.fired.len(),
        fired,
        "Super alone, released under the inhibitor, fires nothing"
    );
    run.stop();
}

/// A suspended client that leaves is forgotten at the next suspension, and its successor's
/// lock is held as any other.
#[test]
fn a_suspended_client_that_leaves_is_forgotten() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    chord(&mut run.state, &CYCLE);
    game.wait(&mut run, &log, &["locked", "unlocked"], "suspended");
    let gone = game.local.id.clone().expect("an id");
    drop(lock);
    drop(game);
    run.until("the client is gone", |state| {
        state
            .display_handle
            .backend_handle()
            .get_client_data(gone.clone())
            .is_err()
    });

    let mut next = Game::start(&mut run);
    let (_lock, log) = next.lock(Lifetime::Persistent);
    next.wait(&mut run, &log, &["locked"], "the next client's lock holds");
    chord(&mut run.state, &CYCLE);
    next.wait(&mut run, &log, &["locked", "unlocked"], "suspended");
    let id = next.local.id.clone().expect("an id");
    assert_eq!(
        run.state.input_hold.suspended,
        [id],
        "the client that left is forgotten"
    );
    run.stop();
}

/// A session lock and its unlock: a persistent lock returns with the keyboard; a suspension
/// outlasts them, and the lock returns only at a click.
#[test]
fn a_session_unlock_keeps_a_suspension() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    let (mut locker, session) = lock_session(&mut run);
    game.wait(
        &mut run,
        &log,
        &["locked", "unlocked"],
        "the session lock unlocks it",
    );
    session.unlock_and_destroy();
    locker.roundtrip(&mut run);
    game.wait(
        &mut run,
        &log,
        &["locked", "unlocked", "locked"],
        "the unlock gives it back",
    );

    chord(&mut run.state, &CYCLE);
    let suspended = ["locked", "unlocked", "locked", "unlocked"];
    game.wait(&mut run, &log, &suspended, "suspended");
    let (mut locker, session) = lock_session(&mut run);
    session.unlock_and_destroy();
    locker.roundtrip(&mut run);
    let server = game.server.clone();
    game.local
        .pump(&mut run, "the window has the keyboard again", |_, state| {
            state.seat.get_keyboard().and_then(|k| k.current_focus()) == Some(server.clone())
        });
    game.settle(&mut run);
    assert_eq!(
        events(&log),
        suspended,
        "the suspension outlasts the session lock"
    );
    let id = game.local.id.clone().expect("an id");
    assert!(
        run.state.input_hold.suspended.contains(&id),
        "still suspended"
    );
    game.click(&mut run);
    let mut lifted = suspended.to_vec();
    lifted.push("locked");
    game.wait(&mut run, &log, &lifted, "the click lifts it");
    run.stop();
}

/// A pen moves the pointer as any absolute device does: under a lock the pointer stays.
#[test]
fn a_pens_motion_keeps_a_lock() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_, tool, at) = super::devices::pen_at(&mut run, &mut game.local, game.at);
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    let before = Game::location(&run);
    run.state.tool_motion(
        "pen".into(),
        &tool,
        at + Point::from((30.0, 0.0)),
        super::devices::pressure(),
        InputTime::now(),
    );
    game.settle(&mut run);
    assert_eq!(Game::location(&run), before, "a locked pointer stays");
    run.stop();
}

/// A pen moves the pointer as any absolute device does: a confined pointer stays in its region.
#[test]
fn a_pens_motion_keeps_a_confinement() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let (_, tool, at) = super::devices::pen_at(&mut run, &mut game.local, game.at);
    let region = middle(&mut game.local);
    let (_confined, log) = game.confine(Some(&region), Lifetime::Persistent);
    game.wait(&mut run, &log, &["confined"], "confined");
    let inside = at + Point::from((5.0, 5.0));
    for (to, stays) in [(inside, inside), (at + Point::from((60.0, 5.0)), inside)] {
        run.state.tool_motion(
            "pen".into(),
            &tool,
            to,
            super::devices::pressure(),
            InputTime::now(),
        );
        game.settle(&mut run);
        assert_eq!(Game::location(&run), stays, "inside the region");
    }
    run.stop();
}

/// A pen put down elsewhere under a lock moves nothing for the landing: the locking client is
/// sent the small move that follows, not the jump from where the pen was lifted.
#[test]
fn a_pen_landing_under_a_lock_sends_no_jump() {
    let mut run = Running::start(None);
    let mut game = Game::start(&mut run);
    let relative: ZwpRelativePointerManagerV1 = game.local.bind(1);
    let motions = Motions::default();
    let _relative =
        relative.get_relative_pointer(&game.pointer, &game.local.queue.handle(), motions.clone());
    let (_lock, log) = game.lock(Lifetime::Persistent);
    game.wait(&mut run, &log, &["locked"], "locked");
    let (tablet, tool, at) = super::devices::pen_at(&mut run, &mut game.local, game.at);
    let before = Game::location(&run);
    let far = at + Point::from((100.0, 0.0));
    for (to, proximity) in [(at, smithay::backend::input::ProximityState::Out), (far, smithay::backend::input::ProximityState::In)] {
        run.state.tool_proximity(
            "pen".into(),
            &tablet,
            &tool,
            to,
            proximity,
            smithay::input::tablet::tool::AxisFrame::default(),
            InputTime::now(),
        );
    }
    run.state.tool_motion(
        "pen".into(),
        &tool,
        far + Point::from((3.0, 0.0)),
        super::devices::pressure(),
        InputTime::now(),
    );
    game.settle(&mut run);
    let mut sent = std::mem::take(&mut *motions.lock().expect("the motions"));
    // The sample of the lift repeats the last position: a move of nothing.
    sent.retain(|motion| *motion != [0.0; 4]);
    assert_eq!(sent, [[3.0, 0.0, 3.0, 0.0]], "only the small move");
    assert_eq!(Game::location(&run), before, "the pointer stays");
    run.stop();
}
