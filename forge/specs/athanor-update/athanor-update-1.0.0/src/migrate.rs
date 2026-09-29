//! `athanor-update migrate`: move the machine, once, onto the signed reference
//! (docs/architecture/doc_update_trust.md, UT4). One mechanism for a fresh install from
//! the ISO and for a machine installed before the policy existed.
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
    /// Nothing was done and nothing is wrong: the unit succeeds and the next boot tries again.
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
    if status.booted.enforcing {
        ctx.store.set_migrated().map_err(storage)?;
        return Ok(Outcome::Done);
    }
    let policy = crate::policy::in_force(&ctx.policy);
    if !policy.info.shipped {
        return Ok(Outcome::Waiting("policy-not-in-force"));
    }
    let repository = sigobj::repository_of(&status.booted.image);
    if !policy.scopes.contains_key(repository) {
        return Ok(Outcome::Waiting("reference-out-of-scope"));
    }
    let target = format!("{repository}:{CHANNEL}");
    // The build-time rule of UT5 holds here too: the migration never moves a machine back.
    let candidate = ctx.tools.candidate(&target)?;
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
            if let Err(failure) = ctx.tools.fetch_signature(repository, &staged.digest, &dir) {
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
    fn a_failed_switch_leaves_no_stamp() {
        let machine = Machine::new("migrate-fail", &["real/k1.pub"]);
        let mut tools = Fake::booted(from_media(1000)).offering(SIGNED, 1000);
        tools.download = Err(Failure { code: athanor_trust_state::ErrorCode::Policy, host: None });
        assert_eq!(run(&machine.ctx(&tools, 5000)).map_err(|failure| failure.code), Err(athanor_trust_state::ErrorCode::Policy));
        assert!(!machine.store.migrated());
    }
}
