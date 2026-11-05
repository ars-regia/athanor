//! A test pointer on `zwlr_virtual_pointer_v1`, for a rig without a pointer device (the dock plan,
//! task 4). Reads commands on standard input, one per line, and answers `ok` on standard output
//! once the compositor has received each:
//!
//! - `move <x> <y>`: to the point, in logical pixels of a 1280 by 800 output;
//! - `click <button>`: presses and releases the button, as an evdev code (272 is the left one).

use smithay_client_toolkit::reexports::protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};
use std::error::Error;
use std::io::BufRead;
use std::process::ExitCode;
use std::time::Instant;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_pointer::ButtonState;
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};

/// The output's size, which the absolute motion is relative to.
const EXTENT: (u32, u32) = (1280, 800);

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);

fn run() -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ())?;
    let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
    let start = Instant::now();
    let time = || u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["move", x, y] => {
                pointer.motion_absolute(time(), x.parse()?, y.parse()?, EXTENT.0, EXTENT.1);
                pointer.frame();
            }
            ["click", button] => {
                let button = button.parse()?;
                pointer.button(time(), button, ButtonState::Pressed);
                pointer.frame();
                pointer.button(time(), button, ButtonState::Released);
                pointer.frame();
            }
            _ => return Err(format!("unknown command {line:?}").into()),
        }
        queue.roundtrip(&mut State)?;
        println!("ok");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pointer: {error}");
            ExitCode::FAILURE
        }
    }
}
