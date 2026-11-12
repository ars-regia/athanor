//! wp_fifo_manager_v1 and wp_commit_timing_manager_v1 (doc_own_stack.md decision 33,
//! standard): when a client's content updates are applied.
//!
//! A commit that waits on its surface's FIFO barrier is applied once a frame has been drawn
//! after the commit that set the barrier: `State::frame_done` clears the barrier of the
//! current content of each FIFO surface shown on the output it drew, and of one that no output
//! shows, so that a minimised or hidden window still makes progress, as the protocol allows.
//! A commit that carries a timestamp is held until that time on the presentation clock, then
//! applied (`State::hold_until`).

use crate::state::State;
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::{Monotonic, Time};
use smithay::wayland::commit_timing::{
    CommitTimerBarrierStateUserData, CommitTimerStateUserData, CommitTimingManagerState, Timestamp,
};
use smithay::wayland::compositor::{with_states, CompositorHandler};
use smithay::wayland::fifo::{FifoBarrierCachedState, FifoCachedState, FifoManagerState};

/// Offers both globals to every client. Smithay holds a FIFO commit on its barrier itself; the
/// timestamps are the compositor's to hold, so that a commit is applied at its time whether or
/// not an output draws then.
pub fn globals(dh: &DisplayHandle) {
    FifoManagerState::new::<State>(dh);
    CommitTimingManagerState::unmanaged::<State>(dh);
}

/// Signals every barrier of `surface` whose timestamp is `now` or earlier.
pub fn timestamps_due(surface: &WlSurface, now: Time<Monotonic>) {
    with_states(surface, |states| {
        if let Some(barriers) = states.data_map.get::<CommitTimerBarrierStateUserData>() {
            if let Ok(mut barriers) = barriers.lock() {
                barriers.signal_until(now);
            }
        }
    });
}

impl State {
    /// Before a commit of `surface` is applied: remembers the surface if the commit waits on
    /// its FIFO barrier, and holds the commit until its timestamp if it carries one.
    pub(crate) fn pace(&mut self, surface: &WlSurface) {
        let (waits, timestamp) = with_states(surface, |states| {
            let waits = states
                .cached_state
                .get::<FifoCachedState>()
                .pending()
                .wait_barrier;
            let timestamp = states
                .data_map
                .get::<CommitTimerStateUserData>()
                .and_then(|timer| timer.borrow_mut().timestamp.take());
            (waits, timestamp)
        });
        if waits && !self.fifo_surfaces.contains(surface) {
            self.fifo_surfaces.retain(WlSurface::is_alive);
            self.fifo_surfaces.push(surface.clone());
        }
        if let Some(timestamp) = timestamp {
            self.hold_to(surface, timestamp);
        }
    }

    fn hold_to(&mut self, surface: &WlSurface, timestamp: Timestamp) {
        let barrier = with_states(surface, |states| {
            let barriers = states
                .data_map
                .get_or_insert(CommitTimerBarrierStateUserData::default);
            barriers
                .lock()
                .ok()
                .map(|mut barriers| barriers.register(timestamp))
        });
        let Some(barrier) = barrier else {
            return;
        };
        // ponytail: the commit is applied at its timestamp, so it shows at the first refresh
        // after it; apply it at the refresh before when a client needs presentation on time.
        let wait = Time::elapsed(&self.clock.now(), Time::<Monotonic>::from(timestamp));
        self.hold_until(surface, barrier, wait);
    }

    /// After a frame of `output`: clears the FIFO barrier of each remembered surface that
    /// `output` shows or that no output shows, and tries the commits of their clients.
    pub(crate) fn fifo_frame_done(&mut self, output: &Output) {
        self.fifo_surfaces.retain(WlSurface::is_alive);
        let mut cleared: Vec<Client> = Vec::new();
        for surface in &self.fifo_surfaces {
            let primary = with_states(surface, |states| {
                smithay::desktop::utils::surface_primary_scanout_output(surface, states)
            });
            let here = match primary {
                Some(primary) => {
                    primary == *output || !self.space.outputs().any(|shown| *shown == primary)
                }
                None => true,
            };
            if !here {
                continue;
            }
            let barrier = with_states(surface, |states| {
                states
                    .cached_state
                    .get::<FifoBarrierCachedState>()
                    .current()
                    .barrier
                    .take()
            });
            if let Some(barrier) = barrier {
                barrier.signal();
                if let Some(client) = surface.client() {
                    if !cleared.contains(&client) {
                        cleared.push(client);
                    }
                }
            }
        }
        let dh = self.display_handle.clone();
        for client in cleared {
            self.client_compositor_state(&client)
                .blocker_cleared(self, &dh);
        }
    }
}
