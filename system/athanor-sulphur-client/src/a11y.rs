//! The magnifier and the colour filters (doc_accessibility.md AX10): cosmic-comp's
//! `cosmic_a11y_manager_v1` up to version 3, bound at connect, else athanor-sulphur's
//! `athanor_a11y_manager_v1`, which it offers to the bar alone, bound at the first
//! `Client::watch_accessibility`, so a client that never watches never binds it. Every name of
//! the latter in this crate is in this file; the rest of the crate sees one interface over both.
//! The keyboard aids (AX11) are the latter's alone.

use wayland_client::globals::GlobalList;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::connection::{bind, Client, Error, State};
use crate::model::{Event as ClientEvent, KeyboardAid, KeyboardAids, ScreenFilter, MAX_AID_DELAY};
use crate::protocols::a11y::client::cosmic_a11y_manager_v1::{
    self, CosmicA11yManagerV1, Filter as CosmicFilter,
};
use crate::protocols::athanor_a11y::client::athanor_a11y_manager_v1::{
    self, Aid as AthanorAid, AthanorA11yManagerV1, Filter as AthanorFilter,
};

/// The version of `athanor_a11y_manager_v1` that has `set_reduced_motion`.
const REDUCED_MOTION: u32 = 2;

/// The version of `cosmic_a11y_manager_v1` that replaced the screen filter's messages.
const SCREEN_FILTER2: u32 = 3;

pub(crate) enum Manager {
    Athanor(AthanorA11yManagerV1),
    Cosmic(CosmicA11yManagerV1),
}

/// cosmic-comp's manager, at connect.
pub(crate) fn bind_cosmic(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<Manager> {
    bind(globals, qh, 2..=SCREEN_FILTER2).map(Manager::Cosmic)
}

impl Client {
    /// Binds athanor-sulphur's manager, so that `accessibility()` and its setters reach it.
    /// Nothing to do under cosmic-comp, whose manager is bound at connect, nor where a manager
    /// is already bound.
    pub fn watch_accessibility(&self) -> Result<(), Error> {
        self.live()?;
        {
            let mut state = self.inner.state.borrow_mut();
            if state.globals.a11y.is_some() {
                return Ok(());
            }
            state.globals.a11y =
                bind(&self.inner.registry, &self.inner.qh, 1..=2).map(Manager::Athanor);
        }
        self.flush()
    }
}

fn check_delays(aids: &KeyboardAids) -> Result<(), Error> {
    if aids.slow_delay.max(aids.bounce_delay) > MAX_AID_DELAY {
        return Err(Error::InvalidArgument(
            "a keyboard aid's delay above 2000 ms",
        ));
    }
    Ok(())
}

/// Whether a manager bound at `version` has `set_reduced_motion`.
fn has_reduced_motion(version: u32) -> bool {
    version >= REDUCED_MOTION
}

impl Manager {
    /// Sends every setting of the keyboard aids. The compositor ends a connection that sends a
    /// delay above `MAX_AID_DELAY`, so this refuses it first.
    pub(crate) fn set_keyboard_aids(&self, aids: KeyboardAids) -> Result<(), Error> {
        check_delays(&aids)?;
        let Manager::Athanor(manager) = self else {
            return Err(Error::Unavailable("athanor_a11y_manager_v1"));
        };
        manager.set_sticky_keys(athanor_state(aids.sticky), athanor_state(aids.two_key_off));
        manager.set_slow_keys(athanor_state(aids.slow), aids.slow_delay);
        manager.set_bounce_keys(athanor_state(aids.bounce), aids.bounce_delay);
        manager.set_aid_gestures(athanor_state(aids.gestures));
        Ok(())
    }

    /// Sends the user's reduced-motion setting (AX9). Version 2 of athanor-sulphur's manager
    /// has the request: a version 1 compositor, and cosmic-comp, are refused here, since a
    /// request above the bound version ends the connection.
    pub(crate) fn set_reduced_motion(&self, reduced: bool) -> Result<(), Error> {
        match self {
            Manager::Athanor(manager) if has_reduced_motion(manager.version()) => {
                manager.set_reduced_motion(athanor_state(reduced));
                Ok(())
            }
            _ => Err(Error::Unavailable("athanor_a11y_manager_v1 version 2")),
        }
    }

    pub(crate) fn set_magnifier(&self, enabled: bool) {
        match self {
            Manager::Athanor(manager) => manager.set_magnifier(athanor_state(enabled)),
            Manager::Cosmic(manager) => manager.set_magnifier(cosmic_state(enabled)),
        }
    }

    /// `filter` is never `Unknown`: the caller refuses it.
    pub(crate) fn set_screen_filter(&self, inverted: bool, filter: ScreenFilter) {
        match self {
            Manager::Athanor(manager) => {
                let filter = match filter {
                    ScreenFilter::None => AthanorFilter::None,
                    ScreenFilter::Greyscale => AthanorFilter::Greyscale,
                    ScreenFilter::Protanopia => AthanorFilter::Protanopia,
                    ScreenFilter::Deuteranopia => AthanorFilter::Deuteranopia,
                    ScreenFilter::Tritanopia => AthanorFilter::Tritanopia,
                    ScreenFilter::Unknown => AthanorFilter::Unknown,
                };
                manager.set_screen_filter(athanor_state(inverted), filter);
            }
            Manager::Cosmic(manager) => {
                let selected = match filter {
                    ScreenFilter::None => None,
                    ScreenFilter::Greyscale => Some(CosmicFilter::Greyscale),
                    ScreenFilter::Protanopia => Some(CosmicFilter::DaltonizeProtanopia),
                    ScreenFilter::Deuteranopia => Some(CosmicFilter::DaltonizeDeuteranopia),
                    ScreenFilter::Tritanopia => Some(CosmicFilter::DaltonizeTritanopia),
                    ScreenFilter::Unknown => Some(CosmicFilter::Unknown),
                };
                if manager.version() >= SCREEN_FILTER2 {
                    // Version 3 refuses `disabled`: no filter is the selected one turned off,
                    // and `unknown` keeps the selection.
                    manager.set_screen_filter2(
                        cosmic_state(inverted),
                        selected.unwrap_or(CosmicFilter::Unknown),
                        cosmic_state(selected.is_some()),
                    );
                } else {
                    manager.set_screen_filter(
                        cosmic_state(inverted),
                        selected.unwrap_or(CosmicFilter::Disabled),
                    );
                }
            }
        }
    }
}

fn athanor_state(enabled: bool) -> athanor_a11y_manager_v1::ActiveState {
    if enabled {
        athanor_a11y_manager_v1::ActiveState::Enabled
    } else {
        athanor_a11y_manager_v1::ActiveState::Disabled
    }
}

fn cosmic_state(enabled: bool) -> cosmic_a11y_manager_v1::ActiveState {
    if enabled {
        cosmic_a11y_manager_v1::ActiveState::Enabled
    } else {
        cosmic_a11y_manager_v1::ActiveState::Disabled
    }
}

fn keyboard_aid(aid: WEnum<AthanorAid>) -> Option<KeyboardAid> {
    match aid {
        WEnum::Value(AthanorAid::StickyKeys) => Some(KeyboardAid::Sticky),
        WEnum::Value(AthanorAid::SlowKeys) => Some(KeyboardAid::Slow),
        WEnum::Value(AthanorAid::BounceKeys) => Some(KeyboardAid::Bounce),
        WEnum::Unknown(_) => None,
    }
}

fn cosmic_filter(filter: WEnum<CosmicFilter>) -> ScreenFilter {
    match filter {
        WEnum::Value(CosmicFilter::Disabled) => ScreenFilter::None,
        WEnum::Value(CosmicFilter::Greyscale) => ScreenFilter::Greyscale,
        WEnum::Value(CosmicFilter::DaltonizeProtanopia) => ScreenFilter::Protanopia,
        WEnum::Value(CosmicFilter::DaltonizeDeuteranopia) => ScreenFilter::Deuteranopia,
        WEnum::Value(CosmicFilter::DaltonizeTritanopia) => ScreenFilter::Tritanopia,
        WEnum::Value(CosmicFilter::Unknown) | WEnum::Unknown(_) => ScreenFilter::Unknown,
    }
}

impl Dispatch<CosmicA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &CosmicA11yManagerV1,
        event: cosmic_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let enabled = |value: WEnum<cosmic_a11y_manager_v1::ActiveState>| {
            value == WEnum::Value(cosmic_a11y_manager_v1::ActiveState::Enabled)
        };
        let mut next = state.accessibility;
        match event {
            cosmic_a11y_manager_v1::Event::Magnifier { active } => next.magnifier = enabled(active),
            cosmic_a11y_manager_v1::Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = cosmic_filter(filter);
            }
            cosmic_a11y_manager_v1::Event::ScreenFilter2 {
                inverted,
                filter,
                filter_state,
            } => {
                next.inverted = enabled(inverted);
                next.filter = if enabled(filter_state) {
                    cosmic_filter(filter)
                } else {
                    ScreenFilter::None
                };
            }
        }
        state.set_accessibility(next);
    }
}

impl Dispatch<AthanorA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &AthanorA11yManagerV1,
        event: athanor_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_a11y_manager_v1::{ActiveState, Event};
        let enabled = |value: WEnum<ActiveState>| value == WEnum::Value(ActiveState::Enabled);
        let mut next = state.accessibility;
        match event {
            Event::Magnifier { active, .. } => next.magnifier = enabled(active),
            Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = match filter {
                    WEnum::Value(AthanorFilter::None) => ScreenFilter::None,
                    WEnum::Value(AthanorFilter::Greyscale) => ScreenFilter::Greyscale,
                    WEnum::Value(AthanorFilter::Protanopia) => ScreenFilter::Protanopia,
                    WEnum::Value(AthanorFilter::Deuteranopia) => ScreenFilter::Deuteranopia,
                    WEnum::Value(AthanorFilter::Tritanopia) => ScreenFilter::Tritanopia,
                    WEnum::Value(AthanorFilter::Unknown) | WEnum::Unknown(_) => {
                        ScreenFilter::Unknown
                    }
                };
            }
            Event::Modifiers { latched, locked } => {
                next.latched = latched.into();
                next.locked = locked.into();
            }
            Event::KeyRejected { aid } => {
                state
                    .events
                    .extend(keyboard_aid(aid).map(ClientEvent::AidRejected));
            }
            Event::Gesture { aid } => {
                state
                    .events
                    .extend(keyboard_aid(aid).map(ClientEvent::AidGesture));
            }
            Event::AidOff { aid } => {
                state
                    .events
                    .extend(keyboard_aid(aid).map(ClientEvent::AidOff));
            }
        }
        state.set_accessibility(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use wayland_backend::protocol::Message;
    use wayland_backend::server::{
        Backend as ServerBackend, ClientData, ClientId, GlobalHandler, GlobalId,
        Handle as ServerHandle, ObjectData, ObjectId,
    };
    use wayland_client::globals::registry_queue_init;

    use crate::protocols::athanor_a11y::client::__interfaces::ATHANOR_A11Y_MANAGER_V1_INTERFACE;

    #[test]
    fn a_delay_above_the_bound_never_reaches_the_compositor() {
        let aids = |slow_delay, bounce_delay| KeyboardAids {
            slow_delay,
            bounce_delay,
            ..KeyboardAids::default()
        };
        assert!(check_delays(&aids(MAX_AID_DELAY, MAX_AID_DELAY)).is_ok());
        assert!(check_delays(&aids(MAX_AID_DELAY + 1, 0)).is_err());
        assert!(check_delays(&aids(0, MAX_AID_DELAY + 1)).is_err());
    }

    /// A server that offers `athanor_a11y_manager_v1` at `version` and records the opcode of
    /// every request the client sends to the objects it binds.
    struct Offer {
        seen: Arc<Mutex<Vec<u16>>>,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    struct Recorder(Arc<Mutex<Vec<u16>>>);

    impl ObjectData<()> for Recorder {
        fn request(
            self: Arc<Self>,
            _: &ServerHandle,
            _: &mut (),
            _: ClientId,
            msg: Message<ObjectId, OwnedFd>,
        ) -> Option<Arc<dyn ObjectData<()>>> {
            self.0.lock().unwrap().push(msg.opcode);
            None
        }

        fn destroyed(self: Arc<Self>, _: &ServerHandle, _: &mut (), _: ClientId, _: ObjectId) {}
    }

    struct Global(Arc<Mutex<Vec<u16>>>);

    impl GlobalHandler<()> for Global {
        fn bind(
            self: Arc<Self>,
            _: &ServerHandle,
            _: &mut (),
            _: ClientId,
            _: GlobalId,
            _: ObjectId,
        ) -> Arc<dyn ObjectData<()>> {
            Arc::new(Recorder(self.0.clone()))
        }
    }

    impl ClientData for Offer {}

    impl Offer {
        fn start(version: u32) -> (Offer, UnixStream) {
            let (client, server) = UnixStream::pair().unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (recorded, stopped) = (seen.clone(), stop.clone());
            let thread = std::thread::spawn(move || {
                let backend = ServerBackend::<()>::new().unwrap();
                let handle = backend.handle();
                handle.create_global::<()>(
                    &ATHANOR_A11Y_MANAGER_V1_INTERFACE,
                    version,
                    Arc::new(Global(recorded.clone())),
                );
                handle.insert_client(server, Arc::new(Dummy)).unwrap();
                while !stopped.load(Ordering::Relaxed) {
                    backend.dispatch_all_clients(&mut ()).unwrap();
                    backend.flush(None).unwrap();
                    std::thread::sleep(Duration::from_millis(2));
                }
            });
            (Offer { seen, stop, thread: Some(thread) }, client)
        }

        fn opcodes(&self) -> Vec<u16> {
            self.seen.lock().unwrap().clone()
        }
    }

    struct Dummy;

    impl ClientData for Dummy {}

    impl Drop for Offer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    /// Binds the manager at exactly `version` of a server that offers it, then calls
    /// `set_reduced_motion`, and returns the result with the requests the server received.
    fn reduced_motion_at(version: u32) -> (Result<(), Error>, Vec<u16>) {
        let (offer, socket) = Offer::start(version);
        let connection = Connection::from_socket(socket).unwrap();
        let (globals, mut queue) = registry_queue_init::<State>(&connection).unwrap();
        let manager: AthanorA11yManagerV1 =
            bind(&globals, &queue.handle(), version..=version).expect("the manager is offered");
        assert_eq!(manager.version(), version);
        let result = Manager::Athanor(manager).set_reduced_motion(true);
        connection.flush().unwrap();
        queue.roundtrip(&mut State::default()).unwrap();
        (result, offer.opcodes())
    }

    /// A request above the bound version ends the connection, so a manager bound at version 1
    /// gets the refusal and nothing reaches the wire.
    #[test]
    fn a_version_1_manager_refuses_the_reduced_motion_request() {
        let (result, requests) = reduced_motion_at(1);
        assert!(matches!(result, Err(Error::Unavailable(_))), "{result:?}");
        assert_eq!(requests, Vec::<u16>::new());
    }

    #[test]
    fn a_version_2_manager_sends_the_reduced_motion_request() {
        let (result, requests) = reduced_motion_at(2);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(requests.len(), 1);
    }

    #[test]
    fn the_aids_are_named_as_the_protocol_numbers_them() {
        assert_eq!(keyboard_aid(WEnum::Unknown(3)), None);
        for (value, aid) in [
            (AthanorAid::StickyKeys, KeyboardAid::Sticky),
            (AthanorAid::SlowKeys, KeyboardAid::Slow),
            (AthanorAid::BounceKeys, KeyboardAid::Bounce),
        ] {
            assert_eq!(keyboard_aid(WEnum::Value(value)), Some(aid));
        }
    }
}
