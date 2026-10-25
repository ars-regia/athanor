//! ext-session-lock-v1 (doc_own_stack.md, C1 task 4): while the session is locked the outputs
//! show only the lock surfaces, the keyboard goes only to a lock surface, and no window can be
//! captured (spike Q7). Locking ends any keyboard grab, an input method's included, and takes
//! the text inputs' focus away (OS9 item 3, `input_method.rs`). The lock is confirmed once every
//! output has been drawn locked.

use crate::state::{Backend, State};
use smithay::backend::input::InputTime;
use smithay::input::keyboard::KeyboardSource;
use smithay::input::pointer::MotionEvent;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1;
use smithay::reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::session_lock::{
    LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A locked session: the client's lock, its confirmation until the outputs have been drawn
/// locked, and the lock surface of each output.
pub struct Lock {
    lock: ExtSessionLockV1,
    locker: Option<SessionLocker>,
    surfaces: Vec<(Output, LockSurface)>,
}

impl Lock {
    /// The lock surface shown on `output`, if the locker has given it one.
    pub fn surface_on(&self, output: &Output) -> Option<&WlSurface> {
        self.surfaces
            .iter()
            .find(|(on, surface)| on == output && surface.alive())
            .map(|(_, surface)| surface.wl_surface())
    }
}

impl Lock {
    /// Each lock surface takes the size of its output, after the outputs were laid out again.
    pub fn resize(&self, space: &smithay::desktop::Space<smithay::desktop::Window>) {
        for (output, surface) in self.surfaces.iter().filter(|(_, surface)| surface.alive()) {
            let Some(area) = space.output_geometry(output) else {
                continue;
            };
            let size = (area.size.w.max(1) as u32, area.size.h.max(1) as u32);
            surface.with_pending_state(|state| state.size = Some(size.into()));
            surface.send_configure();
        }
    }
}

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.lock_state
    }

    /// Locks the session, unless a live lock holds it: dropping the locker answers `finished`.
    /// A lock whose client has gone leaves the session locked, and a new lock takes it over, as
    /// the protocol allows.
    fn lock(&mut self, locker: SessionLocker) {
        if self.lock.as_ref().is_some_and(|lock| lock.lock.is_alive()) {
            return;
        }
        // Capture stops here: the sessions are dropped, which stops them, and no new one starts
        // while locked (`capture_constraints`).
        self.capture_sessions.clear();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.unset_grab(self);
        }
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), InputTime::now());
        }
        self.lock = Some(Lock {
            lock: locker.ext_session_lock().clone(),
            locker: Some(locker),
            surfaces: Vec::new(),
        });
        self.text_input_focus(None);
        self.set_keyboard_focus(None);
        self.refresh_pointer();
        self.lock_taken();
    }

    fn unlock(&mut self) {
        self.lock = None;
        self.focus_top();
        self.refresh_pointer();
    }

    /// Sized to the output, before Smithay sends the first configure; the first lock surface
    /// takes the keyboard.
    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        // Only the surfaces of the lock in force: a refused lock's are never shown.
        let Some(lock) = self
            .lock
            .as_mut()
            .filter(|lock| surface.ext_session_lock() == &lock.lock)
        else {
            return;
        };
        if let Some(area) = self.space.output_geometry(&output) {
            let size = (area.size.w.max(1) as u32, area.size.h.max(1) as u32);
            surface.with_pending_state(|state| state.size = Some(size.into()));
        }
        lock.surfaces.push((output, surface));
        self.set_keyboard_focus(None);
        self.refresh_pointer();
    }
}

impl State {
    /// Whether a virtual keyboard's key presses and a virtual pointer's motion, button presses
    /// and scrolling reach no client: while the session is locked (open question 12 of the OS9
    /// plan, and the maintainer's decision of 2026-10-23 for the pointer), so that no script
    /// types into the lock surface, probes its password or clicks on it; and while the session
    /// is paused on another virtual terminal, where the user cannot see what it would do, nor a
    /// virtual Ctrl+Alt+F`n` switch the terminal (OS9 task 6 audit). A release of what the seat
    /// holds still goes on, and the hardware's input is unaffected.
    pub fn drops_virtual_input(&self) -> bool {
        self.lock.is_some() || self.session_paused()
    }

    /// Whether a key pressed on `source` reaches no client: a virtual keyboard's while
    /// `drops_virtual_input`.
    pub fn drops_key(&self, source: KeyboardSource) -> bool {
        source != KeyboardSource::Physical && self.drops_virtual_input()
    }

    /// Gives the keyboard to `surface`; while the session is locked, to the first lock surface
    /// whatever `surface` is. Every change of the keyboard focus but a popup grab's goes
    /// through here, and `grab` refuses grabs while locked.
    pub fn set_keyboard_focus(&mut self, surface: Option<WlSurface>) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let surface = match &self.lock {
            Some(lock) => lock
                .surfaces
                .iter()
                .find(|(_, surface)| surface.alive())
                .map(|(_, surface)| surface.wl_surface().clone()),
            None => surface,
        };
        keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
    }

    /// Gives the pointer to what is under it now. Locking and unlocking change that without a
    /// motion, and a button goes to the pointer's focus: without this, a click on the lock would
    /// reach the window under it.
    pub(crate) fn refresh_pointer(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = pointer.current_location();
        let under = self.surface_under(location);
        let motion = MotionEvent {
            location,
            serial: SERIAL_COUNTER.next_serial(),
            time: InputTime::now(),
        };
        pointer.motion(self, under, &motion);
        pointer.frame(self);
    }

    /// After a frame of every output: a lock waiting for its confirmation has been drawn, and
    /// is confirmed.
    pub fn locked_drawn(&mut self) {
        if let Some(locker) = self.lock.as_mut().and_then(|lock| lock.locker.take()) {
            locker.lock();
        }
    }
}

/// When the request to logind in flight started: a press meanwhile escapes locally and asks no
/// more. The method timeout bounds the call but not the connection or its handshake, so a
/// request older than twice `LOCK_TIMEOUT` counts as gone.
static REQUEST_STARTED: Mutex<Option<Instant>> = Mutex::new(None);

/// Marks a request as started at `now`, unless one is in flight; whether it was.
fn begin_request(slot: &Mutex<Option<Instant>>, now: Instant) -> bool {
    let mut started = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if started.is_some_and(|at| now.duration_since(at) < LOCK_TIMEOUT * 2) {
        return false;
    }
    *started = Some(now);
    true
}

/// Ends the request started at `now`, unless a newer one replaced it.
fn end_request(slot: &Mutex<Option<Instant>>, now: Instant) {
    let mut started = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if *started == Some(now) {
        *started = None;
    }
}

/// How long logind has to answer `Lock`.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

impl State {
    /// Super+Escape: the keyboard leaves an exclusive surface at the press (`escape_exclusive`),
    /// whether or not a lock client ever takes the lock, and the user asks to lock the session.
    /// On a console the compositor asks logind to lock its session, as `loginctl lock-session`
    /// does, and the session's lock client reacts to logind's `Lock` signal and nothing else
    /// (doc_lock_and_prompts.md LP4); the lock itself reaches `SessionLockHandler::lock`.
    /// Nested or headless there is no logind session of the compositor's own, and the request is
    /// only counted. A lock the user asks for is never refused.
    pub fn request_lock(&mut self) {
        self.lock_requests += 1;
        self.escape_exclusive();
        if !matches!(self.backend, Backend::Drm(_)) {
            tracing::info!("a lock was asked for; only a console session has a logind to ask");
            return;
        }
        let now = Instant::now();
        if !begin_request(&REQUEST_STARTED, now) {
            tracing::debug!("a lock request is in flight; not asked again");
            return;
        }
        // On its own thread: the call waits for logind, and the loop draws meanwhile.
        let spawned = std::thread::Builder::new()
            .name("lock-request".into())
            .spawn(move || {
                if let Err(error) = lock_session() {
                    tracing::error!(%error, "logind did not lock the session");
                }
                end_request(&REQUEST_STARTED, now);
            });
        if let Err(error) = spawned {
            end_request(&REQUEST_STARTED, now);
            tracing::error!(%error, "the lock request could not be started");
        }
    }
}

/// `org.freedesktop.login1.Session.Lock` on the compositor's own session (`session/auto` is the
/// session of the caller), which is what `loginctl lock-session` calls.
fn lock_session() -> Result<(), zbus::Error> {
    zbus::blocking::connection::Builder::system()?
        .method_timeout(LOCK_TIMEOUT)
        .build()?
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "Lock",
            &(),
        )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_that_never_ends_is_given_up_after_twice_the_timeout() {
        let slot = Mutex::new(None);
        let t0 = Instant::now();
        assert!(begin_request(&slot, t0));
        assert!(!begin_request(&slot, t0 + LOCK_TIMEOUT));
        let later = t0 + LOCK_TIMEOUT * 2;
        assert!(begin_request(&slot, later));
        end_request(&slot, t0);
        assert!(
            !begin_request(&slot, later),
            "the old request does not end the new one"
        );
        end_request(&slot, later);
        assert!(begin_request(&slot, later));
    }
}
