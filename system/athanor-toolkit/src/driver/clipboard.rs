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
    DataDeviceManagerState, WritePipe,
};
use smithay_client_toolkit::primary_selection::{
    device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler},
    selection::{PrimarySelectionSource, PrimarySelectionSourceHandler},
    PrimarySelectionManagerState,
};
use smithay_client_toolkit::reexports::calloop::PostAction;
use smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
};
use std::io::{self, Read, Write};
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

/// The seat's clipboards, as far as the compositor has them.
#[derive(Default)]
pub(super) struct Clipboards {
    manager: Option<DataDeviceManagerState>,
    primary_manager: Option<PrimarySelectionManagerState>,
    device: Option<DataDevice>,
    primary_device: Option<PrimarySelectionDevice>,
    /// The text copied last, and the source that offers it until another one replaces it.
    copied: Option<(CopyPasteSource, String)>,
    /// The text selected last, and the source that offers it until another one replaces it.
    selected: Option<(PrimarySelectionSource, String)>,
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
                Clip::Copy(text) => self.copy(text),
                Clip::Select(text) => self.select(text),
                Clip::Paste(widget) => self.paste(id, widget, false),
                Clip::PastePrimary(widget) => self.paste(id, widget, true),
            }
        }
    }

    /// Offers `text` on the clipboard, with the serial of the key that copied it.
    fn copy(&mut self, text: String) {
        let clipboards = &mut self.clipboards;
        let (Some(manager), Some(device)) = (&clipboards.manager, &clipboards.device) else {
            eprintln!("athanor-toolkit: nothing copied: the compositor has no clipboard");
            return;
        };
        let Some(serial) = self.shell.grab.serial() else {
            return;
        };
        let source = manager.create_copy_paste_source(&self.shell.qh, MIMES);
        source.set_selection(device, serial);
        clipboards.copied = Some((source, text));
    }

    /// Offers `text` as the primary selection: the source already offered takes the new text,
    /// until another client's selection cancels it.
    fn select(&mut self, text: String) {
        let clipboards = &mut self.clipboards;
        if let Some((_, selected)) = &mut clipboards.selected {
            *selected = text;
            return;
        }
        let (Some(manager), Some(device), Some(serial)) = (
            &clipboards.primary_manager,
            &clipboards.primary_device,
            self.shell.grab.serial(),
        ) else {
            return;
        };
        let source = manager.create_selection_source(&self.shell.qh, MIMES);
        source.set_selection(device, serial);
        clipboards.selected = Some((source, text));
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
        let mut incoming = Incoming::default();
        let read = self.loop_handle.insert_source(pipe, move |(), pipe, app| {
            match incoming.read(&**pipe) {
                None => PostAction::Continue,
                Some(Ok(text)) => {
                    app.pasted(id, widget, &text);
                    PostAction::Remove
                }
                Some(Err(error)) => {
                    eprintln!("athanor-toolkit: nothing pasted: {error}");
                    PostAction::Remove
                }
            }
        });
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

    /// Writes `text` to a reader's pipe as the event loop finds it writable, then closes it; a
    /// source no longer offered writes nothing, and the pipe closes at once.
    fn offer(&mut self, text: Option<String>, pipe: WritePipe) {
        let Some(text) = text else {
            return;
        };
        let mut outgoing = Outgoing {
            text: text.into_bytes(),
            written: 0,
        };
        let written = self.loop_handle.insert_source(pipe, move |(), pipe, _| {
            match outgoing.write(&**pipe) {
                None => PostAction::Continue,
                Some(Ok(())) => PostAction::Remove,
                Some(Err(error)) => {
                    eprintln!("athanor-toolkit: the clipboard's reader took no text: {error}");
                    PostAction::Remove
                }
            }
        });
        if let Err(error) = written {
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
            .filter(|(s, _)| s.inner() == source)
            .map(|(_, t)| t.clone());
        self.offer(text, pipe);
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &ZwpPrimarySelectionSourceV1,
    ) {
        let selected = self.clipboards.selected.as_ref();
        if selected.is_some_and(|(s, _)| s.inner() == source) {
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
}
