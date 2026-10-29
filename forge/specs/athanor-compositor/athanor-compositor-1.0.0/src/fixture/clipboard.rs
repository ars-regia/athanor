//! The fixture's tests of primary selection and data control (doc_own_stack.md OS17, decision
//! 30; `clipboard.rs`). A child of `fixture`, which owns the compositor and the client they run
//! against; the selection protocols' client side is here, as user data on each object.

use super::{focused_window, sandbox, Client, Fake, Local, Running, FIXTURE_ALLOW_LIST};
use crate::identity::{AllowList, FileId, Process};
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wayland_client::protocol::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_seat,
};
use wayland_client::{
    delegate_noop, event_created_child, Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1, ext_data_control_manager_v1, ext_data_control_offer_v1,
    ext_data_control_source_v1,
};
use wayland_protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_manager_v1, zwp_primary_selection_device_v1,
    zwp_primary_selection_offer_v1, zwp_primary_selection_source_v1,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1, zwlr_data_control_manager_v1, zwlr_data_control_offer_v1,
    zwlr_data_control_source_v1,
};

const PRIMARY: &str = "zwp_primary_selection_device_manager_v1";
const EXT: &str = "ext_data_control_manager_v1";
const WLR: &str = "zwlr_data_control_manager_v1";
const TEXT: &str = "text/plain";

/// What a client heard on its devices and was asked of its sources.
#[derive(Default)]
struct Log {
    /// Each `selection` event: the offer, or none when the selection was cleared.
    clipboard: Vec<Option<Offer>>,
    /// Each `primary_selection` event (or the primary selection device's `selection`).
    primary: Vec<Option<Offer>>,
    /// The mime type of every `send` one of the client's sources was asked for.
    sent: Vec<String>,
}

type Shared = Arc<Mutex<Log>>;
type Mimes = Arc<Mutex<Vec<String>>>;

fn log(shared: &Shared) -> std::sync::MutexGuard<'_, Log> {
    shared.lock().expect("log")
}

/// A source's data: where `send` is logged, and what it writes.
struct SourceData {
    log: Shared,
    text: &'static str,
}

#[derive(Clone)]
enum Handle {
    Core(wl_data_offer::WlDataOffer),
    Primary(zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1),
    Ext(ext_data_control_offer_v1::ExtDataControlOfferV1),
    Wlr(zwlr_data_control_offer_v1::ZwlrDataControlOfferV1),
}

/// An offer the client was sent, with the mime types its `offer` events named.
#[derive(Clone)]
struct Offer {
    handle: Handle,
    mimes: Mimes,
}

impl Offer {
    fn mimes(&self) -> Vec<String> {
        self.mimes.lock().expect("mimes").clone()
    }

    fn receive(&self, mime: &str, fd: BorrowedFd<'_>) {
        let mime = mime.to_owned();
        match &self.handle {
            Handle::Core(offer) => offer.receive(mime, fd),
            Handle::Primary(offer) => offer.receive(mime, fd),
            Handle::Ext(offer) => offer.receive(mime, fd),
            Handle::Wlr(offer) => offer.receive(mime, fd),
        }
    }
}

fn write_out(data: &SourceData, mime_type: String, fd: std::os::fd::OwnedFd) {
    log(&data.log).sent.push(mime_type);
    let mut pipe = std::fs::File::from(fd);
    // The reader holds the other end open; a short text fits the pipe whole.
    if let Err(error) = pipe.write_all(data.text.as_bytes()) {
        panic!("a source cannot write its text: {error}");
    }
}

fn mimes_of(object: &Mimes, mime_type: String) {
    object.lock().expect("mimes").push(mime_type);
}

// wl_data_device, the regular clipboard of the application.
delegate_noop!(Client: wl_data_device_manager::WlDataDeviceManager);

impl Dispatch<wl_data_device::WlDataDevice, Shared> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        shared: &Shared,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_device::Event::Selection { id } = event {
            let offer = id.and_then(|id| {
                let mimes = id.data::<Mimes>()?.clone();
                Some(Offer {
                    handle: Handle::Core(id),
                    mimes,
                })
            });
            log(shared).clipboard.push(offer);
        }
    }

    event_created_child!(Client, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, Mimes::default()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, Mimes> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        mimes: &Mimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            mimes_of(mimes, mime_type);
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, SourceData> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_source::Event::Send { mime_type, fd } = event {
            write_out(data, mime_type, fd);
        }
    }
}

// zwp_primary_selection_device_manager_v1.
delegate_noop!(Client: zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1);

impl Dispatch<zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1, Shared> for Client {
    fn event(
        _: &mut Self,
        _: &zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
        event: zwp_primary_selection_device_v1::Event,
        shared: &Shared,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_device_v1::Event::Selection { id } = event {
            let offer = id.and_then(|id| {
                let mimes = id.data::<Mimes>()?.clone();
                Some(Offer {
                    handle: Handle::Primary(id),
                    mimes,
                })
            });
            log(shared).primary.push(offer);
        }
    }

    event_created_child!(Client, zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1, [
        zwp_primary_selection_device_v1::EVT_DATA_OFFER_OPCODE =>
            (zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1, Mimes::default()),
    ]);
}

impl Dispatch<zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1, Mimes> for Client {
    fn event(
        _: &mut Self,
        _: &zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1,
        event: zwp_primary_selection_offer_v1::Event,
        mimes: &Mimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_offer_v1::Event::Offer { mime_type } = event {
            mimes_of(mimes, mime_type);
        }
    }
}

impl Dispatch<zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1, SourceData> for Client {
    fn event(
        _: &mut Self,
        _: &zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
        event: zwp_primary_selection_source_v1::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_source_v1::Event::Send { mime_type, fd } = event {
            write_out(data, mime_type, fd);
        }
    }
}

/// The client side of one data control protocol: ext-data-control-v1 and wlr-data-control have
/// the same requests and events under other names.
macro_rules! data_control {
    ($manager:ident::$Manager:ident, $device:ident::$Device:ident, $offer:ident::$Offer:ident,
     $source:ident::$Source:ident, $handle:ident) => {
        delegate_noop!(Client: $manager::$Manager);

        impl Dispatch<$device::$Device, Shared> for Client {
            fn event(
                _: &mut Self,
                _: &$device::$Device,
                event: $device::Event,
                shared: &Shared,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                let offered = |id: Option<$offer::$Offer>| {
                    id.and_then(|id| {
                        let mimes = id.data::<Mimes>()?.clone();
                        Some(Offer {
                            handle: Handle::$handle(id),
                            mimes,
                        })
                    })
                };
                match event {
                    $device::Event::Selection { id } => log(shared).clipboard.push(offered(id)),
                    $device::Event::PrimarySelection { id } => {
                        log(shared).primary.push(offered(id))
                    }
                    _ => {}
                }
            }

            event_created_child!(Client, $device::$Device, [
                $device::EVT_DATA_OFFER_OPCODE => ($offer::$Offer, Mimes::default()),
            ]);
        }

        impl Dispatch<$offer::$Offer, Mimes> for Client {
            fn event(
                _: &mut Self,
                _: &$offer::$Offer,
                event: $offer::Event,
                mimes: &Mimes,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let $offer::Event::Offer { mime_type } = event {
                    mimes_of(mimes, mime_type);
                }
            }
        }

        impl Dispatch<$source::$Source, SourceData> for Client {
            fn event(
                _: &mut Self,
                _: &$source::$Source,
                event: $source::Event,
                data: &SourceData,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let $source::Event::Send { mime_type, fd } = event {
                    write_out(data, mime_type, fd);
                }
            }
        }
    };
}

data_control!(
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_device_v1::ExtDataControlDeviceV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::ExtDataControlSourceV1,
    Ext
);
data_control!(
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
    zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
    zwlr_data_control_source_v1::ZwlrDataControlSourceV1,
    Wlr
);

/// The selection devices of one client, each bound to the seat, with what they heard.
struct Devices {
    log: Shared,
    core: Option<(
        wl_data_device_manager::WlDataDeviceManager,
        wl_data_device::WlDataDevice,
    )>,
    primary: Option<(
        zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1,
        zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    )>,
    ext: Option<(
        ext_data_control_manager_v1::ExtDataControlManagerV1,
        ext_data_control_device_v1::ExtDataControlDeviceV1,
    )>,
    wlr: Option<(
        zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
        zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
    )>,
}

/// Which selection a source is set as.
#[derive(Clone, Copy, PartialEq)]
enum Which {
    Clipboard,
    Primary,
}

impl Devices {
    /// The wl_data_device and the primary selection device of an application.
    fn application(local: &mut Local, run: &mut Running) -> Self {
        let seat: wl_seat::WlSeat = local.bind(1);
        let log = Shared::default();
        let qh = local.queue.handle();
        let manager: wl_data_device_manager::WlDataDeviceManager = local.bind(3);
        let device = manager.get_data_device(&seat, &qh, log.clone());
        let primary_manager: zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1 =
            local.bind(1);
        let primary = primary_manager.get_device(&seat, &qh, log.clone());
        local.roundtrip(run);
        Self {
            log,
            core: Some((manager, device)),
            primary: Some((primary_manager, primary)),
            ext: None,
            wlr: None,
        }
    }

    /// The ext data control device of a clipboard manager, if it sees the manager.
    fn ext(local: &mut Local, run: &mut Running) -> Self {
        let seat: wl_seat::WlSeat = local.bind(1);
        let log = Shared::default();
        let manager: ext_data_control_manager_v1::ExtDataControlManagerV1 = local.bind(1);
        let device = manager.get_data_device(&seat, &local.queue.handle(), log.clone());
        local.roundtrip(run);
        Self {
            log,
            core: None,
            primary: None,
            ext: Some((manager, device)),
            wlr: None,
        }
    }

    /// The wlr data control device of a clipboard manager, if it sees the manager.
    fn wlr(local: &mut Local, run: &mut Running) -> Self {
        let seat: wl_seat::WlSeat = local.bind(1);
        let log = Shared::default();
        let manager: zwlr_data_control_manager_v1::ZwlrDataControlManagerV1 = local.bind(2);
        let device = manager.get_data_device(&seat, &local.queue.handle(), log.clone());
        local.roundtrip(run);
        Self {
            log,
            core: None,
            primary: None,
            ext: None,
            wlr: Some((manager, device)),
        }
    }

    /// Sets `which` to a source offering `text/plain` that writes `text`.
    fn set(&self, local: &Local, which: Which, text: &'static str) {
        let qh = &local.queue.handle();
        let data = || SourceData {
            log: self.log.clone(),
            text,
        };
        match (which, &self.core, &self.primary, &self.ext, &self.wlr) {
            (Which::Clipboard, Some((manager, device)), ..) => {
                let source = manager.create_data_source(qh, data());
                source.offer(TEXT.into());
                device.set_selection(Some(&source), 0);
            }
            (Which::Primary, _, Some((manager, device)), ..) => {
                let source = manager.create_source(qh, data());
                source.offer(TEXT.into());
                device.set_selection(Some(&source), 0);
            }
            (_, _, _, Some((manager, device)), _) => {
                let source = manager.create_data_source(qh, data());
                source.offer(TEXT.into());
                match which {
                    Which::Clipboard => device.set_selection(Some(&source)),
                    Which::Primary => device.set_primary_selection(Some(&source)),
                }
            }
            (_, _, _, _, Some((manager, device))) => {
                let source = manager.create_data_source(qh, data());
                source.offer(TEXT.into());
                match which {
                    Which::Clipboard => device.set_selection(Some(&source)),
                    Which::Primary => device.set_primary_selection(Some(&source)),
                }
            }
            _ => panic!("the client has no device for this selection"),
        }
        local.connection.flush().expect("flush");
    }

    /// The offer of the latest `which` event, if it was not a clear.
    fn latest(&self, which: Which) -> Option<Offer> {
        let log = log(&self.log);
        match which {
            Which::Clipboard => log.clipboard.last().cloned().flatten(),
            Which::Primary => log.primary.last().cloned().flatten(),
        }
    }

    /// Waits for an offer of `which` after the events heard so far were forgotten by `forget`.
    fn offered(&self, local: &mut Local, run: &mut Running, which: Which) -> Offer {
        let shared = self.log.clone();
        local.pump(run, "an offer arrives", |_, _| {
            let log = log(&shared);
            match which {
                Which::Clipboard => log.clipboard.iter().any(Option::is_some),
                Which::Primary => log.primary.iter().any(Option::is_some),
            }
        });
        self.latest(which).expect("an offer")
    }

    /// Forgets what the devices heard.
    fn forget(&self) {
        let mut log = log(&self.log);
        log.clipboard.clear();
        log.primary.clear();
    }
}

/// Reads `offer` as `reader` does, through a pipe: the client that owns the source, `owner`,
/// is asked for the data and writes it.
fn read(
    run: &mut Running,
    reader: &mut Local,
    offer: &Offer,
    owner: &mut Local,
    owner_devices: &Devices,
) -> String {
    assert!(
        offer.mimes().iter().any(|mime| mime == TEXT),
        "{:?}",
        offer.mimes()
    );
    let asked = log(&owner_devices.log).sent.len();
    let (mut from, to) = std::io::pipe().expect("pipe");
    offer.receive(TEXT, to.as_fd());
    drop(to);
    reader.connection.flush().expect("flush");
    let shared = owner_devices.log.clone();
    owner.pump(run, "the source is asked for its data", |_, _| {
        log(&shared).sent.len() > asked
    });
    let mut text = String::new();
    from.read_to_string(&mut text).expect("the data");
    text
}

/// The primary selection set by one client reaches the client that gets the keyboard focus, and
/// no other: a client without the focus is offered nothing, and is offered the selection when
/// it gains the focus.
#[test]
fn a_primary_selection_reaches_the_client_that_gets_the_focus() {
    let mut run = Running::start(None);
    let (mut copier, copier_window) = focused_window(&mut run);
    let (mut pasting, pasting_window) = focused_window(&mut run);
    let copier_devices = Devices::application(&mut copier, &mut run);
    let pasting_devices = Devices::application(&mut pasting, &mut run);

    // The copier takes the keyboard back and selects text.
    run.state.focus(&copier_window);
    copier.roundtrip(&mut run);
    pasting.roundtrip(&mut run);
    pasting_devices.forget();
    copier_devices.set(&copier, Which::Primary, "selected text");
    copier.roundtrip(&mut run);
    pasting.roundtrip(&mut run);
    assert!(
        pasting_devices.latest(Which::Primary).is_none(),
        "a client without the focus is offered the primary selection"
    );

    // The other window gains the focus and is offered it.
    run.state.focus(&pasting_window);
    let offer = pasting_devices.offered(&mut pasting, &mut run, Which::Primary);
    assert_eq!(
        read(&mut run, &mut pasting, &offer, &mut copier, &copier_devices),
        "selected text"
    );
    run.stop();
}

/// A session lock moves the keyboard to the lock surface, and the primary selection follows
/// the keyboard: the lock surface is offered it, a window behind the lock is offered nothing
/// while locked, and the offer comes when the unlock gives the window the focus back.
#[test]
fn a_locked_session_offers_the_primary_selection_to_no_window() {
    use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1;
    use wayland_client::protocol::{wl_compositor, wl_output, wl_shm};
    let mut run = Running::start(None);
    let (mut copier, copier_window) = focused_window(&mut run);
    let (mut pasting, pasting_window) = focused_window(&mut run);
    let copier_devices = Devices::application(&mut copier, &mut run);
    let pasting_devices = Devices::application(&mut pasting, &mut run);
    run.state.focus(&copier_window);
    copier.roundtrip(&mut run);
    pasting.roundtrip(&mut run);
    pasting_devices.forget();

    copier_devices.set(&copier, Which::Primary, "locked text");
    copier.roundtrip(&mut run);
    pasting.roundtrip(&mut run);
    pasting_devices.forget();

    let mut locker = Local::connect(&mut run, None);
    let locker_devices = Devices::application(&mut locker, &mut run);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let shm: wl_shm::WlShm = locker.bind(1);
    let output: wl_output::WlOutput = locker.bind(4);
    let qh = locker.queue.handle();
    let lock = locks.lock(&qh, ());
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    // The lock surface gets the keyboard, and the primary selection with it.
    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    locker.pump(&mut run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = locker.client.lock_configure.expect("a configure");
    lock_surface.ack_configure(serial);
    let (buffer, _) = locker.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let offer = locker_devices.offered(&mut locker, &mut run, Which::Primary);
    assert_eq!(
        read(&mut run, &mut locker, &offer, &mut copier, &copier_devices),
        "locked text"
    );

    run.state.focus(&pasting_window);
    pasting.roundtrip(&mut run);
    pasting.roundtrip(&mut run);
    assert!(
        pasting_devices.latest(Which::Primary).is_none(),
        "a window behind the lock is offered the primary selection"
    );

    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    run.state.focus(&pasting_window);
    let offer = pasting_devices.offered(&mut pasting, &mut run, Which::Primary);
    assert_eq!(
        read(&mut run, &mut pasting, &offer, &mut copier, &copier_devices),
        "locked text"
    );
    run.stop();
}

/// The focus holder disconnects, and the window that gets the focus next is offered what
/// another application selected.
#[test]
fn the_next_focus_is_offered_the_primary_selection_when_the_holder_disconnects() {
    let mut run = Running::start(None);
    let (mut copier, copier_window) = focused_window(&mut run);
    let (mut holder, holder_window) = focused_window(&mut run);
    let (mut next, next_window) = focused_window(&mut run);
    let copier_devices = Devices::application(&mut copier, &mut run);
    let holder_devices = Devices::application(&mut holder, &mut run);
    let next_devices = Devices::application(&mut next, &mut run);

    run.state.focus(&copier_window);
    copier.roundtrip(&mut run);
    copier_devices.set(&copier, Which::Primary, "kept text");
    copier.roundtrip(&mut run);
    run.state.focus(&holder_window);
    let offer = holder_devices.offered(&mut holder, &mut run, Which::Primary);
    assert!(next_devices.latest(Which::Primary).is_none());

    let windows = run.state.space.elements().count();
    // The connection closes when its last proxy goes.
    drop((offer, holder_devices, holder));
    run.until("the holder's window is gone", |state| {
        state.space.elements().count() == windows - 1
    });
    run.state.focus(&next_window);
    let offer_next = next_devices.offered(&mut next, &mut run, Which::Primary);
    assert_eq!(
        read(&mut run, &mut next, &offer_next, &mut copier, &copier_devices),
        "kept text"
    );
    run.stop();
}

/// The identity of the clipboard managers of the tests: an executable and a unit each, one
/// granted the ext data control and the other the wlr one. The shipped allow-list grants
/// neither to anyone.
const EXT_MANAGER: (&str, &str) = (
    "/usr/bin/athanor-test-clipboard-ext",
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-test-clipboard-ext.service\n",
);
const WLR_MANAGER: (&str, &str) = (
    "/usr/bin/athanor-test-clipboard-wlr",
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-test-clipboard-wlr.service\n",
);
const EXT_FILE: FileId = FileId { dev: 1, ino: 21 };
const WLR_FILE: FileId = FileId { dev: 1, ino: 22 };

/// The fixture's process table plus the two managers' executables.
struct Managers(Fake);

impl Process for Managers {
    fn executable(&self, pid: i32) -> std::io::Result<PathBuf> {
        self.0.executable(pid)
    }
    fn cgroup(&self, pid: i32) -> std::io::Result<String> {
        self.0.cgroup(pid)
    }
    fn executable_file(&self, pid: i32) -> std::io::Result<FileId> {
        self.0.executable_file(pid)
    }
    fn file(&self, path: &Path) -> std::io::Result<FileId> {
        if path == Path::new(EXT_MANAGER.0) {
            Ok(EXT_FILE)
        } else if path == Path::new(WLR_MANAGER.0) {
            Ok(WLR_FILE)
        } else {
            self.0.file(path)
        }
    }
}

/// The compositor with the managers' lines added to the fixture's allow-list.
fn running() -> Running {
    let mut run = Running::start(None);
    let lines = format!(
        "{FIXTURE_ALLOW_LIST}{} athanor-test-clipboard-ext.service {EXT}\n\
         {} athanor-test-clipboard-wlr.service {WLR}\n",
        EXT_MANAGER.0, WLR_MANAGER.0
    );
    run.state.allow_list = AllowList::parse(&lines).expect("allow-list");
    run.state.process = Box::new(Managers(Fake(
        run.identity.clone(),
        run.executable_file.clone(),
        run.executable_stats.clone(),
    )));
    run
}

/// Connects as `who`, inside `context` if any; the clients connecting after it do as well.
fn connect_as(
    run: &mut Running,
    who: (&'static str, &'static str),
    file: FileId,
    context: Option<smithay::wayland::security_context::SecurityContext>,
) -> Local {
    *run.identity.lock().expect("identity") = who;
    *run.executable_file.lock().expect("file") = file;
    Local::connect(run, context)
}

/// Primary selection is every client's, a security context included. Data control is
/// privileged, each protocol by its own line: a client the allow-list does not name, the bar
/// included, and a client inside a security context, whoever it is, are offered neither; a
/// client granted one is offered that one only. The shipped allow-list names no client for
/// either.
#[test]
fn primary_selection_is_standard_and_data_control_is_privileged() {
    let mut run = running();
    let context = sandbox(&run);

    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(PRIMARY), "the bar");
    assert!(!bar.sees(EXT) && !bar.sees(WLR), "the bar, with no line");

    let ext = connect_as(&mut run, EXT_MANAGER, EXT_FILE, None);
    assert!(ext.sees(EXT), "the client granted ext");
    assert!(!ext.sees(WLR), "the client granted ext only");
    let wlr = connect_as(&mut run, WLR_MANAGER, WLR_FILE, None);
    assert!(wlr.sees(WLR), "the client granted wlr");
    assert!(!wlr.sees(EXT), "the client granted wlr only");

    let sandboxed = connect_as(&mut run, EXT_MANAGER, EXT_FILE, Some(context));
    assert!(sandboxed.sees(PRIMARY), "inside a security context");
    assert!(
        !sandboxed.sees(EXT) && !sandboxed.sees(WLR),
        "a granted client inside a security context"
    );

    *run.identity.lock().expect("identity") = (super::SH, super::OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(other.sees(PRIMARY), "a client with no line");
    assert!(
        !other.sees(EXT) && !other.sees(WLR),
        "a client with no line"
    );
    run.stop();

    let shipped = include_str!("../../data/privileged-clients");
    assert!(
        !shipped
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .any(|line| line.contains("data_control")),
        "the shipped allow-list grants data control: no client may be granted it before the \
         session lock is handled, as Smithay updates data control devices with no focus check \
         and new_selection cannot veto, so a granted client reads and overwrites both \
         selections during a lock; a grant first needs a lock-aware mechanism (a lock flag in \
         the filter, and dropping bound data control clients at the lock) and its test"
    );
}

/// A client granted data control reads and sets both selections without the keyboard focus
/// (ext-data-control-v1 and wlr-data-control alike): it is offered what an application selects
/// and copies, and what it sets reaches the application that has the focus.
#[test]
fn a_granted_client_reads_and_sets_the_selections_through_data_control() {
    for (name, who, file, devices) in [
        (
            "ext",
            EXT_MANAGER,
            EXT_FILE,
            Devices::ext as fn(&mut Local, &mut Running) -> Devices,
        ),
        ("wlr", WLR_MANAGER, WLR_FILE, Devices::wlr),
    ] {
        let mut run = running();
        let (mut app, _window) = focused_window(&mut run);
        let app_devices = Devices::application(&mut app, &mut run);
        let mut manager = connect_as(&mut run, who, file, None);
        let manager_devices = devices(&mut manager, &mut run);

        for (which, label) in [(Which::Clipboard, "copied"), (Which::Primary, "selected")] {
            // Read: the application sets, the manager is offered it.
            manager_devices.forget();
            let text: &'static str = if which == Which::Clipboard {
                "copied in the application"
            } else {
                "selected in the application"
            };
            app_devices.set(&app, which, text);
            let offer = manager_devices.offered(&mut manager, &mut run, which);
            assert_eq!(
                read(&mut run, &mut manager, &offer, &mut app, &app_devices),
                text,
                "{name}: the manager reads what is {label}"
            );

            // Set: the manager sets, the application with the focus is offered it.
            app_devices.forget();
            let text: &'static str = if which == Which::Clipboard {
                "copied by the manager"
            } else {
                "selected by the manager"
            };
            manager_devices.set(&manager, which, text);
            let offer = app_devices.offered(&mut app, &mut run, which);
            assert_eq!(
                read(&mut run, &mut app, &offer, &mut manager, &manager_devices),
                text,
                "{name}: the application reads what the manager set as {label}"
            );
        }
        run.stop();
    }
}
