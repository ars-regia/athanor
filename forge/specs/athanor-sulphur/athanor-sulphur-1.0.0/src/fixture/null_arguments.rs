//! A request with a null where its signature forbids one ends its client with a protocol error,
//! and the compositor goes on serving the others (vendor/wayland-backend/PATCHES.md).

use super::*;
use std::io::{ErrorKind, Read};

/// A client that writes its requests raw, nulls included, as no client library would.
struct Wire {
    stream: UnixStream,
    /// The events it got: their object, their opcode and their arguments.
    events: Vec<(u32, u16, Vec<u8>)>,
    unread: Vec<u8>,
    closed: bool,
}

/// The ids the client gives its registry and the callback of its first sync.
const REGISTRY: u32 = 2;
const CALLBACK: u32 = 3;

impl Wire {
    /// Connects outside a security context and lists the globals.
    fn connect(run: &mut Running) -> Self {
        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        let data = run.state.client_state(&server_end, None);
        run.state
            .display_handle
            .insert_client(server_end, data)
            .expect("client");
        client_end.set_nonblocking(true).expect("non-blocking");
        let mut wire = Self {
            stream: client_end,
            events: Vec::new(),
            unread: Vec::new(),
            closed: false,
        };
        wire.send(1, 1, &[REGISTRY]);
        wire.send(1, 0, &[CALLBACK]);
        wire.pump(run, "the globals are listed", |wire| {
            wire.events
                .iter()
                .any(|(object, opcode, _)| (*object, *opcode) == (CALLBACK, 0))
        });
        wire
    }

    /// Sends the request `opcode` of `object` with its argument words.
    fn send(&mut self, object: u32, opcode: u16, args: &[u32]) {
        let size = u32::try_from(8 + 4 * args.len()).expect("a short request");
        let bytes: Vec<u8> = [object, size << 16 | u32::from(opcode)]
            .iter()
            .chain(args)
            .flat_map(|word| word.to_ne_bytes())
            .collect();
        self.stream.write_all(&bytes).expect("send");
    }

    /// The name of the global of `interface`.
    fn global(&self, interface: &str) -> u32 {
        self.events
            .iter()
            .filter(|(object, opcode, _)| (*object, *opcode) == (REGISTRY, 0))
            .find(|(_, _, args)| args.get(8..8 + interface.len()) == Some(interface.as_bytes()))
            .and_then(|(_, _, args)| word(args, 0))
            .unwrap_or_else(|| panic!("{interface} is listed"))
    }

    /// Runs the compositor until `done` holds of the client.
    fn pump(&mut self, run: &mut Running, what: &str, done: impl Fn(&Self) -> bool) {
        let started = Instant::now();
        while !done(self) {
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            run.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut run.state)
                .expect("dispatch");
            let mut chunk = [0; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => self.closed = true,
                Ok(read) => self.unread.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => panic!("{what}: {error}"),
            }
            while let (Some(object), Some(header)) = (word(&self.unread, 0), word(&self.unread, 4))
            {
                let size = usize::try_from(header >> 16).expect("a size");
                if self.unread.len() < size {
                    break;
                }
                let message: Vec<u8> = self.unread.drain(..size).collect();
                let opcode = u16::try_from(header & 0xffff).expect("an opcode");
                self.events.push((object, opcode, message[8..].to_vec()));
            }
        }
    }
}

/// The word at `at` of `bytes`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let word = bytes.get(at..at + 4)?;
    Some(u32::from_ne_bytes(word.try_into().ok()?))
}

/// A string argument's words: its length with the NUL, its bytes, the NUL, the padding.
fn string(text: &str) -> Vec<u32> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    let length = u32::try_from(text.len() + 1).expect("a short string");
    std::iter::once(length)
        .chain(
            bytes
                .chunks(4)
                .map(|word| u32::from_ne_bytes(word.try_into().expect("four bytes"))),
        )
        .collect()
}

/// Binds the data device manager as object 4.
fn data_device_manager(wire: &mut Wire) {
    let name = wire.global("wl_data_device_manager");
    let mut args = vec![name];
    args.extend(string("wl_data_device_manager"));
    args.extend([3, 4]);
    wire.send(REGISTRY, 0, &args);
}

/// The requests a case sends on its own client.
type Requests = fn(&mut Wire);

/// Each null a signature forbids: a string, an object and a new id, the first two in the
/// requests whose handlers took them as present. Each ends its own client with a
/// `wl_display.error`; the next client, and the fixture's, are served still.
#[test]
fn a_null_the_signature_forbids_ends_only_its_client() {
    let mut run = Running::start(None);
    let cases: [(&str, Requests); 4] = [
        ("a bind with a null interface", |wire| {
            let name = wire.global("wl_compositor");
            wire.send(REGISTRY, 0, &[name, 0, 1, 4]);
        }),
        ("an offer of a null type", |wire| {
            data_device_manager(wire);
            // wl_data_device_manager.create_data_source, then wl_data_source.offer.
            wire.send(4, 0, &[5]);
            wire.send(5, 0, &[0]);
        }),
        ("a data device of a null seat", |wire| {
            data_device_manager(wire);
            // wl_data_device_manager.get_data_device.
            wire.send(4, 1, &[5, 0]);
        }),
        ("a sync with a null callback", |wire| wire.send(1, 0, &[0])),
    ];
    for (what, requests) in cases {
        let mut wire = Wire::connect(&mut run);
        requests(&mut wire);
        wire.pump(&mut run, what, |wire| wire.closed);
        assert!(
            wire.events
                .iter()
                .any(|(object, opcode, _)| (*object, *opcode) == (1, 0)),
            "{what}: no wl_display.error before the client was disconnected"
        );
    }
    let local = Local::connect(&mut run, None);
    assert!(
        local
            .client
            .globals
            .iter()
            .any(|(_, interface, _)| interface == "wl_compositor"),
        "a client after them is served"
    );
    run.stop();
}
