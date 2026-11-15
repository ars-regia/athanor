//! wp_drm_lease_device_v1 (doc_own_stack.md decision 36, unconfined): the connectors the kernel
//! marks `non-desktop`, such as a headset's, leased to a client outside a security context.
//!
//! The udev backend offers one global, for the GPU it draws on (`udev.rs`), and only the
//! connectors marked `non-desktop`, which are never an output of the session. This module holds
//! what decides when a lease ends, apart from DRM so that it is tested without a GPU:
//! - no lease is granted from the lock request until the unlock, from a switch of virtual
//!   terminal the compositor starts until the resume or the switch's failure, while the session
//!   is paused, nor while a lease waits to end;
//! - `locked` is sent only once every lease has ended: `Confirmation::confirm`, the only way
//!   to send it, takes a `Revoked`, which `Leases::proof` makes only when no lease is held and
//!   none waits to end;
//! - a lease ends when the lock is requested, when its device is lost, before a switch of
//!   virtual terminal the compositor makes, and when one of its connectors is disconnected or
//!   stops being `non-desktop`.
//!
//! A lease ends in two steps. It is dropped, which tells its client `finished` (Smithay's
//! `DrmLease`, whose drop also asks the kernel to revoke it, ignoring a failure). Then `finish`
//! revokes it as the device lists it, confirms from the device's list of lessees that it is
//! gone, and disables its CRTCs; only then is it ended. Until then it waits: no lease is
//! granted, `locked` waits, and it is tried again at the next scan of the device, at the resume
//! and, while a lock waits, once a second (`lock.rs`). While the session is paused the device is
//! not touched, the drop included, as libseat requires once the pause is acknowledged: a switch
//! logind makes is acknowledged before the compositor hears of it, so the lessee can go on
//! driving its display until the resume, where the leases end before any output is drawn.

use crate::state::State;
use smithay::backend::drm::DrmNode;
use smithay::reexports::drm::control::{connector, crtc};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::drm_lease::{DrmLease, DrmLeaseState, Error};
use smithay::wayland::session_lock::SessionLocker;

/// Offers the lease device of `node` to every client outside a security context.
pub fn global(dh: &DisplayHandle, node: &DrmNode) -> Result<DrmLeaseState, Error> {
    DrmLeaseState::new_with_filter::<State, _>(dh, node, crate::state::unconfined)
}

/// The lease the compositor's `Leases` hold: Smithay's, or, in the fixture, a probe that
/// records that it was dropped. It is never read, only dropped: its drop tells the client.
#[allow(dead_code)]
pub enum Lease {
    Drm(Box<DrmLease>),
    #[cfg(test)]
    Probe(Probe),
}

/// A lease of the fixture: its flag is set when it is dropped.
#[cfg(test)]
pub struct Probe(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

#[cfg(test)]
impl Drop for Probe {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A granted lease: its device (`DrmNode::dev_id`), its lease id, the connectors and CRTCs it
/// holds, and the lease.
pub struct Held<L> {
    pub device: u64,
    pub id: u32,
    pub connectors: Vec<connector::Handle>,
    pub crtcs: Vec<crtc::Handle>,
    pub lease: L,
}

/// A lease that waits to end: its id, its connectors, its CRTCs, and why the last attempt to
/// end it failed.
pub struct Ended {
    pub id: u32,
    pub connectors: Vec<connector::Handle>,
    pub crtcs: Vec<crtc::Handle>,
    cause: Option<String>,
}

/// The leases granted and those waiting to end, and what holds new ones off.
pub struct Leases<L> {
    /// From the lock request to the unlock.
    locked: bool,
    /// From a switch of virtual terminal the compositor starts to the resume or its failure.
    switching: bool,
    /// From the pause to the resume: the device is not touched.
    paused: bool,
    held: Vec<Held<L>>,
    /// Each with its lease until it is dropped, which a pause defers.
    ending: Vec<(Ended, Option<L>)>,
}

impl<L> Default for Leases<L> {
    fn default() -> Self {
        Self {
            locked: false,
            switching: false,
            paused: false,
            held: Vec::new(),
            ending: Vec::new(),
        }
    }
}

/// That no lease is held and the device confirmed the end of every one: only `Leases::proof`
/// makes one.
#[must_use]
pub struct Revoked(());

impl<L> Leases<L> {
    /// Whether a new lease may be granted.
    pub fn may_grant(&self) -> bool {
        !self.locked && !self.switching && !self.paused && self.ending.is_empty()
    }

    /// A lease was granted. One granted while no grant may be made ends at once.
    pub fn granted(&mut self, held: Held<L>, finish: impl FnMut(&Ended) -> Result<(), String>) {
        let refused = !self.may_grant();
        self.held.push(held);
        if refused {
            self.end(|_| true, finish);
        }
    }

    /// The client ended lease `id`; the device has the last word on whether it is revoked.
    pub fn destroyed(&mut self, id: u32, finish: impl FnMut(&Ended) -> Result<(), String>) {
        self.end(|held| held.id == id, finish);
    }

    /// A lock was requested: from now on no lease is granted, and every one ends.
    pub fn lock_requested(&mut self, finish: impl FnMut(&Ended) -> Result<(), String>) {
        self.locked = true;
        self.end(|_| true, finish);
    }

    /// The session was unlocked.
    pub fn unlocked(&mut self) {
        self.locked = false;
    }

    /// The compositor starts a switch of virtual terminal.
    pub fn switching(&mut self) {
        self.switching = true;
    }

    /// The switch the compositor asked for failed: no pause follows.
    pub fn switch_failed(&mut self) {
        self.switching = false;
    }

    /// The session paused: the device is not touched until the resume.
    pub fn paused(&mut self) {
        self.paused = true;
    }

    /// The session resumed: the device may be touched again.
    pub fn resumed(&mut self) {
        self.paused = false;
        self.switching = false;
    }

    /// The proof that `locked` needs: no lease is held, and none waits to end.
    pub fn proof(&self) -> Option<Revoked> {
        (self.held.is_empty() && self.ending.is_empty()).then_some(Revoked(()))
    }

    /// Tries again to end the leases that wait. Whether none waits any more.
    pub fn retry(&mut self, finish: impl FnMut(&Ended) -> Result<(), String>) -> bool {
        self.end(|_| false, finish)
    }

    /// Device `device` is lost, on a switch of virtual terminal or a removal.
    pub fn revoke_device(&mut self, device: u64, finish: impl FnMut(&Ended) -> Result<(), String>) {
        self.end(|held| held.device == device, finish);
    }

    /// `connector` of `device` was disconnected or stopped being `non-desktop`: the whole lease
    /// that holds it ends, and `finish` is handed every connector of it.
    pub fn revoke_connector(
        &mut self,
        device: u64,
        connector: connector::Handle,
        finish: impl FnMut(&Ended) -> Result<(), String>,
    ) {
        self.end(
            |held| held.device == device && held.connectors.contains(&connector),
            finish,
        );
    }

    /// Moves the leases `which` picks to those waiting to end. Unless the session is paused,
    /// each waiting lease is then dropped and handed to `finish`, which revokes it, confirms
    /// from the device that it is gone and disables its CRTCs; one that failed waits for the
    /// next attempt, its failure logged when it is new. Whether none waits.
    fn end(
        &mut self,
        which: impl Fn(&Held<L>) -> bool,
        mut finish: impl FnMut(&Ended) -> Result<(), String>,
    ) -> bool {
        let (picked, kept): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.held).into_iter().partition(which);
        self.held = kept;
        for held in picked {
            let Held {
                id,
                connectors,
                crtcs,
                lease,
                ..
            } = held;
            let ended = Ended {
                id,
                connectors,
                crtcs,
                cause: None,
            };
            self.ending.push((ended, Some(lease)));
        }
        if self.paused {
            return self.ending.is_empty();
        }
        self.ending.retain_mut(|(ended, lease)| {
            drop(lease.take());
            match finish(ended) {
                Ok(()) => false,
                Err(cause) => {
                    if ended.cause.as_ref() != Some(&cause) {
                        tracing::error!(lease = ended.id, connectors = ?ended.connectors, %cause, "a lease could not be ended: it is tried again");
                        ended.cause = Some(cause);
                    }
                    true
                }
            }
        });
        self.ending.is_empty()
    }
}

/// What sends `locked`.
pub trait Confirm {
    fn confirm(self);
}

impl Confirm for SessionLocker {
    fn confirm(self) {
        self.lock();
    }
}

/// A lock's confirmation, held until the outputs show the lock and every lease has ended.
/// `locked` is sent only through `confirm`, which takes the proof that every lease ended.
pub struct Confirmation<C = SessionLocker>(C);

impl<C: Confirm> Confirmation<C> {
    pub fn new(locker: C) -> Self {
        Self(locker)
    }

    /// Sends `locked`.
    pub fn confirm(self, _revoked: Revoked) {
        self.0.confirm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<String>>>;

    /// A lease that logs when it is dropped, as Smithay's tells its client.
    struct Logged {
        id: u32,
        log: Log,
    }

    impl Drop for Logged {
        fn drop(&mut self) {
            self.log.borrow_mut().push(format!("dropped {}", self.id));
        }
    }

    impl<F: FnOnce()> Confirm for F {
        fn confirm(self) {
            self()
        }
    }

    fn connector(id: u32) -> connector::Handle {
        smithay::reexports::drm::control::from_u32(id).expect("a nonzero id")
    }

    fn crtc(id: u32) -> crtc::Handle {
        smithay::reexports::drm::control::from_u32(id).expect("a nonzero id")
    }

    /// Lease `id` of `device`, of connector `id` driven by CRTC `id + 100`.
    fn held(log: &Log, device: u64, id: u32) -> Held<Logged> {
        Held {
            device,
            id,
            connectors: vec![connector(id)],
            crtcs: vec![crtc(id + 100)],
            lease: Logged {
                id,
                log: log.clone(),
            },
        }
    }

    /// Logs each lease as revoked and its CRTCs as disabled, and all of it as confirmed.
    fn finished(log: &Log) -> impl FnMut(&Ended) -> Result<(), String> + '_ {
        move |ended| {
            log.borrow_mut().push(format!("revoked {}", ended.id));
            for crtc in &ended.crtcs {
                let id: u32 = (*crtc).into();
                log.borrow_mut().push(format!("disabled {id}"));
            }
            Ok(())
        }
    }

    /// Every attempt fails with `cause`, and is logged.
    fn refused<'a>(log: &'a Log, cause: &'a str) -> impl FnMut(&Ended) -> Result<(), String> + 'a {
        move |ended| {
            log.borrow_mut().push(format!("refused {}", ended.id));
            Err(cause.into())
        }
    }

    fn logged(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.borrow_mut())
    }

    #[test]
    fn no_lease_is_granted_from_the_lock_request_to_the_unlock() {
        let log = Log::default();
        let mut leases = Leases::default();
        assert!(leases.may_grant());
        leases.lock_requested(finished(&log));
        assert!(leases.proof().is_some());
        assert!(!leases.may_grant(), "from the lock request");
        // A lease that got through anyway ends at once.
        leases.granted(held(&log, 1, 7), finished(&log));
        assert_eq!(logged(&log), ["dropped 7", "revoked 7", "disabled 107"]);
        assert!(leases.proof().is_some());
        leases.unlocked();
        assert!(leases.may_grant(), "after the unlock");
        leases.granted(held(&log, 1, 8), finished(&log));
        assert!(logged(&log).is_empty(), "a lease after the unlock is kept");
        assert!(leases.proof().is_none(), "a lease is held");
    }

    #[test]
    fn every_lease_is_dropped_revoked_and_disabled_before_locked() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        let mut two = held(&log, 1, 8);
        two.connectors.push(connector(9));
        two.crtcs.push(crtc(109));
        leases.granted(two, finished(&log));
        let confirmation = Confirmation::new(|| log.borrow_mut().push("locked".into()));
        leases.lock_requested(finished(&log));
        confirmation.confirm(leases.proof().expect("every lease ended"));
        assert_eq!(
            logged(&log),
            [
                "dropped 7",
                "revoked 7",
                "disabled 107",
                "dropped 8",
                "revoked 8",
                "disabled 108",
                "disabled 109",
                "locked"
            ]
        );
    }

    /// A failed revocation makes no proof: the lease waits and refuses grants, each attempt
    /// tries it again, and its failure is recorded once for each cause.
    #[test]
    fn a_failed_revocation_withholds_the_proof_until_a_retry_succeeds() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        leases.lock_requested(refused(&log, "EACCES"));
        assert_eq!(logged(&log), ["dropped 7", "refused 7"]);
        assert!(leases.proof().is_none());
        assert!(!leases.retry(refused(&log, "EACCES")));
        assert_eq!(leases.ending[0].0.cause.as_deref(), Some("EACCES"));
        assert!(!leases.retry(refused(&log, "EIO")));
        assert_eq!(leases.ending[0].0.cause.as_deref(), Some("EIO"));
        assert_eq!(logged(&log), ["refused 7", "refused 7"]);
        leases.unlocked();
        assert!(!leases.may_grant(), "a lease waits to end");
        assert!(leases.retry(finished(&log)));
        assert_eq!(logged(&log), ["revoked 7", "disabled 107"]);
        assert!(leases.may_grant());
        assert!(leases.proof().is_some());
    }

    /// While paused nothing is dropped or revoked, not even for a lock; at the resume it all
    /// ends.
    #[test]
    fn a_paused_session_leaves_the_device_alone_until_the_resume() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        leases.paused();
        assert!(!leases.may_grant());
        leases.revoke_device(1, finished(&log));
        leases.lock_requested(finished(&log));
        assert!(!leases.retry(finished(&log)));
        assert!(logged(&log).is_empty(), "the device is not touched");
        assert!(leases.proof().is_none());
        leases.resumed();
        assert!(leases.retry(finished(&log)));
        assert_eq!(logged(&log), ["dropped 7", "revoked 7", "disabled 107"]);
        assert!(leases.proof().is_some());
    }

    #[test]
    fn no_lease_is_granted_from_a_switch_to_the_resume_or_its_failure() {
        let mut leases = Leases::<Logged>::default();
        leases.switching();
        assert!(!leases.may_grant(), "from the switch");
        leases.switch_failed();
        assert!(leases.may_grant(), "after a failed switch");
        leases.switching();
        leases.paused();
        leases.resumed();
        assert!(leases.may_grant(), "after the resume");
    }

    #[test]
    fn a_lost_device_ends_its_leases() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        leases.granted(held(&log, 2, 8), finished(&log));
        leases.revoke_device(1, finished(&log));
        assert_eq!(logged(&log), ["dropped 7", "revoked 7", "disabled 107"]);
        assert_eq!(leases.held.len(), 1);
        assert!(leases.may_grant(), "a lost device is no lock");
    }

    #[test]
    fn a_disconnected_connector_ends_its_lease() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        leases.granted(held(&log, 1, 8), finished(&log));
        // The same connector id on another device is another connector.
        leases.revoke_connector(2, connector(7), finished(&log));
        assert!(logged(&log).is_empty());
        leases.revoke_connector(1, connector(7), finished(&log));
        assert_eq!(logged(&log), ["dropped 7", "revoked 7", "disabled 107"]);
        assert_eq!(leases.held.len(), 1);
    }

    /// The lease's other connector is handed to `finish`, which offers it anew: Smithay forgets
    /// a lease when one of its connectors is withdrawn, and would never offer the others again.
    #[test]
    fn a_withdrawn_connector_hands_back_the_rest_of_its_lease() {
        let log = Log::default();
        let mut leases = Leases::default();
        let mut both = held(&log, 1, 7);
        both.connectors.push(connector(9));
        both.crtcs.push(crtc(109));
        leases.granted(both, finished(&log));
        leases.granted(held(&log, 1, 8), finished(&log));
        let mut handed = Vec::new();
        leases.revoke_connector(1, connector(9), |ended| {
            handed.extend(ended.connectors.iter().map(|c| u32::from(*c)));
            handed.extend(ended.crtcs.iter().map(|c| u32::from(*c)));
            Ok(())
        });
        assert_eq!(handed, [7, 9, 107, 109]);
        assert_eq!(logged(&log), ["dropped 7"]);
        assert_eq!(leases.held.len(), 1, "lease 8 holds neither");
    }

    /// A lease its client destroyed is checked and ended like any other.
    #[test]
    fn a_lease_the_client_ends_is_revoked_and_disabled() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), finished(&log));
        leases.granted(held(&log, 1, 8), finished(&log));
        leases.destroyed(8, finished(&log));
        assert_eq!(logged(&log), ["dropped 8", "revoked 8", "disabled 108"]);
        leases.destroyed(8, finished(&log));
        assert!(
            logged(&log).is_empty(),
            "an id ended twice ends nothing more"
        );
    }
}
