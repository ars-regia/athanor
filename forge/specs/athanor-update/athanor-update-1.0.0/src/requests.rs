//! The two requests of UT6, free of D-Bus so they are tested with fakes. `serve.rs` adds
//! the bus, polkit and logind around them.
use crate::check::Context;
use crate::sigobj;
use crate::tools::Tools;

pub const ACTION_APPLY: &str = "os.athanor.update.apply";
/// `Apply()` while another session is open: a restart ends that work too (ADR-0103 D22).
pub const ACTION_APPLY_MULTI_SESSION: &str = "os.athanor.update.apply-multi-session";

/// The bus sender of `Apply()`, as far as the request needs it.
pub trait Caller {
    /// Whether a counting session other than the caller's is open (UT6). It fails closed:
    /// when the sessions or the caller's own session cannot be read, the answer is `true`.
    async fn others_open(&self) -> bool;
    /// polkit's answer for the caller on `action`, with user interaction allowed.
    ///
    /// # Errors
    /// [`Refusal::NotAuthorized`] on a refusal, a timeout or a polkit that could not be asked.
    async fn authorize(&self, action: &str) -> Result<(), Refusal>;
}

/// logind, as far as the requests need it.
pub trait Power {
    /// Whether a block inhibitor on shutdown is held.
    ///
    /// # Errors
    /// [`Refusal::Failed`] when logind could not say: the request is refused, never waved through.
    async fn blocked(&self) -> Result<bool, Refusal>;
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
    NotAuthorized,
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
/// With another session open it asks for [`ACTION_APPLY_MULTI_SESSION`], counted when the
/// request arrives and again before the reboot (UT6, ADR-0103 D22).
///
/// # Errors
/// See [`Refusal`]. After any refusal the staged deployment is locked again.
pub async fn apply<T: Tools, P: Power, C: Caller>(ctx: &Context<'_, T>, power: &P, caller: &C) -> Result<(), Refusal> {
    let others = caller.others_open().await;
    caller.authorize(if others { ACTION_APPLY_MULTI_SESSION } else { ACTION_APPLY }).await?;
    let _lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
    let policy = crate::policy::in_force(&ctx.policy);
    // bootc rollback discards the staged deployment; one staged after it would replace the
    // queued return (doc_recovery.md, R5), so a queued rollback has nothing to apply.
    let ready = !status.rollback_queued && status.staged.as_ref().is_some_and(|staged| {
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
    if power.blocked().await? {
        return Err(Refusal::Blocked);
    }
    ctx.tools.apply_downloaded().map_err(|_| Refusal::Failed)?;
    republish(ctx);
    // The prompt and the unlock can take minutes: a session opened meanwhile was not asked
    // about. Locked again first, so no prompt ever waits on an unlocked deployment.
    if !others && caller.others_open().await {
        let relocked = ctx.tools.relock();
        republish(ctx);
        if relocked.is_err() {
            tracing::error!("the deployment could not be locked again: it applies at the next shutdown");
            return Err(Refusal::Failed);
        }
        caller.authorize(ACTION_APPLY_MULTI_SESSION).await?;
        ctx.tools.apply_downloaded().map_err(|_| Refusal::Failed)?;
        republish(ctx);
    }
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
    if power.blocked().await? {
        return Err(Refusal::Blocked);
    }
    // Held first: if the rollback then fails, the worst case is a digest not offered again.
    ctx.store.set_held(&status.booted.digest).map_err(|_| Refusal::Failed)?;
    // A queued rollback (greenboot after a failed health check, doc_recovery.md R5) already
    // makes the previous deployment the next boot; a second `bootc rollback` would swap back.
    if !status.rollback_queued {
        ctx.tools.rollback().map_err(|_| Refusal::Failed)?;
    }
    republish(ctx);
    power.reboot().await.map_err(Refusal::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{deployed, digest, Fake, Machine};
    use crate::tools::Deployed;
    use athanor_trust_state::UpdateState;
    use std::cell::{Cell, RefCell};

    struct FakePower {
        blocked: Result<bool, Refusal>,
        reboot: Result<(), RebootError>,
        rebooted: Cell<bool>,
    }

    impl FakePower {
        fn new(blocked: bool, reboot: Result<(), RebootError>) -> Self {
            Self { blocked: Ok(blocked), reboot, rebooted: Cell::new(false) }
        }
    }

    impl Power for FakePower {
        async fn blocked(&self) -> Result<bool, Refusal> {
            self.blocked
        }
        async fn reboot(&self) -> Result<(), RebootError> {
            self.rebooted.set(self.reboot.is_ok());
            self.reboot
        }
    }

    /// logind's sessions and polkit as `Apply()` sees them: the answer of each count in turn,
    /// the actions polkit grants, and each action asked with the tool calls made by then.
    struct FakeCaller<'a> {
        counts: RefCell<std::collections::VecDeque<bool>>,
        grants: &'a [&'a str],
        tools: &'a Fake,
        asked: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl<'a> FakeCaller<'a> {
        fn new(tools: &'a Fake, counts: &[bool], grants: &'a [&'a str]) -> Self {
            Self { counts: RefCell::new(counts.iter().copied().collect()), grants, tools, asked: RefCell::new(Vec::new()) }
        }

        /// No other session at either count, and polkit grants everything.
        fn alone(tools: &'a Fake) -> Self {
            Self::new(tools, &[false, false], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION])
        }

        fn asked(&self) -> Vec<String> {
            self.asked.borrow().iter().map(|(action, _)| action.clone()).collect()
        }
    }

    impl Caller for FakeCaller<'_> {
        async fn others_open(&self) -> bool {
            self.counts.borrow_mut().pop_front().expect("a count the test did not expect")
        }
        async fn authorize(&self, action: &str) -> Result<(), Refusal> {
            self.asked.borrow_mut().push((action.into(), self.tools.calls.borrow().clone()));
            if self.grants.contains(&action) {
                Ok(())
            } else {
                Err(Refusal::NotAuthorized)
            }
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
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Ok(()));
        assert!(power.rebooted.get());
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown);
    }

    // A machine per request: a store lock released and taken again can meet a child process
    // of another test between fork and exec (goback::tests::SPAWN).
    #[tokio::test]
    async fn apply_refuses_with_nothing_downloaded_and_never_downloads() {
        let unlocked = downloaded();
        unlocked.status.borrow_mut().staged.as_mut().expect("staged").download_only = false;
        let held = downloaded();
        let queued = downloaded();
        queued.status.borrow_mut().rollback_queued = true;
        for (case, tools) in [("offered", Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000)), ("unlocked", unlocked), ("held", held), ("queued", queued)] {
            let machine = Machine::new(&format!("apply-nothing-{case}"), &["real/k1.pub"]);
            machine.store.set_held(&digest(2)).expect("held");
            let power = FakePower::new(false, Ok(()));
            assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Err(Refusal::NothingDownloaded));
            assert!(tools.calls.borrow().is_empty() && !power.rebooted.get());
        }
    }

    #[tokio::test]
    async fn apply_with_an_inhibitor_held_unlocks_nothing() {
        let machine = Machine::new("apply-blocked", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(true, Ok(())));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Err(Refusal::Blocked));
        assert!(tools.calls.borrow().is_empty());
    }

    #[tokio::test]
    async fn an_unanswered_inhibitor_query_refuses_as_failed_and_unlocks_nothing() {
        let machine = Machine::new("apply-unanswered", &["real/k1.pub"]);
        let tools = downloaded();
        let power = FakePower { blocked: Err(Refusal::Failed), ..FakePower::new(false, Ok(())) };
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Err(Refusal::Failed));
        assert!(tools.calls.borrow().is_empty() && !power.rebooted.get());
    }

    #[tokio::test]
    async fn a_reboot_refused_after_the_unlock_locks_the_deployment_again() {
        let machine = Machine::new("apply-relock", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Err(RebootError::Blocked)));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Err(Refusal::Blocked));
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock"]);
        assert_eq!(published(&machine), UpdateState::Downloaded);

        let machine = Machine::new("apply-relock-stuck", &["real/k1.pub"]);
        let mut stuck = downloaded();
        stuck.relock = Err(crate::tools::Failure { code: athanor_trust_state::ErrorCode::Internal, host: None });
        assert_eq!(apply(&machine.ctx(&stuck, 5000), &FakePower::new(false, Err(RebootError::Failed)), &FakeCaller::alone(&stuck)).await, Err(Refusal::Failed));
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown, "the truthful state when the re-lock fails");
    }

    #[tokio::test]
    async fn a_request_during_a_check_is_told_busy() {
        let machine = Machine::new("apply-busy", &["real/k1.pub"]);
        let _check = machine.store.lock().expect("lock");
        let tools = downloaded();
        assert_eq!(apply(&machine.ctx(&tools, 5000), &FakePower::new(false, Ok(())), &FakeCaller::alone(&tools)).await, Err(Refusal::Busy));
    }

    #[tokio::test]
    async fn apply_alone_asks_apply_once() {
        let machine = Machine::new("apply-alone", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let caller = FakeCaller::alone(&tools);
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Ok(()));
        assert_eq!(caller.asked(), [ACTION_APPLY]);
        assert!(power.rebooted.get());
    }

    #[tokio::test]
    async fn another_session_asks_apply_multi_session_before_anything_is_unlocked() {
        let machine = Machine::new("apply-multi", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let caller = FakeCaller::new(&tools, &[true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]);
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Ok(()));
        assert_eq!(*caller.asked.borrow(), [(ACTION_APPLY_MULTI_SESSION.to_owned(), vec![])], "asked once, before the unlock; no second count");
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        assert!(power.rebooted.get());

        let machine = Machine::new("apply-multi-refused", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let caller = FakeCaller::new(&tools, &[true], &[ACTION_APPLY]);
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Err(Refusal::NotAuthorized), "apply alone does not do");
        assert!(tools.calls.borrow().is_empty() && !power.rebooted.get());
    }

    #[tokio::test]
    async fn a_session_opened_before_the_reboot_is_asked_about_with_the_deployment_locked() {
        let machine = Machine::new("apply-second-count", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let caller = FakeCaller::new(&tools, &[false, true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]);
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Ok(()));
        let locked_while_asking = vec!["apply_downloaded".to_owned(), "relock".to_owned()];
        assert_eq!(*caller.asked.borrow(), [(ACTION_APPLY.to_owned(), vec![]), (ACTION_APPLY_MULTI_SESSION.to_owned(), locked_while_asking)]);
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock", "apply_downloaded"], "unlocked again only once granted");
        assert!(power.rebooted.get());
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown);
    }

    #[tokio::test]
    async fn a_session_opened_before_the_reboot_and_refused_stays_locked() {
        let machine = Machine::new("apply-second-refused", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let caller = FakeCaller::new(&tools, &[false, true], &[ACTION_APPLY]);
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Err(Refusal::NotAuthorized));
        assert_eq!(caller.asked(), [ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]);
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock"]);
        assert!(!power.rebooted.get());
        assert_eq!(published(&machine), UpdateState::Downloaded);

        let machine = Machine::new("apply-second-stuck", &["real/k1.pub"]);
        let mut stuck = downloaded();
        stuck.relock = Err(crate::tools::Failure { code: athanor_trust_state::ErrorCode::Internal, host: None });
        let (power, caller) = (FakePower::new(false, Ok(())), FakeCaller::new(&stuck, &[false, true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]));
        assert_eq!(apply(&machine.ctx(&stuck, 5000), &power, &caller).await, Err(Refusal::Failed));
        assert_eq!(caller.asked(), [ACTION_APPLY], "never asked while the deployment is unlocked");
        assert!(!power.rebooted.get());
        assert_eq!(published(&machine), UpdateState::WillApplyAtNextShutdown, "the truthful state when the re-lock fails");
    }

    #[tokio::test]
    async fn go_back_holds_the_digest_it_leaves() {
        let machine = Machine::new("go-back", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(2), 2000));
        assert_eq!(go_back(&machine.ctx(&tools, 5000), &FakePower::new(false, Ok(()))).await, Err(Refusal::NoPreviousVersion));
        let machine = Machine::new("go-back-again", &["real/k1.pub"]);
        tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
        let power = FakePower::new(false, Ok(()));
        assert_eq!(go_back(&machine.ctx(&tools, 5000), &power).await, Ok(()));
        assert_eq!(machine.store.held(), Some(digest(2)));
        assert_eq!(*tools.calls.borrow(), ["rollback"]);
        assert!(power.rebooted.get());
    }

    #[tokio::test]
    async fn go_back_after_a_queued_rollback_only_holds_and_reboots() {
        let machine = Machine::new("go-back-queued", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(2), 2000));
        tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
        tools.status.borrow_mut().rollback_queued = true;
        let power = FakePower::new(false, Ok(()));
        assert_eq!(go_back(&machine.ctx(&tools, 5000), &power).await, Ok(()));
        assert_eq!(machine.store.held(), Some(digest(2)));
        assert!(tools.calls.borrow().is_empty(), "no second rollback");
        assert!(power.rebooted.get());
    }
}
