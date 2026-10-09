//! `athanor-update migrate`: move the machine, once, onto the signed reference
//! (docs/architecture/doc_update_trust.md, UT4). One mechanism for a fresh install from
//! the ISO and for a machine installed before the policy existed.
use crate::check::Context;
use crate::sigobj;
use crate::tools::{Failure, Tools};

/// The tag machines follow (decision D1).
pub const CHANNEL: &str = "stable";

/// How many deployments of one digest may fail to boot before the migration stops staging it:
/// the first, and one more. A staging that was discarded unbooted (a power cycle before a
/// locked update was applied, or before a clean shutdown) is not a failure.
pub const ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The stamp exists, or the booted reference already enforces the policy.
    Done,
    /// The reference was switched; the signed digest boots at the next restart, and the run
    /// after that boot writes the stamp.
    Switched,
    /// Nothing was done and nothing is wrong: the unit succeeds and the next boot, or the next
    /// run of the migration timer, tries again.
    Waiting(&'static str),
}

/// # Errors
/// bootc, the registry or the disk failed: the unit fails and systemd starts it again.
pub fn run<T: Tools>(ctx: &Context<'_, T>) -> Result<Outcome, Failure> {
    let storage = |_| Failure { code: athanor_trust_state::ErrorCode::Storage, host: None };
    // The stamp ends the migration: bootc is not asked, so a persistent `bootc status` failure
    // does not fail the unit at every run of its timer.
    if ctx.store.migrated() {
        return Ok(Outcome::Done);
    }
    let policy = crate::policy::in_force(&ctx.policy);
    let status = ctx.tools.status()?;
    let repository = sigobj::repository_of(&status.booted.image);
    if status.booted.local_changes {
        // bootc switch refuses a deployment with local packages; the next boot tries again.
        return Ok(Outcome::Waiting("local-changes"));
    }
    if status.booted.enforcing {
        ctx.store.set_migrated().map_err(storage)?;
        return Ok(Outcome::Done);
    }
    if status.rollback_queued {
        // A switch would stage over the queued return (doc_recovery.md, R5); the next boot
        // tries again.
        return Ok(Outcome::Waiting("rollback-queued"));
    }
    if !policy.info.shipped {
        return Ok(Outcome::Waiting("policy-not-in-force"));
    }
    if !policy.scopes.contains_key(repository) {
        return Ok(Outcome::Waiting("reference-out-of-scope"));
    }
    let target = format!("{repository}:{CHANNEL}");
    // The switch of an earlier run is staged and boots at the next restart. A deployment that
    // did not boot, or was removed, is no longer staged, and the machine switches again, within
    // the bound below.
    if status.staged.as_ref().is_some_and(|staged| staged.enforcing && staged.image == target) {
        return Ok(Outcome::Waiting("restart-pending"));
    }
    // The build-time rule of UT5 holds here too: the migration never moves a machine back.
    let Some(candidate) = ctx.tools.candidate(&target)? else {
        // The channel is not published yet (A2-4). Record it for the state and wait: the unit
        // succeeds, and the migration timer starts it again (athanor-update-migrate.timer).
        ctx.store.set_channel_absent(true).map_err(storage)?;
        return Ok(Outcome::Waiting("channel-absent"));
    };
    ctx.store.set_channel_absent(false).map_err(storage)?;
    let mut record = ctx.store.move_record(&candidate.digest);
    if let Some(staged) = record.staged.take() {
        // The staging of an earlier run is gone (restart-pending returned above). Deployed and
        // left, it is the rollback deployment now: it did not boot for good, and greenboot or
        // the person went back. Not there, a power cycle discarded it before it was finalized,
        // which says nothing about the digest (spike U1, section 4). Only the deployment's name
        // tells the two apart once an earlier deployment of the digest is the rollback.
        let left = status.rollback.as_ref().is_some_and(|rollback| rollback.digest == candidate.digest && rollback.deployment.as_deref() == Some(staged.as_str()));
        if left {
            record.failed_boots += 1;
            tracing::warn!(digest = %candidate.digest, failed_boots = record.failed_boots, "the switched deployment did not boot");
        }
        ctx.store.set_move_record(&record).map_err(storage)?;
    }
    // Nothing releases a held digest, and the migration never stages one: the user went back
    // from it, or greenboot rolled it back. Nor does it stage again a digest whose deployments
    // failed to boot ATTEMPTS times; that is the migration's own record, not the held file,
    // which keeps the digest the person went back from. Either way the machine stays where it
    // is until the tag names a newer build.
    let held = ctx.store.held().as_deref() == Some(candidate.digest.as_str());
    let failed = record.failed_boots >= ATTEMPTS;
    if held {
        return Ok(Outcome::Waiting("held"));
    }
    if failed {
        return Ok(Outcome::Waiting("boot-failed"));
    }
    if candidate.build_time < status.booted.build_time {
        return Ok(Outcome::Waiting("channel-older-than-booted"));
    }
    ctx.tools.switch(&target)?;
    // What bootc staged is what counts: a switch that staged nothing, or not the target with
    // the policy enforced, failed, and the unit tries again.
    let Some(staged) = ctx.tools.status()?.staged.filter(|staged| staged.enforcing && staged.image == target) else {
        tracing::error!(%target, "bootc switch returned without staging the target with the policy enforced");
        return Err(Failure { code: athanor_trust_state::ErrorCode::Internal, host: None });
    };
    // Unnamed, the staging is not followed: a deployment that does not boot is still left by
    // greenboot, whose queued return holds the digest (check.rs), and a held digest is never
    // staged again.
    record.staged.clone_from(&staged.deployment);
    ctx.store.set_move_record(&record).map_err(storage)?;
    if staged.build_time > status.booted.build_time {
        // A newer build is an update, and an update waits for the user (SH11).
        ctx.tools.relock()?;
    }
    if let Some(dir) = ctx.store.signature_dir(&staged.digest) {
        if let Err(failure) = ctx.tools.fetch_signature(repository, &staged.digest, &dir) {
            tracing::warn!(code = ?failure.code, "the signature object was not fetched; the next check fetches it");
        }
    }
    // No stamp yet: the machine has migrated once it boots the signed reference, so a
    // deployment that does not boot leaves it to switch again, up to ATTEMPTS failed boots.
    Ok(Outcome::Switched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{deployed, digest, Fake, Machine, REPO, SIGNED};
    use crate::tools::Deployed;

    fn on_media(digest: &str, build_time: i64) -> Deployed {
        Deployed { enforcing: false, image: format!("{REPO}:35355843782"), ..deployed(digest, build_time) }
    }

    fn from_media(build_time: i64) -> Deployed {
        on_media(&digest(9), build_time)
    }

    /// The restart: the staged deployment boots.
    fn restart(tools: &Fake) {
        let mut status = tools.status.borrow_mut();
        status.booted = status.staged.take().expect("staged");
    }

    fn switches(tools: &Fake) -> usize {
        tools.calls.borrow().iter().filter(|call| call.starts_with("switch")).count()
    }

    #[test]
    fn a_machine_from_media_is_switched_to_the_channel_once() {
        let machine = Machine::new("migrate", &["real/k1.pub"]);
        let tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        assert!(tools.called(&format!("switch {REPO}:stable")));
        assert!(!tools.called("relock"), "the same build is not an update");
        assert!(machine.store.signature_dir(SIGNED).expect("dir").join("manifest.json").exists());
        assert!(!machine.store.migrated(), "the stamp waits for the boot of the signed reference");
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("restart-pending")));
        restart(&tools);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        assert!(machine.store.migrated());
        assert_eq!(switches(&tools), 1);
    }

    #[test]
    fn a_switch_that_stages_nothing_fails_and_leaves_no_stamp() {
        let machine = Machine::new("migrate-stages-nothing", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        tools.stages = false;
        assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Internal));
        assert!(!machine.store.migrated() && !tools.called("fetch_signature"));
    }

    /// The staged deployment was deployed and did not boot: the machine is back on the old
    /// deployment, and the one that failed is the rollback (greenboot spent its tries).
    fn failed_boot(tools: &Fake) {
        let mut status = tools.status.borrow_mut();
        status.rollback = Some(status.staged.take().expect("staged"));
    }

    /// A power cycle before the staged deployment was finalized: it is gone, nothing booted it
    /// (spike U1, section 4: `status.staged: null`, the booted version unchanged).
    fn power_off(tools: &Fake) {
        tools.status.borrow_mut().staged = None;
    }

    #[test]
    fn a_switched_deployment_that_does_not_boot_is_switched_once_more_and_then_left() {
        // The same build finalizes on its own at shutdown; a newer one is locked and applied
        // by the user. Either way the deployment then fails to boot, twice.
        let cases = [(from_media(1000), 1000, false), (on_media(&digest(1), 1000), 2000, true)];
        for (n, (booted, build_time, locked)) in cases.into_iter().enumerate() {
            let machine = Machine::new(&format!("migrate-no-boot-{n}"), &["real/k1.pub"]);
            // The digest the person went back from earlier stays held, whatever the move does.
            machine.store.set_held(&digest(7)).expect("GoBack()");
            let tools = Fake::booted(booted).offering(SIGNED, build_time);
            for now in [5000, 6000] {
                assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Switched));
                assert_eq!(tools.called("relock"), locked);
                if locked {
                    tools.apply_downloaded().expect("Apply()");
                }
                failed_boot(&tools);
            }
            // Two failed boots of the digest: no third switch, at this run or any later one.
            for now in [7000, 8000] {
                assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Waiting("boot-failed")), "{n}");
            }
            assert_eq!(switches(&tools), 2);
            assert_eq!(machine.store.held(), Some(digest(7)), "the held digest of GoBack() is not released");
            assert!(!machine.store.migrated());
            // A newer build under the tag has not failed: the move resumes, locked for the user.
            let newer = Fake::booted(tools.status.borrow().booted.clone()).offering(&digest(4), 3000);
            assert_eq!(run(&machine.ctx(&newer, 9000)), Ok(Outcome::Switched));
            assert!(newer.called("relock"));
        }
    }

    #[test]
    fn without_deployment_names_no_boot_is_counted_and_greenboot_still_holds() {
        // A power-off after a deployment of the digest is the rollback: the original finding.
        let machine = Machine::new("migrate-unnamed-power-off", &["real/k1.pub"]);
        let mut tools = Fake::booted(on_media(&digest(1), 1000)).offering(SIGNED, 2000);
        tools.names = false;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        tools.apply_downloaded().expect("Apply()");
        failed_boot(&tools);
        for now in [6000, 7000, 8000, 9000] {
            assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Switched), "{now}");
            power_off(&tools);
        }
        assert_eq!(machine.store.move_record(SIGNED).failed_boots, 0);
        // A real failure: greenboot queues the return and the check of that boot holds the digest.
        let machine = Machine::new("migrate-unnamed-greenboot", &["real/k1.pub"]);
        let mut tools = Fake::booted(on_media(&digest(1), 1000)).offering(SIGNED, 2000);
        tools.names = false;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        tools.apply_downloaded().expect("Apply()");
        restart(&tools);
        tools.status.borrow_mut().rollback_queued = true;
        crate::check::run(&machine.ctx(&tools, 5500), true).expect("offline");
        back_on_media(&tools);
        assert_held_on_media(&machine, &tools);
    }

    #[test]
    fn a_power_off_after_one_failed_boot_is_not_a_second_one() {
        let machine = Machine::new("migrate-fail-then-power-off", &["real/k1.pub"]);
        let tools = Fake::booted(on_media(&digest(1), 1000)).offering(SIGNED, 2000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        tools.apply_downloaded().expect("Apply()");
        failed_boot(&tools);
        // The failed deployment stays the rollback while the next stagings are powered off.
        for now in [6000, 7000, 8000] {
            assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Switched), "{now}");
            power_off(&tools);
        }
        assert_eq!(run(&machine.ctx(&tools, 9000)), Ok(Outcome::Switched));
        tools.apply_downloaded().expect("Apply()");
        failed_boot(&tools);
        assert_eq!(run(&machine.ctx(&tools, 10000)), Ok(Outcome::Waiting("boot-failed")));
        assert_eq!(switches(&tools), 5);
    }

    #[test]
    fn a_staged_move_that_is_powered_off_is_staged_again_and_never_left() {
        // A newer build is locked and waits for Apply(); the same build waits for a clean
        // shutdown. A power cycle discards either, which says nothing about the digest.
        for (n, (booted, build_time)) in [(on_media(&digest(1), 1000), 2000), (from_media(1000), 1000)].into_iter().enumerate() {
            let machine = Machine::new(&format!("migrate-power-off-{n}"), &["real/k1.pub"]);
            machine.store.set_held(&digest(7)).expect("GoBack()");
            let tools = Fake::booted(booted).offering(SIGNED, build_time);
            for now in [5000, 6000, 7000, 8000] {
                assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Switched), "{n} at {now}");
                power_off(&tools);
            }
            assert_eq!(switches(&tools), 4);
            assert_eq!(machine.store.held(), Some(digest(7)));
        }
    }

    /// The return to the deployment the machine was installed with, the switched one kept as the
    /// rollback.
    fn back_on_media(tools: &Fake) {
        let mut status = tools.status.borrow_mut();
        status.rollback = Some(std::mem::replace(&mut status.booted, on_media(&digest(1), 1000)));
        status.rollback_queued = false;
    }

    fn assert_held_on_media(machine: &Machine, tools: &Fake) {
        for now in [6000, 7000] {
            assert_eq!(run(&machine.ctx(tools, now)), Ok(Outcome::Waiting("held")));
        }
        assert_eq!(switches(tools), 1, "the held digest is never staged again");
        assert!(tools.status.borrow().staged.is_none());
    }

    #[test]
    fn a_stamped_machine_does_not_ask_bootc() {
        let machine = Machine::new("migrate-finished", &["real/k1.pub"]);
        machine.store.set_migrated().expect("stamp");
        let mut tools = Fake::booted(deployed(SIGNED, 1000));
        tools.status_fails = true;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
    }

    #[test]
    fn a_newer_channel_is_staged_locked_and_an_older_one_is_not_followed() {
        let machine = Machine::new("migrate-newer", &["real/k1.pub"]);
        let newer = Fake::booted(from_media(1000)).offering(SIGNED, 2000);
        assert_eq!(run(&machine.ctx(&newer, 5000)), Ok(Outcome::Switched));
        assert!(newer.called("relock"));

        let machine = Machine::new("migrate-older", &["real/k1.pub"]);
        let older = Fake::booted(from_media(1000)).offering(SIGNED, 500);
        assert_eq!(run(&machine.ctx(&older, 5000)), Ok(Outcome::Waiting("channel-older-than-booted")));
        assert!(!older.called("switch") && !machine.store.migrated());
    }

    #[test]
    fn without_the_policy_in_force_nothing_is_switched_and_no_stamp_is_written() {
        let machine = Machine::new("migrate-policy", &["real/k1.pub"]);
        std::fs::remove_file(&machine.policy.etc_registries).expect("unlink");
        let tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("policy-not-in-force")));
        assert!(tools.calls.borrow().is_empty() && !machine.store.migrated());
    }

    #[test]
    fn a_queued_rollback_waits() {
        let machine = Machine::new("migrate-rollback-queued", &["real/k1.pub"]);
        let tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        tools.status.borrow_mut().rollback_queued = true;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("rollback-queued")));
        assert!(tools.calls.borrow().is_empty() && !machine.store.migrated());
    }

    #[test]
    fn a_machine_with_local_changes_waits() {
        let machine = Machine::new("migrate-local", &["real/k1.pub"]);
        let tools = Fake::booted(Deployed { local_changes: true, ..from_media(1000) }).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("local-changes")));
        assert!(tools.calls.borrow().is_empty() && !machine.store.migrated());
    }

    #[test]
    fn a_channel_that_is_not_published_yet_waits_and_reads_channel_absent() {
        let machine = Machine::new("migrate-absent", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        tools.candidate = Ok(None);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("channel-absent")));
        assert!(!tools.called("switch") && !machine.store.migrated());
        // The later checks keep saying why, until the channel appears.
        let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::ChannelAbsent);

        let published = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&published, 6000)), Ok(Outcome::Switched));
        let state = crate::check::run(&machine.ctx(&published, 6000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::Media);
    }

    #[test]
    fn the_stamp_clears_a_stale_channel_absent() {
        let machine = Machine::new("migrate-stamp-clears", &["real/k1.pub"]);
        machine.store.set_channel_absent(true).expect("flag");
        let tools = Fake::booted(Deployed { enforcing: true, ..from_media(1000) });
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        assert!(machine.store.migrated() && !machine.store.channel_absent());
    }

    #[test]
    fn any_other_registry_failure_still_fails() {
        let machine = Machine::new("migrate-registry", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_media(1000));
        for code in [athanor_trust_state::ErrorCode::Registry, athanor_trust_state::ErrorCode::Network] {
            tools.candidate = Err(Failure { code, host: None });
            assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(code));
        }
        assert!(!machine.store.channel_absent() && !machine.store.migrated());
    }

    #[test]
    fn a_verified_machine_on_a_run_tag_is_pinned_and_left_where_it_is() {
        let machine = Machine::new("migrate-pinned", &["real/k1.pub"]);
        machine.store_signature(SIGNED, "real");
        for suffix in [":37691917204".to_owned(), format!("@{SIGNED}")] {
            let tools = Fake::booted(Deployed { image: format!("{REPO}{suffix}"), ..deployed(SIGNED, 1000) }).offering(SIGNED, 1000);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
            assert!(!tools.called("switch"), "{suffix}");
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::PinnedBuild, "{suffix}");
        }
        // A channel, no tag, and the moving tag of a derived image all follow newer builds.
        for suffix in [":latest", "", ":prod", ":2026-10"] {
            let tools = Fake::booted(Deployed { image: format!("{REPO}{suffix}"), ..deployed(SIGNED, 1000) });
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::Signature, "{suffix}");
        }
    }

    #[test]
    fn another_registry_or_image_name_is_left_out_of_scope() {
        let machine = Machine::new("migrate-owner-foreign", &["real/k1.pub"]);
        for image in ["registry.example/previous/athanor-system:latest", "localhost:5000/previous/something-else:latest", "localhost:5000/fork/athanor-system:latest"] {
            let tools = Fake::booted(Deployed { enforcing: false, image: image.into(), ..deployed(SIGNED, 1000) }).offering(SIGNED, 1000);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("reference-out-of-scope")), "{image}");
            assert!(tools.calls.borrow().is_empty());
        }
    }

    #[test]
    fn a_failed_switch_leaves_no_stamp() {
        let machine = Machine::new("migrate-fail", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        tools.download = Err(Failure { code: athanor_trust_state::ErrorCode::Policy, host: None });
        assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Policy));
        assert!(!machine.store.migrated());
    }
}
