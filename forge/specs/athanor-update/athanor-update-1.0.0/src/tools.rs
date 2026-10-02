//! The external programs `athanor-update` drives, behind a trait so the logic of the
//! check and of the two requests is tested with fakes. Every message matched here was
//! recorded by spike U1 against bootc 1.16.11, skopeo 1.22.2 and containers/image 5.39.
use athanor_trust_state::ErrorCode;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub booted: Deployed,
    pub staged: Option<Deployed>,
    pub rollback: Option<Deployed>,
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
    fn candidate(&self, image: &str) -> Result<Candidate, Failure>;
    fn download(&self) -> Result<(), Failure>;
    fn apply_downloaded(&self) -> Result<(), Failure>;
    fn relock(&self) -> Result<(), Failure>;
    fn rollback(&self) -> Result<(), Failure>;
    fn switch(&self, image: &str) -> Result<(), Failure>;
    /// Copies `<repository>:sha256-<hex>.sig` into `dest` under the attachments policy.
    fn fetch_signature(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure>;
    /// NetworkManager's `Metered` is 1 (yes) or 3 (guessed yes). No NetworkManager: false.
    fn metered(&self) -> bool;
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
struct BootcHost {
    booted: Option<BootcEntry>,
    staged: Option<BootcEntry>,
    rollback: Option<BootcEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootcEntry {
    image: Option<BootcImage>,
    #[serde(default)]
    download_only: bool,
    #[serde(default)]
    incompatible: bool,
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
    Some(Status { booted, staged: host.staged.and_then(deployed), rollback: host.rollback.and_then(deployed) })
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
    /// The base image's commit time, which is its `org.opencontainers.image.created`; the
    /// deployment's own `timestamp` is when the packages were layered on this machine.
    base_timestamp: Option<i64>,
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
        build_time: booted.base_timestamp.unwrap_or(0),
        enforcing: reference.starts_with("ostree-image-signed:"),
        download_only: false,
        local_changes: true,
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

fn run(program: &str, args: &[&str], host: Option<String>) -> Result<String, Failure> {
    let output = Command::new(program).args(args).env("LC_ALL", "C").output().map_err(|_| Failure { code: ErrorCode::Internal, host: None })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The text stays in the journal of this unit; only the code leaves the process.
    tracing::warn!(program, status = ?output.status.code(), %stderr, "command failed");
    let code = classify(&stderr);
    let host = host.filter(|_| matches!(code, ErrorCode::Network | ErrorCode::Registry));
    Err(Failure { code, host })
}

impl Tools for System {
    fn status(&self) -> Result<Status, Failure> {
        let json = run(BOOTC, &["status", "--format", "json"], None)?;
        let mut failure = None;
        let status = parse_status(&json, || run(RPM_OSTREE, &["status", "--json", "--booted"], None).map_err(|err| failure = Some(err)).ok().and_then(|json| parse_local(&json)));
        status.ok_or_else(|| failure.unwrap_or(Failure { code: ErrorCode::Internal, host: None }))
    }

    fn candidate(&self, image: &str) -> Result<Candidate, Failure> {
        let host = host_of(image);
        let digest = run(SKOPEO, &["inspect", "--format", "{{.Digest}}", &format!("docker://{image}")], host.clone())?.trim().to_owned();
        // The configuration is read by digest, so both facts describe one image even if
        // the tag moves between the two calls.
        let pinned = format!("docker://{}@{digest}", crate::sigobj::repository_of(image));
        let config = run(SKOPEO, &["inspect", "--config", &pinned], host)?;
        let labels = serde_json::from_str::<serde_json::Value>(&config).ok().map(|value| value["config"]["Labels"].clone()).unwrap_or_default();
        let label = |name: &str| labels[name].as_str().unwrap_or_default().to_owned();
        Ok(Candidate {
            digest,
            version: label("org.opencontainers.image.version"),
            build_time: unix_time(&label("org.opencontainers.image.created")).unwrap_or(0),
        })
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
        let hex = digest.strip_prefix("sha256:").filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).ok_or(Failure { code: ErrorCode::Internal, host: None })?;
        let source = format!("docker://{repository}:sha256-{hex}.sig");
        let dest = format!("dir:{}", dest.display());
        run(SKOPEO, &["copy", "--policy", ATTACHMENTS_POLICY, &source, &dest], host_of(repository)).map(drop)
    }

    fn metered(&self) -> bool {
        let property = ["--system", "get-property", "org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", "org.freedesktop.NetworkManager", "Metered"];
        matches!(run(BUSCTL, &property, None).as_deref().map(str::trim), Ok("u 1" | "u 3"))
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
      "version":"43.20261001.137","timestamp":1790852801,"base-timestamp":1790849302,"requested-packages":["nvidia-container-toolkit"]}]}"#;

    #[test]
    fn an_incompatible_booted_deployment_is_read_from_rpm_ostree() {
        let status = parse_status(INCOMPATIBLE, || parse_local(RPM_OSTREE_BOOTED)).expect("status");
        let booted = status.booted;
        assert_eq!(booted.image, "ghcr.io/owner/athanor-system-nvidia:latest");
        assert_eq!(booted.digest, "sha256:b6a2b14800b8c6799d04c4adf870e806fb5b8b655cd8fa622e3b9618a985d61d");
        assert_eq!((booted.version.as_str(), booted.build_time), ("43.20261001.137", 1_790_849_302), "the base image's time, not the layering's");
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
    fn times_and_hosts() {
        assert_eq!(unix_time("2026-09-15T10:00:00Z"), unix_time("2026-09-15T12:00:00.5+02:00"));
        assert_eq!(unix_time("yesterday"), None);
        assert_eq!(host_of("localhost:5000/spike/athanor-system:stable").as_deref(), Some("localhost:5000"));
    }
}
