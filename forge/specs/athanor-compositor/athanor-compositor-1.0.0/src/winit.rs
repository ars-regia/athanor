//! The nested backend: a window of the parent compositor is the output; its events are input,
//! resizes and the redraws that paint the space.

use crate::state::State;
use crate::Error;
use smithay::backend::renderer::element::default_primary_scanout_output_compare;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{WinitEvent, WinitEventLoop};
use smithay::desktop::space::render_output;
use smithay::desktop::utils::{
    surface_presentation_feedback_flags_from_states, surface_primary_scanout_output,
    update_surface_primary_scanout_output, OutputPresentationFeedback,
};
use smithay::output::Mode;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::wayland::presentation::Refresh;
use std::time::Duration;

/// The output's refresh, in mHz: the parent compositor paces the redraws.
const REFRESH: i32 = 60_000;

const BACKGROUND: [f32; 4] = [0.1, 0.1, 0.1, 1.0];

pub fn listen(handle: &LoopHandle<'static, State>, events: WinitEventLoop) -> Result<(), Error> {
    handle
        .insert_source(events, |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                let mode = Mode {
                    size,
                    refresh: REFRESH,
                };
                state
                    .output
                    .change_current_state(Some(mode), None, None, None);
                state.output.set_preferred(mode);
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => state.redraw(),
            WinitEvent::CloseRequested => state.loop_signal.stop(),
            WinitEvent::Focus(_) => {}
        })
        .map_err(|e| e.error)?;
    Ok(())
}

impl State {
    /// Paints the space, tells the clients what was presented and asks for their next frame.
    fn redraw(&mut self) {
        let frame_period = Duration::from_secs_f64(1_000.0 / f64::from(REFRESH));
        let frame_target = self.clock.now() + frame_period;
        let age = self.backend.buffer_age().unwrap_or(0);
        let rendered = match self.backend.bind() {
            Ok((renderer, mut framebuffer)) => {
                render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                    &self.output,
                    renderer,
                    &mut framebuffer,
                    1.0,
                    age,
                    [&self.space],
                    &[],
                    &mut self.damage_tracker,
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
                    if let Err(error) = self.backend.submit(Some(damage)) {
                        tracing::warn!(%error, "a frame could not be submitted");
                    }
                }
                for window in self.space.elements() {
                    window.with_surfaces(|surface, surface_states| {
                        update_surface_primary_scanout_output(
                            surface,
                            &self.output,
                            surface_states,
                            None,
                            &states,
                            default_primary_scanout_output_compare,
                        );
                    });
                }
                if damage.is_some() {
                    let mut feedback = OutputPresentationFeedback::new(&self.output);
                    for window in self.space.elements() {
                        window.take_presentation_feedback(
                            &mut feedback,
                            surface_primary_scanout_output,
                            |surface, _| {
                                surface_presentation_feedback_flags_from_states(
                                    surface, None, &states,
                                )
                            },
                        );
                    }
                    feedback.presented(
                        frame_target,
                        Refresh::fixed(frame_period),
                        0,
                        wp_presentation_feedback::Kind::Vsync,
                    );
                }
            }
            Err(error) => tracing::warn!(%error, "the output could not be drawn"),
        }

        let output = self.output.clone();
        for window in self.space.elements() {
            window.send_frame(&output, frame_target, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        self.space.refresh();
        self.popups.cleanup();
        if let Err(error) = self.display_handle.flush_clients() {
            tracing::warn!(%error, "the clients could not be flushed");
        }
        self.backend.window().request_redraw();
    }
}
