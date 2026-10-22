//! `athanor-update check`: derive the verification of the booted digest again, ask the
//! registry, download, unlock a security-class digest, publish
//! (docs/architecture/doc_update_trust.md, UT1, UT5, UT6, UT12, UT13).
use crate::attestation::{self, Class};
use crate::lineage::Lineage;
use crate::policy::{InForce, PolicyPaths};
use crate::sigobj;
use crate::store::Store;
use crate::tools::{Candidate, Deployed, Failure, Status, Tools};
use athanor_trust_state::{Deployment, ErrorCode, Reason, State, UpdateState, SCHEMA};
use p256::ecdsa::VerifyingKey;
use std::path::PathBuf;

pub struct Context<'a, T: Tools> {
    pub tools: &'a T,
    pub store: &'a Store,
    pub policy: PolicyPaths,
    pub efivars: PathBuf,
    pub lockdown: PathBuf,
    /// Seconds since the epoch.
    pub now: i64,
    /// `/proc/sys/kernel/random/boot_id`, by which the lineage of UT13 counts boots; `None`
    /// when it cannot be read, and then no boot is counted.
    pub boot_id: Option<String>,
}

impl<'a, T: Tools> Context<'a, T> {
    /// The paths of an installed machine.
    #[must_use]
    pub fn system(tools: &'a T, store: &'a Store, now: i64) -> Self {
        Self {
            tools,
            store,
            policy: PolicyPaths::system(),
            efivars: "/sys/firmware/efi/efivars".into(),
            lockdown: "/sys/kernel/security/lockdown".into(),
            now,
            boot_id: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map(|id| id.trim().to_owned())
                .inspect_err(|err| tracing::error!(%err, "cannot read the boot id; no boot of the security lineage is counted"))
                .ok(),
        }
    }
}

fn deployment(deployed: &Deployed) -> Deployment {
    Deployment { image: deployed.image.clone(), digest: deployed.digest.clone(), version: deployed.version.clone(), build_time: deployed.build_time }
}

/// Why the booted image is, or is not, verified. Never a stored answer: the stored
/// signature object is verified again, against the keys the shipped policy names today.
fn reason<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, status: &Status) -> Reason {
    let booted = &status.booted;
    if booted.local_changes {
        // What runs is the image plus local packages: no signature covers that tree.
        return Reason::LocalChanges;
    }
    if !policy.info.shipped {
        return Reason::PolicyNotInForce;
    }
    let repository = sigobj::repository_of(&booted.image);
    if !policy.scopes.contains_key(repository) {
        return Reason::ReferenceOutOfScope;
    }
    if !booted.enforcing {
        let pending = status.staged.as_ref().is_some_and(|staged| staged.enforcing);
        // The migration ran and nothing enforcing is staged: the reference was switched
        // afterwards without the policy. Reported, not repaired: the migration does not run
        // again (UT4).
        if ctx.store.migrated() && !pending {
            return Reason::OriginNotEnforcing;
        }
        // The installer's reference, or an install that has not migrated yet (UT4).
        return if ctx.store.channel_absent() { Reason::ChannelAbsent } else { Reason::Media };
    }
    match booted_claim(ctx, policy, booted) {
        // Verified, but a run-number tag or a digest names one build: nothing newer is ever
        // published under it (doc_update_delivery.md, UD51).
        Some(true) if !follows_a_channel(&booted.image[repository.len()..]) => Reason::PinnedBuild,
        Some(true) => Reason::Signature,
        Some(false) => Reason::KeyNotInPolicy,
        // The install from media, staged to boot the signed copy of its own build (UT5).
        None if status.staged.as_ref().is_some_and(|staged| staged.enforcing && staged.image == booted.image && same_build(booted, &staged.digest, &staged.version, staged.build_time)) => Reason::Media,
        None => Reason::NoSignature,
    }
}

/// What the stored signature objects say of the booted digest: `Some(true)` when a key the
/// policy in force names for its repository signed it, `Some(false)` when only other keys
/// did, `None` when nothing covers it.
fn booted_claim<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, booted: &Deployed) -> Option<bool> {
    let repository = sigobj::repository_of(&booted.image);
    let keys = policy_keys(policy, repository)?;
    let claims = ctx.store.signature_dir(&booted.digest).and_then(|dir| sigobj::claims(&dir, &keys).ok()).unwrap_or_default();
    let covers = |claim: &&sigobj::Claim| claim.manifest_digest == booted.digest && claim.repository == repository;
    claims.iter().filter(covers).map(|claim| claim.backed).max()
}

/// The keys the policy in force names for `repository`, which verify its image signatures
/// (UT5) and its release attestations (UT13); `None` when the policy has no scope for it.
fn policy_keys(policy: &InForce, repository: &str) -> Option<Vec<VerifyingKey>> {
    let key_paths = policy.scopes.get(repository)?;
    let key = |path: &PathBuf| {
        let key = std::fs::read_to_string(path).ok().and_then(|pem| sigobj::load_key(&pem));
        if key.is_none() {
            tracing::warn!(path = %path.display(), "a key of the policy in force cannot be read; nothing it signed verifies");
        }
        key
    };
    Some(key_paths.iter().filter_map(key).collect())
}

/// The class of `digest` by the release attestation stored for it, verified again with the
/// keys of the policy in force (UT13): no verdict is kept. Nothing stored is the feature class.
fn stored_class<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, repository: &str, digest: &str) -> Class {
    match ctx.store.attestation_dir(digest) {
        Some(dir) => attestation::class_of(&dir, &policy_keys(policy, repository).unwrap_or_default(), repository, digest),
        None => Class::Feature,
    }
}

/// The class of the candidate: its release attestation is fetched, then verified. A failed
/// fetch is logged and not a failure of the check: the attestation stored by an earlier check
/// decides, and with none the digest is of the feature class and waits for `Apply()`; the next
/// check fetches again. A fetched copy replaces a stored attestation that verifies as the
/// security class only when it verifies as that class too: the attachments policy accepts
/// anything, so a registry could otherwise turn a waiting security digest into a feature one.
/// Only the attestations of the booted, the staged and the candidate digest are kept.
fn candidate_class<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, status: &Status, digest: &str) -> Class {
    let repository = sigobj::repository_of(&status.booted.image);
    let keep: Vec<&str> = [Some(status.booted.digest.as_str()), status.staged.as_ref().map(|staged| staged.digest.as_str()), Some(digest)].into_iter().flatten().collect();
    if let Err(err) = ctx.store.retain_attestations(&keep) {
        tracing::error!(%err, "cannot remove the release attestations of earlier digests");
    }
    let Some(dir) = ctx.store.attestation_dir(digest) else {
        return Class::Feature;
    };
    // The copy goes to a sibling and replaces the stored attestation only once complete: skopeo
    // leaves a partial directory behind when it fails, and a registry that fails on purpose must
    // not erase an attestation that verified. The copy is bounded neither in size nor in time: #658.
    let partial = dir.with_extension("partial");
    let fetched = remove_dir(&partial).map_err(|err| tracing::error!(%err, "cannot clear an earlier partial attestation copy")).and_then(|()| {
        ctx.tools.fetch_attestation(repository, digest, &partial).map_err(|failure| {
            tracing::warn!(code = ?failure.code, %digest, "the release attestation could not be fetched; the one stored, if any, decides");
        })
    });
    if fetched.is_ok() {
        let keys = policy_keys(policy, repository).unwrap_or_default();
        let security = |dir: &std::path::Path| matches!(attestation::class_of(dir, &keys, repository, digest), Class::Security { .. });
        let stored = if security(&partial) || !security(&dir) {
            replace_copy(&partial, &dir)
        } else {
            tracing::warn!(%digest, "the fetched release attestation does not verify as the security class; the stored one, which does, is kept");
            remove_dir(&partial)
        };
        if let Err(err) = stored {
            tracing::error!(%err, %digest, "cannot store the fetched release attestation");
        }
    }
    let class = stored_class(ctx, policy, repository, digest);
    if class == Class::Feature {
        // A feature release, or an attestation no key of the policy verifies: the log cannot
        // tell them apart, and either way nothing applies by itself.
        tracing::warn!(%digest, "the release attestation does not verify as the security class; the candidate is of the feature class");
    }
    class
}

/// Moves the attestation copy in `from` over the one in `to`, in an order that leaves `to` a
/// whole copy at every step, so a crash never loses a stored attestation: the blobs first,
/// named by their digest, then the manifest that names them, then what only the earlier copy
/// held. A crash before the manifest leaves the earlier copy with extra blobs.
fn replace_copy(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if !to.exists() {
        return std::fs::rename(from, to);
    }
    let names = std::fs::read_dir(from)?.map(|entry| entry.map(|entry| entry.file_name())).collect::<std::io::Result<Vec<_>>>()?;
    for name in names.iter().filter(|name| *name != "manifest.json") {
        std::fs::rename(from.join(name), to.join(name))?;
    }
    std::fs::rename(from.join("manifest.json"), to.join("manifest.json"))?;
    for entry in std::fs::read_dir(to)? {
        let entry = entry?;
        if !names.contains(&entry.file_name()) {
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    std::fs::remove_dir(from)
}

/// Removes a directory and what it holds; one that does not exist is no error.
fn remove_dir(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// A security-class digest is staged and locked: the check unlocks it for the next shutdown
/// with no request and no reboot (UT13), unless the opt-out is on or the lineage's postpone,
/// unexpired, holds it. `run` then opens the lineage, unless one is open.
///
/// # Errors
/// The unlock fails: the digest stays downloaded and locked, and the next check tries again.
fn settle_security<T: Tools>(ctx: &Context<'_, T>, status: &Status, lineage: &Lineage, automatic: bool) -> Result<UpdateState, Failure> {
    let postponed = lineage.open.as_ref().is_some_and(|open| open.postponed) && !lineage.expired(ctx.now);
    if !automatic || postponed {
        return Ok(UpdateState::Downloaded);
    }
    ctx.tools.apply_downloaded()?;
    tracing::info!(digest = ?status.staged.as_ref().map(|staged| &staged.digest), "security update unlocked for the next shutdown");
    Ok(UpdateState::WillApplyAtNextShutdown)
}

/// The pair after `settle_security`: a failed unlock leaves the digest downloaded.
fn settled_security(update: Result<UpdateState, Failure>, failure: Option<Failure>) -> (UpdateState, Option<Failure>) {
    match update {
        Ok(update) => (update, failure),
        Err(unlock) => (UpdateState::Downloaded, Some(unlock)),
    }
}

/// True when `digest` is the build `booted` runs under another digest: the same build time
/// and the same version, both known. A build time of 0 is an absent label, which says
/// nothing about the build.
fn same_build(booted: &Deployed, digest: &str, version: &str, build_time: i64) -> bool {
    digest != booted.digest && booted.build_time > 0 && build_time == booted.build_time && version == booted.version
}

/// True when `candidate` is the signed copy of the booted build (UT5). bootc-image-builder
/// installs from an OCI layout, which gives the build a digest no signature covers, and the
/// kickstart already writes the signed channel reference (UD2). Only such a boot moves: the
/// machine follows a channel under the policy, and nothing verifies the digest it runs.
fn signed_copy<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, booted: &Deployed, candidate: &Candidate) -> bool {
    enforced(policy, booted)
        && follows_a_channel(&booted.image[sigobj::repository_of(&booted.image).len()..])
        && same_build(booted, &candidate.digest, &candidate.version, candidate.build_time)
        && booted_claim(ctx, policy, booted) != Some(true)
}

/// The update state that needs no network: what is staged, and what was refused.
///
/// # Errors
/// `Storage` when the held or refused digest cannot be read: nothing is offered then.
fn local_update(ctx: &Context<'_, impl Tools>, status: &Status) -> Result<UpdateState, Failure> {
    Ok(match &status.staged {
        Some(staged) if !staged.download_only => UpdateState::WillApplyAtNextShutdown,
        Some(staged) if offerable(ctx.store, &status.booted, &staged.digest, staged.build_time, false)? == UpdateState::Available => UpdateState::Downloaded,
        _ if store_read(ctx.store.refused())?.is_some() => UpdateState::Refused,
        _ => UpdateState::None,
    })
}

/// A state file that exists and cannot be read is a storage failure, logged, never "no value".
fn store_read<V>(read: std::io::Result<V>) -> Result<V, Failure> {
    read.map_err(|err| {
        tracing::error!(%err, "cannot read the update state on disk");
        Failure { code: ErrorCode::Storage, host: None }
    })
}

/// The pair a check step returns: the state, and the failure that goes with it.
fn settle(update: Result<UpdateState, Failure>, failure: Option<Failure>) -> (UpdateState, Option<Failure>) {
    match update {
        Ok(update) => (update, failure),
        Err(unreadable) => (UpdateState::None, Some(unreadable)),
    }
}

/// True when the tag or digest after the repository, `suffix`, may move to a newer build. A
/// digest and a run tag of the pipeline (`github.run_id`, digits only) name one build; any
/// other tag, `latest`, `stable` or the moving tag of a derived image, is followed. No tag is
/// `latest`.
fn follows_a_channel(suffix: &str) -> bool {
    let run_tag = suffix.strip_prefix(':').is_some_and(|tag| !tag.is_empty() && tag.bytes().all(|b| b.is_ascii_digit()));
    !(suffix.starts_with('@') || run_tag)
}

/// `Available` when `digest` may be offered; otherwise why not. The order of the build
/// times is the only ordering: bootc has none, and a moved tag is a downgrade the policy
/// cannot see. The one exception is `signed_copy`, the booted build itself.
///
/// # Errors
/// `Storage` when the held digest cannot be read: an unreadable file must not release it (UT6).
fn offerable(store: &Store, booted: &Deployed, digest: &str, build_time: i64, signed_copy: bool) -> Result<UpdateState, Failure> {
    Ok(if digest == booted.digest {
        UpdateState::None
    } else if store_read(store.held())?.as_deref() == Some(digest) {
        UpdateState::Held
    } else if build_time <= booted.build_time && !signed_copy {
        UpdateState::OlderThanBooted
    } else {
        UpdateState::Available
    })
}

/// True when a download would be verified: the booted origin makes bootc apply the host
/// policy, and that policy demands a signature for the repository the machine follows.
/// The policy need not be the shipped one: the recovery of UT2 installs a local one. A
/// deployment with local changes is never upgraded: bootc refuses it.
fn enforced(policy: &InForce, booted: &Deployed) -> bool {
    booted.enforcing && !booted.local_changes && policy.scopes.contains_key(sigobj::repository_of(&booted.image))
}

/// A download the policy refused: the digest is recorded, and the refusal survives a reboot.
fn refused(store: &Store, digest: &str, failure: Failure) -> (UpdateState, Option<Failure>) {
    let stored = store.set_refused(Some(digest));
    (UpdateState::Refused, Some(stored.map_or(Failure { code: ErrorCode::Storage, host: None }, |()| failure)))
}

/// After a download that passed the policy: the refusal is cleared and the signature object
/// of `staged` is stored, unless it already is.
fn downloaded(ctx: &Context<'_, impl Tools>, status: &Status, staged: &str) -> Option<Failure> {
    let failure = ctx.store.set_refused(None).err().map(|_| Failure { code: ErrorCode::Storage, host: None });
    match ctx.store.signature_dir(staged).filter(|dir| !dir.join("manifest.json").exists()) {
        Some(dir) => failure.or(ctx.tools.fetch_signature(sigobj::repository_of(&status.booted.image), staged, &dir).err()),
        None => failure,
    }
}

/// The check with the network. `automatic` is the opt-out of UT13 as read from the store, and
/// `lineage` the lineage the run persists.
fn online_update<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, status: &mut Status, automatic: bool, lineage: &Lineage) -> (UpdateState, Option<Failure>) {
    let local = match local_update(ctx, status) {
        Ok(local) => local,
        Err(failure) => return (UpdateState::None, Some(failure)),
    };
    if !enforced(policy, &status.booted) || local == UpdateState::WillApplyAtNextShutdown {
        return (local, None);
    }
    let candidate = match ctx.tools.candidate(&status.booted.image) {
        Ok(Some(candidate)) => candidate,
        // The followed tag has no manifest: a registry answer, but nothing to offer.
        Ok(None) => return (local, Some(Failure { code: ErrorCode::Registry, host: crate::tools::host_of(&status.booted.image) })),
        Err(failure) => return (local, Some(failure)),
    };
    // The registry answered: that is a successful check, whatever it said.
    if let Err(err) = ctx.store.set_last_success(ctx.now) {
        tracing::error!(%err, "cannot record the successful check");
        return (local, Some(Failure { code: ErrorCode::Storage, host: None }));
    }
    let copy = signed_copy(ctx, policy, &status.booted, &candidate);
    let verdict = match offerable(ctx.store, &status.booted, &candidate.digest, candidate.build_time, copy) {
        Ok(verdict) => verdict,
        Err(failure) => return (local, Some(failure)),
    };
    if verdict != UpdateState::Available {
        return (if verdict == UpdateState::None { local } else { verdict }, None);
    }
    if copy {
        return move_to_signed_copy(ctx, status, &candidate);
    }
    // Classified at every check while it waits, so an attestation that failed to fetch once
    // does not leave a security digest waiting for Apply().
    let security = matches!(candidate_class(ctx, policy, status, &candidate.digest), Class::Security { .. });
    if status.staged.as_ref().is_some_and(|staged| staged.download_only && staged.digest == candidate.digest) {
        return if security { settled_security(settle_security(ctx, status, lineage, automatic), None) } else { (UpdateState::Downloaded, None) };
    }
    // UT12 holds a feature update on a metered connection; a security update downloads, unless
    // the opt-out holds it too (UT13).
    if ctx.tools.metered() && !(security && automatic) {
        return (UpdateState::Available, None);
    }
    // Residual: the class is the candidate's, but `bootc upgrade --download-only` takes what
    // the tag names when it runs and cannot be given a digest (`bootc switch` to a digest
    // would pin the origin). A tag moved in between can download a feature digest on a
    // metered link. The unlock below stays bound to `staged == candidate`.
    match ctx.tools.download() {
        Err(failure) if failure.code == ErrorCode::Policy => refused(ctx.store, &candidate.digest, failure),
        Err(failure) => (UpdateState::Available, Some(failure)),
        Ok(()) => {
            *status = match ctx.tools.status() {
                Ok(status) => status,
                Err(failure) => return (UpdateState::Available, Some(failure)),
            };
            // What bootc staged is what counts, not what the tag said a moment earlier.
            let after = match local_update(ctx, status) {
                Ok(after) => after,
                Err(failure) => return (UpdateState::Available, Some(failure)),
            };
            if after != UpdateState::Downloaded {
                return (UpdateState::Available, None);
            }
            let staged = status.staged.as_ref().map(|staged| staged.digest.clone()).unwrap_or_default();
            let failure = downloaded(ctx, status, &staged);
            // The class is the candidate's: a tag that moved during the download staged another
            // digest. A failure left by the download step unlocks nothing: the next check settles it.
            if security && staged == candidate.digest && failure.is_none() {
                return settled_security(settle_security(ctx, status, lineage, automatic), failure);
            }
            (UpdateState::Downloaded, failure)
        }
    }
}

/// The signed copy of the booted build is not an update (UT5). Downloaded under the policy,
/// it is unlocked to boot at the next shutdown with no prompt and no relock, as the migration
/// stages the same build. Only the copy the registry named in this check is unlocked, and a
/// copy whose deployments failed to boot `ATTEMPTS` times is not staged again, as in the
/// migration. Until it is unlocked the state never reads `downloaded`: it is no update to
/// announce or to apply.
fn move_to_signed_copy<T: Tools>(ctx: &Context<'_, T>, status: &mut Status, candidate: &Candidate) -> (UpdateState, Option<Failure>) {
    let mut record = match crate::migrate::record_of(ctx, status, &candidate.digest) {
        Ok(record) => record,
        Err(failure) => return (UpdateState::None, Some(failure)),
    };
    if record.failed_boots >= crate::migrate::ATTEMPTS {
        return (UpdateState::Held, None);
    }
    let staged_copy = |status: &Status| status.staged.as_ref().is_some_and(|staged| staged.download_only && staged.enforcing && staged.image == status.booted.image && staged.digest == candidate.digest);
    let mut failure = None;
    if !staged_copy(status) {
        if ctx.tools.metered() {
            return (UpdateState::Available, None);
        }
        match ctx.tools.download() {
            Err(refusal) if refusal.code == ErrorCode::Policy => return refused(ctx.store, &candidate.digest, refusal),
            Err(failure) => return (UpdateState::Available, Some(failure)),
            Ok(()) => {}
        }
        *status = match ctx.tools.status() {
            Ok(status) => status,
            Err(failure) => return (UpdateState::Available, Some(failure)),
        };
        if !staged_copy(status) {
            tracing::error!(digest = %candidate.digest, "the download did not stage the signed copy the registry named");
            return (UpdateState::Available, None);
        }
        failure = downloaded(ctx, status, &candidate.digest);
    }
    // The record names the deployment before it is unlocked: once unlocked, a copy that does
    // not boot is counted, whatever fails after. Without a record, nothing is unlocked.
    record.staged = status.staged.as_ref().and_then(|staged| staged.deployment.clone());
    let unlocked = match ctx.store.set_move_record(&record) {
        Ok(()) => ctx.tools.apply_downloaded().and_then(|()| ctx.tools.status()),
        Err(_) => Err(Failure { code: ErrorCode::Storage, host: None }),
    };
    *status = match unlocked {
        Ok(status) => status,
        Err(unlock) => {
            if let Some(earlier) = failure {
                tracing::warn!(code = ?earlier.code, "the signature object of the signed copy was not stored; the next check fetches it");
            }
            tracing::error!(code = ?unlock.code, digest = %candidate.digest, "the signed copy was not unlocked, or its status not read; the next check tries again");
            return (UpdateState::None, Some(unlock));
        }
    };
    settle(local_update(ctx, status), failure)
}

/// The evaluation at the shutdown that would apply (UT13; ADR-0127 point 13): a staged
/// security digest whose postpone expired is unlocked, so this shutdown applies it. Nothing
/// else: no state is published and no lineage changes. The lock is never waited for, so the
/// shutdown is not delayed: a busy store leaves the deployment as it is, and the next boot's
/// check unlocks it for the following shutdown.
///
/// # Errors
/// `Storage` when the store is busy or its state unreadable; bootc's failure otherwise.
pub fn at_shutdown<T: Tools>(ctx: &Context<'_, T>) -> Result<(), Failure> {
    let storage = |what: &str, err: std::io::Error| {
        tracing::error!(%err, "{what}; nothing is unlocked at this shutdown");
        Failure { code: ErrorCode::Storage, host: None }
    };
    let _lock = ctx.store.try_lock().map_err(|err| storage("the update store is busy", err))?;
    let lineage = ctx.store.lineage().map_err(|err| storage("cannot read the lineage of security updates", err))?;
    if !lineage.expired(ctx.now) || !ctx.store.automatic_security_updates().map_err(|err| storage("cannot read the opt-out of automatic security updates", err))? {
        return Ok(());
    }
    let status = ctx.tools.status()?;
    let Some(staged) = status.staged.as_ref().filter(|staged| staged.download_only && !status.rollback_queued) else {
        return Ok(());
    };
    // The check's own gates: a held digest, or one not newer than the booted build, stays locked.
    if local_update(ctx, &status)? != UpdateState::Downloaded {
        return Ok(());
    }
    let policy = crate::policy::in_force(&ctx.policy);
    if !matches!(stored_class(ctx, &policy, sigobj::repository_of(&staged.image), &staged.digest), Class::Security { .. }) {
        return Ok(());
    }
    ctx.tools.apply_downloaded()?;
    tracing::info!(digest = %staged.digest, "the postpone expired: the security update applies at this shutdown");
    Ok(())
}

/// Runs one check and publishes the state. `offline` is the run at boot, before the
/// network is up, and the run after a request: it verifies and publishes, nothing else.
///
/// # Errors
/// Only when bootc gives no status or the state cannot be published; everything else is
/// an error code inside the published state.
pub fn run<T: Tools>(ctx: &Context<'_, T>, offline: bool) -> Result<State, Failure> {
    let mut status = ctx.tools.status()?;
    let storage = |_| Failure { code: ErrorCode::Storage, host: None };
    // An unreadable record of the newest build is a storage failure, not a reason to publish
    // nothing. The file is left as it is, and `i64::MAX` is published: the badge reads it as a
    // build newer than the booted one (Attention), so a machine that went back never shows
    // "Check again" on a record it cannot trust. The administrator removes the file
    // (RECOVERY.md); nothing is offered until then.
    let (newest, unreadable_newest) = match ctx.store.record_booted(status.booted.build_time) {
        Ok(newest) => (newest, None),
        Err(err) => {
            tracing::error!(%err, "cannot read the newest booted build time; publishing it as unknown");
            (i64::MAX, Some(Failure { code: ErrorCode::Storage, host: None }))
        }
    };
    // `refused` is no gate: a refused image is verified again when it is downloaded again. An
    // unreadable record is reported and cleared, so it cannot block every download for ever.
    let unreadable_refused = match ctx.store.refused() {
        Ok(_) => None,
        Err(err) => {
            tracing::error!(%err, "cannot read the refused digest; it is cleared");
            if let Err(err) = ctx.store.set_refused(None) {
                tracing::error!(%err, "cannot clear the refused digest");
            }
            Some(Failure { code: ErrorCode::Storage, host: None })
        }
    };
    let policy = crate::policy::in_force(&ctx.policy);
    // A queued rollback leaves the booted digest (doc_recovery.md, R5): it is held, as GoBack()
    // holds it, and nothing is downloaded, since a staged deployment would become the default
    // of the next boot in place of the return.
    // The digest is held whatever the state of the newest-booted record.
    let held = if status.rollback_queued { ctx.store.set_held(&status.booted.digest).err().map(|_| Failure { code: ErrorCode::Storage, host: None }) } else { None };
    // The opt-out and the lineage of UT13, read once. An unreadable file is reported and left
    // as it is, never read as the default: the check then downloads and unlocks nothing, as
    // with an unreadable held digest.
    let automatic = ctx.store.automatic_security_updates().inspect_err(|err| tracing::error!(%err, "cannot read the opt-out of automatic security updates"));
    let mut lineage = ctx.store.lineage().inspect_err(|err| tracing::error!(%err, "cannot read the lineage of security updates"));
    let stored_lineage = lineage.as_ref().ok().cloned();
    if let Ok(lineage) = lineage.as_mut() {
        if let Some(boot_id) = &ctx.boot_id {
            lineage.boot(boot_id);
        }
        // Applied (ADR-0127 point 10, as amended on 2026-10-22): the booted digest verifies as
        // the security class, was built after the anchor, greenboot is not returning from it,
        // and greenboot declared the boot good: `greenboot-success.target` is active, read here
        // at run time, since a unit ordering only holds within one transaction.
        // `boot-complete.target` is not that verdict: it becomes active whatever the health
        // check says. The state service at boot runs before the verdict and never counts it. A boot that failed its health
        // checks is not counted, and the check still runs and downloads.
        let booted = &status.booted;
        if !offline && !status.rollback_queued && booted.build_time > lineage.anchor_build_time.unwrap_or(0) && matches!(stored_class(ctx, &policy, sigobj::repository_of(&booted.image), &booted.digest), Class::Security { .. }) && ctx.tools.boot_healthy() {
            lineage.applied(&booted.digest, booted.build_time);
        }
    }
    let (update, failure) = if unreadable_newest.is_some() {
        (UpdateState::None, unreadable_newest.or(held))
    } else if status.rollback_queued {
        settle(local_update(ctx, &status), held)
    } else {
        match (&automatic, lineage.as_ref()) {
            (Ok(automatic), Ok(lineage)) if offline => match local_update(ctx, &status) {
                // A staged security digest that no unexpired postpone holds is unlocked at boot
                // and after a request, with no network, from the attestation the check stored
                // (UT13): a postpone that expired, the opt-out turned back on, an earlier
                // unlock that failed.
                Ok(UpdateState::Downloaded) if status.staged.as_ref().is_some_and(|staged| matches!(stored_class(ctx, &policy, sigobj::repository_of(&staged.image), &staged.digest), Class::Security { .. })) => {
                    settled_security(settle_security(ctx, &status, lineage, *automatic), None)
                }
                local => settle(local, None),
            },
            (Ok(automatic), Ok(lineage)) => online_update(ctx, &policy, &mut status, *automatic, lineage),
            // The unreadable file is the failure published below.
            _ => settle(local_update(ctx, &status), None),
        }
    };
    // A security digest unlocked for the next shutdown opens the lineage, unless one is open,
    // whoever unlocked it and whether or not the run that unlocked it recorded the lineage.
    if let (Ok(lineage), UpdateState::WillApplyAtNextShutdown, Some(staged)) = (lineage.as_mut(), update, status.staged.as_ref()) {
        if matches!(stored_class(ctx, &policy, sigobj::repository_of(&staged.image), &staged.digest), Class::Security { .. }) {
            lineage.notice(ctx.now, ctx.boot_id.as_deref().unwrap_or_default());
        }
    }
    // Persisted once per run, under the store lock the caller holds, and only when it changed.
    let unwritten_lineage = match &lineage {
        Ok(lineage) if Some(lineage) != stored_lineage.as_ref() => ctx.store.set_lineage(lineage).err().map(|err| {
            tracing::error!(%err, "cannot record the lineage of security updates");
            Failure { code: ErrorCode::Storage, host: None }
        }),
        _ => None,
    };
    if !offline && enforced(&policy, &status.booted) {
        // A machine whose signature object was never stored, or was lost, heals here. A
        // failure is not the check's: the reason below already says `no-signature`.
        if let Some(dir) = ctx.store.signature_dir(&status.booted.digest).filter(|dir| !dir.join("manifest.json").exists()) {
            if let Err(failure) = ctx.tools.fetch_signature(sigobj::repository_of(&status.booted.image), &status.booted.digest, &dir) {
                tracing::warn!(code = ?failure.code, "the signature object of the booted digest could not be fetched");
            }
        }
    }
    // Read after the check, which rewrites it when the registry answers: a garbled record heals.
    let (last_successful_check, unreadable_last) = match ctx.store.last_success() {
        Ok(last) => (last, None),
        Err(err) => {
            tracing::error!(%err, "cannot read the time of the last successful check");
            (None, Some(Failure { code: ErrorCode::Storage, host: None }))
        }
    };
    // An unreadable opt-out is published off, which is what the machine then does (it unlocks
    // nothing), and an unreadable lineage as none. The published bool is for display: the
    // decisions above read the store and fail on an error.
    let (automatic_security_updates, unreadable_opt_out) = match automatic {
        Ok(automatic) => (automatic, None),
        Err(_) => (false, Some(Failure { code: ErrorCode::Storage, host: None })),
    };
    let (postpone, unreadable_lineage) = match &lineage {
        Ok(lineage) => (lineage.published(), None),
        Err(_) => (None, Some(Failure { code: ErrorCode::Storage, host: None })),
    };
    let failure = unreadable_refused.or(unreadable_opt_out).or(unreadable_lineage).or(unwritten_lineage).or(failure).or(unreadable_last);
    let state = State {
        schema: SCHEMA,
        verified: reason(ctx, &policy, &status).into(),
        booted: deployment(&status.booted),
        downloaded: status.staged.as_ref().map(deployment),
        previous: status.rollback.as_ref().map(deployment),
        update,
        policy: policy.info,
        secure_boot: crate::secureboot::read(&ctx.efivars, &ctx.lockdown),
        newest_booted_build_time: newest,
        last_successful_check,
        last_error: failure.as_ref().map_or(ErrorCode::None, |failure| failure.code),
        last_error_host: failure.and_then(|failure| failure.host),
        automatic_security_updates,
        postpone,
    };
    ctx.store.publish(&state).map_err(storage)?;
    Ok(state)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::lineage::Open;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::path::Path;

    pub(crate) const REPO: &str = "localhost:5000/spike/athanor-system";
    /// The repository the release attestations under tests/vectors/made name; every
    /// `Machine` trusts made/a.pub, their key, for it.
    pub(crate) const ATT_REPO: &str = "registry.example/owner/athanor-system";
    /// The digest the signature object under tests/vectors/real covers.
    pub(crate) const SIGNED: &str = "sha256:08d9f3ab2f3fd065175df48841e6434914170493578a3f59d6f6fc5dcdb971f9";

    pub(crate) fn digest(n: u8) -> String {
        format!("sha256:{}", format!("{n:02x}").repeat(32))
    }

    pub(crate) fn deployed(digest: &str, build_time: i64) -> Deployed {
        Deployed { image: format!("{REPO}:stable"), digest: digest.into(), version: format!("43.{build_time}"), build_time, enforcing: true, download_only: false, local_changes: false, deployment: None }
    }

    /// bootc, skopeo, ostree and NetworkManager as one scripted object that records its calls.
    pub(crate) struct Fake {
        pub status: RefCell<Status>,
        pub candidate: Result<Option<Candidate>, Failure>,
        /// What `download` stages, or how it fails.
        pub download: Result<Deployed, Failure>,
        pub metered: bool,
        pub relock: Result<(), Failure>,
        /// How `apply_downloaded` fails, if it does.
        pub unlock: Result<(), Failure>,
        /// `bootc status` fails from the next unlock on.
        pub broken_after_unlock: bool,
        /// Whether `switch` stages a deployment, or returns as if it had and stages nothing.
        pub stages: bool,
        /// `bootc status` fails, as it does when bootc cannot read the sysroot.
        pub status_fails: Cell<bool>,
        /// Whether bootc names a deployment by its ostree checksum and deploy serial.
        pub names: bool,
        /// The release attestations the registry serves, by digest; any other fetch fails.
        pub attestations: RefCell<HashMap<String, Class>>,
        /// The attestation copy fails midway, as skopeo's does, leaving only `version` behind.
        pub attestation_breaks: Cell<bool>,
        /// The test vector the registry serves in place of the one of the digest's class; ""
        /// serves a manifest with no layers.
        pub served: Cell<Option<&'static str>>,
        /// `greenboot-success.target` is active: greenboot's required health checks passed.
        pub boot_healthy: bool,
        pub calls: RefCell<Vec<String>>,
    }

    impl Fake {
        pub(crate) fn booted(booted: Deployed) -> Self {
            Self {
                status: RefCell::new(Status { booted, staged: None, rollback: None, rollback_queued: false }),
                candidate: Err(Failure { code: ErrorCode::Network, host: Some("localhost:5000".into()) }),
                download: Err(Failure { code: ErrorCode::Internal, host: None }),
                metered: false,
                relock: Ok(()),
                unlock: Ok(()),
                broken_after_unlock: false,
                stages: true,
                status_fails: Cell::new(false),
                names: true,
                attestations: RefCell::new(HashMap::new()),
                attestation_breaks: Cell::new(false),
                served: Cell::new(None),
                boot_healthy: true,
                calls: RefCell::new(Vec::new()),
            }
        }

        /// The registry serves a release attestation of `class` for `digest`: the signed vector
        /// of that class, which covers only `attested()` in `ATT_REPO`.
        pub(crate) fn attestation(self, digest: &str, class: Class) -> Self {
            self.attestations.borrow_mut().insert(digest.into(), class);
            self
        }

        pub(crate) fn offering(mut self, digest: &str, build_time: i64) -> Self {
            self.candidate = Ok(Some(Candidate { digest: digest.into(), version: format!("43.{build_time}"), build_time }));
            self.download = Ok(Deployed { download_only: true, ..deployed(digest, build_time) });
            self
        }

        fn call(&self, what: impl Into<String>) {
            self.calls.borrow_mut().push(what.into());
        }

        pub(crate) fn called(&self, what: &str) -> bool {
            self.calls.borrow().iter().any(|call| call.starts_with(what))
        }
    }

    impl Tools for Fake {
        fn status(&self) -> Result<Status, Failure> {
            if self.status_fails.get() {
                return Err(Failure { code: ErrorCode::Internal, host: None });
            }
            Ok(self.status.borrow().clone())
        }
        fn candidate(&self, image: &str) -> Result<Option<Candidate>, Failure> {
            self.call(format!("candidate {image}"));
            self.candidate.clone()
        }
        fn download(&self) -> Result<(), Failure> {
            self.call("download");
            self.status.borrow_mut().staged = Some(self.download.clone()?);
            Ok(())
        }
        fn apply_downloaded(&self) -> Result<(), Failure> {
            self.call("apply_downloaded");
            self.unlock.clone()?;
            self.status_fails.set(self.broken_after_unlock);
            if let Some(staged) = self.status.borrow_mut().staged.as_mut() {
                staged.download_only = false;
            }
            Ok(())
        }
        fn relock(&self) -> Result<(), Failure> {
            self.call("relock");
            self.relock.clone()?;
            if let Some(staged) = self.status.borrow_mut().staged.as_mut() {
                staged.download_only = true;
            }
            Ok(())
        }
        fn rollback(&self) -> Result<(), Failure> {
            self.call("rollback");
            Ok(())
        }
        fn switch(&self, image: &str) -> Result<(), Failure> {
            self.call(format!("switch {image}"));
            let deployed = self.download.clone()?;
            if self.stages {
                // bootc switch stages a deployment of `image` that is not locked against
                // finalization, a new ostree deployment each time.
                let serial = self.calls.borrow().iter().filter(|call| call.starts_with("switch")).count();
                self.status.borrow_mut().staged = Some(Deployed { image: image.into(), download_only: false, deployment: self.names.then(|| format!("{}.{serial}", deployed.digest)), ..deployed });
            }
            Ok(())
        }
        fn fetch_signature(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure> {
            self.call(format!("fetch_signature {repository} {digest}"));
            if digest != SIGNED {
                return Err(Failure { code: ErrorCode::Registry, host: Some("localhost:5000".into()) });
            }
            let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/real");
            std::fs::create_dir_all(dest).expect("mkdir");
            for entry in std::fs::read_dir(real).expect("vectors") {
                let entry = entry.expect("entry");
                std::fs::copy(entry.path(), dest.join(entry.file_name())).expect("copy");
            }
            Ok(())
        }
        fn fetch_attestation(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure> {
            self.call(format!("fetch_attestation {repository} {digest}"));
            if self.attestation_breaks.get() {
                if dest.exists() {
                    std::fs::remove_dir_all(dest).expect("clean");
                }
                std::fs::create_dir_all(dest).expect("mkdir");
                std::fs::write(dest.join("version"), "Directory Transport Version: 1.1\n").expect("write");
                return Err(Failure { code: ErrorCode::Network, host: Some("registry.example".into()) });
            }
            let vector = match (self.served.get(), self.attestations.borrow().get(digest)) {
                (Some(served), _) => served,
                (None, Some(Class::Security { .. })) => "att-security",
                (None, Some(Class::Feature)) => "att-feature",
                (None, None) => return Err(Failure { code: ErrorCode::Registry, host: Some("registry.example".into()) }),
            };
            // skopeo replaces what an earlier copy left in the directory.
            if dest.exists() {
                std::fs::remove_dir_all(dest).expect("clean");
            }
            std::fs::create_dir_all(dest).expect("mkdir");
            if vector.is_empty() {
                std::fs::write(dest.join("manifest.json"), r#"{"schemaVersion":2,"layers":[]}"#).expect("write");
                return Ok(());
            }
            for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/made").join(vector)).expect("vectors") {
                let entry = entry.expect("entry");
                std::fs::copy(entry.path(), dest.join(entry.file_name())).expect("copy");
            }
            Ok(())
        }
        fn metered(&self) -> bool {
            self.metered
        }
        fn boot_healthy(&self) -> bool {
            self.boot_healthy
        }
    }

    /// A scratch machine: the store, and a shipped policy in force that names `keys` for REPO.
    pub(crate) struct Machine {
        pub store: Store,
        pub policy: PolicyPaths,
        pub root: PathBuf,
    }

    impl Machine {
        pub(crate) fn new(test: &str, keys: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!("athanor-update-check-{}-{test}", std::process::id()));
            if root.exists() {
                std::fs::remove_dir_all(&root).expect("clean");
            }
            let store = Store { run: root.join("run"), var: root.join("var") };
            for dir in [store.run.clone(), store.var.join("signatures"), store.var.join("attestations"), root.join("etc/registries.d"), root.join("usr/registries.d")] {
                std::fs::create_dir_all(dir).expect("mkdir");
            }
            let vectors = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
            let key_paths: Vec<_> = keys.iter().map(|key| vectors.join(key).display().to_string()).collect();
            let policy = serde_json::json!({"default": [{"type": "reject"}], "transports": {"docker": {
                "": [{"type": "insecureAcceptAnything"}],
                REPO: [{"type": "sigstoreSigned", "keyPaths": key_paths, "signedIdentity": {"type": "matchRepository"}}],
                ATT_REPO: [{"type": "sigstoreSigned", "keyPaths": [vectors.join("made/a.pub").display().to_string()], "signedIdentity": {"type": "matchRepository"}}]}}});
            std::fs::write(root.join("usr/policy.json"), policy.to_string()).expect("write");
            std::fs::write(root.join("usr/registries.d/athanor.yaml"), "docker: {}\n").expect("write");
            let paths = PolicyPaths { etc_policy: root.join("etc/policy.json"), etc_registries: root.join("etc/registries.d/athanor.yaml"), shipped: root.join("usr") };
            std::os::unix::fs::symlink(root.join("usr/policy.json"), &paths.etc_policy).expect("symlink");
            std::os::unix::fs::symlink(root.join("usr/registries.d/athanor.yaml"), &paths.etc_registries).expect("symlink");
            Self { store, policy: paths, root }
        }

        pub(crate) fn ctx<'a>(&'a self, tools: &'a Fake, now: i64) -> Context<'a, Fake> {
            Context { tools, store: &self.store, policy: self.policy.clone(), efivars: self.root.join("efivars"), lockdown: self.root.join("lockdown"), now, boot_id: Some("boot-0".into()) }
        }

        pub(crate) fn store_signature(&self, digest: &str, vector: &str) {
            let dest = self.store.signature_dir(digest).expect("digest");
            std::fs::create_dir_all(&dest).expect("mkdir");
            for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors").join(vector)).expect("vectors") {
                let entry = entry.expect("entry");
                std::fs::copy(entry.path(), dest.join(entry.file_name())).expect("copy");
            }
        }
    }

    #[test]
    fn a_newer_digest_is_downloaded_and_its_signature_stored() {
        let machine = Machine::new("download", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(SIGNED, 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!(state.update, UpdateState::Downloaded);
        assert!(!tools.called("apply_downloaded"), "a newer build waits for the user");
        assert_eq!(state.downloaded.as_ref().map(|d| d.digest.as_str()), Some(SIGNED));
        assert_eq!((state.last_error, state.last_successful_check), (ErrorCode::None, Some(5000)));
        assert!(machine.store.signature_dir(SIGNED).expect("dir").join("manifest.json").exists());
        assert_eq!(athanor_trust_state::read_owned_by(&machine.store.run.join("state.json"), std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&machine.root).expect("meta"))), Ok(state));
    }

    #[test]
    fn a_queued_rollback_holds_the_booted_digest_and_downloads_nothing() {
        for offline in [true, false] {
            let machine = Machine::new(&format!("rollback-queued-{offline}"), &["real/k1.pub"]);
            let tools = Fake::booted(deployed(&digest(2), 2000)).offering(&digest(3), 3000);
            tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
            tools.status.borrow_mut().rollback_queued = true;
            let state = run(&machine.ctx(&tools, 5000), offline).expect("state");
            assert_eq!(machine.store.held().expect("read"), Some(digest(2)));
            assert_eq!(state.update, UpdateState::None);
            assert!(!tools.called("candidate") && !tools.called("download"));
        }
    }

    #[test]
    fn the_held_digest_is_never_downloaded_but_a_newer_one_is() {
        let machine = Machine::new("held", &["real/k1.pub"]);
        machine.store.set_held(&digest(2)).expect("held");
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        for _ in 0..3 {
            assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::Held);
        }
        assert!(!tools.called("download"));
        let newer = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(3), 3000);
        assert_eq!(run(&machine.ctx(&newer, 5000), false).expect("check").update, UpdateState::Downloaded);
    }

    #[test]
    fn an_unreadable_held_file_offers_and_downloads_nothing() {
        let machine = Machine::new("held-unreadable", &["real/k1.pub"]);
        // A directory in place of the file fails with EISDIR, whoever runs the test.
        std::fs::create_dir(machine.store.var.join("held")).expect("mkdir");
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("state");
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Storage));
        assert!(!tools.called("download"));
        // The staged download is not announced as downloaded either.
        tools.status.borrow_mut().staged = Some(Deployed { download_only: true, ..deployed(&digest(2), 2000) });
        let state = run(&machine.ctx(&tools, 5000), true).expect("state");
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Storage));
    }

    #[test]
    fn a_garbled_newest_booted_is_published_as_unknown_and_left_alone() {
        let machine = Machine::new("newest-garbled", &["real/k1.pub"]);
        std::fs::write(machine.store.var.join("newest-booted"), "soon\n").expect("write");
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("a state is published");
        assert_eq!((state.update, state.last_error, state.newest_booted_build_time), (UpdateState::None, ErrorCode::Storage, i64::MAX));
        assert!(!tools.called("download"));
        assert_eq!(std::fs::read_to_string(machine.store.var.join("newest-booted")).expect("read"), "soon\n", "the file is not rewritten");
        assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("state").last_error, ErrorCode::Storage, "it stays so until the file is removed");
        std::fs::remove_file(machine.store.var.join("newest-booted")).expect("the administrator removes it");
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.newest_booted_build_time), (UpdateState::Downloaded, 1000));
    }

    #[test]
    fn a_queued_rollback_holds_the_booted_digest_even_with_a_garbled_newest_booted() {
        let machine = Machine::new("rollback-newest-garbled", &["real/k1.pub"]);
        std::fs::write(machine.store.var.join("newest-booted"), "soon\n").expect("write");
        let tools = Fake::booted(deployed(&digest(2), 2000)).offering(&digest(3), 3000);
        tools.status.borrow_mut().rollback = Some(deployed(&digest(1), 1000));
        tools.status.borrow_mut().rollback_queued = true;
        let state = run(&machine.ctx(&tools, 5000), false).expect("a state is published");
        assert_eq!(machine.store.held().expect("read"), Some(digest(2)));
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Storage));
        assert!(!tools.called("download"));
    }

    #[test]
    fn an_unreadable_last_success_is_published_as_storage_and_healed_by_the_check() {
        let machine = Machine::new("last-success-garbled", &["real/k1.pub"]);
        std::fs::write(machine.store.var.join("last-success"), "soon\n").expect("write");
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), true).expect("a state is published");
        assert_eq!((state.last_error, state.last_successful_check), (ErrorCode::Storage, None));
        let state = run(&machine.ctx(&tools, 6000), false).expect("check");
        assert_eq!(state.last_successful_check, Some(6000));
    }

    #[test]
    fn an_unreadable_refused_is_cleared_reported_and_does_not_stop_the_download() {
        let machine = Machine::new("refused-garbled", &["real/k1.pub"]);
        std::fs::write(machine.store.var.join("refused"), [0xff, 0xfe]).expect("write");
        let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert!(tools.called("download"));
        assert_eq!((state.update, state.last_error), (UpdateState::Downloaded, ErrorCode::Storage));
        assert_eq!(machine.store.refused().expect("cleared"), None);
    }

    #[test]
    fn the_opt_out_and_the_lineage_are_published_as_stored() {
        let machine = Machine::new("lineage-published", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(1), 1000));
        let state = run(&machine.ctx(&tools, 5000), true).expect("check");
        assert_eq!((state.schema, state.automatic_security_updates, state.postpone), (2, true, None), "a fresh machine: on, no lineage");
        let mut lineage = crate::lineage::Lineage::default();
        lineage.applied(&digest(1), 1000);
        lineage.notice(4000, "boot-0");
        machine.store.set_lineage(&lineage).expect("write");
        machine.store.set_automatic_security_updates(false).expect("write");
        let state = run(&machine.ctx(&tools, 5000), true).expect("check");
        assert!(!state.automatic_security_updates);
        assert_eq!(state.postpone, lineage.published());
        assert_eq!(state.postpone.as_ref().map(|postpone| (postpone.anchor.clone(), postpone.first_notice_at)), Some((Some(digest(1)), 4000)));
        assert_eq!(state.last_error, ErrorCode::None);
    }

    #[test]
    fn an_unreadable_opt_out_or_lineage_is_published_as_storage_and_left_alone() {
        for (name, text) in [("automatic-security-updates", "maybe\n"), ("lineage.json", "")] {
            let machine = Machine::new(&format!("unreadable-{name}"), &["real/k1.pub"]);
            std::fs::write(machine.store.var.join(name), text).expect("write");
            let tools = Fake::booted(deployed(&digest(1), 1000));
            let state = run(&machine.ctx(&tools, 5000), true).expect("a state is published");
            assert_eq!(state.last_error, ErrorCode::Storage, "{name}");
            assert_eq!(std::fs::read_to_string(machine.store.var.join(name)).expect("read"), text, "{name} is not reset");
            if name == "lineage.json" {
                assert_eq!((state.automatic_security_updates, state.postpone), (true, None));
            } else {
                assert!(!state.automatic_security_updates, "published as what the machine then does: nothing unlocks");
            }
        }
    }

    #[test]
    fn a_tag_moved_to_an_older_build_or_a_run_tag_on_the_same_build_is_published_and_not_downloaded() {
        let machine = Machine::new("older", &["real/k1.pub"]);
        let pinned = Deployed { image: format!("{REPO}:37691917204"), ..deployed(&digest(1), 1000) };
        let by_digest = Deployed { image: format!("{REPO}@{}", digest(1)), ..deployed(&digest(1), 1000) };
        for (booted, build_time) in [(deployed(&digest(1), 1000), 500), (pinned.clone(), 500), (pinned, 1000), (by_digest, 1000)] {
            let tools = Fake::booted(booted).offering(&digest(2), build_time);
            assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::OlderThanBooted);
            assert!(!tools.called("download"));
        }
    }

    #[test]
    fn a_fresh_install_moves_to_the_signed_digest_of_its_own_build_at_the_next_shutdown() {
        // UD2: the kickstart writes the signed `:stable` reference, and the machine boots the
        // digest bootc-image-builder gave the build; the channel's digest of it is signed.
        let machine = Machine::new("signed-copy", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000), true).expect("offline").verified.reason, Reason::NoSignature);
        assert_eq!(crate::migrate::run(&machine.ctx(&tools, 5000)), Ok(crate::migrate::Outcome::Done));
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.reason, state.update, state.last_error), (Reason::Media, UpdateState::WillApplyAtNextShutdown, ErrorCode::None));
        assert!(tools.called("download") && tools.called("apply_downloaded"));
        assert!(!tools.called("relock") && !tools.called("switch"), "{:?}", tools.calls.borrow());
        assert!(machine.store.signature_dir(SIGNED).expect("dir").join("manifest.json").exists());
        // The next check leaves the staged copy as it is.
        assert_eq!(run(&machine.ctx(&tools, 6000), false).expect("check").update, UpdateState::WillApplyAtNextShutdown);
        assert_eq!(tools.calls.borrow().iter().filter(|call| *call == "download").count(), 1);
        // The shutdown finalizes it, and the restart boots the signed digest.
        let restarted = Fake::booted(tools.status.borrow().staged.clone().expect("staged"));
        let state = run(&machine.ctx(&restarted, 7000), true).expect("offline");
        assert_eq!((state.verified.value, state.verified.reason, state.update), (true, Reason::Signature, UpdateState::None));
    }

    fn downloads(tools: &Fake) -> usize {
        tools.calls.borrow().iter().filter(|call| *call == "download").count()
    }

    #[test]
    fn a_verified_boot_never_takes_another_digest_of_its_build() {
        let machine = Machine::new("signed-copy-verified", &["real/k1.pub"]);
        machine.store_signature(SIGNED, "real");
        let tools = Fake::booted(deployed(SIGNED, 1000)).offering(&digest(2), 1000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.reason, state.update), (Reason::Signature, UpdateState::OlderThanBooted));
        assert!(!tools.called("download") && !tools.called("apply_downloaded"));
    }

    #[test]
    fn an_unlabelled_build_or_another_version_is_not_the_same_build() {
        let machine = Machine::new("signed-copy-unknown", &["real/k1.pub"]);
        let unlabelled = Fake::booted(deployed(&digest(9), 0)).offering(SIGNED, 0);
        let mut other_version = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        if let Ok(Some(candidate)) = other_version.candidate.as_mut() {
            candidate.version = "43.20261014.1".into();
        }
        for (case, tools) in [("unlabelled", unlabelled), ("other version", other_version)] {
            assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::OlderThanBooted, "{case}");
            assert!(!tools.called("download"), "{case}");
        }
    }

    #[test]
    fn only_the_copy_the_registry_named_is_unlocked() {
        let machine = Machine::new("signed-copy-staged", &["real/k1.pub"]);
        let locked = |deployed: Deployed| Deployed { download_only: true, ..deployed };
        let cases = [
            ("not enforcing", Deployed { enforcing: false, ..locked(deployed(SIGNED, 1000)) }),
            ("another image", Deployed { image: format!("{REPO}:latest"), ..locked(deployed(SIGNED, 1000)) }),
            ("another digest", locked(deployed(&digest(5), 1000))),
        ];
        for (case, staged) in cases {
            let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
            tools.download = Err(Failure { code: ErrorCode::Network, host: None });
            tools.status.borrow_mut().staged = Some(staged);
            let state = run(&machine.ctx(&tools, 5000), false).expect("check");
            assert_eq!((state.update, state.last_error), (UpdateState::Available, ErrorCode::Network), "{case}");
            assert!(!tools.called("apply_downloaded"), "{case}");
        }
        // A machine that does not enforce the policy, with a copy an administrator staged.
        let tools = Fake::booted(Deployed { enforcing: false, ..deployed(&digest(9), 1000) }).offering(SIGNED, 1000);
        tools.status.borrow_mut().staged = Some(Deployed { enforcing: false, ..locked(deployed(SIGNED, 1000)) });
        assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::None);
        assert!(tools.calls.borrow().is_empty(), "{:?}", tools.calls.borrow());
    }

    #[test]
    fn a_staged_copy_is_not_unlocked_while_the_registry_does_not_answer() {
        let machine = Machine::new("signed-copy-offline-registry", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(9), 1000));
        tools.status.borrow_mut().staged = Some(Deployed { download_only: true, ..deployed(SIGNED, 1000) });
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Network));
        assert!(!tools.called("apply_downloaded"));
    }

    #[test]
    fn a_failed_unlock_is_no_update_and_the_next_check_tries_again() {
        let machine = Machine::new("signed-copy-unlock", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.unlock = Err(Failure { code: ErrorCode::Internal, host: None });
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Internal));
        assert_eq!(run(&machine.ctx(&tools, 5000), true).expect("offline").update, UpdateState::None, "never announced as downloaded");
        tools.unlock = Ok(());
        let state = run(&machine.ctx(&tools, 6000), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::WillApplyAtNextShutdown, ErrorCode::None));
        assert_eq!(downloads(&tools), 1, "the staged copy is unlocked, not downloaded again");
    }

    #[test]
    fn a_status_that_fails_after_the_unlock_is_no_update() {
        let machine = Machine::new("signed-copy-status", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.broken_after_unlock = true;
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::Internal));
    }

    #[test]
    fn a_copy_unlocked_before_a_failed_status_is_counted_when_it_does_not_boot() {
        let machine = Machine::new("signed-copy-status-no-boot", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.download = Ok(Deployed { download_only: true, deployment: Some("copy.0".into()), ..deployed(SIGNED, 1000) });
        tools.broken_after_unlock = true;
        for now in [5000, 6000] {
            assert_eq!(run(&machine.ctx(&tools, now), false).expect("check").update, UpdateState::None);
            assert!(tools.status.borrow().staged.as_ref().is_some_and(|staged| !staged.download_only), "unlocked");
            // bootc answers again; the copy is deployed, does not boot, and the machine is back.
            tools.status_fails.set(false);
            let mut status = tools.status.borrow_mut();
            status.rollback = status.staged.take();
        }
        assert_eq!(run(&machine.ctx(&tools, 7000), false).expect("check").update, UpdateState::Held);
        assert_eq!(downloads(&tools), 2);
    }

    #[test]
    fn a_held_copy_or_a_metered_network_downloads_nothing() {
        let machine = Machine::new("signed-copy-held", &["real/k1.pub"]);
        machine.store.set_held(SIGNED).expect("GoBack()");
        let tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::Held);
        assert!(!tools.called("download"));

        let machine = Machine::new("signed-copy-metered", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.metered = true;
        assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::Available);
        assert!(!tools.called("download"));
    }

    #[test]
    fn a_signed_copy_that_does_not_boot_is_staged_once_more_and_then_held() {
        let machine = Machine::new("signed-copy-no-boot", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.download = Ok(Deployed { download_only: true, deployment: Some("copy.0".into()), ..deployed(SIGNED, 1000) });
        for now in [5000, 6000] {
            assert_eq!(run(&machine.ctx(&tools, now), false).expect("check").update, UpdateState::WillApplyAtNextShutdown);
            // The copy is deployed, does not boot, and the machine is back on the install.
            let mut status = tools.status.borrow_mut();
            status.rollback = status.staged.take();
        }
        for now in [7000, 8000] {
            assert_eq!(run(&machine.ctx(&tools, now), false).expect("check").update, UpdateState::Held);
        }
        assert_eq!(downloads(&tools), 2);
    }

    #[test]
    fn a_signed_copy_that_fails_the_policy_is_refused_and_nothing_is_applied() {
        let machine = Machine::new("signed-copy-refused", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        tools.download = Err(Failure { code: ErrorCode::Policy, host: None });
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.reason, state.update, state.last_error), (Reason::NoSignature, UpdateState::Refused, ErrorCode::Policy));
        assert!(state.downloaded.is_none() && !tools.called("apply_downloaded"));
        assert_eq!(state.booted.digest, digest(9));
    }

    #[test]
    fn a_refusal_is_a_code_survives_a_reboot_and_clears_when_a_download_passes() {
        let machine = Machine::new("refused", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        tools.download = Err(Failure { code: ErrorCode::Policy, host: None });
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.last_error, state.last_error_host.clone()), (UpdateState::Refused, ErrorCode::Policy, None));
        let text = std::fs::read_to_string(machine.store.run.join("state.json")).expect("state");
        assert!(!text.contains("signature was required") && !text.contains("http"), "no registry text in the file");

        let after_reboot = Fake::booted(deployed(&digest(1), 1000));
        assert_eq!(run(&machine.ctx(&after_reboot, 5000), true).expect("offline").update, UpdateState::Refused);

        let passing = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(3), 3000);
        assert_eq!(run(&machine.ctx(&passing, 6000), false).expect("check").update, UpdateState::Downloaded);
        assert_eq!(machine.store.refused().expect("read"), None);
    }

    #[test]
    fn a_metered_connection_checks_and_does_not_download() {
        let machine = Machine::new("metered", &["real/k1.pub"]);
        let mut tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), 2000);
        tools.metered = true;
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.update, state.last_successful_check), (UpdateState::Available, Some(5000)));
        assert!(tools.called("candidate") && !tools.called("download"));
    }

    #[test]
    fn a_registry_that_does_not_answer_is_a_code_a_host_and_no_successful_check() {
        let machine = Machine::new("network", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(1), 1000));
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.last_error, state.last_error_host.as_deref(), state.last_successful_check), (ErrorCode::Network, Some("localhost:5000"), None));
    }

    #[test]
    fn nothing_is_downloaded_through_a_reference_that_does_not_enforce_the_policy() {
        let machine = Machine::new("media", &["real/k1.pub"]);
        let tools = Fake::booted(Deployed { enforcing: false, ..deployed(&digest(1), 1000) }).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.reason, state.update), (Reason::Media, UpdateState::None));
        assert!(tools.calls.borrow().is_empty(), "no registry call at all: {:?}", tools.calls.borrow());
    }

    #[test]
    fn a_migrated_machine_on_a_reference_that_does_not_enforce_the_policy_says_so() {
        let machine = Machine::new("origin-not-enforcing", &["real/k1.pub"]);
        machine.store.set_migrated().expect("stamp");
        let unverified = || Deployed { enforcing: false, ..deployed(&digest(1), 1000) };
        let tools = Fake::booted(unverified()).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.value, state.verified.reason, state.update), (false, Reason::OriginNotEnforcing, UpdateState::None));
        assert!(tools.calls.borrow().is_empty(), "reported, not repaired: {:?}", tools.calls.borrow());

        // A staged deployment that does not enforce either changes nothing.
        tools.status.borrow_mut().staged = Some(Deployed { download_only: true, ..unverified() });
        assert_eq!(run(&machine.ctx(&tools, 5000), true).expect("offline").verified.reason, Reason::OriginNotEnforcing);
        // One that does is the migration waiting for the restart.
        tools.status.borrow_mut().staged = Some(deployed(&digest(2), 2000));
        assert_eq!(run(&machine.ctx(&tools, 5000), true).expect("offline").verified.reason, Reason::Media);
    }

    #[test]
    fn verified_is_derived_from_the_stored_object_and_the_keys_of_the_policy() {
        let booted = || Fake::booted(deployed(SIGNED, 1000));
        let with_key = Machine::new("reason-signature", &["real/k1.pub"]);
        with_key.store_signature(SIGNED, "real");
        let state = run(&with_key.ctx(&booted(), 5000), true).expect("offline");
        assert!(state.verified.value);
        assert_eq!(state.verified.reason, Reason::Signature);

        // The same object once its key has left the policy: nothing stored says "verified".
        let rotated = Machine::new("reason-rotated", &["made/a.pub"]);
        rotated.store_signature(SIGNED, "real");
        assert_eq!(run(&rotated.ctx(&booted(), 5000), true).expect("offline").verified.reason, Reason::KeyNotInPolicy);

        let none = Machine::new("reason-none", &["real/k1.pub"]);
        assert_eq!(run(&none.ctx(&booted(), 5000), true).expect("offline").verified.reason, Reason::NoSignature);

        // A valid object of another digest, as a copy from another of our repositories is.
        let other = Machine::new("reason-other", &["made/a.pub"]);
        other.store_signature(SIGNED, "made/other-repo");
        assert_eq!(run(&other.ctx(&booted(), 5000), true).expect("offline").verified.reason, Reason::NoSignature);
    }

    #[test]
    fn a_missing_signature_object_of_the_booted_digest_is_fetched_by_the_next_check() {
        let machine = Machine::new("heal", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(SIGNED, 1000)).offering(SIGNED, 1000);
        assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").verified.reason, Reason::Signature);
    }

    #[test]
    fn a_shadowed_policy_and_a_foreign_repository_have_their_own_reasons() {
        let machine = Machine::new("shadow", &["real/k1.pub"]);
        machine.store_signature(SIGNED, "real");
        let foreign = Fake::booted(Deployed { image: "registry.example/someone/else:latest".into(), ..deployed(SIGNED, 1000) });
        assert_eq!(run(&machine.ctx(&foreign, 5000), true).expect("offline").verified.reason, Reason::ReferenceOutOfScope);

        std::fs::remove_file(&machine.policy.etc_policy).expect("unlink");
        std::fs::write(&machine.policy.etc_policy, r#"{"default":[{"type":"insecureAcceptAnything"}]}"#).expect("write");
        let tools = Fake::booted(deployed(SIGNED, 1000)).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.reason, state.policy.shipped), (Reason::PolicyNotInForce, false));
        assert!(!tools.called("download"), "a permissive policy downloads nothing");
    }

    #[test]
    fn a_deployment_with_local_changes_is_published_unverified() {
        let machine = Machine::new("local-changes", &["real/k1.pub"]);
        machine.store_signature(SIGNED, "real");
        // Migrated first, layered afterwards: the origin still enforces the policy.
        let tools = Fake::booted(Deployed { local_changes: true, ..deployed(SIGNED, 1000) }).offering(&digest(2), 2000);
        let state = run(&machine.ctx(&tools, 5000), false).expect("check");
        assert_eq!((state.verified.value, state.verified.reason), (false, Reason::LocalChanges));
        assert_eq!((state.update, state.last_error), (UpdateState::None, ErrorCode::None));
        assert_eq!(state.booted.digest, SIGNED);
        assert!(tools.calls.borrow().is_empty(), "bootc refuses to upgrade it, so nothing is asked: {:?}", tools.calls.borrow());
    }

    #[test]
    fn the_newest_booted_build_time_is_published_after_going_back() {
        let machine = Machine::new("went-back", &["real/k1.pub"]);
        run(&machine.ctx(&Fake::booted(deployed(&digest(2), 2000)), 5000), true).expect("offline");
        let state = run(&machine.ctx(&Fake::booted(deployed(&digest(1), 1000)), 6000), true).expect("offline");
        assert_eq!((state.booted.build_time, state.newest_booted_build_time), (1000, 2000));
    }

    const NOW: i64 = 1_790_000_000;

    /// The digest the release attestations under tests/vectors/made cover.
    fn attested() -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/made/image-digest")).expect("vector").trim().to_owned()
    }

    fn in_att_repo(deployed: Deployed) -> Deployed {
        Deployed { image: format!("{ATT_REPO}:stable"), ..deployed }
    }

    fn security() -> Class {
        Class::Security { advisories: vec!["CVE-2026-0001".into()] }
    }

    /// A machine on `ATT_REPO` at digest(1), offered `attested()` with an attestation of
    /// `class`, or none. The signature object of the candidate is already stored, so the only
    /// fetch that can fail is the attestation's.
    fn offered_attested(test: &str, class: Option<Class>) -> (Machine, Fake) {
        let machine = Machine::new(test, &["real/k1.pub"]);
        machine.store_signature(&attested(), "real");
        let mut tools = Fake::booted(in_att_repo(deployed(&digest(1), 1000))).offering(&attested(), 2000);
        tools.download = tools.download.map(in_att_repo);
        if let Some(class) = class {
            tools = tools.attestation(&attested(), class);
        }
        (machine, tools)
    }

    /// The candidate is staged and locked already, as `Postpone()` or an earlier check left it.
    fn stage(tools: &Fake) {
        tools.status.borrow_mut().staged = Some(tools.download.clone().expect("a deployment"));
    }

    fn open_lineage(first_notice_at: i64, postponed: bool) -> Lineage {
        Lineage { anchor: None, anchor_build_time: None, open: Some(Open { first_notice_at, boots_remaining: 7, postponed, last_boot_id: "boot-0".into() }) }
    }

    /// The release attestation of `attested()` is stored, as the check that offered it left it.
    fn store_attestation(machine: &Machine, tools: &Fake) {
        tools.fetch_attestation(ATT_REPO, &attested(), &machine.store.attestation_dir(&attested()).expect("dir")).expect("stored");
    }

    fn position(tools: &Fake, what: &str) -> Option<usize> {
        tools.calls.borrow().iter().position(|call| call.starts_with(what))
    }

    #[test]
    fn a_security_digest_downloads_and_unlocks() {
        let (machine, tools) = offered_attested("security", Some(security()));
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::WillApplyAtNextShutdown, ErrorCode::None));
        assert!(position(&tools, "download").is_some() && position(&tools, "download") < position(&tools, "apply_downloaded"), "{:?}", tools.calls.borrow());
        assert!(!tools.called("rollback") && !tools.called("switch") && !tools.called("relock"), "nothing restarts and nothing re-locks: {:?}", tools.calls.borrow());
        let postpone = state.postpone.clone().expect("the first unlock opens the lineage");
        assert_eq!((postpone.first_notice_at, postpone.boots_remaining, postpone.postponed, postpone.anchor), (NOW, 7, false, None));
        assert_eq!(machine.store.lineage().expect("stored").published(), state.postpone);
    }

    #[test]
    fn a_feature_digest_stays_downloaded() {
        let (machine, tools) = offered_attested("feature", Some(Class::Feature));
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error, state.postpone), (UpdateState::Downloaded, ErrorCode::None, None));
        assert!(tools.called("fetch_attestation") && tools.called("download") && !tools.called("apply_downloaded"));
    }

    #[test]
    fn a_digest_without_an_attestation_is_feature() {
        let (machine, tools) = offered_attested("unattested", None);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error, state.postpone), (UpdateState::Downloaded, ErrorCode::None, None));
        assert!(tools.called(&format!("fetch_attestation {ATT_REPO} {}", attested())) && !tools.called("apply_downloaded"));
    }

    #[test]
    fn a_staged_digest_attested_later_is_settled_as_security() {
        let (machine, tools) = offered_attested("attested-later", None);
        assert_eq!(run(&machine.ctx(&tools, NOW), false).expect("check").update, UpdateState::Downloaded);
        tools.attestations.borrow_mut().insert(attested(), security());
        let state = run(&machine.ctx(&tools, NOW + 3600), false).expect("check");
        assert_eq!(state.update, UpdateState::WillApplyAtNextShutdown);
        assert_eq!(downloads(&tools), 1, "the staged digest is not downloaded again");
        assert!(tools.called("apply_downloaded"));
        assert_eq!(state.postpone.map(|postpone| postpone.first_notice_at), Some(NOW + 3600));
    }

    #[test]
    fn a_security_digest_downloads_on_a_metered_connection() {
        let (machine, mut tools) = offered_attested("security-metered", Some(security()));
        tools.metered = true;
        assert_eq!(run(&machine.ctx(&tools, NOW), false).expect("check").update, UpdateState::WillApplyAtNextShutdown);
        assert!(tools.called("download") && tools.called("apply_downloaded"));
    }

    #[test]
    fn a_feature_digest_waits_on_a_metered_connection() {
        for class in [Some(Class::Feature), None] {
            let (machine, mut tools) = offered_attested(&format!("feature-metered-{}", class.is_some()), class);
            tools.metered = true;
            assert_eq!(run(&machine.ctx(&tools, NOW), false).expect("check").update, UpdateState::Available);
            assert!(!tools.called("download"));
        }
    }

    #[test]
    fn with_the_opt_out_on_nothing_downloads_on_metered_and_nothing_unlocks() {
        for metered in [true, false] {
            let (machine, mut tools) = offered_attested(&format!("opt-out-{metered}"), Some(security()));
            machine.store.set_automatic_security_updates(false).expect("write");
            tools.metered = metered;
            let state = run(&machine.ctx(&tools, NOW), false).expect("check");
            let expected = if metered { UpdateState::Available } else { UpdateState::Downloaded };
            assert_eq!((state.update, state.automatic_security_updates, state.postpone, state.last_error), (expected, false, None, ErrorCode::None));
            assert_eq!(tools.called("download"), !metered);
            assert!(!tools.called("apply_downloaded"));
        }
    }

    #[test]
    fn a_postponed_lineage_keeps_a_newer_security_digest_locked() {
        let (machine, tools) = offered_attested("postponed", Some(security()));
        let lineage = open_lineage(NOW - 86_400, true);
        machine.store.set_lineage(&lineage).expect("write");
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!(state.update, UpdateState::Downloaded);
        assert!(tools.called("download") && !tools.called("apply_downloaded"));
        assert_eq!(state.postpone, lineage.published());
        assert_eq!(machine.store.lineage().expect("stored"), lineage, "first_notice_at does not move");
    }

    #[test]
    fn an_expired_postpone_unlocks_at_the_next_check() {
        let (machine, tools) = offered_attested("expired", Some(security()));
        let lineage = open_lineage(NOW - 7 * 86_400, true);
        machine.store.set_lineage(&lineage).expect("write");
        stage(&tools);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!(state.update, UpdateState::WillApplyAtNextShutdown);
        assert!(!tools.called("download") && tools.called("apply_downloaded"));
        assert_eq!(state.postpone, lineage.published(), "the lineage stays open until the digest boots");
    }

    #[test]
    fn an_expired_postpone_unlocks_at_boot_without_the_network() {
        let (machine, tools) = offered_attested("expired-offline", Some(security()));
        tools.fetch_attestation(ATT_REPO, &attested(), &machine.store.attestation_dir(&attested()).expect("dir")).expect("an earlier check stored it");
        stage(&tools);
        machine.store.set_lineage(&open_lineage(NOW - 86_400, true)).expect("write");
        assert_eq!(run(&machine.ctx(&tools, NOW), true).expect("offline").update, UpdateState::Downloaded);
        machine.store.set_lineage(&open_lineage(NOW - 7 * 86_400, true)).expect("write");
        assert_eq!(run(&machine.ctx(&tools, NOW), true).expect("offline").update, UpdateState::WillApplyAtNextShutdown);
        assert!(!tools.called("candidate") && tools.called("apply_downloaded"));
    }

    #[test]
    fn a_held_security_digest_is_not_downloaded_three_times() {
        let (machine, tools) = offered_attested("held-security", Some(security()));
        machine.store.set_held(&attested()).expect("held");
        for hour in 0..3 {
            assert_eq!(run(&machine.ctx(&tools, NOW + hour * 3600), false).expect("check").update, UpdateState::Held);
        }
        assert!(!tools.called("download") && !tools.called("apply_downloaded"));
    }

    /// The machine boots `attested()`, whose attestation the check that downloaded it stored.
    fn booted_attested(machine: &Machine, rollback_queued: bool) -> Fake {
        let tools = Fake::booted(in_att_repo(deployed(&attested(), 2000))).attestation(&attested(), security());
        tools.fetch_attestation(ATT_REPO, &attested(), &machine.store.attestation_dir(&attested()).expect("dir")).expect("stored");
        tools.status.borrow_mut().rollback = Some(in_att_repo(deployed(&digest(1), 1000)));
        tools.status.borrow_mut().rollback_queued = rollback_queued;
        tools
    }

    #[test]
    fn a_greenboot_return_does_not_count_as_applied() {
        let machine = Machine::new("greenboot-return", &["real/k1.pub"]);
        let lineage = Lineage { anchor: Some(digest(1)), anchor_build_time: Some(1000), ..open_lineage(NOW - 86_400, false) };
        machine.store.set_lineage(&lineage).expect("write");
        // The state service's check at boot runs before greenboot's verdict: it never counts.
        let state = run(&machine.ctx(&booted_attested(&machine, false), NOW), true).expect("offline");
        assert_eq!(machine.store.lineage().expect("stored"), lineage, "not counted before the health checks");
        // Greenboot marked the boot bad: the check after the health checks sees the queued rollback.
        let state_after = run(&machine.ctx(&booted_attested(&machine, true), NOW), false).expect("check");
        assert_eq!((state.postpone.clone(), state_after.postpone), (lineage.published(), lineage.published()));
        assert_eq!(machine.store.lineage().expect("stored"), lineage, "the lineage stays open and the anchor does not move");
    }

    #[test]
    fn booting_a_security_digest_resets_the_lineage() {
        let machine = Machine::new("security-booted", &["real/k1.pub"]);
        machine.store.set_lineage(&Lineage { anchor: Some(digest(1)), anchor_build_time: Some(1000), ..open_lineage(NOW - 86_400, true) }).expect("write");
        let tools = booted_attested(&machine, false);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!(state.postpone, None);
        assert_eq!(machine.store.lineage().expect("stored"), Lineage { anchor: Some(attested()), anchor_build_time: Some(2000), open: None });
    }

    #[test]
    fn a_boot_that_failed_its_health_checks_is_not_applied() {
        // Greenboot's health check failed, and no rollback is queued: someone intervenes
        // by hand. The check still runs and asks the registry; the boot is not counted.
        let machine = Machine::new("health-failed", &["real/k1.pub"]);
        let lineage = Lineage { anchor: Some(digest(1)), anchor_build_time: Some(1000), ..open_lineage(NOW - 86_400, false) };
        machine.store.set_lineage(&lineage).expect("write");
        let mut tools = booted_attested(&machine, false);
        tools.boot_healthy = false;
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!(state.postpone, lineage.published());
        assert_eq!(machine.store.lineage().expect("stored"), lineage);
        assert!(tools.called("candidate"), "the check still runs: {:?}", tools.calls.borrow());
    }

    #[test]
    fn going_back_onto_an_earlier_security_digest_keeps_the_lineage() {
        // The anchor is the security digest applied last, built at 3000. GoBack() returns to the
        // security digest applied before it, then to one older still: neither is applied again.
        for (name, build_time) in [("previous", 2000), ("older", 1000)] {
            let machine = Machine::new(&format!("go-back-{name}"), &["real/k1.pub"]);
            let lineage = Lineage { anchor: Some(digest(3)), anchor_build_time: Some(3000), ..open_lineage(NOW - 86_400, true) };
            machine.store.set_lineage(&lineage).expect("write");
            let tools = booted_attested(&machine, false);
            tools.status.borrow_mut().booted.build_time = build_time;
            let state = run(&machine.ctx(&tools, NOW), false).expect("check");
            assert_eq!(state.postpone, lineage.published(), "{name}");
            assert_eq!(machine.store.lineage().expect("stored"), lineage, "{name}: the budget is not reset");
        }
    }

    #[test]
    fn a_security_digest_counts_by_the_anchor_s_build_time_alone() {
        // A feature digest built at 3000 booted since the anchor (1000). A security digest built
        // between them is still newer than the last one applied, and counts.
        let machine = Machine::new("between", &["real/k1.pub"]);
        machine.store.set_lineage(&Lineage { anchor: Some(digest(1)), anchor_build_time: Some(1000), ..open_lineage(NOW - 86_400, false) }).expect("write");
        machine.store.record_booted(3000).expect("a newer feature boot");
        let state = run(&machine.ctx(&booted_attested(&machine, false), NOW), false).expect("check");
        assert_eq!((state.newest_booted_build_time, state.postpone), (3000, None));
        assert_eq!(machine.store.lineage().expect("stored"), Lineage { anchor: Some(attested()), anchor_build_time: Some(2000), open: None });
        // A digest without a build time label says nothing about its age: it never counts.
        let machine = Machine::new("no-label", &["real/k1.pub"]);
        let tools = booted_attested(&machine, false);
        tools.status.borrow_mut().booted.build_time = 0;
        run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!(machine.store.lineage().expect("stored"), Lineage::default());
    }

    #[test]
    fn a_check_keeps_only_the_attestations_of_the_booted_staged_and_candidate_digests() {
        let (machine, tools) = offered_attested("prune", Some(Class::Feature));
        let (staged, stale) = (digest(4), digest(5));
        for digest in [digest(1), staged.clone(), stale] {
            std::fs::create_dir_all(machine.store.attestation_dir(&digest).expect("dir").join("leftover")).expect("mkdir");
        }
        tools.status.borrow_mut().staged = Some(in_att_repo(Deployed { download_only: true, ..deployed(&staged, 1500) }));
        run(&machine.ctx(&tools, NOW), false).expect("check");
        let mut kept: Vec<_> = std::fs::read_dir(machine.store.var.join("attestations")).expect("list").map(|entry| entry.expect("entry").file_name().into_string().expect("utf-8")).collect();
        kept.sort();
        let mut expected: Vec<_> = [digest(1), staged, attested()].iter().map(|digest| digest["sha256:".len()..].to_owned()).collect();
        expected.sort();
        assert_eq!(kept, expected);
    }

    #[test]
    fn a_download_that_left_a_failure_unlocks_nothing() {
        // No signature object is stored for the candidate, and the registry serves none.
        let machine = Machine::new("downloaded-fails", &["real/k1.pub"]);
        let mut tools = Fake::booted(in_att_repo(deployed(&digest(1), 1000))).offering(&attested(), 2000).attestation(&attested(), security());
        tools.download = tools.download.map(in_att_repo);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error, state.postpone), (UpdateState::Downloaded, ErrorCode::Registry, None));
        assert!(tools.called("download") && !tools.called("apply_downloaded"), "{:?}", tools.calls.borrow());
    }

    #[test]
    fn an_unlocked_security_digest_without_a_lineage_opens_one() {
        // The lineage was not recorded after the unlock, or Apply() unlocked the digest.
        for offline in [false, true] {
            let (machine, tools) = offered_attested(&format!("unlocked-no-lineage-{offline}"), Some(security()));
            store_attestation(&machine, &tools);
            stage(&tools);
            tools.status.borrow_mut().staged.as_mut().expect("staged").download_only = false;
            let state = run(&machine.ctx(&tools, NOW), offline).expect("check");
            assert_eq!(state.update, UpdateState::WillApplyAtNextShutdown);
            assert_eq!(state.postpone.as_ref().map(|postpone| (postpone.first_notice_at, postpone.boots_remaining, postpone.postponed)), Some((NOW, 7, false)), "{offline}");
            assert_eq!(machine.store.lineage().expect("stored").published(), state.postpone);
            assert!(!tools.called("download") && !tools.called("apply_downloaded"));
        }
    }

    #[test]
    fn a_failed_refetch_keeps_the_stored_attestation() {
        // The staged digest is postponed; a refetch that fails midway must not erase what verified.
        let (machine, tools) = offered_attested("refetch-breaks", Some(security()));
        store_attestation(&machine, &tools);
        stage(&tools);
        machine.store.set_lineage(&open_lineage(NOW - 86_400, true)).expect("write");
        tools.attestation_breaks.set(true);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::Downloaded, ErrorCode::None));
        assert!(tools.called("fetch_attestation"));
        let policy = crate::policy::in_force(&machine.policy);
        assert_eq!(stored_class(&machine.ctx(&tools, NOW), &policy, ATT_REPO, &attested()), security(), "the stored attestation survives");
        let state = run(&machine.ctx(&tools, NOW + 6 * 86_400), true).expect("offline");
        assert_eq!(state.update, UpdateState::WillApplyAtNextShutdown, "the expired postpone still unlocks at boot");
        assert!(tools.called("apply_downloaded"));
    }

    #[test]
    fn a_refetched_copy_that_does_not_verify_as_security_keeps_the_stored_one() {
        // The attachments policy accepts anything: the registry may serve a feature statement,
        // a copy signed by an unknown key, a bad signature or an empty manifest.
        for served in ["att-feature", "att-unknown-key", "att-bad-signature", ""] {
            let (machine, tools) = offered_attested(&format!("refetch-unverified-{served}"), Some(security()));
            store_attestation(&machine, &tools);
            stage(&tools);
            machine.store.set_lineage(&open_lineage(NOW - 86_400, true)).expect("write");
            tools.served.set(Some(served));
            let state = run(&machine.ctx(&tools, NOW), false).expect("check");
            assert_eq!((state.update, state.last_error), (UpdateState::Downloaded, ErrorCode::None), "{served}");
            assert!(tools.called("fetch_attestation"), "{served}");
            let policy = crate::policy::in_force(&machine.policy);
            assert_eq!(stored_class(&machine.ctx(&tools, NOW), &policy, ATT_REPO, &attested()), security(), "{served}: the stored attestation is kept");
            let dir = machine.store.attestation_dir(&attested()).expect("dir");
            assert!(!dir.with_extension("partial").exists(), "{served}: the copy is removed");
        }
    }

    #[test]
    fn a_refetched_security_copy_replaces_one_that_does_not_verify() {
        let (machine, tools) = offered_attested("refetch-replaces", Some(security()));
        let dir = machine.store.attestation_dir(&attested()).expect("dir");
        tools.served.set(Some("att-unknown-key"));
        store_attestation(&machine, &tools);
        std::fs::write(dir.join("leftover"), "from the earlier copy").expect("write");
        tools.served.set(None);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.last_error), (UpdateState::WillApplyAtNextShutdown, ErrorCode::None));
        let policy = crate::policy::in_force(&machine.policy);
        assert_eq!(stored_class(&machine.ctx(&tools, NOW), &policy, ATT_REPO, &attested()), security());
        let names = |dir: &std::path::Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir).expect("dir").map(|entry| entry.expect("entry").file_name()).collect();
            names.sort();
            names
        };
        let vectors = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/made/att-security");
        assert_eq!(names(&dir), names(&vectors), "only the new copy's files remain");
        assert!(!dir.with_extension("partial").exists());
    }

    #[test]
    fn a_waiting_security_digest_unlocks_at_boot_unless_postponed() {
        // The opt-out turned back on, or an earlier unlock that failed: nothing holds it.
        let (machine, tools) = offered_attested("waiting-offline", Some(security()));
        store_attestation(&machine, &tools);
        stage(&tools);
        let state = run(&machine.ctx(&tools, NOW), true).expect("offline");
        assert_eq!(state.update, UpdateState::WillApplyAtNextShutdown);
        assert!(!tools.called("candidate") && tools.called("apply_downloaded"));
        assert_eq!(state.postpone.map(|postpone| postpone.first_notice_at), Some(NOW));
    }

    #[test]
    fn a_signed_copy_opens_no_lineage() {
        let machine = Machine::new("signed-copy-lineage", &["real/k1.pub"]);
        let tools = Fake::booted(deployed(&digest(9), 1000)).offering(SIGNED, 1000);
        let state = run(&machine.ctx(&tools, NOW), false).expect("check");
        assert_eq!((state.update, state.postpone), (UpdateState::WillApplyAtNextShutdown, None));
        assert!(tools.called("apply_downloaded") && !tools.called("fetch_attestation"));
        assert_eq!(machine.store.lineage().expect("read"), Lineage::default());
    }

    #[test]
    fn check_offline_counts_one_boot_per_boot_id() {
        let machine = Machine::new("boots", &["real/k1.pub"]);
        machine.store.set_lineage(&open_lineage(NOW, false)).expect("write");
        let tools = Fake::booted(deployed(&digest(1), 1000));
        let mut boots = Vec::new();
        for boot_id in ["boot-0", "boot-1", "boot-1", "boot-2"] {
            let mut ctx = machine.ctx(&tools, NOW);
            ctx.boot_id = Some(boot_id.into());
            boots.push(run(&ctx, true).expect("offline").postpone.expect("open").boots_remaining);
        }
        assert_eq!(boots, [7, 6, 6, 5]);
        assert_eq!(machine.store.lineage().expect("stored").open.map(|open| open.boots_remaining), Some(5));
    }

    #[test]
    fn an_unreadable_opt_out_file_unlocks_nothing() {
        // Each case would unlock with readable files: a new security digest, a staged one whose
        // postpone expired (online), the same at boot (offline).
        for (name, text) in [("automatic-security-updates", "maybe\n"), ("lineage.json", "{")] {
            for (staged, offline) in [(false, false), (true, false), (true, true)] {
                let (machine, tools) = offered_attested(&format!("unreadable-{name}-{staged}-{offline}"), Some(security()));
                tools.fetch_attestation(ATT_REPO, &attested(), &machine.store.attestation_dir(&attested()).expect("dir")).expect("stored");
                if name != "lineage.json" {
                    machine.store.set_lineage(&open_lineage(NOW - 7 * 86_400, true)).expect("write");
                }
                std::fs::write(machine.store.var.join(name), text).expect("write");
                if staged {
                    stage(&tools);
                }
                let state = run(&machine.ctx(&tools, NOW), offline).expect("a state is published");
                assert_eq!(state.last_error, ErrorCode::Storage, "{name} {staged} {offline}");
                assert!(!tools.called("apply_downloaded") && !tools.called("download"), "{name} {staged} {offline}: {:?}", tools.calls.borrow());
                assert_eq!(std::fs::read_to_string(machine.store.var.join(name)).expect("read"), text, "{name} is left as it is");
            }
        }
    }

    /// `attested()` is staged and locked, of `class` by its stored attestation, under `lineage`;
    /// the calls of the setup are forgotten.
    fn staged_for_shutdown(test: &str, class: Class, lineage: &Lineage) -> (Machine, Fake) {
        let (machine, tools) = offered_attested(test, Some(class));
        store_attestation(&machine, &tools);
        stage(&tools);
        machine.store.set_lineage(lineage).expect("write");
        tools.calls.borrow_mut().clear();
        (machine, tools)
    }

    fn locked(tools: &Fake) -> bool {
        tools.status.borrow().staged.as_ref().is_some_and(|staged| staged.download_only)
    }

    #[test]
    fn shutdown_unlocks_an_expired_postpone() {
        let lineage = open_lineage(NOW, true);
        let (machine, tools) = staged_for_shutdown("shutdown-expired", security(), &lineage);
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        assert!(!locked(&tools));
        assert_eq!(machine.store.lineage().expect("stored"), lineage, "the lineage stays open until the digest boots");
    }

    #[test]
    fn shutdown_leaves_an_unexpired_postpone_locked() {
        let (machine, tools) = staged_for_shutdown("shutdown-unexpired", security(), &open_lineage(NOW, true));
        at_shutdown(&machine.ctx(&tools, NOW + 604_799)).expect("evaluated");
        assert!(tools.calls.borrow().is_empty() && locked(&tools));
    }

    #[test]
    fn shutdown_never_touches_a_feature_digest() {
        let (machine, tools) = staged_for_shutdown("shutdown-feature", Class::Feature, &open_lineage(NOW, true));
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(tools.calls.borrow().is_empty() && locked(&tools));
    }

    #[test]
    fn shutdown_unlocks_nothing_the_opt_out_or_a_return_holds() {
        let (machine, tools) = staged_for_shutdown("shutdown-opt-out", security(), &open_lineage(NOW, true));
        machine.store.set_automatic_security_updates(false).expect("write");
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(tools.calls.borrow().is_empty() && locked(&tools), "the opt-out holds it");
        let (machine, tools) = staged_for_shutdown("shutdown-rollback", security(), &open_lineage(NOW, true));
        tools.status.borrow_mut().rollback_queued = true;
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(tools.calls.borrow().is_empty() && locked(&tools), "a queued return to the previous deployment holds it");
    }

    #[test]
    fn shutdown_unlocks_nothing_the_check_would_not_offer() {
        let (machine, tools) = staged_for_shutdown("shutdown-held", security(), &open_lineage(NOW, true));
        machine.store.set_held(&attested()).expect("held");
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(!tools.called("apply_downloaded") && locked(&tools), "a held digest stays locked");
        let (machine, tools) = staged_for_shutdown("shutdown-older", security(), &open_lineage(NOW, true));
        tools.status.borrow_mut().booted.build_time = 2000;
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(!tools.called("apply_downloaded") && locked(&tools), "a digest not newer than the booted build stays locked");
    }

    #[test]
    fn shutdown_unlocks_once_the_boots_run_out() {
        let mut lineage = open_lineage(NOW, true);
        lineage.open.as_mut().expect("open").boots_remaining = 0;
        let (machine, tools) = staged_for_shutdown("shutdown-boots", security(), &lineage);
        at_shutdown(&machine.ctx(&tools, NOW + 1)).expect("evaluated");
        assert_eq!(*tools.calls.borrow(), ["apply_downloaded"]);
        assert!(!locked(&tools));
    }

    #[test]
    fn shutdown_with_an_unreadable_file_unlocks_nothing() {
        for (name, text) in [("automatic-security-updates", "maybe\n"), ("lineage.json", "{")] {
            let (machine, tools) = staged_for_shutdown(&format!("shutdown-unreadable-{name}"), security(), &open_lineage(NOW, true));
            std::fs::write(machine.store.var.join(name), text).expect("write");
            let failure = at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect_err("unreadable");
            assert_eq!(failure.code, ErrorCode::Storage, "{name}");
            assert!(!tools.called("apply_downloaded") && locked(&tools), "{name}");
            assert_eq!(std::fs::read_to_string(machine.store.var.join(name)).expect("read"), text, "{name} is left as it is");
        }
    }

    #[test]
    fn shutdown_unlocks_nothing_whose_attestation_no_longer_verifies() {
        let (machine, tools) = staged_for_shutdown("shutdown-rotated", security(), &open_lineage(NOW, true));
        // The key of the attestations was rotated since the check stored this one.
        let policy = machine.root.join("usr/policy.json");
        let text = std::fs::read_to_string(&policy).expect("read");
        assert_eq!(text.matches("made/a.pub").count(), 1);
        std::fs::write(&policy, text.replace("made/a.pub", "real/k1.pub")).expect("write");
        at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect("evaluated");
        assert!(!tools.called("apply_downloaded") && locked(&tools));
    }

    #[test]
    fn shutdown_with_the_store_busy_changes_nothing() {
        let _spawning = crate::goback::tests::SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (machine, tools) = staged_for_shutdown("shutdown-busy", security(), &open_lineage(NOW, true));
        let _held = machine.store.lock().expect("lock");
        let failure = at_shutdown(&machine.ctx(&tools, NOW + 604_800)).expect_err("busy");
        assert_eq!(failure.code, ErrorCode::Storage);
        assert!(tools.calls.borrow().is_empty() && locked(&tools));
    }
}
