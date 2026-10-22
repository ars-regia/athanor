//! The requests of UT6 and UT13, free of D-Bus so they are tested with fakes. `serve.rs`
//! adds the bus, polkit and logind around them.
use crate::attestation::Class;
use crate::check::{self, Context};
use crate::sigobj;
use crate::tools::{Deployed, Status, Tools};

pub const ACTION_APPLY: &str = "os.athanor.update.apply";
/// `Apply()` while another session is open: a restart ends that work too (ADR-0103 D22).
pub const ACTION_APPLY_MULTI_SESSION: &str = "os.athanor.update.apply-multi-session";
pub const ACTION_POSTPONE: &str = "os.athanor.update.postpone";
/// Asked on every call: the action is never kept (ADR-0094).
pub const ACTION_SET_AUTOMATIC: &str = "os.athanor.update.set-automatic";

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
    /// `Postpone()`: the lineage's one postpone is used, or its budget expired (UT13).
    AlreadyPostponed,
    /// `Postpone()`: the staged digest is of the feature class, or a signed copy of the
    /// booted build (UT5).
    NotSecurityClass,
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

/// The staged deployment `Apply()` may unlock: downloaded and locked, newer than the booted
/// one, not held, and on a repository the policy in force covers.
///
/// # Errors
/// [`Refusal::Failed`] when the held digest cannot be read: that is no answer, and it must not
/// release the digest the user went back from (UT6).
fn downloaded<'s, T: Tools>(ctx: &Context<'_, T>, status: &'s Status) -> Result<Option<&'s Deployed>, Refusal> {
    // bootc rollback discards the staged deployment; one staged after it would replace the
    // queued return (doc_recovery.md, R5), so a queued rollback has nothing to apply.
    if status.rollback_queued {
        return Ok(None);
    }
    let held = ctx.store.held().map_err(|err| {
        tracing::error!(%err, "cannot read the held digest; Apply() refuses");
        // The state names the failure, so the shield stops offering "Restart to update".
        republish(ctx);
        Refusal::Failed
    })?;
    let policy = crate::policy::in_force(&ctx.policy);
    Ok(status.staged.as_ref().filter(|staged| {
        staged.download_only
            && status.booted.enforcing
            && policy.scopes.contains_key(sigobj::repository_of(&status.booted.image))
            && staged.build_time > status.booted.build_time
            && held.as_deref() != Some(staged.digest.as_str())
    }))
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
    let mut lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
    let Some(staged) = downloaded(ctx, &status)?.cloned() else {
        return Err(Refusal::NothingDownloaded);
    };
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
        // The prompt can wait for minutes: checks and other requests are not held up by it.
        drop(lock);
        caller.authorize(ACTION_APPLY_MULTI_SESSION).await?;
        lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
        // A check or another request may have run during the prompt: only the deployment
        // the caller was asked about is unlocked, and only while nothing inhibits a restart.
        let now = ctx.tools.status().map_err(|_| Refusal::Failed)?;
        if downloaded(ctx, &now)? != Some(&staged) {
            tracing::error!("the staged deployment changed while polkit was asked: it stays locked");
            return Err(Refusal::Failed);
        }
        if power.blocked().await? {
            return Err(Refusal::Blocked);
        }
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
    drop(lock);
    Ok(())
}

/// True when `staged` is a security update: of the security class by the attestation the
/// check stored, and not a signed copy of the booted build (UT5), which updates nothing.
fn staged_security<T: Tools>(ctx: &Context<'_, T>, status: &Status, staged: &Deployed) -> bool {
    let policy = crate::policy::in_force(&ctx.policy);
    !check::same_build(&status.booted, &staged.digest, &staged.version, staged.build_time)
        && matches!(check::stored_class(ctx, &policy, sigobj::repository_of(&staged.image), &staged.digest), Class::Security { .. })
}

/// `Postpone()`: spends the lineage's one postpone (UT13) and locks the staged security
/// digest again, so the next shutdown does not apply it. Returns the expiry, after which the
/// check or the shutdown unlocks it. The refusals are those of ADR-0127 point 14.
///
/// # Errors
/// See [`Refusal`]. A refusal changes nothing; an unreadable opt-out or lineage is
/// [`Refusal::Failed`], never read as the default. polkit has asked for [`ACTION_POSTPONE`]
/// before `ctx` is made, so `ctx.now` is the time of the answer, however long the prompt took.
pub fn postpone<T: Tools>(ctx: &Context<'_, T>) -> Result<i64, Refusal> {
    let _lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let automatic = ctx.store.automatic_security_updates().map_err(|err| {
        tracing::error!(%err, "cannot read the opt-out of automatic security updates; Postpone() refuses");
        Refusal::Failed
    })?;
    let mut lineage = ctx.store.lineage().map_err(|err| {
        tracing::error!(%err, "cannot read the lineage of security updates; Postpone() refuses");
        Refusal::Failed
    })?;
    let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
    // With the opt-out on nothing applies by itself, so there is nothing to postpone.
    let staged = status.staged.as_ref().filter(|_| automatic && !status.rollback_queued).ok_or(Refusal::NothingDownloaded)?;
    let postponed = lineage.open.as_ref().is_some_and(|open| open.postponed);
    // Locked and not postponed: nothing applies at the next shutdown.
    if staged.download_only && !postponed {
        return Err(Refusal::NothingDownloaded);
    }
    if !staged_security(ctx, &status, staged) {
        return Err(Refusal::NotSecurityClass);
    }
    if postponed || lineage.expired(ctx.now) {
        return Err(Refusal::AlreadyPostponed);
    }
    // The check opens the lineage when it unlocks; a digest `Apply()` unlocked has none yet.
    lineage.notice(ctx.now, ctx.boot_id.as_deref().unwrap_or_default());
    let until = lineage.postpone(ctx.now).map_err(|_| Refusal::AlreadyPostponed)?;
    // Locked first: a postpone recorded over a deployment that still applies would be false.
    ctx.tools.relock().map_err(|_| Refusal::Failed)?;
    if let Err(err) = ctx.store.set_lineage(&lineage) {
        tracing::error!(%err, "cannot record the postpone; the update is unlocked again");
        republish(ctx);
        return Err(Refusal::Failed);
    }
    republish(ctx);
    Ok(until)
}

/// `SetAutomaticSecurityUpdates(enabled)`: writes the opt-out of UT13 in any state, an
/// unreadable one included. Turned off, it first locks an unlocked security digest again;
/// turned on, the run after the request unlocks a waiting one and opens its lineage (ADR-0127
/// point 8). Returns whether the value changed. polkit has asked for [`ACTION_SET_AUTOMATIC`]
/// before `ctx` is made, as for [`postpone`].
///
/// # Errors
/// See [`Refusal`]. When the relock fails nothing is written.
pub fn set_automatic<T: Tools>(ctx: &Context<'_, T>, enabled: bool) -> Result<bool, Refusal> {
    let _lock = ctx.store.try_lock().map_err(|_| Refusal::Busy)?;
    let previous = ctx.store.automatic_security_updates().ok();
    if !enabled {
        // Locked first, as in `postpone`: an opt-out written over a deployment that still
        // applies at the next shutdown would be false.
        let status = ctx.tools.status().map_err(|_| Refusal::Failed)?;
        let unlocked = status.staged.as_ref().filter(|staged| !staged.download_only && !status.rollback_queued);
        if unlocked.is_some_and(|staged| staged_security(ctx, &status, staged)) && ctx.tools.relock().is_err() {
            tracing::error!("the security update could not be locked again; the opt-out is unchanged");
            return Err(Refusal::Failed);
        }
    }
    if let Err(err) = ctx.store.set_automatic_security_updates(enabled) {
        tracing::error!(%err, "cannot write the opt-out of automatic security updates");
        // The opt-out still reads as before, so the run unlocks again what was just locked.
        republish(ctx);
        return Err(Refusal::Failed);
    }
    republish(ctx);
    Ok(previous != Some(enabled))
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
    use crate::check::tests::{attested, deployed, digest, in_att_repo, locked, offered_attested, open_lineage, security, stage, staged_for_shutdown, store_attestation, Fake, Machine, NOW};
    use crate::lineage::Lineage;
    use crate::tools::{Deployed, Failure};
    use athanor_trust_state::{ErrorCode, State, UpdateState};
    use std::cell::{Cell, RefCell};

    struct FakePower {
        blocked: Cell<Result<bool, Refusal>>,
        reboot: Result<(), RebootError>,
        rebooted: Cell<bool>,
    }

    impl FakePower {
        fn new(blocked: bool, reboot: Result<(), RebootError>) -> Self {
            Self { blocked: Cell::new(Ok(blocked)), reboot, rebooted: Cell::new(false) }
        }
    }

    impl Power for FakePower {
        async fn blocked(&self) -> Result<bool, Refusal> {
            self.blocked.get()
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
        /// Runs while polkit is asked, as the prompt waits.
        meanwhile: Option<&'a dyn Fn(&str)>,
    }

    impl<'a> FakeCaller<'a> {
        fn new(tools: &'a Fake, counts: &[bool], grants: &'a [&'a str]) -> Self {
            Self { counts: RefCell::new(counts.iter().copied().collect()), grants, tools, asked: RefCell::new(Vec::new()), meanwhile: None }
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
            if let Some(meanwhile) = self.meanwhile {
                meanwhile(action);
            }
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

    fn state(machine: &Machine) -> State {
        let text = std::fs::read_to_string(machine.store.run.join("state.json")).expect("state");
        athanor_trust_state::parse(&text).expect("schema 2")
    }

    fn published(machine: &Machine) -> UpdateState {
        state(machine).update
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
    async fn apply_refuses_when_the_held_digest_cannot_be_read() {
        let machine = Machine::new("apply-held-unreadable", &["real/k1.pub"]);
        // A directory in place of the file fails with EISDIR, whoever runs the test.
        std::fs::create_dir(machine.store.var.join("held")).expect("mkdir");
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &FakeCaller::alone(&tools)).await, Err(Refusal::Failed));
        assert!(tools.calls.borrow().is_empty() && !power.rebooted.get());
        assert_eq!(published(&machine), UpdateState::None, "the shield stops offering the restart");
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
        let power = FakePower { blocked: Cell::new(Err(Refusal::Failed)), ..FakePower::new(false, Ok(())) };
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

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn a_session_opened_before_the_reboot_is_asked_about_with_the_deployment_locked() {
        let _spawn = no_spawn();
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

    /// Held by a test whose request releases the store lock and takes it again (SPAWN).
    fn no_spawn() -> std::sync::MutexGuard<'static, ()> {
        crate::goback::tests::SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // SPAWN is held across the request on purpose: no test may fork while the store lock is
    // released and taken again, or the next try_lock meets the child's copy and says busy.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn the_second_prompt_waits_with_the_store_lock_free() {
        let _spawn = no_spawn();
        let machine = Machine::new("apply-second-unheld", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let free = RefCell::new(Vec::new());
        let probe = |action: &str| free.borrow_mut().push((action.to_owned(), machine.store.try_lock().is_ok()));
        let caller = FakeCaller { meanwhile: Some(&probe), ..FakeCaller::new(&tools, &[false, true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]) };
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Ok(()));
        assert_eq!(*free.borrow(), [(ACTION_APPLY.to_owned(), true), (ACTION_APPLY_MULTI_SESSION.to_owned(), true)]);
        assert!(power.rebooted.get());
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn a_deployment_changed_during_the_second_prompt_stays_locked() {
        let _spawn = no_spawn();
        let machine = Machine::new("apply-second-changed", &["real/k1.pub"]);
        let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
        let restaged = |action: &str| {
            if action == ACTION_APPLY_MULTI_SESSION {
                tools.status.borrow_mut().staged = Some(Deployed { download_only: true, ..deployed(&digest(3), 3000) });
            }
        };
        let caller = FakeCaller { meanwhile: Some(&restaged), ..FakeCaller::new(&tools, &[false, true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]) };
        assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Err(Refusal::Failed));
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock"], "never unlocked again");
        assert!(!power.rebooted.get());
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn an_inhibitor_taken_during_the_second_prompt_is_honoured() {
        let _spawn = no_spawn();
        for (case, inhibitor, refusal) in [("blocked", Ok(true), Refusal::Blocked), ("unanswered", Err(Refusal::Failed), Refusal::Failed)] {
            let machine = Machine::new(&format!("apply-second-{case}"), &["real/k1.pub"]);
            let (tools, power) = (downloaded(), FakePower::new(false, Ok(())));
            let inhibit = |action: &str| {
                if action == ACTION_APPLY_MULTI_SESSION {
                    power.blocked.set(inhibitor);
                }
            };
            let caller = FakeCaller { meanwhile: Some(&inhibit), ..FakeCaller::new(&tools, &[false, true], &[ACTION_APPLY, ACTION_APPLY_MULTI_SESSION]) };
            assert_eq!(apply(&machine.ctx(&tools, 5000), &power, &caller).await, Err(refusal), "{case}");
            assert_eq!(*tools.calls.borrow(), ["apply_downloaded", "relock"], "{case}: never unlocked again");
            assert!(!power.rebooted.get(), "{case}");
            assert_eq!(published(&machine), UpdateState::Downloaded, "{case}");
        }
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
        assert_eq!(machine.store.held().expect("read"), Some(digest(2)));
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
        assert_eq!(machine.store.held().expect("read"), Some(digest(2)));
        assert!(tools.calls.borrow().is_empty(), "no second rollback");
        assert!(power.rebooted.get());
    }

    /// `attested()` staged and unlocked for the next shutdown, of `class` by its stored
    /// attestation, under `lineage`; the calls of the setup are forgotten.
    fn will_apply(test: &str, class: Class, lineage: &Lineage) -> (Machine, Fake) {
        let (machine, tools) = staged_for_shutdown(test, class, lineage);
        tools.status.borrow_mut().staged.as_mut().expect("staged").download_only = false;
        (machine, tools)
    }

    fn lineage_file(machine: &Machine) -> Vec<u8> {
        std::fs::read(machine.store.var.join("lineage.json")).expect("lineage.json")
    }

    fn first_notice_at(machine: &Machine) -> Option<i64> {
        machine.store.lineage().expect("lineage").open.map(|open| open.first_notice_at)
    }

    /// `postpone` refuses with `refusal`, calls no tool and leaves the lineage file as it was.
    fn refused_untouched(machine: &Machine, tools: &Fake, now: i64, refusal: Refusal) {
        let before = lineage_file(machine);
        assert_eq!(postpone(&machine.ctx(tools, now)), Err(refusal));
        assert!(tools.calls.borrow().is_empty(), "{:?}", tools.calls.borrow());
        assert_eq!(lineage_file(machine), before, "the lineage is untouched");
    }

    #[test]
    fn postpone_relocks_and_records() {
        let (machine, tools) = will_apply("postpone", security(), &open_lineage(NOW, false));
        assert_eq!(postpone(&machine.ctx(&tools, NOW + 60)), Ok(NOW + 604_800));
        assert_eq!(*tools.calls.borrow(), ["relock"]);
        let state = state(&machine);
        assert_eq!(state.update, UpdateState::Downloaded);
        assert!(state.postpone.expect("open").postponed);
    }

    #[test]
    fn postpone_opens_the_lineage_of_a_digest_apply_unlocked() {
        let (machine, tools) = will_apply("postpone-no-lineage", security(), &Lineage::default());
        assert_eq!(postpone(&machine.ctx(&tools, NOW)), Ok(NOW + 604_800));
        assert_eq!(first_notice_at(&machine), Some(NOW));
        assert!(locked(&tools));
    }

    #[test]
    fn a_second_postpone_is_already_postponed() {
        let (machine, tools) = will_apply("postpone-twice", security(), &open_lineage(NOW, false));
        assert_eq!(postpone(&machine.ctx(&tools, NOW)), Ok(NOW + 604_800));
        tools.calls.borrow_mut().clear();
        refused_untouched(&machine, &tools, NOW + 60, Refusal::AlreadyPostponed);
        assert_eq!(first_notice_at(&machine), Some(NOW));
    }

    #[test]
    fn a_newer_digest_of_the_lineage_is_already_postponed() {
        // The lineage's postpone is spent and the check left a newer security digest locked.
        let (machine, tools) = staged_for_shutdown("postpone-newer", security(), &open_lineage(NOW - 86_400, true));
        refused_untouched(&machine, &tools, NOW, Refusal::AlreadyPostponed);
        assert_eq!(first_notice_at(&machine), Some(NOW - 86_400));
    }

    #[test]
    fn an_expired_budget_is_already_postponed() {
        let (machine, tools) = will_apply("postpone-expired", security(), &open_lineage(NOW, false));
        refused_untouched(&machine, &tools, NOW + 604_800, Refusal::AlreadyPostponed);
        let mut booted_out = open_lineage(NOW, false);
        booted_out.open.as_mut().expect("open").boots_remaining = 0;
        let (machine, tools) = will_apply("postpone-booted-out", security(), &booted_out);
        refused_untouched(&machine, &tools, NOW, Refusal::AlreadyPostponed);
    }

    #[test]
    fn a_feature_digest_is_not_security_class() {
        let (machine, tools) = will_apply("postpone-feature", Class::Feature, &open_lineage(NOW, false));
        refused_untouched(&machine, &tools, NOW, Refusal::NotSecurityClass);
    }

    #[test]
    fn postpone_with_nothing_staged_is_nothing_downloaded() {
        let (machine, tools) = offered_attested("postpone-nothing", Some(security()));
        store_attestation(&machine, &tools);
        machine.store.set_lineage(&open_lineage(NOW, false)).expect("write");
        tools.calls.borrow_mut().clear();
        refused_untouched(&machine, &tools, NOW, Refusal::NothingDownloaded);
        // Staged and locked with nothing postponed: an unlock that failed applies nothing.
        stage(&tools);
        refused_untouched(&machine, &tools, NOW, Refusal::NothingDownloaded);
        // A queued rollback: the return, not the staged digest, is the next boot.
        let (machine, tools) = will_apply("postpone-queued", security(), &open_lineage(NOW, false));
        tools.status.borrow_mut().rollback_queued = true;
        refused_untouched(&machine, &tools, NOW, Refusal::NothingDownloaded);
    }

    #[test]
    fn postpone_with_the_opt_out_on_is_nothing_downloaded() {
        let (machine, tools) = will_apply("postpone-opt-out", security(), &open_lineage(NOW, false));
        machine.store.set_automatic_security_updates(false).expect("write");
        refused_untouched(&machine, &tools, NOW, Refusal::NothingDownloaded);
    }

    #[test]
    fn postpone_of_a_signed_copy_is_not_security_class() {
        // The staged digest is the booted build under another digest, however it is attested.
        let (machine, tools) = will_apply("postpone-signed-copy", security(), &open_lineage(NOW, false));
        tools.status.borrow_mut().booted = in_att_repo(deployed(&digest(1), 2000));
        refused_untouched(&machine, &tools, NOW, Refusal::NotSecurityClass);
    }

    #[test]
    fn postpone_with_an_unreadable_lineage_is_failed() {
        let (machine, tools) = will_apply("postpone-unreadable", security(), &open_lineage(NOW, false));
        std::fs::write(machine.store.var.join("lineage.json"), b"").expect("write");
        refused_untouched(&machine, &tools, NOW, Refusal::Failed);
        let (machine, tools) = will_apply("postpone-unreadable-opt-out", security(), &open_lineage(NOW, false));
        std::fs::write(machine.store.var.join("automatic-security-updates"), b"").expect("write");
        refused_untouched(&machine, &tools, NOW, Refusal::Failed);
    }

    #[test]
    fn turning_automatic_off_relocks_an_unlocked_security_digest() {
        let (machine, tools) = will_apply("automatic-off", security(), &open_lineage(NOW, false));
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Ok(true));
        assert_eq!(*tools.calls.borrow(), ["relock"]);
        let state = state(&machine);
        assert_eq!((state.update, state.automatic_security_updates), (UpdateState::Downloaded, false));
        assert!(!machine.store.automatic_security_updates().expect("read"));
        // A feature digest unlocked by Apply() is the user's restart: it stays unlocked.
        let (machine, tools) = will_apply("automatic-off-feature", Class::Feature, &Lineage::default());
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Ok(true));
        assert!(tools.calls.borrow().is_empty() && !locked(&tools));
    }

    #[test]
    fn turning_it_on_unlocks_a_waiting_one_and_starts_the_clock() {
        let (machine, tools) = staged_for_shutdown("automatic-on", security(), &Lineage::default());
        machine.store.set_automatic_security_updates(false).expect("write");
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), true), Ok(true));
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        let state = state(&machine);
        assert_eq!((state.update, state.automatic_security_updates), (UpdateState::WillApplyAtNextShutdown, true));
        assert_eq!(state.postpone.map(|postpone| postpone.first_notice_at), Some(NOW));
        assert_eq!(attested(), tools.status.borrow().staged.as_ref().expect("staged").digest);
    }

    #[test]
    fn set_automatic_reports_only_a_change() {
        let (machine, tools) = staged_for_shutdown("automatic-change", security(), &Lineage::default());
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), true), Ok(false), "on by default");
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Ok(true));
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Ok(false));
        // An unreadable opt-out is rewritten, and the write is a change.
        std::fs::write(machine.store.var.join("automatic-security-updates"), b"").expect("write");
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Ok(true));
        assert!(!machine.store.automatic_security_updates().expect("read"));
    }

    #[test]
    fn turning_automatic_off_writes_nothing_when_the_relock_fails() {
        let (machine, mut tools) = will_apply("automatic-off-relock-fails", security(), &open_lineage(NOW, false));
        tools.relock = Err(Failure { code: ErrorCode::Internal, host: None });
        assert_eq!(set_automatic(&machine.ctx(&tools, NOW), false), Err(Refusal::Failed));
        assert!(machine.store.automatic_security_updates().expect("read"), "the opt-out is unchanged");
        assert_eq!(*tools.calls.borrow(), ["relock"]);
    }

    #[tokio::test]
    async fn go_back_does_not_reset_the_lineage() {
        let machine = Machine::new("go-back-lineage", &["real/k1.pub"]);
        let lineage = open_lineage(NOW, true);
        machine.store.set_lineage(&lineage).expect("write");
        let tools = Fake::booted(deployed(&digest(2), 2000));
        tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
        assert_eq!(go_back(&machine.ctx(&tools, NOW), &FakePower::new(false, Ok(()))).await, Ok(()));
        assert_eq!(machine.store.lineage().expect("read"), lineage);
    }
}
