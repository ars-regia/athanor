//! The protocol fuzz target of issue #1172 (`fuzz/fuzz_targets/sulphur_protocol.rs`): the bytes
//! a fuzzer mutates become the requests one client writes on its socket, to the compositor on
//! its headless backend on Pixman. Each input runs on a compositor of its own, so the replay of
//! a corpus file is the run that found it. A breach of these properties is the [`Finding`] that
//! `run` returns, and the fuzz target (through `athanor_fuzz_entries::must`) and the replay test
//! of athanor-fuzz-entries end on it:
//! - the compositor never panics (`panic = "abort"`) and its loop never fails;
//! - a protocol error ends only the client that made it: a second client still has its answer;
//! - a client inside a security context is offered only the `standard` globals of
//!   `data/globals`, one outside it no `privileged` global, and a global without a row is a
//!   finding too (doc_own_stack.md OS4, OS17).
//!
//! The input: the low bit of its first byte puts the client inside a security context, then
//! operations follow until the input ends, each an opcode byte `b` and its operands. With
//! `p = b / 3`, `b % 3` is:
//! - 0, bind: a byte `k` picks the `k`-th global offered (modulo their number), bound at the
//!   offered version when `p` is 0 and at `p` otherwise, to the next new id;
//! - 1, request: an object byte (an index, modulo their number, into the ids the client has
//!   used: wl_display, the registry, then each new id), an opcode byte, then `p % 8` arguments,
//!   each a type byte `t` and, by `t % 5`: 0 a u32 of four bytes, 1 the next new id, 2 an object
//!   byte, 3 a string and 4 an array, each of a length byte and as many bytes;
//! - 2, dispatch: the compositor's loop runs once, draws a frame when `p` is odd, and the
//!   client reads its events.
//!
//! No request carries a file descriptor: one that needs it is a protocol error. Buffers come
//! from wp_single_pixel_buffer_manager_v1. ponytail: send a memfd with SCM_RIGHTS when wl_shm's
//! pools need coverage.

use crate::headless::{self, VirtualOutput};
use crate::identity::AllowList;
use crate::state::State;
use crate::Error;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::wayland::security_context::SecurityContext;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// One small output: Pixman draws it in a fraction of a millisecond.
const OUTPUT: VirtualOutput = VirtualOutput {
    width: 320,
    height: 200,
    scale: 1.0,
};
/// The registry is the client's first new id; wl_display is 1.
const REGISTRY: u32 = 2;
/// How many turns of the loop the second client's answer may take.
const TURNS: usize = 100;
/// The frame period `tick` draws for.
const PERIOD: Duration = Duration::from_millis(16);

/// A breach of one of the target's properties: the fuzz target ends on it, as on a panic.
#[derive(Debug)]
pub struct Finding(String);

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "athanor-sulphur fuzz finding: {}", self.0)
    }
}

impl std::error::Error for Finding {}

/// Runs `data` against a new compositor. `Err` is a [`Finding`], or a failure of the harness.
pub fn run(data: &[u8]) -> Result<(), Error> {
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let mut state = headless::start(&mut event_loop, display, None, &[OUTPUT])?;
    // The host's allow-list grants nothing to the fuzzer's executable; an empty one says so
    // whatever the host has installed.
    state.allow_list = AllowList::default();

    // The second client, outside a security context: the creator of the first one's.
    let (server_end, witness) = UnixStream::pair()?;
    let data_state = state.client_state(&server_end, None);
    let creator = state
        .display_handle
        .insert_client(server_end, data_state)?
        .id();
    let mut witness = Peer::new(witness)?;

    let mut input = data.iter().copied();
    let confined = input.next().is_some_and(|byte| byte & 1 == 1);
    let (server_end, client) = UnixStream::pair()?;
    let context = confined.then(|| SecurityContext {
        sandbox_engine: Some("org.flatpak".into()),
        app_id: Some("org.example.Fuzz".into()),
        instance_id: None,
        creator_client_id: creator,
    });
    let data_state = state.client_state(&server_end, context);
    state.display_handle.insert_client(server_end, data_state)?;
    let mut client = Fuzzed {
        peer: Peer::new(client)?,
        confined,
        globals: Vec::new(),
        ids: vec![1, REGISTRY],
        next: REGISTRY + 1,
    };
    let mut run = Run { event_loop, state };

    client.send(&mut run, 1, 1, &REGISTRY.to_le_bytes())?;
    run.turn(false)?;
    client.receive()?;
    while let Some(op) = input.next() {
        let p = op / 3;
        match op % 3 {
            0 => client.bind(&mut run, &mut input, p)?,
            1 => client.request(&mut run, &mut input, p)?,
            _ => {
                run.turn(p % 2 == 1)?;
                client.receive()?;
            }
        }
    }

    // wl_display.sync to the new id 2: its wl_callback.done is the answer.
    witness.send(&mut run, 1, 0, &2u32.to_le_bytes())?;
    let mut answered = false;
    for _ in 0..TURNS {
        run.turn(false)?;
        client.receive()?;
        witness.receive()?;
        if witness.gone {
            return Err(finding(
                "a client outside the protocol error was disconnected",
            ));
        }
        answered = witness
            .events
            .iter()
            .any(|event| event.object == 2 && event.opcode == 0);
        if answered {
            break;
        }
    }
    if !answered {
        return Err(finding("the compositor no longer answers another client"));
    }
    // The client hangs up with whatever it still holds: the compositor destroys it all, then
    // draws a frame without it.
    drop(client);
    run.turn(false)?;
    run.turn(true)
}

/// The compositor and its loop.
struct Run {
    event_loop: EventLoop<'static, State>,
    state: State,
}

impl Run {
    /// One turn of the loop, as `main`'s, and a frame when `draw` holds.
    fn turn(&mut self, draw: bool) -> Result<(), Error> {
        if let Err(error) = self
            .event_loop
            .dispatch(Some(Duration::ZERO), &mut self.state)
        {
            return Err(finding(format!("the compositor's loop failed: {error}")));
        }
        self.state.refresh_idle_inhibit();
        if draw {
            self.state.tick(PERIOD);
        }
        match &self.state.fatal {
            Some(reason) => Err(finding(format!("a client ended the compositor: {reason}"))),
            None => Ok(()),
        }
    }
}

/// An event a client read.
struct Event {
    object: u32,
    opcode: u16,
    args: Vec<u8>,
}

/// One end of a client's socket, written and read without blocking.
struct Peer {
    stream: UnixStream,
    inbox: Vec<u8>,
    events: Vec<Event>,
    /// The compositor has closed the connection, after a protocol error.
    gone: bool,
}

impl Peer {
    fn new(stream: UnixStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            inbox: Vec::new(),
            events: Vec::new(),
            gone: false,
        })
    }

    /// Writes one request, turning the loop while the socket is full.
    fn send(&mut self, run: &mut Run, object: u32, opcode: u16, args: &[u8]) -> Result<(), Error> {
        if self.gone {
            return Ok(());
        }
        let size = u16::try_from(8 + args.len())?;
        let mut message = Vec::with_capacity(usize::from(size));
        message.extend_from_slice(&object.to_le_bytes());
        message.extend_from_slice(&((u32::from(size) << 16) | u32::from(opcode)).to_le_bytes());
        message.extend_from_slice(args);
        let mut written = 0;
        for _ in 0..TURNS {
            match self.stream.write(&message[written..]) {
                Ok(n) => written += n,
                Err(error) if error.kind() == ErrorKind::WouldBlock => run.turn(false)?,
                Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                    self.gone = true;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
            if written == message.len() {
                return Ok(());
            }
        }
        Err(finding(
            "the compositor no longer reads a client's requests",
        ))
    }

    /// Reads what the socket holds and splits it into the events that are whole.
    fn receive(&mut self) -> Result<(), Error> {
        let mut buffer = [0; 4096];
        loop {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    self.gone = true;
                    break;
                }
                Ok(n) => self.inbox.extend_from_slice(&buffer[..n]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::ConnectionReset => {
                    self.gone = true;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let mut start = 0;
        while let (Some(object), Some(header)) =
            (word(&self.inbox, start), word(&self.inbox, start + 4))
        {
            let size = (header >> 16) as usize;
            if size < 8 || !size.is_multiple_of(4) {
                return Err(finding(format!(
                    "the compositor sent a message of {size} bytes"
                )));
            }
            if self.inbox.len() < start + size {
                break;
            }
            self.events.push(Event {
                object,
                opcode: (header & 0xffff) as u16,
                args: self.inbox[start + 8..start + size].to_vec(),
            });
            // wl_display.error: the compositor closes the connection after it.
            if object == 1 && header & 0xffff == 0 {
                self.gone = true;
            }
            start += size;
        }
        self.inbox.drain(..start);
        Ok(())
    }
}

/// The little-endian u32 at `at`, if `bytes` holds one there.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let word = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(word.try_into().ok()?))
}

/// The client the input drives.
struct Fuzzed {
    peer: Peer,
    confined: bool,
    /// The globals offered: name, interface and version.
    globals: Vec<(u32, String, u32)>,
    /// Every id the client has used, in order.
    ids: Vec<u32>,
    next: u32,
}

impl Fuzzed {
    fn send(&mut self, run: &mut Run, object: u32, opcode: u16, args: &[u8]) -> Result<(), Error> {
        self.peer.send(run, object, opcode, args)
    }

    /// Reads the client's events and checks each global offered against its class.
    fn receive(&mut self) -> Result<(), Error> {
        self.peer.receive()?;
        for event in std::mem::take(&mut self.peer.events) {
            if event.object == REGISTRY && event.opcode == 0 {
                match global(&event.args) {
                    Some(global) => self.admit(global)?,
                    None => return Err(finding("a wl_registry.global the client cannot read")),
                }
            }
        }
        Ok(())
    }

    /// A breach when the client's class should not see `interface`.
    fn admit(&mut self, (name, interface, version): (u32, String, u32)) -> Result<(), Error> {
        match class(&interface) {
            Some("standard") => {}
            Some("unconfined" | "layer-gated") if !self.confined => {}
            Some(class) => {
                return Err(finding(format!(
                    "{interface} ({class}) offered to a client {}",
                    if self.confined {
                        "inside a security context"
                    } else {
                        "outside the allow-list"
                    }
                )))
            }
            None => return Err(finding(format!("{interface} has no row in data/globals"))),
        }
        self.globals.push((name, interface, version));
        Ok(())
    }

    fn new_id(&mut self) -> u32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.ids.push(id);
        id
    }

    fn object(&self, pick: u8) -> u32 {
        self.ids[usize::from(pick) % self.ids.len()]
    }

    fn bind(
        &mut self,
        run: &mut Run,
        input: &mut impl Iterator<Item = u8>,
        p: u8,
    ) -> Result<(), Error> {
        let Some(pick) = input.next() else {
            return Ok(());
        };
        let Some((name, interface, version)) = self
            .globals
            .get(usize::from(pick) % self.globals.len().max(1))
            .cloned()
        else {
            return Ok(());
        };
        let version = if p == 0 { version } else { u32::from(p) };
        let mut args = name.to_le_bytes().to_vec();
        string(&mut args, interface.as_bytes());
        args.extend_from_slice(&version.to_le_bytes());
        let id = self.new_id();
        args.extend_from_slice(&id.to_le_bytes());
        self.send(run, REGISTRY, 0, &args)
    }

    fn request(
        &mut self,
        run: &mut Run,
        input: &mut impl Iterator<Item = u8>,
        p: u8,
    ) -> Result<(), Error> {
        let (Some(pick), Some(opcode)) = (input.next(), input.next()) else {
            return Ok(());
        };
        let object = self.object(pick);
        let mut args = Vec::new();
        for _ in 0..p % 8 {
            let Some(kind) = input.next() else {
                break;
            };
            match kind % 5 {
                0 => {
                    let bytes: Vec<u8> = input.by_ref().take(4).collect();
                    let mut raw = [0; 4];
                    raw[..bytes.len()].copy_from_slice(&bytes);
                    args.extend_from_slice(&raw);
                }
                1 => {
                    let id = self.new_id();
                    args.extend_from_slice(&id.to_le_bytes());
                }
                2 => {
                    let id = input.next().map_or(0, |pick| self.object(pick));
                    args.extend_from_slice(&id.to_le_bytes());
                }
                kind => {
                    let length = input.next().unwrap_or(0);
                    let bytes: Vec<u8> = input.by_ref().take(usize::from(length)).collect();
                    if kind == 3 {
                        string(&mut args, &bytes);
                    } else {
                        array(&mut args, &bytes);
                    }
                }
            }
        }
        self.send(run, object, u16::from(opcode), &args)
    }
}

/// A string argument: its length with the NUL, its bytes, the NUL, padded to four bytes.
fn string(args: &mut Vec<u8>, bytes: &[u8]) {
    let mut with_nul = bytes.to_vec();
    with_nul.push(0);
    array(args, &with_nul);
}

/// An array argument: its length, its bytes, padded to four bytes.
fn array(args: &mut Vec<u8>, bytes: &[u8]) {
    args.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    args.extend_from_slice(bytes);
    args.resize(args.len().next_multiple_of(4), 0);
}

/// wl_registry.global: a name, an interface and a version.
fn global(args: &[u8]) -> Option<(u32, String, u32)> {
    let name = word(args, 0)?;
    let length = word(args, 4)? as usize;
    let text = args.get(8..8 + length.checked_sub(1)?)?;
    let version = word(args, 8 + length.next_multiple_of(4))?;
    Some((name, String::from_utf8_lossy(text).into_owned(), version))
}

/// The class of `interface` in `data/globals`.
fn class(interface: &str) -> Option<&'static str> {
    include_str!("../data/globals")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let (name, class) = line.split_once(' ')?;
            (name == interface).then_some(class.trim())
        })
}

/// The error of a breach of the target's properties.
fn finding(what: impl Into<String>) -> Error {
    Box::new(Finding(what.into()))
}
