//! The external programs `athanor-update` drives, behind a trait so the logic of the
//! check and of the two requests is tested with fakes. Every message matched here was
//! recorded by spike U1 against bootc 1.16.11, skopeo 1.22.2 and containers/image 5.39.
use athanor_trust_state::ErrorCode;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// A failed call, reduced to what may enter the state file: a code and the registry host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    pub host: Option<String>,
}

/// One deployment as `bootc status --format json` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployed {
    pub image: String,
    pub digest: String,
    pub version: String,
    /// `org.opencontainers.image.created`, seconds since the epoch; 0 when absent.
    pub build_time: i64,
    /// The deployment's origin is `ostree-image-signed:`, so bootc applies the host policy.
    pub enforcing: bool,
    /// Staged and locked against finalization (`bootc upgrade --download-only`).
    pub download_only: bool,
    /// bootc calls the deployment incompatible: packages were layered, removed or replaced
    /// with rpm-ostree, so bootc neither describes it as an image nor upgrades it.
    pub local_changes: bool,
    /// The ostree deployment, `<checksum>.<deploySerial>` as `ostree admin status` names it: two
    /// deployments of one digest differ here. `None` when bootc does not report it.
    pub deployment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub booted: Deployed,
    pub staged: Option<Deployed>,
    pub rollback: Option<Deployed>,
    /// The rollback deployment is already the default of the next boot: greenboot queued it
    /// after a failed health check (doc_recovery.md, R5), or `bootc rollback` was run.
    pub rollback_queued: bool,
}

/// What the tag points at in the registry, read without downloading a layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub digest: String,
    pub version: String,
    pub build_time: i64,
}

pub trait Tools {
    fn status(&self) -> Result<Status, Failure>;
    /// `None` when the registry answers that the tag has no manifest (skopeo: `reading manifest
    /// TAG in REPO: manifest unknown`): the tag was never published, which is not a failure.
    fn candidate(&self, image: &str) -> Result<Option<Candidate>, Failure>;
    fn download(&self) -> Result<(), Failure>;
    fn apply_downloaded(&self) -> Result<(), Failure>;
    fn relock(&self) -> Result<(), Failure>;
    fn rollback(&self) -> Result<(), Failure>;
    fn switch(&self, image: &str) -> Result<(), Failure>;
    /// Copies `<repository>:sha256-<hex>.sig` into `dest` under the attachments policy.
    fn fetch_signature(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure>;
    /// Copies the release attestation `<repository>:sha256-<hex>.att` into `dest` under the
    /// attachments policy (ADR-0127, point 2). Its class is read by `attestation::class_of`.
    fn fetch_attestation(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure>;
    /// NetworkManager's `Metered` is 1 (yes) or 3 (guessed yes). No NetworkManager: false.
    fn metered(&self) -> bool;
    /// `greenboot-success.target` is active: greenboot's required health checks passed on this
    /// boot. `boot-complete.target` does not say it: it becomes active whatever they say. Any
    /// other answer, an error included, is false.
    fn boot_healthy(&self) -> bool;
}

/// Seconds since the epoch of an RFC 3339 time, as the image labels carry it.
#[must_use]
pub fn unix_time(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|time| time.timestamp())
}

/// The registry host of `registry/path[:tag]`.
#[must_use]
pub fn host_of(image: &str) -> Option<String> {
    image.split('/').next().filter(|host| !host.is_empty()).map(str::to_owned)
}

/// True for the registry's answer that a tag, or the whole image, has no manifest:
/// `manifest unknown` or `name unknown` (the distribution specification), and the answers
/// ghcr.io gives for a package that does not exist, which it does not tell from a private
/// one: a pull token refused with 403, or `denied`. Narrower than the `Registry` code, which
/// also covers `unauthorized`: that must keep failing.
#[must_use]
pub fn manifest_absent(stderr: &str) -> bool {
    (stderr.contains("reading manifest") && (stderr.contains("manifest unknown") || stderr.contains("name unknown"))) || access_denied(stderr)
}

/// True for the two answers of ghcr.io that [`manifest_absent`] reads as absent although the
/// registry did not say "not found": a package that does not exist, but also one made
/// private, or a credential that expired, answers the same.
#[must_use]
pub fn access_denied(stderr: &str) -> bool {
    stderr.contains("Requesting bearer token: received unexpected HTTP status: 403 Forbidden") || stderr.contains("denied: requested access to the resource is denied")
}

/// Maps the error text of bootc, skopeo or ostree to a code. The text goes no further.
#[must_use]
pub fn classify(stderr: &str) -> ErrorCode {
    let has = |needles: &[&str]| needles.iter().any(|needle| stderr.contains(needle));
    if has(&["A signature was required, but no signature exists", "cryptographic signature verification failed", "Source image rejected", "refusing usage"]) {
        ErrorCode::Policy
    } else if has(&["pinging container registry", "i/o timeout", "connection refused", "no such host", "dial tcp", "network is unreachable", "TLS handshake"]) {
        ErrorCode::Network
    } else if has(&["manifest unknown", "unauthorized", "denied", "toomanyrequests", "received unexpected HTTP status"]) {
        ErrorCode::Registry
    } else if has(&["No space left on device", "Read-only file system", "Input/output error"]) {
        ErrorCode::Storage
    } else {
        ErrorCode::Internal
    }
}

#[derive(Deserialize)]
struct BootcStatus {
    status: BootcHost,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootcHost {
    booted: Option<BootcEntry>,
    staged: Option<BootcEntry>,
    rollback: Option<BootcEntry>,
    #[serde(default)]
    rollback_queued: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootcEntry {
    image: Option<BootcImage>,
    #[serde(default)]
    download_only: bool,
    #[serde(default)]
    incompatible: bool,
    ostree: Option<BootcOstree>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootcOstree {
    checksum: Option<String>,
    deploy_serial: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootcImage {
    image: BootcReference,
    version: Option<String>,
    timestamp: Option<String>,
    image_digest: String,
}

#[derive(Deserialize)]
struct BootcReference {
    image: String,
    signature: Option<serde_json::Value>,
}

fn deployed(entry: BootcEntry) -> Option<Deployed> {
    let image = entry.image?;
    Some(Deployed {
        enforcing: image.image.signature.as_ref().and_then(serde_json::Value::as_str) == Some("containerPolicy"),
        image: image.image.image,
        digest: image.image_digest,
        version: image.version.unwrap_or_default(),
        build_time: image.timestamp.as_deref().and_then(unix_time).unwrap_or(0),
        download_only: entry.download_only,
        local_changes: false,
        deployment: entry.ostree.and_then(|ostree| Some(format!("{}.{}", ostree.checksum?, ostree.deploy_serial?))),
    })
}

/// Parses `bootc status --format json`. `None` when the host is not booted from an image.
/// bootc reports an incompatible booted deployment without its image, so `local` is asked for
/// it; an incompatible staged or rollback deployment is left out, as bootc cannot act on it.
#[must_use]
pub fn parse_status(json: &str, local: impl FnOnce() -> Option<Deployed>) -> Option<Status> {
    let host = serde_json::from_str::<BootcStatus>(json).ok()?.status;
    let booted = host.booted?;
    let booted = if booted.incompatible { local()? } else { deployed(booted)? };
    Some(Status {
        booted,
        staged: host.staged.and_then(deployed),
        rollback: host.rollback.and_then(deployed),
        rollback_queued: host.rollback_queued,
    })
}

#[derive(Deserialize)]
struct RpmOstreeStatus {
    deployments: Vec<RpmOstreeDeployment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RpmOstreeDeployment {
    #[serde(default)]
    booted: bool,
    container_image_reference: Option<String>,
    container_image_reference_digest: Option<String>,
    version: Option<String>,
    /// The base commit's metadata. Its `ostree.container.image-config` is the image's
    /// configuration as a JSON string, which carries the `org.opencontainers.image.created`
    /// label. Neither `timestamp`, when the packages were layered on this machine, nor
    /// `base-timestamp`, the configuration's `created` that the build pins to a fixed epoch
    /// (UD29), is the build time.
    #[serde(default)]
    base_commit_meta: serde_json::Map<String, serde_json::Value>,
}

/// The `org.opencontainers.image.created` label of the image configuration in a base commit's
/// metadata, seconds since the epoch; 0 when absent, as `bootc status` reports it.
fn label_time(meta: &serde_json::Map<String, serde_json::Value>) -> i64 {
    let config = meta.get("ostree.container.image-config").and_then(serde_json::Value::as_str).and_then(|config| serde_json::from_str::<serde_json::Value>(config).ok());
    config.as_ref().and_then(|config| config["config"]["Labels"]["org.opencontainers.image.created"].as_str()).and_then(unix_time).unwrap_or(0)
}

/// The image a container reference of rpm-ostree names, without its transport:
/// `ostree-unverified-registry:IMG`, `ostree-image-signed:docker://IMG` or
/// `ostree-remote-registry:REMOTE:IMG`.
fn image_of(reference: &str) -> Option<&str> {
    let (scheme, rest) = reference.split_once(':')?;
    let rest = if scheme.starts_with("ostree-remote-") { rest.split_once(':')?.1 } else { rest };
    let image = rest.strip_prefix("docker://").or_else(|| rest.strip_prefix("registry:")).unwrap_or(rest);
    Some(image).filter(|image| !image.is_empty())
}

/// Parses `rpm-ostree status --json --booted` into the booted deployment, for a deployment
/// bootc calls incompatible. Its digest is the base image's: what runs is that image plus the
/// local changes, which is why it is never verified.
#[must_use]
pub fn parse_local(json: &str) -> Option<Deployed> {
    let status = serde_json::from_str::<RpmOstreeStatus>(json).ok()?;
    let booted = status.deployments.into_iter().find(|deployment| deployment.booted)?;
    let reference = booted.container_image_reference?;
    Some(Deployed {
        image: image_of(&reference)?.to_owned(),
        digest: booted.container_image_reference_digest?,
        version: booted.version.unwrap_or_default(),
        build_time: label_time(&booted.base_commit_meta),
        enforcing: reference.starts_with("ostree-image-signed:"),
        download_only: false,
        local_changes: true,
        deployment: None,
    })
}

/// The real programs, at their absolute paths in the image.
pub struct System;

const BOOTC: &str = "/usr/bin/bootc";
const SKOPEO: &str = "/usr/bin/skopeo";
const OSTREE: &str = "/usr/bin/ostree";
const RPM_OSTREE: &str = "/usr/bin/rpm-ostree";
const BUSCTL: &str = "/usr/bin/busctl";
pub const ATTACHMENTS_POLICY: &str = "/usr/share/athanor/containers/attachments-policy.json";

/// How long each `skopeo` call of an attachment fetch may run: the bound
/// `forge/scripts/registry_probe.sh` gives a registry read.
const FETCH_TIME: Duration = Duration::from_secs(30);

/// What one attachment fetch may bring into the store: the caps of the verifier that reads the
/// object, and the time each call may take.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    manifest: u64,
    blob: u64,
    time: Duration,
}

/// The members of an attachment's manifest the fetch bounds before it copies anything.
#[derive(Deserialize)]
struct AttachmentManifest {
    config: Blob,
    layers: Vec<Blob>,
    /// Present in an index, whose instances skopeo would fetch beyond this manifest.
    manifests: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Blob {
    size: u64,
}

/// Copies `<repository>:sha256-<hex>.<suffix>` into `dest` under the attachments policy. The
/// manifest is read first, within `bounds.manifest`, and nothing is copied when it is an index or
/// names a blob above `bounds.blob`, which the verifier would not read (#658). The copy names the
/// digest of the manifest read, so the registry cannot serve it another one. Each call is killed
/// past `bounds.time`.
fn fetch_attachment(skopeo: &str, repository: &str, digest: &str, suffix: &str, dest: &Path, bounds: Bounds) -> Result<(), Failure> {
    let hex = digest.strip_prefix("sha256:").filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).ok_or(Failure { code: ErrorCode::Internal, host: None })?;
    let host = host_of(repository);
    let tagged = format!("docker://{repository}:sha256-{hex}.{suffix}");
    let raw = run_bounded(skopeo, &["inspect", "--raw", &tagged], host.clone(), Some(bounds.manifest), bounds.time)?;
    let out_of_bounds = Failure { code: ErrorCode::Registry, host: host.clone() };
    let Ok(manifest) = serde_json::from_slice::<AttachmentManifest>(&raw) else {
        tracing::warn!(%tagged, "the attachment's manifest is not an image manifest; nothing is copied");
        return Err(out_of_bounds);
    };
    if manifest.manifests.is_some() || std::iter::once(&manifest.config).chain(&manifest.layers).any(|blob| blob.size > bounds.blob) {
        tracing::warn!(%tagged, cap = bounds.blob, "the attachment is an index or names a blob above the verifier's cap; nothing is copied");
        return Err(out_of_bounds);
    }
    let pinned = format!("docker://{repository}@sha256:{}", hex::encode(Sha256::digest(&raw)));
    let dest = format!("dir:{}", dest.display());
    run_bounded(skopeo, &["copy", "--policy", ATTACHMENTS_POLICY, &pinned, &dest], host, None, bounds.time).map(drop)
}

fn run(program: &str, args: &[&str], host: Option<String>) -> Result<String, Failure> {
    run_with_stderr(program, args, host).map_err(|(failure, _)| failure)
}

/// Like [`run`], and hands the error text to the caller that must tell two failures of one
/// code apart. The text must not leave the process.
fn run_with_stderr(program: &str, args: &[&str], host: Option<String>) -> Result<String, (Failure, String)> {
    let output = Command::new(program).args(args).env("LC_ALL", "C").output().map_err(|_| (Failure { code: ErrorCode::Internal, host: None }, String::new()))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(failed(program, output.status, &output.stderr, host))
}

/// The failure of a call that exited with `status`, by its error text.
fn failed(program: &str, status: ExitStatus, stderr: &[u8], host: Option<String>) -> (Failure, String) {
    let stderr = String::from_utf8_lossy(stderr);
    // The text stays in the journal of this unit; only the code leaves the process.
    tracing::warn!(program, status = ?status.code(), %stderr, "command failed");
    let code = classify(&stderr);
    let host = host.filter(|_| matches!(code, ErrorCode::Network | ErrorCode::Registry));
    (Failure { code, host }, stderr.into_owned())
}

/// Like [`run`], within two bounds: the call is killed once it has run for `time`, and an
/// output longer than `cap` bytes is refused and read no further. `None` discards the output.
fn run_bounded(program: &str, args: &[&str], host: Option<String>, cap: Option<u64>, time: Duration) -> Result<Vec<u8>, Failure> {
    let internal = || Failure { code: ErrorCode::Internal, host: None };
    let stdout = if cap.is_some() { Stdio::piped() } else { Stdio::null() };
    let mut child = Command::new(program).args(args).env("LC_ALL", "C").stdin(Stdio::null()).stdout(stdout).stderr(Stdio::piped()).spawn().map_err(|_| internal())?;
    let limit = cap.map_or(0, |cap| cap.saturating_add(1));
    let stdout = child.stdout.take().map(|pipe| std::thread::spawn(move || read_all(pipe.take(limit))));
    let stderr = child.stderr.take().map(|pipe| std::thread::spawn(move || read_all(pipe)));
    let deadline = Instant::now() + time;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                if let Err(err) = child.kill() {
                    tracing::warn!(program, %err, "cannot kill a call past its time limit");
                }
                child.wait().map_err(|_| internal())?;
                break None;
            }
            Err(_) => return Err(internal()),
        }
    };
    // Both pipes close when the call ends, so neither reader outlives it.
    let joined = |reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>| reader.map_or(Ok(Vec::new()), |reader| reader.join().map_err(|_| internal())?.map_err(|_| internal()));
    let (stdout, stderr) = (joined(stdout)?, joined(stderr)?);
    if cap.is_some_and(|cap| stdout.len() as u64 > cap) {
        tracing::warn!(program, ?cap, "the output exceeds its cap; the call is refused");
        return Err(Failure { code: ErrorCode::Registry, host });
    }
    let Some(status) = status else {
        tracing::warn!(program, ?time, "the call exceeded its time limit and was killed");
        return Err(Failure { code: ErrorCode::Network, host });
    };
    if status.success() {
        Ok(stdout)
    } else {
        Err(failed(program, status, &stderr, host).0)
    }
}

fn read_all(mut pipe: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).map(|_| bytes)
}

impl Tools for System {
    fn status(&self) -> Result<Status, Failure> {
        let json = run(BOOTC, &["status", "--format", "json"], None)?;
        let mut failure = None;
        let status = parse_status(&json, || run(RPM_OSTREE, &["status", "--json", "--booted"], None).map_err(|err| failure = Some(err)).ok().and_then(|json| parse_local(&json)));
        status.ok_or_else(|| failure.unwrap_or(Failure { code: ErrorCode::Internal, host: None }))
    }

    fn candidate(&self, image: &str) -> Result<Option<Candidate>, Failure> {
        let host = host_of(image);
        let digest = match run_with_stderr(SKOPEO, &["inspect", "--format", "{{.Digest}}", &format!("docker://{image}")], host.clone()) {
            Ok(out) => out.trim().to_owned(),
            Err((_, stderr)) if manifest_absent(&stderr) => {
                if access_denied(&stderr) {
                    tracing::warn!(%image, "the registry denied access, read as an absent image: the package may be private, or a credential expired");
                }
                return Ok(None);
            }
            Err((failure, _)) => return Err(failure),
        };
        // The configuration is read by digest, so both facts describe one image even if
        // the tag moves between the two calls.
        let pinned = format!("docker://{}@{digest}", crate::sigobj::repository_of(image));
        let config = run(SKOPEO, &["inspect", "--config", &pinned], host)?;
        let labels = serde_json::from_str::<serde_json::Value>(&config).ok().map(|value| value["config"]["Labels"].clone()).unwrap_or_default();
        let label = |name: &str| labels[name].as_str().unwrap_or_default().to_owned();
        Ok(Some(Candidate {
            digest,
            version: label("org.opencontainers.image.version"),
            build_time: unix_time(&label("org.opencontainers.image.created")).unwrap_or(0),
        }))
    }

    fn download(&self) -> Result<(), Failure> {
        let host = self.status().ok().and_then(|status| host_of(&status.booted.image));
        run(BOOTC, &["upgrade", "--download-only"], host).map(drop)
    }

    fn apply_downloaded(&self) -> Result<(), Failure> {
        run(BOOTC, &["upgrade", "--from-downloaded"], None).map(drop)
    }

    fn relock(&self) -> Result<(), Failure> {
        run(OSTREE, &["admin", "lock-finalization"], None).map(drop)
    }

    fn rollback(&self) -> Result<(), Failure> {
        run(BOOTC, &["rollback"], None).map(drop)
    }

    fn switch(&self, image: &str) -> Result<(), Failure> {
        run(BOOTC, &["switch", "--enforce-container-sigpolicy", "--transport", "registry", image], host_of(image)).map(drop)
    }

    fn fetch_signature(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure> {
        let bounds = Bounds { manifest: crate::sigobj::MAX_MANIFEST, blob: crate::sigobj::MAX_PAYLOAD, time: FETCH_TIME };
        fetch_attachment(SKOPEO, repository, digest, "sig", dest, bounds)
    }

    fn fetch_attestation(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure> {
        let bounds = Bounds { manifest: crate::attestation::MAX_MANIFEST, blob: crate::attestation::MAX_PAYLOAD, time: FETCH_TIME };
        fetch_attachment(SKOPEO, repository, digest, "att", dest, bounds)
    }

    fn metered(&self) -> bool {
        let property = ["--system", "get-property", "org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", "org.freedesktop.NetworkManager", "Metered"];
        matches!(run(BUSCTL, &property, None).as_deref().map(str::trim), Ok("u 1" | "u 3"))
    }

    fn boot_healthy(&self) -> bool {
        let property = ["--system", "get-property", "org.freedesktop.systemd1", "/org/freedesktop/systemd1/unit/greenboot_2dsuccess_2etarget", "org.freedesktop.systemd1.Unit", "ActiveState"];
        matches!(run(BUSCTL, &property, None).as_deref().map(str::trim), Ok(r#"s "active""#))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bootc status --format json` of spike U1 after `--download-only`, reduced to the members read.
    const STAGED: &str = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost","status":{
      "staged":{"downloadOnly":true,"softRebootCapable":false,"pinned":false,"image":{"imageDigest":"sha256:e64f7608","timestamp":"2026-09-15T10:00:00Z","version":"43.20260915.2",
        "image":{"image":"localhost:5000/spike/athanor-system:stable","signature":"containerPolicy","transport":"registry"}}},
      "booted":{"image":{"imageDigest":"sha256:08d9f3ab","timestamp":"2026-09-10T10:00:00Z","version":"43.20260910.1",
        "image":{"image":"localhost:5000/spike/athanor-system:stable","signature":"containerPolicy","transport":"registry"}},"cachedUpdate":{"imageDigest":"sha256:stale"}},
      "rollback":{"image":{"imageDigest":"sha256:40daa320","timestamp":null,"version":null,
        "image":{"image":"ghcr.io/owner/athanor-system:35355843782","transport":"registry"}}}}}"#;

    #[test]
    fn the_status_gives_the_lock_the_build_time_and_the_enforcement() {
        let status = parse_status(STAGED, || None).expect("status");
        assert_eq!(status.booted.build_time, 1_789_034_400);
        assert!(status.booted.enforcing && !status.booted.download_only);
        let staged = status.staged.expect("staged");
        assert!(staged.download_only);
        assert_eq!(staged.digest, "sha256:e64f7608");
        let rollback = status.rollback.expect("rollback");
        assert!(!rollback.enforcing, "an origin without `signature` is ostree-unverified-registry");
        assert_eq!((rollback.build_time, rollback.version.as_str()), (0, ""));
        assert!(!status.rollback_queued);
        let queued = STAGED.replacen(r#""status":{"#, r#""status":{"rollbackQueued":true,"#, 1);
        assert!(parse_status(&queued, || None).expect("status").rollback_queued);
    }

    #[test]
    fn a_deployment_is_named_by_its_checksum_and_serial_when_bootc_gives_them() {
        assert_eq!(parse_status(STAGED, || None).expect("status").staged.expect("staged").deployment, None);
        // The members of the desktop's booted entry of 2026-10-01 (INCOMPATIBLE), on the staged one.
        let named = STAGED.replacen(r#""downloadOnly":true,"#, r#""downloadOnly":true,"ostree":{"checksum":"b87dc949","deploySerial":1,"stateroot":"default"},"#, 1);
        assert_eq!(parse_status(&named, || None).expect("status").staged.expect("staged").deployment.as_deref(), Some("b87dc949.1"));
        // A bootc that leaves out one of the two loses the name, not the status.
        for partial in [r#""checksum":"b87dc949","#, r#""deploySerial":1,"#] {
            let staged = parse_status(&named.replacen(partial, "", 1), || None).expect("status").staged.expect("staged");
            assert_eq!((staged.digest.as_str(), staged.deployment), ("sha256:e64f7608", None), "{partial}");
        }
    }

    #[test]
    fn a_host_not_booted_from_an_image_has_no_status() {
        assert_eq!(parse_status(r#"{"status":{"booted":null,"staged":null,"rollback":null}}"#, || None), None);
        assert_eq!(parse_status("not json", || None), None);
    }

    /// The desktop of 2026-10-01 with nvidia-container-toolkit layered, reduced to the members read.
    const INCOMPATIBLE: &str = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost","status":{"staged":null,
      "booted":{"image":null,"cachedUpdate":null,"incompatible":true,"pinned":false,"downloadOnly":false,"store":"ostreeContainer",
        "ostree":{"checksum":"b87dc9499ddc437d7bf39318ad5f7baff4d1ae62faa508f01d02605e678864e6","deploySerial":0,"stateroot":"default"}},
      "rollback":{"image":null,"incompatible":true,"downloadOnly":false}}}"#;
    const RPM_OSTREE_BOOTED: &str = r#"{"deployments":[{"booted":true,"staged":false,
      "container-image-reference":"ostree-unverified-registry:ghcr.io/owner/athanor-system-nvidia:latest",
      "container-image-reference-digest":"sha256:b6a2b14800b8c6799d04c4adf870e806fb5b8b655cd8fa622e3b9618a985d61d",
      "version":"43.20261001.137","timestamp":1790852801,"base-timestamp":1704067200,"requested-packages":["nvidia-container-toolkit"],
      "base-commit-meta":{"ostree.manifest-digest":"sha256:b6a2b14800b8c6799d04c4adf870e806fb5b8b655cd8fa622e3b9618a985d61d",
        "ostree.container.image-config":"{\"created\":\"2024-01-01T00:00:00Z\",\"config\":{\"Labels\":{\"org.opencontainers.image.created\":\"2026-10-01T10:08:22Z\"}}}"}}]}"#;

    #[test]
    fn an_incompatible_booted_deployment_is_read_from_rpm_ostree() {
        let status = parse_status(INCOMPATIBLE, || parse_local(RPM_OSTREE_BOOTED)).expect("status");
        let booted = status.booted;
        assert_eq!(booted.image, "ghcr.io/owner/athanor-system-nvidia:latest");
        assert_eq!(booted.digest, "sha256:b6a2b14800b8c6799d04c4adf870e806fb5b8b655cd8fa622e3b9618a985d61d");
        assert_eq!((booted.version.as_str(), booted.build_time), ("43.20261001.137", 1_790_849_302), "the label's time, not the layering's nor the pinned epoch's");
        assert!(booted.local_changes && !booted.enforcing && !booted.download_only);
        assert_eq!((status.staged, status.rollback), (None, None));
        assert_eq!(parse_status(INCOMPATIBLE, || None), None, "no description of the booted deployment, no status");
    }

    #[test]
    fn the_transport_of_an_rpm_ostree_reference_is_dropped() {
        for (reference, image) in [
            ("ostree-unverified-registry:ghcr.io/o/a:latest", Some("ghcr.io/o/a:latest")),
            ("ostree-image-signed:docker://ghcr.io/o/a:stable", Some("ghcr.io/o/a:stable")),
            ("ostree-unverified-image:registry:ghcr.io/o/a:stable", Some("ghcr.io/o/a:stable")),
            ("ostree-remote-registry:fedora:quay.io/f/b:43", Some("quay.io/f/b:43")),
            ("ostree-unverified-registry:", None),
            ("nonsense", None),
        ] {
            assert_eq!(image_of(reference), image, "{reference}");
        }
        let signed = RPM_OSTREE_BOOTED.replace("ostree-unverified-registry:", "ostree-image-signed:docker://");
        assert!(parse_local(&signed).expect("local").enforcing);
        assert_eq!(parse_local(r#"{"deployments":[{"booted":false}]}"#), None);
    }

    #[test]
    fn a_local_build_time_comes_from_the_label_alone() {
        let unlabelled = RPM_OSTREE_BOOTED.replace(r#"\"Labels\":{\"org.opencontainers.image.created\":\"2026-10-01T10:08:22Z\"}"#, r#"\"Labels\":{}"#);
        assert_ne!(unlabelled, RPM_OSTREE_BOOTED);
        assert_eq!(parse_local(&unlabelled).expect("local").build_time, 0, "never the pinned 2024 epoch of base-timestamp");
        let mut without_meta: serde_json::Value = serde_json::from_str(RPM_OSTREE_BOOTED).expect("json");
        assert!(without_meta["deployments"][0].as_object_mut().expect("deployment").remove("base-commit-meta").is_some());
        assert_eq!(parse_local(&without_meta.to_string()).expect("local").build_time, 0);
    }

    #[test]
    fn the_recorded_messages_map_to_their_codes() {
        for (stderr, code) in [
            ("error: Upgrading: … failed to invoke method OpenImage: A signature was required, but no signature exists", ErrorCode::Policy),
            ("… cryptographic signature verification failed: invalid signature when validating ASN.1 encoded signature", ErrorCode::Policy),
            ("containers-policy.json specifies a default of `insecureAcceptAnything`; refusing usage", ErrorCode::Policy),
            ("pinging container registry localhost:5000: Get \"http://localhost:5000/v2/\": dial tcp: i/o timeout", ErrorCode::Network),
            ("reading manifest stable in registry.example/o/athanor-system: manifest unknown", ErrorCode::Registry),
            ("error: Initializing storage: … Read-only file system (os error 30)", ErrorCode::Storage),
            ("error: something nobody has seen yet", ErrorCode::Internal),
        ] {
            assert_eq!(classify(stderr), code, "{stderr}");
        }
    }

    #[test]
    fn only_a_missing_manifest_is_absent() {
        assert!(manifest_absent("Error parsing image name \"docker://r/o/a:stable\": reading manifest stable in r/o/a: manifest unknown"));
        assert!(manifest_absent("reading manifest latest in r/o/a: name unknown: repository name not known to registry"));
        // ghcr.io, for a package that does not exist (observed 2026-10-08 with skopeo 1.22).
        assert!(manifest_absent(
            "Error parsing image name \"docker://ghcr.io/o/absent:latest\": Requesting bearer token: received unexpected HTTP status: 403 Forbidden"
        ));
        assert!(manifest_absent("Error reading manifest latest in ghcr.io/o/absent: denied: requested access to the resource is denied"));
        assert!(!manifest_absent("reading manifest stable in r/o/a: unauthorized: authentication required"));
        assert!(!manifest_absent("Requesting bearer token: received unexpected HTTP status: 401 Unauthorized"));
        assert!(!manifest_absent("pinging container registry r: dial tcp: i/o timeout"));
        assert!(!access_denied("reading manifest latest in r/o/a: name unknown: repository name not known to registry"));
        assert!(access_denied("Error reading manifest latest in ghcr.io/o/absent: denied: requested access to the resource is denied"));
    }

    #[test]
    fn an_attestation_is_fetched_only_for_a_sha256_digest() {
        let refused = System.fetch_attestation("registry.example/o/a", "sha256:../../etc", Path::new("/nonexistent"));
        assert_eq!(refused, Err(Failure { code: ErrorCode::Internal, host: None }));
    }

    /// A stub `skopeo` in a directory of its own: `inspect` prints the file `served`; `copy`
    /// records its arguments in `copied`, writes a blob into `store`, the destination the
    /// fetches below name, and then runs `then`.
    struct Stub {
        dir: std::path::PathBuf,
    }

    const REPO: &str = "registry.example/o/athanor-system";
    const DIGEST: &str = "sha256:e64f7608e64f7608e64f7608e64f7608e64f7608e64f7608e64f7608e64f7608";

    impl Stub {
        fn new(test: &str, served: &[u8], then: &str) -> Stub {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = std::env::temp_dir().join(format!("athanor-update-fetch-{}-{test}", std::process::id()));
            // A directory left by an earlier run of the same test, or none.
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join("served"), served).expect("served");
            let d = dir.display();
            let script = format!("#!/bin/sh\ncase \"$1\" in\ninspect) exec cat '{d}/served' ;;\ncopy) echo \"$@\" > '{d}/copied'; mkdir -p '{d}/store'; echo blob > '{d}/store/blob'; {then} ;;\nesac\n");
            std::fs::write(dir.join("skopeo"), script).expect("stub");
            std::fs::set_permissions(dir.join("skopeo"), std::fs::Permissions::from_mode(0o755)).expect("chmod");
            Stub { dir }
        }

        fn fetch(&self, time: Duration) -> Result<(), Failure> {
            let bounds = Bounds { manifest: crate::sigobj::MAX_MANIFEST, blob: crate::sigobj::MAX_PAYLOAD, time };
            fetch_attachment(self.dir.join("skopeo").to_str().expect("utf-8"), REPO, DIGEST, "sig", &self.dir.join("store"), bounds)
        }

        fn copied(&self) -> bool {
            self.dir.join("copied").exists() || self.dir.join("store").exists()
        }
    }

    impl Drop for Stub {
        fn drop(&mut self) {
            // Test teardown: a directory already gone is fine.
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn manifest(config: u64, layer: u64) -> String {
        let zero = "0".repeat(64);
        format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:{zero}","size":{config}}},"layers":[{{"mediaType":"{}","digest":"sha256:{zero}","size":{layer}}}]}}"#,
            crate::sigobj::LAYER_MEDIA_TYPE
        )
    }

    fn refused() -> Failure {
        Failure { code: ErrorCode::Registry, host: Some("registry.example".to_owned()) }
    }

    #[test]
    fn an_attachment_within_the_caps_is_copied_by_the_digest_of_the_manifest_read() {
        // At both caps exactly: the manifest padded to its cap, its layer at the verifier's.
        let mut served = manifest(233, crate::sigobj::MAX_PAYLOAD).into_bytes();
        served.resize(usize::try_from(crate::sigobj::MAX_MANIFEST).expect("cap"), b' ');
        let stub = Stub::new("within", &served, "true");
        assert_eq!(stub.fetch(Duration::from_secs(10)), Ok(()));
        let copied = std::fs::read_to_string(stub.dir.join("copied")).expect("copied");
        let pinned = format!("docker://{REPO}@sha256:{}", hex::encode(Sha256::digest(&served)));
        assert_eq!(copied.trim(), format!("copy --policy {ATTACHMENTS_POLICY} {pinned} dir:{}", stub.dir.join("store").display()));
    }

    #[test]
    fn a_blob_above_the_verifiers_cap_is_refused_before_anything_is_copied() {
        let above = crate::sigobj::MAX_PAYLOAD + 1;
        let index = r#"{"schemaVersion":2,"config":{"size":2},"layers":[],"manifests":[{"digest":"sha256:0","size":2}]}"#.to_owned();
        for (case, served) in [("layer", manifest(233, above)), ("config", manifest(above, 300)), ("index", index), ("garbage", "not json".to_owned())] {
            let stub = Stub::new(case, served.as_bytes(), "true");
            assert_eq!(stub.fetch(Duration::from_secs(10)), Err(refused()), "{case}");
            assert!(!stub.copied(), "{case}: nothing reaches the store");
        }
    }

    #[test]
    fn a_manifest_above_its_own_cap_is_refused() {
        // A manifest the fetch would accept, one byte past the cap.
        let mut served = manifest(233, 300).into_bytes();
        served.resize(usize::try_from(crate::sigobj::MAX_MANIFEST + 1).expect("cap"), b' ');
        let stub = Stub::new("manifest", &served, "true");
        assert_eq!(stub.fetch(Duration::from_secs(10)), Err(refused()));
        assert!(!stub.copied());
    }

    #[test]
    fn a_copy_past_the_time_limit_is_killed() {
        let stub = Stub::new("stall", manifest(233, 300).as_bytes(), "exec sleep 30");
        let started = Instant::now();
        assert_eq!(stub.fetch(Duration::from_millis(200)), Err(Failure { code: ErrorCode::Network, host: Some("registry.example".to_owned()) }));
        assert!(started.elapsed() < Duration::from_secs(10), "killed at the limit, not after the stalled copy");
    }

    #[test]
    fn times_and_hosts() {
        assert_eq!(unix_time("2026-09-15T10:00:00Z"), unix_time("2026-09-15T12:00:00.5+02:00"));
        assert_eq!(unix_time("yesterday"), None);
        assert_eq!(host_of("localhost:5000/spike/athanor-system:stable").as_deref(), Some("localhost:5000"));
    }
}
