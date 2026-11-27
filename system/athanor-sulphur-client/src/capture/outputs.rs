//! Every output's picture for the portal's screenshot (doc_portal.md, PT8), through
//! ext-image-copy-capture-v1 and the output capture source. The rest of the capture machinery,
//! shared with a window's, is in `capture.rs`. The output capture source is named here and
//! nowhere else in the crate: a binary that never calls `Client::capture_outputs` never
//! binds it, and `verify.py allow-list` derives its grant from this file alone.

use std::time::Duration;

use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::image_capture_source::v1::client::ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

use super::{bound, Forget, Pending, Picture};
use crate::connection::{fresh, Client, Error, State};

/// An output's frame waits for the compositor's next repaint of the outputs, which an idle
/// output may delay; a screenshot is not a preview, so it waits longer.
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(2);

/// An output's picture, and where the output lies in the layout, in logical pixels.
#[derive(Clone, Debug)]
pub struct OutputPicture {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The picture's `wl_output.transform`, 0 to 7: an output is copied as it is scanned out.
    pub transform: u32,
    pub picture: Picture,
}

/// The output capture globals, bound at the first `Client::capture_outputs`.
#[derive(Default)]
pub(super) struct Globals {
    sources: Option<ExtOutputImageCaptureSourceManagerV1>,
    xdg_outputs: Option<ZxdgOutputManagerV1>,
}

impl Client {
    /// Every output's picture, without the pointer, in the order of their registry names,
    /// with the output's place in the layout. It fails when any output's does: a screenshot
    /// with an output missing is not answered.
    pub async fn capture_outputs(&self) -> Result<Vec<OutputPicture>, Error> {
        self.live()?;
        let ids = {
            let mut state = self.inner.state.borrow_mut();
            let state = &mut *state;
            let (registry, qh) = (&self.inner.registry, &self.inner.qh);
            let sources = bound(&mut state.globals.capture.outputs.sources, registry, qh, 1..=1)
                .ok_or(Error::Unavailable("ext_output_image_capture_source_manager_v1"))?;
            let manager = bound(&mut state.globals.capture.image_copy, registry, qh, 1..=1)
                .ok_or(Error::Unavailable("ext_image_copy_capture_manager_v1"))?;
            let xdg_outputs = bound(&mut state.globals.capture.outputs.xdg_outputs, registry, qh, 1..=3)
                .ok_or(Error::Unavailable("zxdg_output_manager_v1"))?;
            if state.globals.shm.is_none() {
                return Err(Error::Unavailable("wl_shm"));
            }
            let outputs = state.wl_outputs();
            if outputs.is_empty() {
                return Err(Error::Capture("no output"));
            }
            let mut ids = Vec::with_capacity(outputs.len());
            for output in &outputs {
                let id = fresh();
                // Its logical place is sent at creation, so before the frame is ready.
                let xdg_output = xdg_outputs.get_xdg_output(output, qh, id);
                let source = sources.create_source(output, qh, ());
                let pending = Pending::new(None, source, &manager, Some(xdg_output), id, qh);
                state.captures.insert(id, pending);
                ids.push(id);
            }
            ids
        };
        // Removes every capture when the call ends or is dropped before it does.
        let guards: Vec<_> = ids.iter().map(|&id| Forget { client: self, id }).collect();
        let answers =
            futures_util::future::join_all(ids.iter().map(|&id| self.taken(id, OUTPUT_TIMEOUT)))
                .await;
        drop(guards);
        self.flush()?;
        answers
            .into_iter()
            .map(|answer| {
                let taken = answer?;
                let ((x, y), (width, height)) = taken
                    .position
                    .zip(taken.logical_size)
                    .ok_or(Error::Capture("the output's place in the layout is unknown"))?;
                Ok(OutputPicture {
                    x,
                    y,
                    width,
                    height,
                    transform: taken.transform,
                    picture: taken.picture,
                })
            })
            .collect()
    }
}

impl Dispatch<ZxdgOutputV1, u64> for State {
    fn event(
        state: &mut State,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(pending) = state.captures.get_mut(id) else { return };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => pending.position = Some((x, y)),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                pending.logical_size = Some((width, height));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ZxdgOutputManagerV1);
