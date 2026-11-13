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
//! wait. The compositor answers the commit-timing manager itself, so that a timestamp that is
//! not a time is the timer's `invalid_timestamp` error when it is set: Smithay's timer, at
//! 62ec78d, keeps any value and posts no such error. Every commit a client has waiting, on a
//! barrier, a timestamp or an earlier commit, counts against `MAX_HELD_COMMITS`.

use crate::state::State;
use smithay::output::Output;
use smithay::reexports::rustix::time::Timespec;
use smithay::reexports::wayland_protocols::wp::commit_timing::v1::server::{
    wp_commit_timer_v1::{self, WpCommitTimerV1},
    wp_commit_timing_manager_v1::{self, WpCommitTimingManagerV1},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, Weak};
use smithay::utils::{Monotonic, Time};
use smithay::wayland::commit_timing::{
    CommitTimerBarrierStateUserData, CommitTimerStateUserData, Timestamp,
};
use smithay::wayland::compositor::{with_states, CompositorHandler};
use smithay::wayland::fifo::{FifoBarrierCachedState, FifoManagerState};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::cell::Cell;
use std::time::Duration;

/// The longest a commit waits for its timestamp. The protocol bounds no timestamp; a client
/// paces frames a few refreshes ahead, and a commit timed further than this is applied after
/// it rather than hold its surface, and its place among the client's waiting commits, for
/// longer.
pub const MAX_TIMED_WAIT: Duration = Duration::from_secs(1);

/// Offers both globals to every client. Smithay holds a FIFO commit on its barrier itself; the
/// timestamps are the compositor's to check and hold, so that a commit is applied at its time
/// whether or not an output draws then.
pub fn globals(dh: &DisplayHandle) {
    FifoManagerState::new::<State>(dh);
    dh.create_global::<State, WpCommitTimingManagerV1, _>(1, TimingManagers);
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

/// The time `set_timestamp` names, or nothing when it is not one: seconds past what the
/// presentation clock counts, or nanoseconds of a second or more.
fn timestamp(tv_sec_hi: u32, tv_sec_lo: u32, tv_nsec: u32) -> Option<Timestamp> {
    let tv_sec = i64::try_from((u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo)).ok()?;
    if tv_nsec >= 1_000_000_000 {
        return None;
    }
    let time = Timespec {
        tv_sec,
        tv_nsec: tv_nsec.into(),
    };
    Some(Timestamp::from(Time::<Monotonic>::from(time)))
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

    /// After a commit of `surface` is applied: remembers the surface while its content has a
    /// FIFO barrier to clear, until a frame clears it or the surface is destroyed; `commit` has
    /// asked for the frame that clears it.
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

/// The user data of the commit-timing manager's global and of every manager.
pub struct TimingManagers;

/// The user data of a commit timer: its surface.
pub struct CommitTimer(Weak<WlSurface>);

/// Whether a surface has a commit timer: the protocol gives it one at a time.
#[derive(Default)]
struct Timed(Cell<bool>);

impl GlobalDispatch2<WpCommitTimingManagerV1, State> for TimingManagers {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpCommitTimingManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, TimingManagers);
    }
}

impl Dispatch2<WpCommitTimingManagerV1, State> for TimingManagers {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        manager: &WpCommitTimingManagerV1,
        request: wp_commit_timing_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let wp_commit_timing_manager_v1::Request::GetTimer { id, surface } = request else {
            return;
        };
        let taken = with_states(&surface, |states| {
            states.data_map.get_or_insert(Timed::default).0.replace(true)
        });
        if taken {
            manager.post_error(
                wp_commit_timing_manager_v1::Error::CommitTimerExists,
                "the surface has a commit timer already",
            );
            return;
        }
        data_init.init(id, CommitTimer(surface.downgrade()));
    }
}

impl Dispatch2<WpCommitTimerV1, State> for CommitTimer {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        timer: &WpCommitTimerV1,
        request: wp_commit_timer_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use wp_commit_timer_v1::Error;
        let wp_commit_timer_v1::Request::SetTimestamp {
            tv_sec_hi,
            tv_sec_lo,
            tv_nsec,
        } = request
        else {
            return;
        };
        let Ok(surface) = self.0.upgrade() else {
            timer.post_error(Error::SurfaceDestroyed, "the timer's surface is destroyed");
            return;
        };
        let Some(timestamp) = timestamp(tv_sec_hi, tv_sec_lo, tv_nsec) else {
            timer.post_error(
                Error::InvalidTimestamp,
                format!("{tv_sec_hi}:{tv_sec_lo} s and {tv_nsec} ns is not a time"),
            );
            return;
        };
        let exists = with_states(&surface, |states| {
            let mut pending = states
                .data_map
                .get_or_insert(CommitTimerStateUserData::default)
                .borrow_mut();
            let exists = pending.timestamp.is_some();
            if !exists {
                pending.timestamp = Some(timestamp);
            }
            exists
        });
        if exists {
            timer.post_error(
                Error::TimestampExists,
                "the commit has a timestamp already",
            );
        }
    }

    fn destroyed(&self, _state: &mut State, _client: ClientId, _timer: &WpCommitTimerV1) {
        if let Ok(surface) = self.0.upgrade() {
            with_states(&surface, |states| {
                if let Some(timed) = states.data_map.get::<Timed>() {
                    timed.0.set(false);
                }
            });
        }
    }
}
