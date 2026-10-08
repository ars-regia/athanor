//! `athanor-update migrate`: move the machine, once, onto the signed reference
//! (docs/architecture/doc_update_trust.md, UT4). One mechanism for a fresh install from
//! the ISO and for a machine installed before the policy existed. A machine that follows the
//! project's previous owner moves to the same image under the owner the policy pins, on the
//! tag it follows (docs/architecture/doc_update_delivery.md, UD45).
use crate::check::Context;
use crate::sigobj;
use crate::tools::{Failure, Tools};

/// The tag machines follow (decision D1).
pub const CHANNEL: &str = "stable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The stamp exists, or the booted reference already enforces the policy.
    Done,
    /// The reference was switched; the signed digest boots at the next restart.
    Switched,
    /// Nothing was done and nothing is wrong: the unit succeeds and the next boot, or the next
    /// run of the check timer, tries again.
    Waiting(&'static str),
}

/// # Errors
/// bootc, the registry or the disk failed: the unit fails and systemd starts it again.
pub fn run<T: Tools>(ctx: &Context<'_, T>) -> Result<Outcome, Failure> {
    let storage = |_| Failure { code: athanor_trust_state::ErrorCode::Storage, host: None };
    if ctx.store.migrated() {
        return Ok(Outcome::Done);
    }
    let status = ctx.tools.status()?;
    if status.booted.local_changes {
        // bootc switch refuses a deployment with local packages; the next boot tries again.
        return Ok(Outcome::Waiting("local-changes"));
    }
    let policy = crate::policy::in_force(&ctx.policy);
    let repository = sigobj::repository_of(&status.booted.image);
    // An origin that enforces the policy of the previous owner is not the signed reference
    // of this image: the policy in force no longer names that repository.
    let successor = if policy.info.shipped { crate::policy::successor(&policy.scopes, repository) } else { None };
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
    // The build-time rule of UT5 holds here too: the migration never moves a machine back.
    let Some(candidate) = ctx.tools.candidate(&target)? else {
        if successor.is_some() {
            // The new owner has no such image or tag yet: the state keeps reading
            // `owner-moved`, and the migration timer starts the unit again.
            return Ok(Outcome::Waiting("successor-absent"));
        }
        // The channel is not published yet (A2-4). Record it for the state and wait: the unit
        // succeeds, and the check timer starts it again (athanor-update-check.service).
        ctx.store.set_channel_absent(true).map_err(storage)?;
        return Ok(Outcome::Waiting("channel-absent"));
    };
    ctx.store.set_channel_absent(false).map_err(storage)?;
    if candidate.build_time < status.booted.build_time {
        return Ok(Outcome::Waiting("channel-older-than-booted"));
    }
    ctx.tools.switch(&target)?;
    let staged = ctx.tools.status()?.staged;
    if let Some(staged) = &staged {
        if staged.build_time > status.booted.build_time {
            // A newer build is an update, and an update waits for the user (SH11).
            ctx.tools.relock()?;
        }
        if let Some(dir) = ctx.store.signature_dir(&staged.digest) {
            if let Err(failure) = ctx.tools.fetch_signature(to, &staged.digest, &dir) {
                tracing::warn!(code = ?failure.code, "the signature object was not fetched; the next check fetches it");
            }
        }
    }
    ctx.store.set_migrated().map_err(storage)?;
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

    #[test]
    fn a_machine_from_media_is_switched_to_the_channel_once() {
        let machine = Machine::new("migrate", &["real/k1.pub"]);
        let tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Switched));
        assert!(tools.called(&format!("switch {REPO}:stable")));
        assert!(!tools.called("relock"), "the same build is not an update");
        assert!(machine.store.signature_dir(SIGNED).expect("dir").join("manifest.json").exists());
        assert_eq!(run(&machine.ctx(&tools, 5000)), Ok(Outcome::Done));
        assert_eq!(tools.calls.borrow().iter().filter(|call| call.starts_with("switch")).count(), 1);
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
            assert!(machine.store.migrated() && !machine.store.channel_absent());
            // Until the restart the machine still runs the previous owner's reference, and the
            // switch is the update that installs at the next restart.
            let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
            assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMoved);
            assert_eq!(state.update, athanor_trust_state::UpdateState::WillApplyAtNextShutdown);
        }
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
        assert!(!tools.called("switch") && !machine.store.migrated() && !machine.store.channel_absent());
        let state = crate::check::run(&machine.ctx(&tools, 5000), true).expect("offline");
        assert_eq!(state.verified.reason, athanor_trust_state::Reason::OwnerMoved);
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
        for image in ["registry.example/previous/athanor-system:latest", "localhost:5000/previous/something-else:latest"] {
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
