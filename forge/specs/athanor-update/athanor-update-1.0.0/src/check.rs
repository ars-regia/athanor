//! `athanor-update check`: derive the verification of the booted digest again, ask the
//! registry, download, publish (docs/architecture/doc_update_trust.md, UT1, UT5, UT6, UT12).
use crate::policy::{InForce, PolicyPaths};
use crate::sigobj;
use crate::store::Store;
use crate::tools::{Deployed, Failure, Status, Tools};
use athanor_trust_state::{Deployment, ErrorCode, Reason, State, UpdateState, SCHEMA};
use std::path::PathBuf;

pub struct Context<'a, T: Tools> {
    pub tools: &'a T,
    pub store: &'a Store,
    pub policy: PolicyPaths,
    pub efivars: PathBuf,
    pub lockdown: PathBuf,
    /// Seconds since the epoch.
    pub now: i64,
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
        }
    }
}

fn deployment(deployed: &Deployed) -> Deployment {
    Deployment { image: deployed.image.clone(), digest: deployed.digest.clone(), version: deployed.version.clone(), build_time: deployed.build_time }
}

/// Why the booted image is, or is not, verified. Never a stored answer: the stored
/// signature object is verified again, against the keys the shipped policy names today.
fn reason<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, booted: &Deployed) -> Reason {
    if !policy.info.shipped {
        return Reason::PolicyNotInForce;
    }
    let repository = sigobj::repository_of(&booted.image);
    let Some(key_paths) = policy.scopes.get(repository) else { return Reason::ReferenceOutOfScope };
    if !booted.enforcing {
        // The installer's reference, or an install that has not migrated yet (UT4).
        return Reason::Media;
    }
    let keys: Vec<_> = key_paths.iter().filter_map(|path| std::fs::read_to_string(path).ok()).filter_map(|pem| sigobj::load_key(&pem)).collect();
    let claims = ctx.store.signature_dir(&booted.digest).and_then(|dir| sigobj::claims(&dir, &keys).ok()).unwrap_or_default();
    let covers = |claim: &&sigobj::Claim| claim.manifest_digest == booted.digest && claim.repository == repository;
    match claims.iter().filter(covers).map(|claim| claim.backed).max() {
        Some(true) => Reason::Signature,
        Some(false) => Reason::KeyNotInPolicy,
        None => Reason::NoSignature,
    }
}

/// The update state that needs no network: what is staged, and what was refused.
fn local_update(ctx: &Context<'_, impl Tools>, status: &Status) -> UpdateState {
    match &status.staged {
        Some(staged) if !staged.download_only => UpdateState::WillApplyAtNextShutdown,
        Some(staged) if offerable(ctx.store, &status.booted, &staged.digest, staged.build_time) == UpdateState::Available => UpdateState::Downloaded,
        _ if ctx.store.refused().is_some() => UpdateState::Refused,
        _ => UpdateState::None,
    }
}

/// `Available` when `digest` may be offered; otherwise why not. The order of the build
/// times is the only ordering: bootc has none, and a moved tag is a downgrade the policy
/// cannot see.
fn offerable(store: &Store, booted: &Deployed, digest: &str, build_time: i64) -> UpdateState {
    if digest == booted.digest {
        UpdateState::None
    } else if store.held().as_deref() == Some(digest) {
        UpdateState::Held
    } else if build_time <= booted.build_time {
        UpdateState::OlderThanBooted
    } else {
        UpdateState::Available
    }
}

/// True when a download would be verified: the booted origin makes bootc apply the host
/// policy, and that policy demands a signature for the repository the machine follows.
/// The policy need not be the shipped one: the recovery of UT2 installs a local one.
fn enforced(policy: &InForce, booted: &Deployed) -> bool {
    booted.enforcing && policy.scopes.contains_key(sigobj::repository_of(&booted.image))
}

fn online_update<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, status: &mut Status) -> (UpdateState, Option<Failure>) {
    let local = local_update(ctx, status);
    if !enforced(policy, &status.booted) || local == UpdateState::WillApplyAtNextShutdown {
        return (local, None);
    }
    let candidate = match ctx.tools.candidate(&status.booted.image) {
        Ok(candidate) => candidate,
        Err(failure) => return (local, Some(failure)),
    };
    // The registry answered: that is a successful check, whatever it said.
    if let Err(err) = ctx.store.set_last_success(ctx.now) {
        tracing::error!(%err, "cannot record the successful check");
        return (local, Some(Failure { code: ErrorCode::Storage, host: None }));
    }
    let verdict = offerable(ctx.store, &status.booted, &candidate.digest, candidate.build_time);
    if verdict != UpdateState::Available {
        return (if verdict == UpdateState::None { local } else { verdict }, None);
    }
    if status.staged.as_ref().is_some_and(|staged| staged.download_only && staged.digest == candidate.digest) {
        return (UpdateState::Downloaded, None);
    }
    if ctx.tools.metered() {
        return (UpdateState::Available, None);
    }
    match ctx.tools.download() {
        Err(failure) if failure.code == ErrorCode::Policy => {
            let stored = ctx.store.set_refused(Some(&candidate.digest));
            (UpdateState::Refused, Some(stored.map_or(Failure { code: ErrorCode::Storage, host: None }, |()| failure)))
        }
        Err(failure) => (UpdateState::Available, Some(failure)),
        Ok(()) => {
            *status = match ctx.tools.status() {
                Ok(status) => status,
                Err(failure) => return (UpdateState::Available, Some(failure)),
            };
            // What bootc staged is what counts, not what the tag said a moment earlier.
            let after = local_update(ctx, status);
            if after != UpdateState::Downloaded {
                return (UpdateState::Available, None);
            }
            let staged = status.staged.as_ref().map(|staged| staged.digest.clone()).unwrap_or_default();
            let mut failure = ctx.store.set_refused(None).err().map(|_| Failure { code: ErrorCode::Storage, host: None });
            if let Some(dir) = ctx.store.signature_dir(&staged).filter(|dir| !dir.join("manifest.json").exists()) {
                failure = failure.or(ctx.tools.fetch_signature(sigobj::repository_of(&status.booted.image), &staged, &dir).err());
            }
            (UpdateState::Downloaded, failure)
        }
    }
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
    let newest = ctx.store.record_booted(status.booted.build_time).map_err(storage)?;
    let policy = crate::policy::in_force(&ctx.policy);
    let (update, failure) = if offline { (local_update(ctx, &status), None) } else { online_update(ctx, &policy, &mut status) };
    if !offline && enforced(&policy, &status.booted) {
        // A machine whose signature object was never stored, or was lost, heals here. A
        // failure is not the check's: the reason below already says `no-signature`.
        if let Some(dir) = ctx.store.signature_dir(&status.booted.digest).filter(|dir| !dir.join("manifest.json").exists()) {
            if let Err(failure) = ctx.tools.fetch_signature(sigobj::repository_of(&status.booted.image), &status.booted.digest, &dir) {
                tracing::warn!(code = ?failure.code, "the signature object of the booted digest could not be fetched");
            }
        }
    }
    let state = State {
        schema: SCHEMA,
        verified: reason(ctx, &policy, &status.booted).into(),
        booted: deployment(&status.booted),
        downloaded: status.staged.as_ref().map(deployment),
        previous: status.rollback.as_ref().map(deployment),
        update,
        policy: policy.info,
        secure_boot: crate::secureboot::read(&ctx.efivars, &ctx.lockdown),
        newest_booted_build_time: newest,
        last_successful_check: ctx.store.last_success(),
        last_error: failure.as_ref().map_or(ErrorCode::None, |failure| failure.code),
        last_error_host: failure.and_then(|failure| failure.host),
    };
    ctx.store.publish(&state).map_err(storage)?;
    Ok(state)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tools::Candidate;
    use std::cell::RefCell;
    use std::path::Path;

    pub(crate) const REPO: &str = "localhost:5000/spike/athanor-system";
    /// The digest the signature object under tests/vectors/real covers.
    pub(crate) const SIGNED: &str = "sha256:08d9f3ab2f3fd065175df48841e6434914170493578a3f59d6f6fc5dcdb971f9";

    pub(crate) fn digest(n: u8) -> String {
        format!("sha256:{}", format!("{n:02x}").repeat(32))
    }

    pub(crate) fn deployed(digest: &str, build_time: i64) -> Deployed {
        Deployed { image: format!("{REPO}:stable"), digest: digest.into(), version: format!("43.{build_time}"), build_time, enforcing: true, download_only: false }
    }

    /// bootc, skopeo, ostree and NetworkManager as one scripted object that records its calls.
    pub(crate) struct Fake {
        pub status: RefCell<Status>,
        pub candidate: Result<Candidate, Failure>,
        /// What `download` stages, or how it fails.
        pub download: Result<Deployed, Failure>,
        pub metered: bool,
        pub relock: Result<(), Failure>,
        pub calls: RefCell<Vec<String>>,
    }

    impl Fake {
        pub(crate) fn booted(booted: Deployed) -> Self {
            Self {
                status: RefCell::new(Status { booted, staged: None, rollback: None }),
                candidate: Err(Failure { code: ErrorCode::Network, host: Some("localhost:5000".into()) }),
                download: Err(Failure { code: ErrorCode::Internal, host: None }),
                metered: false,
                relock: Ok(()),
                calls: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn offering(mut self, digest: &str, build_time: i64) -> Self {
            self.candidate = Ok(Candidate { digest: digest.into(), version: format!("43.{build_time}"), build_time });
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
            Ok(self.status.borrow().clone())
        }
        fn candidate(&self, image: &str) -> Result<Candidate, Failure> {
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
            self.status.borrow_mut().staged = Some(self.download.clone()?);
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
        fn metered(&self) -> bool {
            self.metered
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
            for dir in [store.run.clone(), store.var.join("signatures"), root.join("etc/registries.d"), root.join("usr/registries.d")] {
                std::fs::create_dir_all(dir).expect("mkdir");
            }
            let vectors = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
            let key_paths: Vec<_> = keys.iter().map(|key| vectors.join(key).display().to_string()).collect();
            let policy = serde_json::json!({"default": [{"type": "reject"}], "transports": {"docker": {
                "": [{"type": "insecureAcceptAnything"}],
                REPO: [{"type": "sigstoreSigned", "keyPaths": key_paths, "signedIdentity": {"type": "matchRepository"}}]}}});
            std::fs::write(root.join("usr/policy.json"), policy.to_string()).expect("write");
            std::fs::write(root.join("usr/registries.d/athanor.yaml"), "docker: {}\n").expect("write");
            let paths = PolicyPaths { etc_policy: root.join("etc/policy.json"), etc_registries: root.join("etc/registries.d/athanor.yaml"), shipped: root.join("usr") };
            std::os::unix::fs::symlink(root.join("usr/policy.json"), &paths.etc_policy).expect("symlink");
            std::os::unix::fs::symlink(root.join("usr/registries.d/athanor.yaml"), &paths.etc_registries).expect("symlink");
            Self { store, policy: paths, root }
        }

        pub(crate) fn ctx<'a>(&'a self, tools: &'a Fake, now: i64) -> Context<'a, Fake> {
            Context { tools, store: &self.store, policy: self.policy.clone(), efivars: self.root.join("efivars"), lockdown: self.root.join("lockdown"), now }
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
        assert_eq!(state.downloaded.as_ref().map(|d| d.digest.as_str()), Some(SIGNED));
        assert_eq!((state.last_error, state.last_successful_check), (ErrorCode::None, Some(5000)));
        assert!(machine.store.signature_dir(SIGNED).expect("dir").join("manifest.json").exists());
        assert_eq!(athanor_trust_state::read_owned_by(&machine.store.run.join("state.json"), std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&machine.root).expect("meta"))), Ok(state));
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
    fn a_tag_moved_to_an_older_or_equal_build_is_published_and_not_downloaded() {
        let machine = Machine::new("older", &["real/k1.pub"]);
        for build_time in [500, 1000] {
            let tools = Fake::booted(deployed(&digest(1), 1000)).offering(&digest(2), build_time);
            assert_eq!(run(&machine.ctx(&tools, 5000), false).expect("check").update, UpdateState::OlderThanBooted);
            assert!(!tools.called("download"));
        }
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
        assert_eq!(machine.store.refused(), None);
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
    fn the_newest_booted_build_time_is_published_after_going_back() {
        let machine = Machine::new("went-back", &["real/k1.pub"]);
        run(&machine.ctx(&Fake::booted(deployed(&digest(2), 2000)), 5000), true).expect("offline");
        let state = run(&machine.ctx(&Fake::booted(deployed(&digest(1), 1000)), 6000), true).expect("offline");
        assert_eq!((state.booted.build_time, state.newest_booted_build_time), (1000, 2000));
    }
}
