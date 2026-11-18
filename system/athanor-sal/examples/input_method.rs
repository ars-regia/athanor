//! A test input method on `zwp_input_method_v2` (doc_own_stack.md, OS9 item 3; milestone T1,
//! task 6). It waits until the compositor activates it for a text field, sends the preedit, and
//! commits the text once a line arrives on standard input. Each step is reported on standard
//! output after the compositor has received it.
//!
//! `input_method <preedit> <commit>`. The compositor offers the protocol only to a client outside
//! a security context, as cosmic-comp does (doc_languages.md, section 1).

use smithay_client_toolkit::reexports::protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use std::error::Error;
use std::io::BufRead;
use std::process::ExitCode;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};

#[derive(Default)]
struct State {
    /// `activate` or `deactivate` as last sent, applied at the next `done`.
    pending_active: bool,
    active: bool,
    /// The `done` events so far: the serial each commit carries.
    done: u32,
    unavailable: bool,
}

impl Dispatch<ZwpInputMethodV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_v2::Event::Activate => state.pending_active = true,
            zwp_input_method_v2::Event::Deactivate => state.pending_active = false,
            zwp_input_method_v2::Event::Done => {
                state.done = state.done.wrapping_add(1);
                state.active = state.pending_active;
            }
            zwp_input_method_v2::Event::Unavailable => state.unavailable = true,
            _ => {}
        }
    }
}

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
delegate_noop!(State: ZwpInputMethodManagerV2);

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(preedit), Some(commit)) = (args.next(), args.next()) else {
        return Err("usage: input_method <preedit> <commit>".into());
    };
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ())?;
    let manager: ZwpInputMethodManagerV2 = globals.bind(&qh, 1..=1, ())?;
    let input_method = manager.get_input_method(&seat, &qh, ());
    let mut state = State::default();
    while !state.active {
        if state.unavailable {
            return Err("the seat already has an input method".into());
        }
        queue.blocking_dispatch(&mut state)?;
    }

    // The caret after the preedit.
    let end = i32::try_from(preedit.len())?;
    input_method.set_preedit_string(preedit.clone(), end, end);
    input_method.commit(state.done);
    queue.roundtrip(&mut state)?;
    println!("preedit {preedit}");

    if std::io::stdin().lock().read_line(&mut String::new())? == 0 {
        return Err("standard input closed before the commit".into());
    }
    input_method.commit_string(commit.clone());
    input_method.commit(state.done);
    queue.roundtrip(&mut state)?;
    println!("commit {commit}");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("input_method: {error}");
            ExitCode::FAILURE
        }
    }
}
