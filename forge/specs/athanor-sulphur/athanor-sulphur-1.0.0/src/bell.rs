//! xdg_system_bell_v1 (doc_own_stack.md decision 33, `standard`): a bell is shown on the focus
//! outline of decision 31, never sounded.
//!
//! - It is honoured only from the client of the window with the keyboard, at most once a
//!   second, and never while the session is locked; and only when that window has an outline
//!   to show it on: not maximised or full screen, with `active_hint` on and a width above 0.
//!   Every other bell is dropped, and a dropped bell does not count towards the second.
//! - The outline blinks once: hidden for `BLINK`, then drawn again, with no transition. Under
//!   reduced motion (doc_accessibility.md AX9) it is drawn at twice its width for `WIDEN`
//!   instead.

use crate::focus_theme::Outline;
use crate::state::State;
use smithay::desktop::Window;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::wayland::xdg_system_bell::{XdgSystemBellHandler, XdgSystemBellState};
use std::time::{Duration, Instant};

/// How long a blink hides the outline.
pub(crate) const BLINK: Duration = Duration::from_millis(250);
/// How long, under reduced motion, the outline is drawn at twice its width.
pub(crate) const WIDEN: Duration = Duration::from_millis(500);
/// The least time between two honoured bells.
pub(crate) const GAP: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct Bell {
    /// When the last honoured bell rang.
    pub(crate) rung: Option<Instant>,
    /// The window a bell is shown on, until when, and whether it is widened.
    cue: Option<(Window, Instant, bool)>,
    /// doc_accessibility.md AX9's reduced motion: the outline widens instead of blinking.
    pub(crate) reduced_motion: bool,
}

/// Creates the global.
pub fn global(dh: &DisplayHandle) {
    XdgSystemBellState::new::<State>(dh);
}

impl Bell {
    /// The outline `window` is drawn with, `outline` without a bell.
    pub(crate) fn outline(&self, window: &Window, outline: Option<Outline>) -> Option<Outline> {
        match &self.cue {
            Some((cued, _, widened)) if cued == window => {
                outline.filter(|_| *widened).map(|outline| Outline {
                    width: outline.width * 2,
                    ..outline
                })
            }
            _ => outline,
        }
    }
}

impl XdgSystemBellHandler for State {
    fn ring(&mut self, surface: Option<WlSurface>) {
        let Some(client) = surface.and_then(|surface| surface.client()) else {
            return;
        };
        let Some(window) = self.focused_window() else {
            return;
        };
        let focused = window.toplevel().and_then(|t| t.wl_surface().client());
        if self.lock.is_some()
            || focused.map(|focused| focused.id()) != Some(client.id())
            || !window.toplevel().is_some_and(crate::marks::marked)
            || self.focus_theme.outline().is_none()
            || self.bell.rung.is_some_and(|rung| rung.elapsed() < GAP)
        {
            return;
        }
        let widened = self.bell.reduced_motion;
        let lasts = if widened { WIDEN } else { BLINK };
        let until = Instant::now() + lasts;
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(lasts), move |_, _, state| {
                    if state
                        .bell
                        .cue
                        .as_ref()
                        .is_some_and(|(_, at, _)| *at <= Instant::now())
                    {
                        state.bell.cue = None;
                        state.request_redraw();
                    }
                    TimeoutAction::Drop
                });
        if let Err(error) = timer {
            tracing::warn!(%error, "the bell has no timer: it is not shown");
            return;
        }
        self.bell.rung = Some(Instant::now());
        self.bell.cue = Some((window, until, widened));
        self.request_redraw();
    }
}
