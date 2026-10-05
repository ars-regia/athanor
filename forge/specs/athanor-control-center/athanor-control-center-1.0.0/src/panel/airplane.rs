//! The airplane-mode device: `/dev/rfkill` opened once, read for the radios' state and
//! written to block them all (CC8). The encoding and the arithmetic are in `athanor_control_center::rfkill`.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::rc::Rc;

use gtk4::gio::prelude::InputStreamExtManual;
use gtk4::glib;

use athanor_control_center::rfkill::{self, Event, Radios, EVENT_SIZE};

pub struct Rfkill {
    file: File,
    radios: RefCell<Radios>,
}

/// The device, or `None` where there is none or this session may not use it: the tile is then
/// not built. `O_CLOEXEC` is std's own default, and `O_NONBLOCK` lets the main loop drain the
/// queue without waiting.
pub fn open() -> Option<Rc<Rfkill>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(athanor_control_center::RFKILL);
    match file {
        Ok(file) => Some(Rfkill::new(file)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => {
            tracing::warn!(error = %err, "cannot open {}; there is no airplane-mode tile", athanor_control_center::RFKILL);
            None
        }
    }
}

impl Rfkill {
    /// Takes a descriptor opened non-blocking for reading and writing, and the events the
    /// kernel queued when it was opened (one `ADD` per radio).
    pub fn new(file: File) -> Rc<Rfkill> {
        let rfkill = Rc::new(Rfkill {
            file,
            radios: RefCell::default(),
        });
        rfkill.drain();
        rfkill
    }

    /// Calls `changed` on the main loop after every event the kernel sends, for as long as
    /// the device is held. The stream reads through its own descriptor of the same open file,
    /// and waits for the device on the main loop.
    pub fn watch(self: &Rc<Self>, changed: impl Fn() + 'static) {
        let stream = match self.file.try_clone() {
            Ok(file) => gio_unix::InputStream::take_fd(OwnedFd::from(file)),
            Err(err) => {
                tracing::warn!(error = %err, "cannot follow {}; the airplane-mode tile will not update", crate::RFKILL);
                return;
            }
        };
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            loop {
                let bytes = match stream.read_future([0u8; EVENT_SIZE], glib::Priority::DEFAULT).await {
                    Ok((bytes, EVENT_SIZE)) => bytes,
                    Ok((_, read)) => {
                        tracing::warn!(read, "a short read from {}; the tile stops following it", crate::RFKILL);
                        return;
                    }
                    Err((_, err)) => {
                        tracing::warn!(error = %err, "{} went away; the tile stops following it", crate::RFKILL);
                        return;
                    }
                };
                let Some(rfkill) = weak.upgrade() else { return };
                rfkill.radios.borrow_mut().apply(Event::decode(bytes));
                changed();
            }
        });
    }

    /// Reads every queued event; true when there was one.
    fn drain(&self) -> bool {
        let mut any = false;
        let mut bytes = [0u8; EVENT_SIZE];
        // `&File` reads without `&mut`.
        while let Ok(read) = (&self.file).read(&mut bytes) {
            if read != EVENT_SIZE {
                break;
            }
            self.radios.borrow_mut().apply(Event::decode(bytes));
            any = true;
        }
        any
    }

    pub fn airplane(&self) -> bool {
        self.radios.borrow().airplane()
    }

    /// Soft-blocks, or unblocks, every radio. The tile changes when the kernel reports it.
    pub fn set_blocked(&self, blocked: bool) -> io::Result<()> {
        (&self.file).write_all(&rfkill::block_all(blocked).encode())
    }
}
