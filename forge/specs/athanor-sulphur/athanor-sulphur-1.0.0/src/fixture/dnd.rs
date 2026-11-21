//! Fixtures of drag and drop between clients and of the drag icon (doc_own_stack.md decision
//! 40, `dnd.rs`). Decision 37's toplevel drag, the last case the decision lists, has no fixture:
//! the compositor offers no `xdg_toplevel_drag_manager_v1` to drag a toplevel with.

use super::devices::show_lock_surface;
use super::marks::{at, frame, BLACK, WHITE};
use super::touch::{down, motion, park_the_fixture, up};
use super::*;
use std::io::Read;
use wayland_client::protocol::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source,
};

/// The type a source offers, and the text it holds.
const TEXT: &str = "text/plain;charset=utf-8";
const DATA: &str = "dropped";
/// Where the tests drag an icon: on the first output, clear of every window.
const BARE: (f64, f64) = (700.0, 500.0);

/// What a client's data device heard of drags.
#[derive(Default)]
struct Heard {
    /// Each enter: the surface entered, the offer, and the enter's serial.
    entered: Vec<(ObjectId, Option<wl_data_offer::WlDataOffer>, u32)>,
    left: usize,
    dropped: usize,
}

#[derive(Clone, Default)]
struct DragDevice(Arc<Mutex<Heard>>);

impl DragDevice {
    fn heard(&self) -> std::sync::MutexGuard<'_, Heard> {
        self.0.lock().expect("heard")
    }
}

impl Dispatch<wl_data_device::WlDataDevice, DragDevice> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        device: &DragDevice,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut heard = device.heard();
        match event {
            wl_data_device::Event::Enter {
                serial,
                surface,
                id,
                ..
            } => heard.entered.push((surface.id(), id, serial)),
            wl_data_device::Event::Leave => heard.left += 1,
            wl_data_device::Event::Drop => heard.dropped += 1,
            _ => {}
        }
    }

    event_created_child!(Client, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, Offered),
    ]);
}

/// The user data of an offer a drag made; its events are not read.
struct Offered;

impl Dispatch<wl_data_offer::WlDataOffer, Offered> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        _: wl_data_offer::Event,
        _: &Offered,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// What a drag's source was sent.
#[derive(Default)]
struct Sourced {
    /// The type of each send, answered with `DATA`.
    sent: Vec<String>,
    cancelled: bool,
    performed: bool,
    finished: bool,
    action: Option<wl_data_device_manager::DndAction>,
}

#[derive(Clone, Default)]
struct DragSource(Arc<Mutex<Sourced>>);

impl DragSource {
    fn sourced(&self) -> std::sync::MutexGuard<'_, Sourced> {
        self.0.lock().expect("sourced")
    }
}

impl Dispatch<wl_data_source::WlDataSource, DragSource> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        source: &DragSource,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut sourced = source.sourced();
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                sourced.sent.push(mime_type);
                let mut pipe = std::fs::File::from(fd);
                // The reader holds the other end open; a short text fits the pipe whole.
                pipe.write_all(DATA.as_bytes()).expect("the source writes");
            }
            wl_data_source::Event::Cancelled => sourced.cancelled = true,
            wl_data_source::Event::DndDropPerformed => sourced.performed = true,
            wl_data_source::Event::DndFinished => sourced.finished = true,
            wl_data_source::Event::Action {
                dnd_action: WEnum::Value(action),
            } => sourced.action = Some(action),
            _ => {}
        }
    }
}

/// A client with a window and the seat's data device.
struct Party {
    local: Local,
    id: ClientId,
    surface: wl_surface::WlSurface,
    manager: wl_data_device_manager::WlDataDeviceManager,
    device: wl_data_device::WlDataDevice,
    heard: DragDevice,
}

/// A new client, inside `context` if any, with its window at `at`.
fn party(run: &mut Running, at: (i32, i32), context: Option<SecurityContext>) -> Party {
    let mut local = Local::connect(run, context);
    local.toplevel(run);
    let id = local.id.clone().expect("an id");
    run.state.space.map_element(window_of(run, &id), at, false);
    let seat = local.seat.clone().expect("a seat");
    let manager: wl_data_device_manager::WlDataDeviceManager = local.bind(3);
    let heard = DragDevice::default();
    let device = manager.get_data_device(&seat, &local.queue.handle(), heard.clone());
    local.roundtrip(run);
    let (surface, _) = local.window.clone().expect("a window");
    Party {
        local,
        id,
        surface,
        manager,
        device,
        heard,
    }
}

impl Party {
    /// A source of `TEXT` that allows a copy or a move.
    fn source(&mut self) -> (wl_data_source::WlDataSource, DragSource) {
        let log = DragSource::default();
        let source = self
            .manager
            .create_data_source(&self.local.queue.handle(), log.clone());
        source.offer(TEXT.into());
        source.set_actions(
            wl_data_device_manager::DndAction::Copy | wl_data_device_manager::DndAction::Move,
        );
        (source, log)
    }

    /// A drag of a new source from the client's window, with `serial` and `icon`.
    fn drag(
        &mut self,
        run: &mut Running,
        serial: u32,
        icon: Option<&wl_surface::WlSurface>,
    ) -> (wl_data_source::WlDataSource, DragSource) {
        let (source, log) = self.source();
        self.device
            .start_drag(Some(&source), &self.surface, icon, serial);
        self.local.roundtrip(run);
        (source, log)
    }

    /// Presses the left button on the client's window: the press's serial.
    fn press(&mut self, run: &mut Running) -> u32 {
        let at = location_of(run, &self.id).to_f64() + Point::from((20.0, 20.0));
        self.local.client.button = None;
        press(run, at);
        self.local
            .pump(run, "the press", |client, _| client.button.is_some());
        self.local.client.button.expect("a press")
    }

    /// Waits for the client to hear its `n`th enter: the surface and the offer.
    fn entered(
        &mut self,
        run: &mut Running,
        n: usize,
    ) -> (ObjectId, wl_data_offer::WlDataOffer, u32) {
        let heard = self.heard.clone();
        self.local.pump(run, "an enter", move |_, _| {
            heard.heard().entered.len() >= n
        });
        let heard = self.heard.heard();
        let (surface, offer, serial) = heard.entered.get(n - 1).cloned().expect("an enter");
        (surface, offer.expect("an offer"), serial)
    }

    /// Takes the offer of `TEXT`, by a copy.
    fn accept(&mut self, run: &mut Running, offer: &wl_data_offer::WlDataOffer, serial: u32) {
        let copy = wl_data_device_manager::DndAction::Copy;
        offer.accept(serial, Some(TEXT.into()));
        offer.set_actions(copy, copy);
        self.local.roundtrip(run);
    }

    /// An icon surface of `width` by `height` red pixels, not yet given a role.
    fn icon(&mut self, run: &mut Running) -> wl_surface::WlSurface {
        let compositor: wl_compositor::WlCompositor = self.local.bind(4);
        let icon = compositor.create_surface(&self.local.queue.handle(), ());
        self.local.roundtrip(run);
        icon
    }

    /// Draws `width` by `height` red pixels on `icon`.
    fn paint(&mut self, run: &mut Running, icon: &wl_surface::WlSurface, side: i32) {
        let shm: wl_shm::WlShm = self.local.bind(1);
        let (buffer, _) = self.local.buffer(&shm, side, side, [0, 0, 255, 255]);
        icon.attach(Some(&buffer), 0, 0);
        icon.damage(0, 0, side, side);
        icon.commit();
        self.local.roundtrip(run);
    }
}

/// Reads what `offer` holds as `TEXT`, after a roundtrip of the reader and of the source's
/// client, so that a send the compositor passed on has been answered before the read.
fn read(
    run: &mut Running,
    reader: &mut Party,
    offer: &wl_data_offer::WlDataOffer,
    source: &mut Party,
) -> String {
    let (mut from, to) = std::io::pipe().expect("pipe");
    offer.receive(TEXT.into(), to.as_fd());
    drop(to);
    reader.local.roundtrip(run);
    source.local.roundtrip(run);
    let mut text = String::new();
    from.read_to_string(&mut text).expect("the data");
    text
}

/// The offer reaches the surface under the pointer, as it moves, and no other surface.
#[test]
fn an_offer_reaches_the_surface_under_the_pointer_and_no_other() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut first = party(&mut run, (500, 100), None);
    let mut second = party(&mut run, (900, 100), None);
    let serial = from.press(&mut run);
    let (_source, _) = from.drag(&mut run, serial, None);

    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    let (surface, _, _) = first.entered(&mut run, 1);
    assert_eq!(surface, first.surface.id(), "the surface under the pointer");
    second.local.roundtrip(&mut run);
    assert!(second.heard.heard().entered.is_empty(), "no other surface");

    run.state
        .pointer_to((950.0, 150.0).into(), InputTime::now());
    let (surface, _, _) = second.entered(&mut run, 1);
    assert_eq!(
        surface,
        second.surface.id(),
        "the surface the pointer moved to"
    );
    first.local.roundtrip(&mut run);
    let heard = first.heard.heard();
    assert_eq!(
        (heard.entered.len(), heard.left),
        (1, 1),
        "the surface left"
    );
    drop(heard);
    release(&mut run);
    run.stop();
}

/// The data passes only at the drop: a read while the drag hovers gets nothing and asks the
/// source for nothing; after the drop it gets the source's data, with the action negotiated.
#[test]
fn the_data_is_read_at_the_drop() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut to = party(&mut run, (500, 100), None);
    let serial = from.press(&mut run);
    let (_source, log) = from.drag(&mut run, serial, None);
    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    let (_, offer, serial) = to.entered(&mut run, 1);
    to.accept(&mut run, &offer, serial);

    assert_eq!(
        read(&mut run, &mut to, &offer, &mut from),
        "",
        "before the drop"
    );
    assert!(
        log.sourced().sent.is_empty(),
        "the source was asked before the drop"
    );

    release(&mut run);
    let heard = to.heard.clone();
    to.local
        .pump(&mut run, "the drop", move |_, _| heard.heard().dropped == 1);
    from.local.roundtrip(&mut run);
    assert!(log.sourced().performed, "the source is told of the drop");
    assert_eq!(
        log.sourced().action,
        Some(wl_data_device_manager::DndAction::Copy),
        "the action both sides allow, the target's preferred"
    );
    assert_eq!(
        read(&mut run, &mut to, &offer, &mut from),
        DATA,
        "after the drop"
    );
    offer.finish();
    to.local.roundtrip(&mut run);
    from.local.roundtrip(&mut run);
    assert!(
        log.sourced().finished,
        "the source is told the drop is done"
    );
    run.stop();
}

/// A drag starts only with the serial of the client's own press: a serial no press sent, or the
/// press of another client, is refused, and the pointer then offers nothing.
#[test]
fn a_drag_needs_the_clients_own_press() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut to = party(&mut run, (500, 100), None);
    let over = |run: &mut Running, x: f64| {
        run.state.pointer_to((x, 150.0).into(), InputTime::now());
    };

    let (_source, _) = from.drag(&mut run, SERIAL_COUNTER.next_serial().into(), None);
    over(&mut run, 550.0);
    to.local.roundtrip(&mut run);
    assert!(
        to.heard.heard().entered.is_empty(),
        "a serial no press sent"
    );

    let serial = from.press(&mut run);
    run.state.grab_serials.button = Some((serial.into(), to.id.clone()));
    let (_source, log) = from.drag(&mut run, serial, None);
    assert!(
        log.sourced().cancelled,
        "a press sent to another client is refused"
    );
    over(&mut run, 560.0);
    to.local.roundtrip(&mut run);
    assert!(
        to.heard.heard().entered.is_empty(),
        "another client's press"
    );

    run.state.grab_serials.button = Some((serial.into(), from.id.clone()));
    let (_source, log) = from.drag(&mut run, serial, None);
    over(&mut run, 570.0);
    let (surface, _, _) = to.entered(&mut run, 1);
    assert_eq!(
        surface,
        to.surface.id(),
        "the client's own press starts the drag"
    );
    assert!(!log.sourced().cancelled);
    release(&mut run);
    run.stop();
}

/// A drag starts from a touch point the client holds; the offer and the icon follow the finger,
/// and the drop comes with its lifting.
#[test]
fn a_drag_follows_a_touch() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut to = party(&mut run, (500, 100), None);
    let seat = from.local.seat.clone().expect("a seat");
    let _touch = seat.get_touch(&from.local.queue.handle(), ());
    from.local.roundtrip(&mut run);
    down(&mut run, 0, (120.0, 120.0).into());
    from.local.pump(&mut run, "the touch", |client, _| {
        client.touch.downs.len() == 1
    });
    let serial = from.local.client.touch.downs[0].3;
    let icon = from.icon(&mut run);
    let (_source, log) = from.drag(&mut run, serial, Some(&icon));
    from.paint(&mut run, &icon, 32);

    motion(&mut run, 0, BARE.into());
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 710, 510), RED, "the icon at the finger");

    motion(&mut run, 0, (550.0, 150.0).into());
    let (surface, offer, serial) = to.entered(&mut run, 1);
    assert_eq!(surface, to.surface.id(), "the surface under the finger");
    to.accept(&mut run, &offer, serial);
    up(&mut run, 0);
    let heard = to.heard.clone();
    to.local
        .pump(&mut run, "the drop", move |_, _| heard.heard().dropped == 1);
    from.local.roundtrip(&mut run);
    assert!(log.sourced().performed, "the drop at the lifting");
    assert_eq!(read(&mut run, &mut to, &offer, &mut from), DATA);
    run.stop();
}

/// A lock request cancels the drag before the lock surface shows, and tells the source and the
/// target; while locked, no drag starts, even from a press the client is recorded as sent.
#[test]
fn a_lock_request_cancels_the_drag() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut to = party(&mut run, (500, 100), None);
    let serial = from.press(&mut run);
    let (_source, log) = from.drag(&mut run, serial, None);
    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    to.entered(&mut run, 1);

    let mut locker = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let output: wl_output::WlOutput = locker.bind(4);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let qh = locker.queue.handle();
    let lock = locks.lock(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    locker.roundtrip(&mut run);
    from.local.roundtrip(&mut run);
    to.local.roundtrip(&mut run);
    assert!(log.sourced().cancelled, "the source is told");
    assert_eq!(to.heard.heard().left, 1, "the target is told");
    assert!(!locker.client.locked, "before the lock surface shows");

    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    release(&mut run);
    press(&mut run, (600.0, 400.0).into());
    let pressed = run.state.grab_serials.press.expect("a press");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(pointer.has_grab(pressed), "the press holds the pointer");
    run.state.grab_serials.button = Some((pressed, from.id.clone()));
    let (_source, log) = from.drag(&mut run, pressed.into(), None);
    assert!(log.sourced().cancelled, "no drag starts while locked");
    release(&mut run);
    run.stop();
}

/// An icon at most 256 pixels a side is drawn at the pointer; a larger one is not drawn, and
/// the drag goes on without it. An unconfined client's icon has no border.
#[test]
fn an_icon_larger_than_the_limit_is_not_drawn() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let mut to = party(&mut run, (500, 100), None);
    let serial = from.press(&mut run);
    let icon = from.icon(&mut run);
    let (_source, _) = from.drag(&mut run, serial, Some(&icon));
    from.paint(&mut run, &icon, 256);
    run.state.pointer_to(BARE.into(), InputTime::now());
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 710, 510), RED, "an icon at the limit");
    assert_eq!(at(&shown, 955, 755), RED, "its far corner");
    assert!(background(&at(&shown, 698, 530)), "no border");

    from.paint(&mut run, &icon, 257);
    let shown = frame(&mut run);
    assert!(background(&at(&shown, 710, 510)), "an icon past the limit");

    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    let (surface, _, _) = to.entered(&mut run, 1);
    assert_eq!(surface, to.surface.id(), "the drag goes on");
    release(&mut run);
    run.stop();
}

/// A confined client's icon carries decision 31's three lines, and goes with the drag.
#[test]
fn a_confined_clients_icon_is_bordered() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let context = sandbox(&run);
    let mut from = party(&mut run, (100, 100), Some(context));
    let serial = from.press(&mut run);
    let icon = from.icon(&mut run);
    let (_source, log) = from.drag(&mut run, serial, Some(&icon));
    from.paint(&mut run, &icon, 64);
    run.state.pointer_to(BARE.into(), InputTime::now());
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 710, 530), RED, "the icon");
    assert_eq!(at(&shown, 699, 530), BLACK, "the inner line");
    assert_eq!(at(&shown, 698, 530), WHITE, "the white line");
    assert_eq!(at(&shown, 697, 530), BLACK, "the outer line");
    assert!(background(&at(&shown, 696, 530)), "outside the border");

    release(&mut run);
    from.local.roundtrip(&mut run);
    assert!(log.sourced().cancelled, "dropped on nothing");
    let shown = frame(&mut run);
    assert!(
        background(&at(&shown, 710, 530)),
        "the icon goes with the drag"
    );
    assert!(background(&at(&shown, 698, 530)), "and its border");
    run.stop();
}
