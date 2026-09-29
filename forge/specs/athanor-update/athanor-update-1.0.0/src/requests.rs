//! The two requests of UT6, free of D-Bus so they are tested with fakes. `serve.rs` adds
//! the bus, polkit and logind around them.
use crate::check::Context;
use crate::sigobj;
use crate::tools::Tools;

/// logind, as far as the requests need it.
pub trait Power {
    /// A block inhibitor on shutdown is held.
    async fn blocked(&self) -> bool;
    /// `login1.Manager.Reboot(false)`: inhibitors honoured, never the skip-inhibitors flag.
    async fn reboot(&self) -> Result<(), RebootError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebootError {
    /// `org.freedesktop.login1.BlockedByInhibitorLock`
    Blocked,
    Failed,
}

/// Why a request did nothing. Each becomes one D-Bus error name; none carries free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Busy,
    NothingDownloaded,
    NoPreviousVersion,
    Blocked,
    Failed,
}

impl From<RebootError> for Refusal {
    fn from(err: RebootError) -> Self {
        match err {
            RebootError::Blocked => Self::Blocked,
            RebootError::Failed => Self::Failed,
        }
    }
}

fn republish<T: Tools>(ctx: &Context<'_, T>) {
    if let Err(failure) = crate::check::run(ctx, true) {
        tracing::error!(code = ?failure.code, "cannot publish the state");
    }
}

/// `Apply()`: unlock what the timer downloaded and reboot, in one request. Never downloads.
///
/// # Errors
/// See [`Refusal`]. After any refusal the staged deployment is locked again.
pub async fn apply<T: Tools, P: Power>(ctx: &Context<'_, T>, power: &P) -> Result<(), Refusal> {
    let _lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
    let policy = crate::policy::in_force(&ctx.policy);
    let ready = status.staged.as_ref().is_some_and(|staged| {
        staged.download_only
            && status.booted.enforcing
            && policy.scopes.contains_key(sigobj::repository_of(&status.booted.image))
            && staged.build_time > status.booted.build_time
            && ctx.store.held().as_deref() != Some(staged.digest.as_str())
    });
    if !ready {
        return Err(Refusal::NothingDownloaded);
    }
    // A courtesy that keeps the deployment locked in the common case. The authority is
    // logind's own check inside Reboot, handled below.
    if power.blocked().await {
        return Err(Refusal::Blocked);
    }
    ctx.tools.apply_downloaded().map_err(|_| Refusal::Failed)?;
    republish(ctx);
    if let Err(err) = power.reboot().await {
        if ctx.tools.relock().is_err() {
            tracing::error!("the deployment could not be locked again: it applies at the next shutdown");
        }
        republish(ctx);
        return Err(err.into());
    }
    Ok(())
}

/// `GoBack()`: the immediately previous deployment, the digest left recorded as held, a reboot.
///
/// # Errors
/// See [`Refusal`].
pub async fn go_back<T: Tools, P: Power>(ctx: &Context<'_, T>, power: &P) -> Result<(), Refusal> {
    let _lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
    if status.rollback.is_none() {
        return Err(Refusal::NoPreviousVersion);
    }
    if power.blocked().await {
        return Err(Refusal::Blocked);
    }
    // Held first: if the rollback then fails, the worst case is a digest not offered again.
    ctx.store.set_held(&status.booted.digest).map_err(|_| Refusal::Failed)?;
    ctx.tools.rollback().map_err(|_| Refusal::Failed)?;
    republish(ctx);
    power.reboot().await.map_err(Refusal::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{deployed, digest, Fake, Machine};
    use crate::tools::Deployed;
    use athanor_trust_state::UpdateState;
    use std::cell::Cell;

    struct FakePower {
        blocked: bool,
        reboot: Result<(), RebootError>,
        rebooted: Cell<bool>,
    }

    impl FakePower {
        fn new(blocked: bool, reboot: Result<(), RebootError>) -> Self {
            Self { blocked, reboot, rebooted: Cell::new(false) }
        }
    }

    impl Power for FakePower {
        async fn blocked(&self) -> bool {
            self.blocked
        }
        async fn reboot(&self) -> Result<(), RebootError> {
            self.rebooted.set(self.reboot.is_ok());
            self.reboot
        }
    }

    fn downloaded() -> Fake {
        let tools = Fake::booted(deployed(&digest(1), 1000));
        tools.status.borrow_mut().staged = Some(Deployed { download_only: true, ..deployed(&digest(2), 2000) });
        tools
    }

    fn published(machine: &Machine) -> UpdateState {
        let text = std::fs::read_to_string(machine.store.run.join("state.json")).expect("state");
        athanor_trust_state::parse(&text).expect("schema 1").update
    }

    #[tokio::test]
    async fn apply_unlocks_and_reboots() {
        let machine = Machine::new("apply", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power).await, Ok(()));
        assert!(power.rebooted.get());
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown);
    }

    #[tokio::test]
    async fn apply_refuses_with_nothing_downloaded_and_never_downloads() {
        let machine = Machine::new("apply-nothing", &["real/k1.pub"]);
        let unlocked = downloaded();
        unlocked.status.borrow_mut().staged.as_mut().expect("staged").download_only = false;
        let held = downloaded();
        machine.store.set_held(&digest(2)).expect("held");
        for tools in [Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000), unlocked, held] {
            let power = FakePower::new(false, Ok(()));
            assert_eq!(apply(&machine.ctx(&tools, 5000), &power).await, Err(Refusal::NothingDownloaded));
            assert!(tools.calls.borrow().is_empty() && !power.rebooted.get());
        }
    }

    #[tokio::test]
    async fn apply_with_an_inhibitor_held_unlocks_nothing() {
        let machine = Machine::new("apply-blocked", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(true, Ok(())));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power).await, Err(Refusal::Blocked));
        assert!(tools.calls.borrow().is_empty());
    }

    #[tokio::test]
    async fn a_reboot_refused_after_the_unlock_locks_the_deployment_again() {
        let machine = Machine::new("apply-relock", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Err(RebootError::Blocked)));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power).await, Err(Refusal::Blocked));
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock"]);
        assert_eq!(published(&machine), UpdateState::Downloaded);

        let mut stuck = downloaded();
        stuck.relock = Err(crate::tools::Failure { code: athanor_trust_state::ErrorCode::Internal, host: None });
        assert_eq!(apply(&machine.ctx(&stuck, 5000), &FakePower::new(false, Err(RebootError::Failed))).await, Err(Refusal::Failed));
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown, "the truthful state when the re-lock fails");
    }

    #[tokio::test]
    async fn a_request_during_a_check_is_told_busy() {
        let machine = Machine::new("apply-busy", &["real/k1.pub"]);
        let _check = machine.store.lock().expect("lock");
        assert_eq!(apply(&machine.ctx(&downloaded(), 5000), &FakePower::new(false, Ok(()))).await, Err(Refusal::Busy));
    }

    #[tokio::test]
    async fn go_back_holds_the_digest_it_leaves() {
        let machine = Machine::new("go-back", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(2), 2000));
        assert_eq!(go_back(&machine.ctx(&tools, 5000), &FakePower::new(false, Ok(()))).await, Err(Refusal::NoPreviousVersion));
        tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
        let power = FakePower::new(false, Ok(()));
        assert_eq!(go_back(&machine.ctx(&tools, 5000), &power).await, Ok(()));
        assert_eq!(machine.store.held(), Some(digest(2)));
        assert_eq!(*tools.calls.borrow(), ["rollback"]);
        assert!(power.rebooted.get());
    }
}
