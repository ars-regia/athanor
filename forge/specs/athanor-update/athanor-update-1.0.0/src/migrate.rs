//! `athanor-update migrate`: move the machine, once, onto the signed reference
//! (docs/architecture/doc_update_trust.md, UT4). One mechanism for a fresh install from
//! the ISO and for a machine installed before the policy existed. A machine that follows the
//! project's previous owner moves to the same image under the owner the policy pins, on the
//! tag it follows (docs/architecture/doc_update_delivery.md, UD45).
use crate::check::Context;
use crate::policy::InForce;
use crate::sigobj;
use crate::store::Store;
use crate::tools::{Failure, Tools};

/// The tag machines follow (decision D1).
pub const CHANNEL: &str = "stable";

/// How many times the migration stages one digest: once, and once more after a deployment of
/// it that did not boot. Then the digest is held, as a rollback holds it (UT6).
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

/// True when nothing is left to move, read from files alone: the stamp exists and no image
/// of a previous owner can be booted under the policy in force. bootc is not asked, so a
/// persistent `bootc status` failure does not fail the unit at every run of its timer.
#[must_use]
pub fn finished(policy: &InForce, store: &Store) -> bool {
    store.migrated() && (!policy.info.shipped || policy.moved_from.is_empty())
}

/// # Errors
/// bootc, the registry or the disk failed: the unit fails and systemd starts it again.
pub fn run<T: Tools>(ctx: &Context<'_, T>) -> Result<Outcome, Failure> {
    let storage = |_| Failure { code: athanor_trust_state::ErrorCode::Storage, host: None };
    let policy = crate::policy::in_force(&ctx.policy);
    if finished(&policy, ctx.store) {
        return Ok(Outcome::Done);
    }
    let status = ctx.tools.status()?;
    let repository = sigobj::repository_of(&status.booted.image);
    // An origin that enforces the policy of the previous owner is not the signed reference
    // of this image: the policy in force no longer names that repository.
    let successor = if policy.info.shipped { crate::policy::successor(&policy.scopes, repository, &policy.moved_from) } else { None };
    // The stamp ends the migration, except on an image of the previous owner: a machine stamped
    // while that owner was the policy's still moves (doc_update_delivery.md, UD49).
    if ctx.store.migrated() && successor.is_none() {
        return Ok(Outcome::Done);
    }
    if status.booted.local_changes {
        // bootc switch refuses a deployment with local packages; the next boot tries again.
        return Ok(Outcome::Waiting("local-changes"));
    }
    if status.booted.enforcing && successor.is_none() {
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
    let (to, target) = match successor {
        // The tag, or digest, the machine follows is kept: only the owner changes.
        Some(successor) => (successor, format!("{successor}{}", &status.booted.image[repository.len()..])),
        None if policy.scopes.contains_key(repository) => (repository, format!("{repository}:{CHANNEL}")),
        None => return Ok(Outcome::Waiting("reference-out-of-scope")),
    };
    // The switch of an earlier run is staged and boots at the next restart. A deployment that
    // did not boot, or was removed, is no longer staged, and the machine switches again, within
    // the bound below.
    if status.staged.as_ref().is_some_and(|staged| staged.enforcing && staged.image == target) {
        return Ok(Outcome::Waiting("restart-pending"));
    }
    // The build-time rule of UT5 holds here too: the migration never moves a machine back.
    let Some(candidate) = ctx.tools.candidate(&target)? else {
        // The channel is not published yet (A2-4), or the new owner has no such image or tag
        // yet. Record it for the state and wait: the unit succeeds, and the migration timer
        // starts it again (athanor-update-migrate.timer).
        ctx.store.set_channel_absent(true).map_err(storage)?;
        return Ok(Outcome::Waiting(if successor.is_some() { "successor-absent" } else { "channel-absent" }));
    };
    ctx.store.set_channel_absent(false).map_err(storage)?;
    // Nothing releases a held digest, and the migration never stages one: the user went back
    // from it, greenboot rolled it back, or it was staged ATTEMPTS times and never booted for
    // good. The machine stays where it is until the tag names a newer build.
    let mut held = ctx.store.held().as_deref() == Some(candidate.digest.as_str());
    if !held && ctx.store.move_attempts(&candidate.digest) >= ATTEMPTS {
        tracing::warn!(digest = %candidate.digest, attempts = ATTEMPTS, "the switched deployment did not boot: the digest is held and not staged again");
        ctx.store.set_held(&candidate.digest).map_err(storage)?;
        held = true;
    }
    ctx.store.set_move_held(held).map_err(storage)?;
    if held {
        return Ok(Outcome::Waiting("held"));
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
    ctx.store.record_move_attempt(&staged.digest).map_err(storage)?;
    if staged.build_time > status.booted.build_time {
        // A newer build is an update, and an update waits for the user (SH11).
        ctx.tools.relock()?;
    }
    if let Some(dir) = ctx.store.signature_dir(&staged.digest) {
        if let Err(failure) = ctx.tools.fetch_signature(to, &staged.digest, &dir) {
            tracing::warn!(code = ?failure.code, "the signature object was not fetched; the next check fetches it");
        }
    }
    // No stamp yet: the machine has migrated once it boots the signed reference, so a
    // deployment that does not boot leaves it to switch again, up to ATTEMPTS stagings.
    Ok(Outcome::Switched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{deployed, digest, Fake, Machine, REPO, SIGNED};
    use crate::tools::Deployed;

    fn from_media(build_time: i64) -> Deployed {
        Deployed { enforcing: false, image: format!("{REPO}:35355843782"), ..deployed(&digest(9), build_time) }
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
        for booted in [from_media(1000), from_previous_owner(":latest", false)] {
            let mut tools = Fake::booted(booted).offering(SIGNED, 1000);
            tools.stages = false;
            assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Internal));
            assert!(!machine.store.migrated() && !tools.called("fetch_signature"));
        }
    }

    #[test]
    fn a_switched_deployment_that_does_not_boot_is_switched_once_more_and_then_held() {
        for booted in [from_media(1000), from_previous_owner(":latest", true)] {
            let owner = booted.enforcing;
            let machine = Machine::new(&format!("migrate-no-boot-{owner}"), &["real/k1.pub"]);
            let tools = Fake::booted(booted).offering(SIGNED, 1000);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
            // The deployment failed to boot and the machine is back on the old reference, with
            // nothing staged: one more attempt.
            tools.status.borrow_mut().staged = None;
            assert_eq!(run(&machine.ctx(&tools, 6000)), Ok(Outcome::Switched));
            // It failed again: the digest is held, recorded as a rollback records it, and no
            // third switch is made, at this run or any later one.
            tools.status.borrow_mut().staged = None;
            for now in [7000, 8000] {
                assert_eq!(run(&machine.ctx(&tools, now)), Ok(Outcome::Waiting("held")));
            }
            assert_eq!(switches(&tools), 2);
            assert_eq!(machine.store.held().as_deref(), Some(SIGNED));
            assert!(!machine.store.migrated());
            if owner {
                let state = crate::check::run(&machine.ctx(&tools, 8000), true).expect("offline");
                assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMovedHeld);
            }
            // A newer build under the tag is not held: the move resumes, locked for the user.
            let newer = Fake::booted(tools.status.borrow().booted.clone()).offering(&digest(4), 2000);
            assert_eq!(run(&machine.ctx(&newer, 9000)), Ok(Outcome::Switched));
            assert!(newer.called("relock") && !machine.store.move_held());
        }
    }

    /// The move from an older build of the previous owner to a newer one under the new owner,
    /// booted: the machine is on the new owner and stamped.
    fn moved(test: &str) -> (Machine, Fake) {
        let machine = Machine::new(test, &["real/k1.pub"]);
        let tools = Fake::booted(Deployed { enforcing: false, ..from_previous_owner_at(&digest(1), 1000) }).offering(SIGNED, 2000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        assert!(tools.called("relock"));
        tools.apply_downloaded().expect("Apply()");
        restart(&tools);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        assert!(machine.store.migrated());
        (machine, tools)
    }

    /// The return to the deployment of the previous owner, the moved one kept as the rollback.
    fn back_on_the_previous_owner(tools: &Fake) {
        let mut status = tools.status.borrow_mut();
        status.rollback = Some(std::mem::replace(&mut status.booted, Deployed { enforcing: false, ..from_previous_owner_at(&digest(1), 1000) }));
        status.rollback_queued = false;
    }

    fn assert_held_on_the_previous_owner(machine: &Machine, tools: &Fake) {
        for now in [6000, 7000] {
            assert_eq!(run(&machine.ctx(tools, now)), Ok(Outcome::Waiting("held")));
        }
        assert_eq!(switches(tools), 1, "the held digest is never staged again");
        assert!(tools.status.borrow().staged.is_none());
        let state = crate::check::run(&machine.ctx(tools, 7000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMovedHeld);
    }

    #[test]
    fn going_back_after_the_move_is_not_undone_by_the_migration() {
        let (machine, tools) = moved("migrate-owner-go-back");
        // GoBack() holds the booted digest, then the machine reboots on the previous owner.
        machine.store.set_held(SIGNED).expect("GoBack()");
        back_on_the_previous_owner(&tools);
        assert_held_on_the_previous_owner(&machine, &tools);
    }

    #[test]
    fn a_greenboot_rollback_after_the_move_is_not_undone_by_the_migration() {
        let (machine, tools) = moved("migrate-owner-greenboot");
        // greenboot queued the rollback on the moved deployment; the check of that boot holds it.
        tools.status.borrow_mut().rollback_queued = true;
        crate::check::run(&machine.ctx(&tools, 5500), true).expect("offline");
        assert_eq!(machine.store.held().as_deref(), Some(SIGNED));
        back_on_the_previous_owner(&tools);
        assert_held_on_the_previous_owner(&machine, &tools);
    }

    #[test]
    fn a_stamped_machine_without_a_previous_owner_does_not_ask_bootc() {
        let machine = Machine::new("migrate-finished", &["real/k1.pub"]);
        machine.store.set_migrated().expect("stamp");
        std::fs::write(machine.policy.shipped.join("moved-from"), "").expect("write");
        let mut tools = Fake::booted(deployed(SIGNED, 1000));
        tools.status_fails = true;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        // With a previous owner listed, bootc is asked, and its failure fails the unit.
        std::fs::write(machine.policy.shipped.join("moved-from"), "localhost:5000/previous\n").expect("write");
        assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Internal));
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

    const PREVIOUS: &str = "localhost:5000/previous/athanor-system";

    fn from_previous_owner(suffix: &str, enforcing: bool) -> Deployed {
        Deployed { enforcing, image: format!("{PREVIOUS}{suffix}"), ..deployed(SIGNED, 1000) }
    }

    fn from_previous_owner_at(digest: &str, build_time: i64) -> Deployed {
        Deployed { image: format!("{PREVIOUS}:latest"), ..deployed(digest, build_time) }
    }

    #[test]
    fn a_machine_on_the_previous_owner_moves_to_the_same_tag_under_the_policy_s_owner() {
        for enforcing in [false, true] {
            let machine = Machine::new(&format!("migrate-owner-{enforcing}"), &["real/k1.pub"]);
            let tools = Fake::booted(from_previous_owner(":latest", enforcing)).offering(SIGNED, 1000);
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMoved);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
            assert!(tools.called(&format!("candidate {REPO}:latest")) && tools.called(&format!("switch {REPO}:latest")));
            assert!(!tools.called("relock"), "the same digest is not an update");
            assert!(tools.called(&format!("fetch_signature {REPO} {SIGNED}")), "the signature object of the new owner");
            assert!(!machine.store.migrated() && !machine.store.channel_absent());
            // Until the restart the machine still runs the previous owner's reference, and the
            // switch is the update that installs at the next restart.
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMoved);
            assert_eq!(state.update, athanor_trust_state::UpdateState::WillApplyAtNextShutdown);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("restart-pending")));
            restart(&tools);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
            assert!(machine.store.migrated() && switches(&tools) == 1);
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::Signature);
        }
    }

    #[test]
    fn a_stamp_written_while_the_previous_owner_was_the_policy_s_does_not_stop_the_move() {
        let machine = Machine::new("migrate-owner-stamped", &["real/k1.pub"]);
        machine.store.set_migrated().expect("stamp");
        let tools = Fake::booted(from_previous_owner(":latest", true)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        assert!(tools.called(&format!("switch {REPO}:latest")));
    }

    #[test]
    fn a_digest_reference_moves_by_the_same_digest() {
        let machine = Machine::new("migrate-owner-digest", &["real/k1.pub"]);
        let tools = Fake::booted(from_previous_owner(&format!("@{SIGNED}"), false)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        assert!(tools.called(&format!("switch {REPO}@{SIGNED}")));
    }

    #[test]
    fn a_move_to_a_newer_build_waits_for_the_user_and_an_older_one_is_refused() {
        let machine = Machine::new("migrate-owner-newer", &["real/k1.pub"]);
        let newer = Fake::booted(from_previous_owner(":latest", false)).offering(SIGNED, 2000);
        assert_eq!(run(&machine.ctx(&newer, 5000)), Ok(Outcome::Switched));
        assert!(newer.called("relock"));

        let machine = Machine::new("migrate-owner-older", &["real/k1.pub"]);
        let older = Fake::booted(from_previous_owner(":latest", false)).offering(SIGNED, 500);
        assert_eq!(run(&machine.ctx(&older, 5000)), Ok(Outcome::Waiting("channel-older-than-booted")));
        assert!(!older.called("switch") && !machine.store.migrated());
    }

    #[test]
    fn a_missing_image_under_the_new_owner_waits_without_a_stamp() {
        let machine = Machine::new("migrate-owner-absent", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_previous_owner(":latest", false));
        tools.candidate = Ok(None);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("successor-absent")));
        assert!(!tools.called("switch") && !machine.store.migrated());
        // The shield does not promise a move that cannot happen yet.
        let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMovedWaiting);
        let published = Fake::booted(from_previous_owner(":latest", false)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&published, 6000)), Ok(Outcome::Switched));
        let state = crate::check::run(&machine.ctx(&published, 6000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMoved);
    }

    #[test]
    fn a_queued_rollback_on_the_previous_owner_reads_as_waiting() {
        let machine = Machine::new("migrate-owner-rollback", &["real/k1.pub"]);
        let tools = Fake::booted(from_previous_owner(":latest", false)).offering(SIGNED, 1000);
        tools.status.borrow_mut().rollback_queued = true;
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("rollback-queued")));
        let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMovedWaiting);
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
        let tools = Fake::booted(Deployed { image: format!("{REPO}:latest"), ..deployed(SIGNED, 1000) });
        let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::Signature, "a channel");
    }

    #[test]
    fn a_network_failure_or_a_refused_signature_fails_the_move_without_a_stamp() {
        let machine = Machine::new("migrate-owner-fail", &["real/k1.pub"]);
        let mut offline = Fake::booted(from_previous_owner(":latest", false));
        offline.candidate = Err(Failure { code: athanor_trust_state::ErrorCode::Network, host: None });
        assert_eq!(run(&machine.ctx(&offline, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Network));
        let mut refused = Fake::booted(from_previous_owner(":latest", false)).offering(SIGNED, 1000);
        refused.download = Err(Failure { code: athanor_trust_state::ErrorCode::Policy, host: None });
        assert_eq!(run(&machine.ctx(&refused, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Policy));
        assert!(!machine.store.migrated());
    }

    #[test]
    fn another_registry_or_image_name_is_not_moved() {
        let machine = Machine::new("migrate-owner-foreign", &["real/k1.pub"]);
        for image in ["registry.example/previous/athanor-system:latest", "localhost:5000/previous/something-else:latest", "localhost:5000/fork/athanor-system:latest"] {
            let tools = Fake::booted(Deployed { enforcing: false, image: image.into(), ..deployed(SIGNED, 1000) }).offering(SIGNED, 1000);
            assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Waiting("reference-out-of-scope")), "{image}");
            assert!(tools.calls.borrow().is_empty());
        }
    }

    #[test]
    fn without_the_shipped_policy_an_enforcing_previous_owner_is_left_as_it_is() {
        let machine = Machine::new("migrate-owner-local-policy", &["real/k1.pub"]);
        std::fs::remove_file(&machine.policy.etc_registries).expect("unlink");
        let tools = Fake::booted(from_previous_owner(":latest", true)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        assert!(!tools.called("switch"));
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
