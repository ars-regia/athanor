//! The clipboard and the primary selection on `wl_data_device` and
//! `zwp_primary_selection_device_v1`: the text a field copies or selects is offered to any
//! client that asks, and the text pasted into one is read, both through the event loop so that
//! neither a slow reader nor a slow writer blocks the application.

use super::{App, SurfaceId};
use crate::widget::{Clip, WidgetId};
use smithay_client_toolkit::data_device_manager::{
    data_device::{DataDevice, DataDeviceHandler},
    data_offer::{DataOfferHandler, DragOffer},
    data_source::{CopyPasteSource, DataSourceHandler},
    DataDeviceManagerState, ReadPipe, WritePipe,
};
use smithay_client_toolkit::primary_selection::{
    device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler},
    selection::{PrimarySelectionSource, PrimarySelectionSourceHandler},
    PrimarySelectionManagerState,
};
use smithay_client_toolkit::reexports::calloop::{
    timer::{TimeoutAction, Timer},
    LoopHandle, PostAction, RegistrationToken,
};
use smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
};
use std::cell::Cell;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{
    wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_data_source::WlDataSource,
    wl_seat::WlSeat, wl_surface::WlSurface,
};
use wayland_client::{Connection, QueueHandle};

/// The types of text offered and read, best first: UTF-8 text by its MIME name and by the name
/// X11 clients know it by, then plain text.
const MIMES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// The longest text read from a clipboard, in bytes; a field keeps at most
/// [`crate::Ui::FIELD_MAX_CHARS`] characters of it.
const READ_MAX: usize = 64 * 1024;

/// Linux's `PIPE_BUF`: a pipe that polls writable takes a write this long without blocking.
const PIPE_BUF: usize = 4096;

/// How long a clipboard's owner has to send the whole text of a paste.
const PASTE_TIMEOUT: Duration = Duration::from_secs(3);

/// How long a reader has to take the whole text of an offer.
const OFFER_TIMEOUT: Duration = Duration::from_secs(3);

/// The seat's clipboards, as far as the compositor has them.
#[derive(Default)]
pub(super) struct Clipboards {
    manager: Option<DataDeviceManagerState>,
    primary_manager: Option<PrimarySelectionManagerState>,
    device: Option<DataDevice>,
    primary_device: Option<PrimarySelectionDevice>,
    /// The text copied last, and the source that offers it until another one replaces it.
    copied: Option<(CopyPasteSource, String)>,
    /// The text selected last, the source that offers it until another one replaces it, and
    /// the surface whose field selected it.
    selected: Option<(PrimarySelectionSource, String, SurfaceId)>,
    /// The paste being read, at most one: a new one replaces it.
    pending: Option<Pending>,
}

/// A paste being read: its pipe and its deadline, both sources of the event loop.
pub(super) struct Pending {
    pipe: RegistrationToken,
    deadline: RegistrationToken,
}

/// Reads a paste from `pipe` through the event loop of `handle`, and hands its text to `done`.
/// One paste at a time: the one still in `slot`, which `pending` finds in the loop's data, is
/// dropped with its pipe, and a paste whose owner has not sent the whole text in `timeout` is
/// dropped too, so that an owner that never closes its pipe holds no descriptor.
fn read_paste<D: 'static>(
    handle: &LoopHandle<'static, D>,
    slot: &mut Option<Pending>,
    pending: fn(&mut D) -> &mut Option<Pending>,
    pipe: ReadPipe,
    timeout: Duration,
    mut done: impl FnMut(&mut D, String) + 'static,
) -> Result<(), String> {
    if let Some(old) = slot.take() {
        eprintln!("athanor-toolkit: the paste not yet read gives way to the next");
        handle.remove(old.pipe);
        handle.remove(old.deadline);
    }
    let mut incoming = Incoming::default();
    let loop_handle = handle.clone();
    let read = handle.insert_source(pipe, move |(), pipe, data| {
        let Some(result) = incoming.read(&**pipe) else {
            return PostAction::Continue;
        };
        if let Some(read) = pending(data).take() {
            loop_handle.remove(read.deadline);
        }
        match result {
            Ok(text) => done(data, text),
            Err(error) => eprintln!("athanor-toolkit: nothing pasted: {error}"),
        }
        PostAction::Remove
    });
    let read = read.map_err(|e| e.to_string())?;
    let loop_handle = handle.clone();
    let deadline = handle.insert_source(Timer::from_duration(timeout), move |_, _, data| {
        if let Some(late) = pending(data).take() {
            loop_handle.remove(late.pipe);
        }
        eprintln!(
            "athanor-toolkit: nothing pasted: the clipboard's owner sent no whole text in {timeout:?}"
        );
        TimeoutAction::Drop
    });
    match deadline {
        Ok(deadline) => {
            *slot = Some(Pending {
                pipe: read,
                deadline,
            });
            Ok(())
        }
        Err(error) => {
            handle.remove(read);
            Err(error.to_string())
        }
    }
}

impl Clipboards {
    /// Binds the managers the compositor has: without them, nothing is copied or pasted.
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self {
            manager: DataDeviceManagerState::bind(globals, qh).ok(),
            primary_manager: PrimarySelectionManagerState::bind(globals, qh).ok(),
            ..Self::default()
        }
    }

    /// Gets the devices of `seat`, the first seat seen.
    pub(super) fn seat(&mut self, qh: &QueueHandle<App>, seat: &WlSeat) {
        if self.device.is_none() {
            self.device = self.manager.as_ref().map(|m| m.get_data_device(qh, seat));
        }
        if self.primary_device.is_none() {
            let manager = self.primary_manager.as_ref();
            self.primary_device = manager.map(|m| m.get_selection_device(qh, seat));
        }
    }
}

/// The best type of text of those an offer has.
fn choose(offered: &[String]) -> Option<&'static str> {
    MIMES.into_iter().find(|m| offered.iter().any(|o| o == m))
}

/// A text read from a clipboard's pipe, as it comes.
#[derive(Default)]
struct Incoming(Vec<u8>);

impl Incoming {
    /// Reads once what the pipe has: `None` while more may come, then the text, or why there is
    /// none: it is longer than [`READ_MAX`], or not UTF-8.
    fn read(&mut self, mut pipe: impl Read) -> Option<Result<String, String>> {
        let mut chunk = [0; PIPE_BUF];
        match pipe.read(&mut chunk) {
            Ok(0) => Some(
                String::from_utf8(std::mem::take(&mut self.0))
                    .map_err(|e| format!("the text is not UTF-8: {e}")),
            ),
            Ok(n) if self.0.len() + n > READ_MAX => {
                Some(Err(format!("the text is longer than {READ_MAX} bytes")))
            }
            Ok(n) => {
                self.0.extend_from_slice(&chunk[..n]);
                None
            }
            Err(e) if retry(&e) => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }
}

/// Writes `text` to a reader's `pipe` as the event loop of `handle` finds it writable, then
/// closes it. A reader that has not taken the whole text in `timeout` loses the rest, and the
/// pipe closes, so that a reader that never reads holds no descriptor of the application.
fn write_offer<D: 'static>(
    handle: &LoopHandle<'static, D>,
    text: String,
    pipe: WritePipe,
    timeout: Duration,
) -> Result<(), String> {
    let mut outgoing = Outgoing {
        text: text.into_bytes(),
        written: 0,
    };
    let deadline = Rc::new(Cell::new(None));
    let written = Rc::clone(&deadline);
    let loop_handle = handle.clone();
    let write = handle.insert_source(pipe, move |(), pipe, _| {
        let Some(result) = outgoing.write(&**pipe) else {
            return PostAction::Continue;
        };
        if let Some(deadline) = written.take() {
            loop_handle.remove(deadline);
        }
        if let Err(error) = result {
            eprintln!("athanor-toolkit: the clipboard's reader took no text: {error}");
        }
        PostAction::Remove
    });
    let write = write.map_err(|e| e.to_string())?;
    let loop_handle = handle.clone();
    let late = handle.insert_source(Timer::from_duration(timeout), move |_, _, _| {
        loop_handle.remove(write);
        eprintln!(
            "athanor-toolkit: the clipboard's reader took no whole text in {timeout:?}, and its pipe is closed"
        );
        TimeoutAction::Drop
    });
    match late {
        Ok(late) => {
            deadline.set(Some(late));
            Ok(())
        }
        Err(error) => {
            handle.remove(write);
            Err(error.to_string())
        }
    }
}

/// A text written to a reader's pipe, as it takes it.
struct Outgoing {
    text: Vec<u8>,
    written: usize,
}

impl Outgoing {
    /// Writes once at most [`PIPE_BUF`] bytes of the rest: `None` while some is left, then
    /// whether the reader took all of it. A reader that closed its end early is an error,
    /// `EPIPE`, and not a signal, as Rust's runtime ignores `SIGPIPE`.
    fn write(&mut self, mut pipe: impl Write) -> Option<io::Result<()>> {
        let rest = &self.text[self.written..];
        match pipe.write(&rest[..rest.len().min(PIPE_BUF)]) {
            Ok(n) => {
                self.written += n;
                (self.written == self.text.len()).then_some(Ok(()))
            }
            Err(e) if retry(&e) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Whether an error of a pipe only means: not now.
fn retry(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
    )
}

impl App {
    /// Serves what the widgets of `id` asked of the clipboards.
    pub(super) fn clips(&mut self, id: SurfaceId) {
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        for clip in surface.ui.take_clips() {
            match clip {
                Clip::Copy(text) => {
                    self.copy(text);
                }
                Clip::Cut(widget, text) => {
                    if self.copy(text.clone()) {
                        self.cut(id, widget, &text);
                    }
                }
                Clip::Select(text) => self.select(id, text),
                Clip::Unselect => self.unselect(id),
                Clip::Paste(widget) => self.paste(id, widget, false),
                Clip::PastePrimary(widget) => self.paste(id, widget, true),
            }
        }
    }

    /// Offers `text` on the clipboard, with the serial of the key that copied it; whether it
    /// could.
    fn copy(&mut self, text: String) -> bool {
        let clipboards = &mut self.clipboards;
        let (Some(manager), Some(device)) = (&clipboards.manager, &clipboards.device) else {
            eprintln!("athanor-toolkit: nothing copied: the compositor has no clipboard");
            return false;
        };
        let Some(serial) = self.shell.grab.serial() else {
            eprintln!("athanor-toolkit: nothing copied: no key or button to copy with");
            return false;
        };
        let source = manager.create_copy_paste_source(&self.shell.qh, MIMES);
        source.set_selection(device, serial);
        clipboards.copied = Some((source, text));
        true
    }

    /// Ends the cut of `text` from `widget` of `id`, which the clipboard took.
    fn cut(&mut self, id: SurfaceId, widget: WidgetId, text: &str) {
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        if let Some(event) = surface.ui.cut(widget, text) {
            self.changed(id, Some(event));
        }
    }

    /// Offers `text` as the primary selection, through a new source each time: one the
    /// compositor refused, for a serial it no longer takes, would never be cancelled, and
    /// would hold the text no one can read. Without the protocol nothing is offered.
    fn select(&mut self, id: SurfaceId, text: String) {
        let clipboards = &mut self.clipboards;
        let (Some(manager), Some(device)) =
            (&clipboards.primary_manager, &clipboards.primary_device)
        else {
            return;
        };
        let Some(serial) = self.shell.grab.serial() else {
            eprintln!("athanor-toolkit: nothing selected: no key or button to select with");
            return;
        };
        let source = manager.create_selection_source(&self.shell.qh, MIMES);
        source.set_selection(device, serial);
        // The source it replaces is destroyed as it drops.
        clipboards.selected = Some((source, text, id));
    }

    /// Whether the clipboard holds text a field can paste: what a field's context menu enables
    /// Paste with.
    pub(super) fn can_paste(&self) -> bool {
        let offer = self.clipboards.device.as_ref();
        let offer = offer.and_then(|d| d.data().selection_offer());
        offer.is_some_and(|o| o.with_mime_types(choose).is_some())
    }

    /// Ends the primary selection the fields of `id` offered, if it is still theirs: its source
    /// is destroyed, and with it the selection the compositor holds.
    pub(super) fn unselect(&mut self, id: SurfaceId) {
        let selected = self.clipboards.selected.as_ref();
        if selected.is_some_and(|(_, _, by)| *by == id) {
            self.clipboards.selected = None;
        }
    }

    /// Reads the clipboard's text, or the primary selection's, into `widget` of `id`, as the
    /// event loop finds it readable. An empty clipboard, or one without text, pastes nothing.
    fn paste(&mut self, id: SurfaceId, widget: WidgetId, primary: bool) {
        let clipboards = &self.clipboards;
        let pipe = if primary {
            let offer = clipboards.primary_device.as_ref();
            let offer = offer.and_then(|d| d.data().selection_offer());
            offer.and_then(|o| {
                let mime = o.with_mime_types(choose)?;
                Some(o.receive(mime.into()).map_err(|e| e.to_string()))
            })
        } else {
            let offer = clipboards.device.as_ref();
            let offer = offer.and_then(|d| d.data().selection_offer());
            offer.and_then(|o| {
                let mime = o.with_mime_types(choose)?;
                Some(o.receive(mime.into()).map_err(|e| e.to_string()))
            })
        };
        let pipe = match pipe {
            Some(Ok(pipe)) => pipe,
            Some(Err(error)) => {
                eprintln!("athanor-toolkit: nothing pasted: {error}");
                return;
            }
            None => return,
        };
        let read = read_paste(
            &self.loop_handle,
            &mut self.clipboards.pending,
            |app| &mut app.clipboards.pending,
            pipe,
            PASTE_TIMEOUT,
            move |app, text| app.pasted(id, widget, &text),
        );
        if let Err(error) = read {
            eprintln!("athanor-toolkit: nothing pasted: {error}");
        }
    }

    /// Puts the text read from a clipboard into `widget` of `id`.
    fn pasted(&mut self, id: SurfaceId, widget: WidgetId, text: &str) {
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        if let Some(event) = surface.ui.paste(widget, text) {
            self.changed(id, Some(event));
        }
    }

    /// Writes `text` to a reader's pipe through the event loop; a source no longer offered
    /// writes nothing, and the pipe closes at once.
    fn offer(&mut self, text: Option<String>, pipe: WritePipe) {
        let Some(text) = text else {
            return;
        };
        if let Err(error) = write_offer(&self.loop_handle, text, pipe, OFFER_TIMEOUT) {
            eprintln!("athanor-toolkit: the clipboard's reader took no text: {error}");
        }
    }
}

impl DataDeviceHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        _: f64,
        _: f64,
        _: &WlSurface,
    ) {
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}

    // The offer is asked for at the paste.
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataOfferHandler for App {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for App {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &WlDataSource,
        _: String,
        pipe: WritePipe,
    ) {
        let copied = self.clipboards.copied.as_ref();
        let text = copied
            .filter(|(s, _)| s.inner() == source)
            .map(|(_, t)| t.clone());
        self.offer(text, pipe);
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        let copied = self.clipboards.copied.as_ref();
        if copied.is_some_and(|(s, _)| s.inner() == source) {
            self.clipboards.copied = None;
        }
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

impl PrimarySelectionDeviceHandler for App {
    // The offer is asked for at the paste.
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpPrimarySelectionDeviceV1,
    ) {
    }
}

impl PrimarySelectionSourceHandler for App {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &ZwpPrimarySelectionSourceV1,
        _: String,
        pipe: WritePipe,
    ) {
        let selected = self.clipboards.selected.as_ref();
        let text = selected
            .filter(|(s, ..)| s.inner() == source)
            .map(|(_, t, _)| t.clone());
        self.offer(text, pipe);
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &ZwpPrimarySelectionSourceV1,
    ) {
        let selected = self.clipboards.selected.as_ref();
        if selected.is_some_and(|(s, ..)| s.inner() == source) {
            self.clipboards.selected = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_type_of_text_is_read() {
        let offered = |m: &[&str]| m.iter().map(|m| m.to_string()).collect::<Vec<_>>();
        assert_eq!(
            choose(&offered(&["text/plain", "UTF8_STRING"])),
            Some("UTF8_STRING")
        );
        assert_eq!(
            choose(&offered(&["text/plain;charset=utf-8", "text/plain"])),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(choose(&offered(&["text/plain"])), Some("text/plain"));
        assert_eq!(choose(&offered(&["image/png", "text/html"])), None);
    }

    /// Reads `pipe` until it gives the text or an error.
    fn read_all(mut pipe: impl Read) -> Result<String, String> {
        let mut incoming = Incoming::default();
        loop {
            if let Some(done) = incoming.read(&mut pipe) {
                return done;
            }
        }
    }

    #[test]
    fn a_paste_is_utf_8_text_of_at_most_its_bound() -> io::Result<()> {
        let (reader, mut writer) = io::pipe()?;
        writer.write_all("héllo".as_bytes())?;
        drop(writer);
        assert_eq!(read_all(reader), Ok("héllo".into()));
        let not_utf8 = read_all(&[b'a', 0xff][..]);
        assert!(not_utf8.is_err_and(|e| e.contains("not UTF-8")));
        let long = read_all(io::repeat(b'a'));
        assert!(long.is_err_and(|e| e.contains("longer than")));
        assert_eq!(
            read_all(&vec![b'a'; READ_MAX][..]).map(|t| t.len()),
            Ok(READ_MAX)
        );
        Ok(())
    }

    /// A reader that takes three bytes a write, or none and says so.
    struct Slow(Vec<u8>, bool);

    impl Write for Slow {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            assert!(buf.len() <= PIPE_BUF, "{} bytes in one write", buf.len());
            self.1 = !self.1;
            if self.1 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let n = buf.len().min(3);
            self.0.extend_from_slice(&buf[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_offer_is_written_a_pipe_buffer_at_a_time_as_the_reader_takes_it() {
        let text = "x".repeat(PIPE_BUF + 10);
        let mut outgoing = Outgoing {
            text: text.clone().into_bytes(),
            written: 0,
        };
        let mut reader = Slow(Vec::new(), false);
        let done = loop {
            if let Some(done) = outgoing.write(&mut reader) {
                break done;
            }
        };
        assert!(done.is_ok());
        assert_eq!(reader.0, text.as_bytes());
    }

    #[test]
    fn a_reader_that_closes_early_ends_the_offer_and_not_the_process() -> io::Result<()> {
        let (reader, writer) = io::pipe()?;
        drop(reader);
        let mut outgoing = Outgoing {
            text: b"abc".to_vec(),
            written: 0,
        };
        let done = outgoing.write(&writer).map(|r| r.map_err(|e| e.kind()));
        assert_eq!(done, Some(Err(io::ErrorKind::BrokenPipe)));
        Ok(())
    }

    use smithay_client_toolkit::reexports::calloop::EventLoop;
    use std::error::Error;
    use std::os::fd::OwnedFd;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    /// The loop's data of the pastes below: the paste pending, and the texts pasted.
    #[derive(Default)]
    struct Pastes {
        pending: Option<Pending>,
        pasted: Vec<String>,
    }

    /// Starts a paste from a new pipe, as the clipboard's owner would get it; returns the
    /// owner's end.
    fn paste(
        event_loop: &EventLoop<'static, Pastes>,
        pastes: &mut Pastes,
        timeout: Duration,
    ) -> Result<io::PipeWriter, Box<dyn Error>> {
        let (reader, writer) = io::pipe()?;
        let pipe = ReadPipe::from(OwnedFd::from(reader));
        read_paste(
            &event_loop.handle(),
            &mut pastes.pending,
            |pastes| &mut pastes.pending,
            pipe,
            timeout,
            |pastes, text| pastes.pasted.push(text),
        )?;
        Ok(writer)
    }

    /// Runs the loop until no paste is pending, for at most a second.
    fn settle(
        event_loop: &mut EventLoop<'static, Pastes>,
        pastes: &mut Pastes,
    ) -> Result<(), Box<dyn Error>> {
        for _ in 0..20 {
            if pastes.pending.is_none() {
                break;
            }
            event_loop.dispatch(Some(Duration::from_millis(50)), pastes)?;
        }
        Ok(())
    }

    /// Whether the paste's end of `owner` is closed: a write to it fails with `EPIPE`.
    fn closed(owner: &mut io::PipeWriter) -> bool {
        owner
            .write(b"late")
            .is_err_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
    }

    #[test]
    fn a_paste_whose_owner_never_closes_its_pipe_is_dropped_at_its_deadline(
    ) -> Result<(), Box<dyn Error>> {
        let mut event_loop = EventLoop::try_new()?;
        let mut pastes = Pastes::default();
        let mut owner = paste(&event_loop, &mut pastes, Duration::from_millis(100))?;
        owner.write_all(b"never ends")?;
        settle(&mut event_loop, &mut pastes)?;
        assert!(pastes.pending.is_none());
        assert!(pastes.pasted.is_empty());
        assert!(closed(&mut owner), "the pipe is closed");
        Ok(())
    }

    #[test]
    fn a_new_paste_replaces_the_one_not_yet_read() -> Result<(), Box<dyn Error>> {
        let mut event_loop = EventLoop::try_new()?;
        let mut pastes = Pastes::default();
        let mut first = paste(&event_loop, &mut pastes, Duration::from_secs(10))?;
        let mut second = paste(&event_loop, &mut pastes, Duration::from_secs(10))?;
        assert!(closed(&mut first), "the first pipe is closed");
        second.write_all(b"second")?;
        drop(second);
        settle(&mut event_loop, &mut pastes)?;
        assert!(
            pastes.pending.is_none(),
            "the paste and its deadline are gone"
        );
        assert_eq!(pastes.pasted, ["second"]);
        // A later paste stays pending while its owner sends nothing, before its deadline.
        let mut third = paste(&event_loop, &mut pastes, Duration::from_secs(10))?;
        event_loop.dispatch(Some(Duration::from_millis(50)), &mut pastes)?;
        assert!(pastes.pending.is_some() && !closed(&mut third));
        Ok(())
    }

    /// A text four times the size of a pipe's buffer, which a reader must read for the writer to
    /// finish.
    fn long_text() -> String {
        "x".repeat(4 * 64 * 1024)
    }

    /// Offers `text` through a new pipe, as a reader would ask for it; returns the reader's end.
    fn offer(
        event_loop: &EventLoop<'static, ()>,
        text: String,
        timeout: Duration,
    ) -> Result<io::PipeReader, Box<dyn Error>> {
        let (reader, writer) = io::pipe()?;
        let pipe = WritePipe::from(OwnedFd::from(writer));
        write_offer(&event_loop.handle(), text, pipe, timeout)?;
        Ok(reader)
    }

    /// Runs the loop for `time`.
    fn run(event_loop: &mut EventLoop<'static, ()>, time: Duration) -> Result<(), Box<dyn Error>> {
        let start = Instant::now();
        while start.elapsed() < time {
            event_loop.dispatch(Some(Duration::from_millis(10)), &mut ())?;
        }
        Ok(())
    }

    /// Reads `reader` to its end in a thread while the loop runs, for at most a second: the
    /// bytes read, or `None` while the writer keeps its end open.
    fn drain(
        event_loop: &mut EventLoop<'static, ()>,
        mut reader: io::PipeReader,
    ) -> Result<Option<usize>, Box<dyn Error>> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut text = Vec::new();
            // The test may have stopped waiting, and dropped the receiver.
            let _ = sender.send(reader.read_to_end(&mut text).map(|_| text.len()));
        });
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            if let Ok(read) = receiver.try_recv() {
                return Ok(Some(read?));
            }
            event_loop.dispatch(Some(Duration::from_millis(10)), &mut ())?;
        }
        Ok(None)
    }

    #[test]
    fn an_offer_whose_reader_takes_nothing_is_dropped_at_its_deadline() -> Result<(), Box<dyn Error>>
    {
        let mut event_loop = EventLoop::try_new()?;
        let text = long_text();
        let reader = offer(&event_loop, text.clone(), Duration::from_millis(100))?;
        // The pipe fills, and its reader takes nothing past the deadline.
        run(&mut event_loop, Duration::from_millis(300))?;
        let read = drain(&mut event_loop, reader)?;
        assert!(
            read.is_some_and(|read| read < text.len()),
            "the pipe closes with the text unwritten, read {read:?}"
        );
        Ok(())
    }

    #[test]
    fn a_reader_that_takes_the_text_gets_all_of_it() -> Result<(), Box<dyn Error>> {
        let mut event_loop = EventLoop::try_new()?;
        let text = long_text();
        let reader = offer(&event_loop, text.clone(), Duration::from_secs(10))?;
        assert_eq!(drain(&mut event_loop, reader)?, Some(text.len()));
        Ok(())
    }
}
