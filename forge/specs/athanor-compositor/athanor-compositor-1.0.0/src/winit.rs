//! The nested backend: a window of the parent compositor is the output; its events are input,
//! resizes and the redraws that paint the space.

use crate::state::{new_output, Backend, State};
use crate::Error;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::backend::winit::{WinitEvent, WinitGraphicsBackend};
use smithay::desktop::layer_map_for_output;
use smithay::desktop::space::render_output;
use smithay::output::{Mode, Output};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::Transform;
use std::time::Duration;

/// The output's refresh, in mHz: the parent compositor paces the redraws.
const REFRESH: i32 = 60_000;

const BACKGROUND: [f32; 4] = [0.1, 0.1, 0.1, 1.0];

/// The parent's window, the one output it shows and that output's damage.
pub struct Nested {
    pub backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage_tracker: OutputDamageTracker,
}

/// Opens the window, its output at `scale`, and linux-dmabuf version 3 with the renderer's
/// formats.
// ponytail: no version 4 feedback here; the winit backend does not name its render node.
pub fn start(
    event_loop: &mut EventLoop<'static, State>,
    display: Display<State>,
    scale: f64,
) -> Result<State, Error> {
    let (mut backend, events) = smithay::backend::winit::init::<GlesRenderer>()?;
    let dh = display.handle();
    // Flipped180: the winit backend's framebuffer is upside down.
    let output = new_output(
        &dh,
        "athanor-0",
        "Nested",
        backend.window_size(),
        REFRESH,
        Transform::Flipped180,
        scale,
    );
    let formats = backend.renderer().dmabuf_formats();
    let nested = Nested {
        backend,
        output: output.clone(),
        damage_tracker: OutputDamageTracker::from_output(&output),
    };
    let mut state = State::new(event_loop, display, Backend::Nested(Box::new(nested)))?;
    state.map_output(&output);
    state.dmabuf_state.create_global::<State>(&dh, formats);

    event_loop
        .handle()
        .insert_source(events, |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                if let Backend::Nested(nested) = &state.backend {
                    let mode = Mode {
                        size,
                        refresh: REFRESH,
                    };
                    nested
                        .output
                        .change_current_state(Some(mode), None, None, None);
                    nested.output.set_preferred(mode);
                    layer_map_for_output(&nested.output).arrange();
                }
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => state.redraw(),
            WinitEvent::CloseRequested => state.loop_signal.stop(),
            WinitEvent::Focus(_) => {}
        })
        .map_err(|e| e.error)?;
    Ok(state)
}

impl State {
    /// Paints the space, tells the clients what was presented and asks for their next frame.
    fn redraw(&mut self) {
        let Backend::Nested(nested) = &mut self.backend else {
            return;
        };
        let frame_period = Duration::from_secs_f64(1_000.0 / f64::from(REFRESH));
        let frame_target = self.clock.now() + frame_period;
        let output = nested.output.clone();
        let age = nested.backend.buffer_age().unwrap_or(0);
        let rendered = match nested.backend.bind() {
            Ok((renderer, mut framebuffer)) => {
                render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                    &output,
                    renderer,
                    &mut framebuffer,
                    1.0,
                    age,
                    [&self.space],
                    &[],
                    &mut nested.damage_tracker,
                    BACKGROUND,
                )
                .map(|result| (result.damage.cloned(), result.states))
                .map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        match rendered {
            Ok((damage, states)) => {
                if let Some(damage) = &damage {
                    if let Err(error) = nested.backend.submit(Some(damage)) {
                        tracing::warn!(%error, "a frame could not be submitted");
                    }
                }
                self.frame_done(
                    &output,
                    &states,
                    damage.is_some(),
                    frame_target,
                    frame_period,
                );
            }
            Err(error) => tracing::warn!(%error, "the output could not be drawn"),
        }
        self.space.refresh();
        self.popups.cleanup();
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
        if let Backend::Nested(nested) = &self.backend {
            nested.backend.window().request_redraw();
        }
    }
}
