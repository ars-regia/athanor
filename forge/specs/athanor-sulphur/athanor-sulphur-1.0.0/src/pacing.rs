//! wp_fifo_manager_v1 and wp_commit_timing_manager_v1 (doc_own_stack.md decision 33,
//! standard): when a client's content updates are applied.
//!
//! A commit that waits on its surface's FIFO barrier is applied once a frame has been drawn
//! after the commit that set the barrier: `State::frame_done` clears the barrier of the
//! current content of each FIFO surface shown on the output it drew, and of one that no output
//! shows, so that a minimised or hidden window still makes progress, as the protocol allows.
//! A surface is remembered from the commit that sets its barrier, which asks for a frame, until
//! a frame clears it: an output that draws only on request still draws the frame a later commit
//! waiting on the barrier needs.
//!
//! A commit that carries a timestamp is held until that time on the presentation clock, then
//! applied (`State::hold_until`); one more than `MAX_TIMED_WAIT` ahead is applied after that
//! wait, and a timestamp that is not a time disconnects its client. Every commit a client has
//! waiting, on a barrier, a timestamp or an earlier commit, counts against `MAX_HELD_COMMITS`.

use crate::state::State;
use smithay::output::Output;
use smithay::reexports::rustix::time::Timespec;
use smithay::reexports::wayland_server::backend::protocol::ProtocolError;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::{Monotonic, Time};
use smithay::wayland::commit_timing::{
    CommitTimerBarrierStateUserData, CommitTimerStateUserData, CommitTimingManagerState, Timestamp,
};
use smithay::wayland::compositor::{with_states, CompositorHandler};
use smithay::wayland::fifo::{FifoBarrierCachedState, FifoManagerState};
use std::time::Duration;

/// The longest a commit waits for its timestamp. The protocol bounds no timestamp; a client
/// paces frames a few refreshes ahead, and a commit timed further than this is applied after
/// it rather than hold its surface, and its place among the client's waiting commits, for
/// longer.
pub const MAX_TIMED_WAIT: Duration = Duration::from_secs(1);

/// `wp_commit_timer_v1.error.invalid_timestamp`, the reason recorded for a client closed for a
/// timestamp that is not a time. Smithay keeps the timer object to itself, so the error cannot
/// be posted on it: the client is killed with the code, as `NO_MEMORY` is.
const INVALID_TIMESTAMP: u32 = 0;

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

/// Whether `timestamp` is a time: seconds not negative, nanoseconds under a second. Smithay
/// stores the request's fields as they come.
fn valid(timestamp: Timestamp) -> bool {
    let time = Timespec::from(Time::<Monotonic>::from(timestamp));
    time.tv_sec >= 0 && (0..1_000_000_000).contains(&time.tv_nsec)
}

impl State {
    /// Before a commit of `surface` is applied: holds the commit until its timestamp if it
    /// carries one.
    pub(crate) fn pace(&mut self, surface: &WlSurface) {
        let timestamp = with_states(surface, |states| {
            states
                .data_map
                .get::<CommitTimerStateUserData>()
                .and_then(|timer| timer.borrow_mut().timestamp.take())
        });
        let Some(timestamp) = timestamp else {
            return;
        };
        if !valid(timestamp) {
            if let Some(client) = surface.client() {
                self.invalid_timestamp(&client, timestamp);
            }
            return;
        }
        let now = self.clock.now();
        let timestamp = timestamp.min(Timestamp::from(now + MAX_TIMED_WAIT));
        // ponytail: the commit is applied at its timestamp, so it shows at the first refresh
        // after it; apply it at the refresh before when a client needs presentation on time.
        let wait = Time::elapsed(&now, Time::<Monotonic>::from(timestamp));
        self.hold_until(surface, wait, || {
            with_states(surface, |states| {
                let barriers = states
                    .data_map
                    .get_or_insert(CommitTimerBarrierStateUserData::default);
                barriers
                    .lock()
                    .ok()
                    .map(|mut barriers| barriers.register(timestamp))
            })
        });
    }

    fn invalid_timestamp(&self, client: &Client, timestamp: Timestamp) {
        let id = client.id();
        tracing::warn!(
            ?id,
            ?timestamp,
            "a client's commit timestamp is not a time: disconnected"
        );
        client.kill(
            &self.display_handle,
            ProtocolError {
                code: INVALID_TIMESTAMP,
                object_id: 0,
                object_interface: "wp_commit_timer_v1".into(),
                message: format!("invalid timestamp {timestamp:?}"),
            },
        );
    }

    /// After a commit of `surface` is applied: remembers the surface while its content has a
    /// FIFO barrier to clear; `commit` has asked for the frame that clears it.
    pub(crate) fn fifo_committed(&mut self, surface: &WlSurface) {
        let barrier = with_states(surface, |states| {
            states
                .cached_state
                .get::<FifoBarrierCachedState>()
                .current()
                .barrier
                .as_ref()
                .is_some_and(|barrier| !barrier.is_signaled())
        });
        if barrier {
            self.fifo_surfaces.insert(surface.id(), surface.clone());
        }
    }

    /// After a frame of `output`: clears the FIFO barrier of each remembered surface that
    /// `output` shows or that no output shows, forgets it, and tries the commits of their
    /// clients.
    pub(crate) fn fifo_frame_done(&mut self, output: &Output) {
        let mut cleared: Vec<Client> = Vec::new();
        let space = &self.space;
        self.fifo_surfaces.retain(|_, surface| {
            if !surface.is_alive() {
                return false;
            }
            let primary = with_states(surface, |states| {
                smithay::desktop::utils::surface_primary_scanout_output(surface, states)
            });
            let here = match primary {
                Some(primary) => {
                    primary == *output || !space.outputs().any(|shown| *shown == primary)
                }
                None => true,
            };
            if !here {
                return true;
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
            false
        });
        let dh = self.display_handle.clone();
        for client in cleared {
            self.client_compositor_state(&client)
                .blocker_cleared(self, &dh);
        }
    }
}
