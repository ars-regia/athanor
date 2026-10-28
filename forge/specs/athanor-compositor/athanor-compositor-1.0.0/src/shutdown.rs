//! SIGTERM and SIGINT, which `systemctl --user stop` and a console's Ctrl+C send, end the event
//! loop as a closed nested window does, so that `main` returns and every owner drops: the X11
//! display's files, the satellite, the Wayland socket and the DRM and libseat sessions.

use crate::Error;
use smithay::reexports::calloop::signals::{Signal, Signals};
use smithay::reexports::calloop::EventLoop;

/// Stops `event_loop` at the first SIGTERM or SIGINT. A signalfd serves them, and the source
/// blocks both signals in the calling thread, which threads and children started later
/// inherit (std clears the mask of a child it spawns): call it before any thread starts, or one
/// of them could take the default action and kill the process uncleaned.
pub fn watch<D: 'static>(event_loop: &EventLoop<'static, D>) -> Result<(), Error> {
    let stop = event_loop.get_signal();
    let signals = Signals::new(&[Signal::SIGTERM, Signal::SIGINT])?;
    event_loop
        .handle()
        .insert_source(signals, move |event, _, _| {
            tracing::info!(signal = ?event.signal(), "stopping");
            stop.stop();
        })
        .map_err(|e| e.error)?;
    Ok(())
}
