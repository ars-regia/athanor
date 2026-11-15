//! wp_drm_lease_device_v1 (doc_own_stack.md decision 36, unconfined): the connectors the kernel
//! marks `non-desktop`, such as a headset's, leased to a client outside a security context.
//!
//! The udev backend offers one global, for the GPU it draws on (`udev.rs`), and only the
//! connectors marked `non-desktop`, which are never an output of the session. This module holds
//! what decides when a lease ends, apart from DRM so that it is tested without a GPU:
//! - no lease is granted from the moment a lock is requested until the unlock;
//! - every lease is revoked, and its CRTCs disabled, when the lock is requested and again before
//!   `locked` is sent: `Confirmation::confirm`, the only way to send it, takes the `Revoked` that
//!   only `Leases::revoke_all` makes;
//! - a lease is revoked, and its CRTCs disabled, when its device is lost and when one of its
//!   connectors is disconnected or stops being `non-desktop`.
//!
//! A lease is revoked by dropping it: Smithay's `DrmLease` revokes itself in its `Drop`. Its
//! CRTCs are disabled after that, since a CRTC is the lessee's while it is leased.

use crate::state::State;
use smithay::backend::drm::DrmNode;
use smithay::reexports::drm::control::{connector, crtc};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::drm_lease::{DrmLeaseState, Error};
use smithay::wayland::session_lock::SessionLocker;

/// Offers the lease device of `node` to every client outside a security context.
pub fn global(dh: &DisplayHandle, node: &DrmNode) -> Result<DrmLeaseState, Error> {
    DrmLeaseState::new_with_filter::<State, _>(dh, node, crate::state::unconfined)
}

/// A granted lease: its device (`DrmNode::dev_id`), its lease id, the connectors and CRTCs it
/// holds, and the lease, whose drop revokes it.
pub struct Held<L> {
    pub device: u64,
    pub id: u32,
    pub connectors: Vec<connector::Handle>,
    pub crtcs: Vec<crtc::Handle>,
    pub lease: L,
}

/// The leases granted, and whether a lock holds them off.
pub struct Leases<L> {
    /// From the lock request to the unlock.
    locked: bool,
    held: Vec<Held<L>>,
}

impl<L> Default for Leases<L> {
    fn default() -> Self {
        Self {
            locked: false,
            held: Vec::new(),
        }
    }
}

/// That every lease was revoked and its CRTCs disabled: only `Leases::revoke_all` makes one.
#[must_use]
pub struct Revoked(());

impl<L> Leases<L> {
    /// Whether a new lease may be granted: not from the lock request until the unlock.
    pub fn may_grant(&self) -> bool {
        !self.locked
    }

    /// A lease was granted. One granted while a lock holds the leases off ends at once.
    pub fn granted(&mut self, held: Held<L>, disable: impl FnMut(crtc::Handle)) {
        let ended = self.locked;
        self.held.push(held);
        if ended {
            self.end(|_| true, disable);
        }
    }

    /// The client ended lease `id`.
    pub fn destroyed(&mut self, id: u32, disable: impl FnMut(crtc::Handle)) {
        self.end(|held| held.id == id, disable);
    }

    /// A lock was requested: from now on no lease is granted, and every one is revoked.
    pub fn lock_requested(&mut self, disable: impl FnMut(crtc::Handle)) -> Revoked {
        self.locked = true;
        self.revoke_all(disable)
    }

    /// The session was unlocked: a client may ask again.
    pub fn unlocked(&mut self) {
        self.locked = false;
    }

    /// Revokes every lease and disables its CRTCs.
    pub fn revoke_all(&mut self, disable: impl FnMut(crtc::Handle)) -> Revoked {
        self.end(|_| true, disable);
        Revoked(())
    }

    /// Device `device` is lost, on a session switch or a removal.
    pub fn revoke_device(&mut self, device: u64, disable: impl FnMut(crtc::Handle)) {
        self.end(|held| held.device == device, disable);
    }

    /// `connector` of `device` was disconnected or stopped being `non-desktop`.
    pub fn revoke_connector(
        &mut self,
        device: u64,
        connector: connector::Handle,
        disable: impl FnMut(crtc::Handle),
    ) {
        self.end(
            |held| held.device == device && held.connectors.contains(&connector),
            disable,
        );
    }

    /// Ends the leases `which` picks: each is revoked, then its CRTCs disabled.
    fn end(&mut self, which: impl Fn(&Held<L>) -> bool, mut disable: impl FnMut(crtc::Handle)) {
        let (ended, kept) = std::mem::take(&mut self.held).into_iter().partition(which);
        self.held = kept;
        for held in ended {
            let Held { crtcs, lease, .. } = held;
            drop(lease);
            crtcs.into_iter().for_each(&mut disable);
        }
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

/// A lock's confirmation, held until the outputs show the lock. `locked` is sent only through
/// `confirm`, which takes the proof that every lease was revoked.
pub struct Confirmation<C = SessionLocker>(C);

impl<C: Confirm> Confirmation<C> {
    pub fn new(locker: C) -> Self {
        Self(locker)
    }

    /// Sends `locked`, once every lease is revoked.
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

    /// A lease that logs its revocation when dropped, as Smithay's revokes itself.
    struct Probe {
        id: u32,
        log: Log,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.log.borrow_mut().push(format!("revoked {}", self.id));
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
    fn held(log: &Log, device: u64, id: u32) -> Held<Probe> {
        Held {
            device,
            id,
            connectors: vec![connector(id)],
            crtcs: vec![crtc(id + 100)],
            lease: Probe {
                id,
                log: log.clone(),
            },
        }
    }

    fn disabled(log: &Log) -> impl FnMut(crtc::Handle) + '_ {
        move |crtc| {
            let id: u32 = crtc.into();
            log.borrow_mut().push(format!("disabled {id}"));
        }
    }

    fn logged(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.borrow_mut())
    }

    #[test]
    fn no_lease_is_granted_from_the_lock_request_on() {
        let log = Log::default();
        let mut leases = Leases::default();
        assert!(leases.may_grant());
        let _ = leases.lock_requested(disabled(&log));
        assert!(!leases.may_grant(), "from the lock request");
        // A confirmed lock, and the lock surfaces' clients, change nothing until the unlock.
        let _ = leases.revoke_all(disabled(&log));
        assert!(!leases.may_grant(), "while locked");
        // A lease that got through anyway ends at once, before any confirmation.
        leases.granted(held(&log, 1, 7), disabled(&log));
        assert_eq!(logged(&log), ["revoked 7", "disabled 107"]);
        assert!(leases.held.is_empty());
        leases.unlocked();
        assert!(leases.may_grant(), "after the unlock");
        leases.granted(held(&log, 1, 8), disabled(&log));
        assert!(logged(&log).is_empty(), "a lease after the unlock is kept");
    }

    #[test]
    fn every_lease_is_revoked_and_its_connector_disabled_before_locked() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), disabled(&log));
        let mut two = held(&log, 1, 8);
        two.connectors.push(connector(9));
        two.crtcs.push(crtc(109));
        leases.granted(two, disabled(&log));
        let confirmation = Confirmation::new(|| log.borrow_mut().push("locked".into()));
        let _ = leases.lock_requested(disabled(&log));
        assert_eq!(
            logged(&log),
            [
                "revoked 7",
                "disabled 107",
                "revoked 8",
                "disabled 108",
                "disabled 109"
            ],
            "at the lock request"
        );
        confirmation.confirm(leases.revoke_all(disabled(&log)));
        assert_eq!(logged(&log), ["locked"]);
    }

    #[test]
    fn a_lost_device_revokes_its_leases() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), disabled(&log));
        leases.granted(held(&log, 2, 8), disabled(&log));
        leases.revoke_device(1, disabled(&log));
        assert_eq!(logged(&log), ["revoked 7", "disabled 107"]);
        assert_eq!(leases.held.len(), 1);
        assert!(leases.may_grant(), "a lost device is no lock");
    }

    #[test]
    fn a_disconnected_connector_revokes_its_lease() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), disabled(&log));
        leases.granted(held(&log, 1, 8), disabled(&log));
        // The same connector id on another device is another connector.
        leases.revoke_connector(2, connector(7), disabled(&log));
        assert!(logged(&log).is_empty());
        leases.revoke_connector(1, connector(7), disabled(&log));
        assert_eq!(logged(&log), ["revoked 7", "disabled 107"]);
        assert_eq!(leases.held.len(), 1);
    }

    #[test]
    fn a_connector_no_longer_non_desktop_revokes_the_whole_lease() {
        let log = Log::default();
        let mut leases = Leases::default();
        let mut both = held(&log, 1, 7);
        both.connectors.push(connector(9));
        both.crtcs.push(crtc(109));
        leases.granted(both, disabled(&log));
        leases.revoke_connector(1, connector(9), disabled(&log));
        assert_eq!(logged(&log), ["revoked 7", "disabled 107", "disabled 109"]);
        assert!(leases.held.is_empty());
    }

    #[test]
    fn a_lease_the_client_ends_disables_its_connector() {
        let log = Log::default();
        let mut leases = Leases::default();
        leases.granted(held(&log, 1, 7), disabled(&log));
        leases.granted(held(&log, 1, 8), disabled(&log));
        leases.destroyed(8, disabled(&log));
        assert_eq!(logged(&log), ["revoked 8", "disabled 108"]);
        leases.destroyed(8, disabled(&log));
        assert!(
            logged(&log).is_empty(),
            "an id ended twice ends nothing more"
        );
    }
}
