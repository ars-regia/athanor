//! The nested backend: a window of the parent compositor is the output; its events are input,
//! resizes and the redraws that paint the space.

use crate::headless::report_frame_times;
use crate::state::{draw_view, new_output, Backend, State, View};
use crate::Error;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::backend::winit::{WinitEvent, WinitGraphicsBackend};
use smithay::desktop::layer_map_for_output;
use smithay::output::{Mode, Output};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::Transform;
use std::time::{Duration, Instant};

/// The output's refresh, in mHz: the parent compositor paces the redraws.
const REFRESH: i32 = 60_000;

/// The parent's window, the one output it shows and that output's damage.
pub struct Nested {
    pub backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage_tracker: OutputDamageTracker,
    /// A redraw with nothing drawn has scheduled the next one: further ones do not add another.
    frame_scheduled: bool,
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
        frame_scheduled: false,
    };
    let mut state = State::new(event_loop, display, Backend::Nested(Box::new(nested)))?;
    state.session_calls = crate::session_calls::start(state.lock_epoch.clone());
    state.map_output(&output);
    state.dmabuf_state.create_global::<State>(&dh, formats);
    report_frame_times(event_loop, &mut state)?;

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
                state.output_heads_changed();
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
        let popups = self.composing.popups_on(&self.space, &output);
        let pointer = self.seat.get_pointer().map(|p| p.current_location());
        let look = self
            .a11y
            .look(&output, self.space.output_geometry(&output), pointer);
        let age = nested.backend.buffer_age().unwrap_or(0);
        let rendered = match nested.backend.bind() {
            Ok((renderer, mut framebuffer)) => {
                let view = View::of(&self.space, self.lock.as_ref(), &popups, &output);
                let started = Instant::now();
                let drawn = draw_view(
                    renderer,
                    &mut framebuffer,
                    &output,
                    age,
                    view,
                    &mut nested.damage_tracker,
                    look,
                );
                // The draw alone, as on the headless backend: the submit waits on the parent.
                if let (Ok(_), Some(stats)) = (&drawn, &mut self.frame_stats) {
                    stats.drew(started.elapsed());
                }
                drawn
                    .map(|result| (result.damage.cloned(), result.states))
                    .map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        let shown = match &rendered {
            Ok((Some(damage), _)) => nested
                .backend
                .submit(Some(damage))
                .inspect_err(|error| tracing::warn!(%error, "a frame could not be submitted"))
                .is_ok(),
            Ok((None, _)) => true,
            Err(error) => {
                tracing::warn!(%error, "the output could not be drawn");
                false
            }
        };
        if !shown {
            // The tracker counts a failed frame as shown: a new one draws the next frame whole,
            // so a lock is confirmed only once it is on screen.
            nested.damage_tracker = OutputDamageTracker::from_output(&output);
        }
        let presented = shown && matches!(rendered, Ok((Some(_), _)));
        if let Ok((_, states)) = rendered {
            self.frame_done(&output, &states, presented, frame_target, frame_period);
        }
        self.serve_captures();
        // A lock is confirmed only once the output shows it: drawn and submitted.
        if shown {
            self.locked_drawn();
        }
        self.space.refresh();
        self.send_toplevel_states();
        self.popups.cleanup();
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
        // A presented frame is paced by the parent; one with nothing to draw is not, and asking
        // for the next at once spins a core. It waits a frame period instead.
        // ponytail: an idle output still wakes at 60 Hz; redraw on damage only if that shows.
        // A resize or an input event redraws as well: one timer at a time, or each such redraw
        // would start another chain and the idle rate would double.
        if presented {
            request_redraw(self);
        } else if let Backend::Nested(nested) = &mut self.backend {
            if !nested.frame_scheduled {
                let timer = Timer::from_duration(frame_period);
                match self.loop_handle.insert_source(timer, |_, _, state| {
                    if let Backend::Nested(nested) = &mut state.backend {
                        nested.frame_scheduled = false;
                    }
                    request_redraw(state);
                    TimeoutAction::Drop
                }) {
                    Ok(_) => nested.frame_scheduled = true,
                    // The next parent event redraws: asking at once would spin, as above.
                    Err(error) => {
                        tracing::warn!(error = %error.error, "the next frame could not be scheduled")
                    }
                }
            }
        }
    }
}

fn request_redraw(state: &State) {
    if let Backend::Nested(nested) = &state.backend {
        nested.backend.window().request_redraw();
    }
}
