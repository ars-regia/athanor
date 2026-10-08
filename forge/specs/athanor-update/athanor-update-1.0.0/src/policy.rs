//! The container signature policy: which one is in force, whether it is the shipped one,
//! and which keys it names for a repository (docs/architecture/doc_update_trust.md, UT3, UT5).
//! The policy file is the single list of keys; the key directory is only where files live.
use athanor_trust_state::Policy;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Repository scope -> the `keyPaths` of its `sigstoreSigned` requirements. A scope is
/// listed only when every requirement on it is `sigstoreSigned` with `matchRepository`,
/// so a scope somebody relaxed to `insecureAcceptAnything` is not one of ours any more.
pub type Scopes = BTreeMap<String, Vec<PathBuf>>;

#[must_use]
pub fn scopes(policy_json: &[u8]) -> Scopes {
    let Ok(policy) = serde_json::from_slice::<serde_json::Value>(policy_json) else { return Scopes::new() };
    let Some(docker) = policy["transports"]["docker"].as_object() else { return Scopes::new() };
    let mut found = Scopes::new();
    for (scope, requirements) in docker {
        let Some(requirements) = requirements.as_array().filter(|list| !list.is_empty()) else { continue };
        let strict = requirements.iter().all(|req| req["type"] == "sigstoreSigned" && req["signedIdentity"]["type"] == "matchRepository");
        if scope.is_empty() || !strict {
            continue;
        }
        let keys = requirements.iter().flat_map(|req| req["keyPaths"].as_array().cloned().unwrap_or_default()).filter_map(|path| path.as_str().map(PathBuf::from)).collect();
        found.insert(scope.clone(), keys);
    }
    found
}

/// The one scope with the registry host and the image name of `repository` under another
/// owner, when `repository` is an image of a previous owner in `moved_from`: where a machine
/// that follows the project's previous owner moves (docs/architecture/doc_update_delivery.md,
/// UD45). `None` when `repository` is a scope itself, is not under a previous owner, or when
/// no scope, or more than one, matches.
#[must_use]
pub fn successor<'a>(scopes: &'a Scopes, repository: &str, moved_from: &[String]) -> Option<&'a str> {
    fn host_and_name(repository: &str) -> Option<(&str, &str)> {
        Some((repository.split_once('/')?.0, repository.rsplit_once('/')?.1))
    }
    let (owner, name) = repository.rsplit_once('/')?;
    if scopes.contains_key(repository) || name.is_empty() || !moved_from.iter().any(|moved| moved == owner) {
        return None;
    }
    let wanted = host_and_name(repository)?;
    let mut matches = scopes.keys().filter(|scope| host_and_name(scope) == Some(wanted));
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Where the files of this module live; tests point them at a scratch directory.
#[derive(Debug, Clone)]
pub struct PolicyPaths {
    /// `/etc/containers/policy.json`: what the units' bootc and skopeo resolve. The units
    /// run with `ProtectHome=yes`, so `$HOME/.config/containers/policy.json`, which
    /// containers/image prefers, is not visible to them and cannot shadow this path.
    pub etc_policy: PathBuf,
    pub etc_registries: PathBuf,
    /// `/usr/share/athanor/containers`
    pub shipped: PathBuf,
}

impl PolicyPaths {
    #[must_use]
    pub fn system() -> Self {
        Self {
            etc_policy: "/etc/containers/policy.json".into(),
            etc_registries: "/etc/containers/registries.d/athanor.yaml".into(),
            shipped: "/usr/share/athanor/containers".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InForce {
    pub info: Policy,
    /// The scopes of the policy in force, shipped or not: the download gate reads these.
    pub scopes: Scopes,
    /// The owners the project published under before it moved, as `host/owner`, one per line
    /// of `moved-from` beside the shipped policy: render-policy writes it at the image build
    /// from the bridge variable (docs/architecture/doc_update_delivery.md, UD45). Only an
    /// image under one of these moves; any other owner, a fork included, stays out of scope.
    pub moved_from: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Reads the policy the tools resolve and compares it, and the `registries.d` entry
/// without which a signed image reads as unsigned, with the shipped files by content.
#[must_use]
pub fn in_force(paths: &PolicyPaths) -> InForce {
    let etc = std::fs::read(&paths.etc_policy).unwrap_or_default();
    let same = |etc_file: &Path, shipped_name: &str| match (std::fs::read(etc_file), std::fs::read(paths.shipped.join(shipped_name))) {
        (Ok(a), Ok(b)) => !a.is_empty() && a == b,
        _ => false,
    };
    let shipped = same(&paths.etc_policy, "policy.json") && same(&paths.etc_registries, "registries.d/athanor.yaml");
    InForce {
        info: Policy { path: paths.etc_policy.display().to_string(), sha256: sha256_hex(&etc), shipped },
        scopes: scopes(&etc),
        moved_from: std::fs::read_to_string(paths.shipped.join("moved-from"))
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const SHIPPED: &str = r#"{"default":[{"type":"reject"}],"transports":{
      "docker":{"":[{"type":"insecureAcceptAnything"}],
        "registry.example/owner/athanor-system":[{"type":"sigstoreSigned","keyPaths":["/usr/share/athanor/keys/athanor-image-1.pub","/usr/share/athanor/keys/athanor-image-2.pub"],"signedIdentity":{"type":"matchRepository"}}],
        "registry.example/owner/relaxed":[{"type":"insecureAcceptAnything"}]},
      "oci":{"":[{"type":"insecureAcceptAnything"}]}}}"#;

    #[test]
    fn only_strict_repository_scopes_are_ours_and_they_carry_their_keys() {
        let found = scopes(SHIPPED.as_bytes());
        assert_eq!(found.keys().collect::<Vec<_>>(), ["registry.example/owner/athanor-system"]);
        assert_eq!(found["registry.example/owner/athanor-system"].len(), 2);
        assert!(scopes(br#"{"default":[{"type":"insecureAcceptAnything"}]}"#).is_empty());
        assert!(scopes(b"not json").is_empty());
    }

    #[test]
    fn the_successor_is_the_same_image_under_the_owner_the_policy_pins() {
        let found = scopes(SHIPPED.as_bytes());
        fn moved<'a>(found: &'a Scopes, repository: &str) -> Option<&'a str> {
            successor(found, repository, &["registry.example/previous".into(), "other.example/previous".into()])
        }
        assert_eq!(moved(&found, "registry.example/previous/athanor-system"), Some("registry.example/owner/athanor-system"));
        assert_eq!(moved(&found, "registry.example/owner/athanor-system"), None, "already in scope");
        assert_eq!(moved(&found, "registry.example/fork/athanor-system"), None, "not a previous owner");
        assert_eq!(moved(&found, "registry.example/previous/nested/athanor-system"), None, "not a previous owner");
        assert_eq!(moved(&found, "other.example/previous/athanor-system"), None, "another registry");
        assert_eq!(moved(&found, "registry.example/previous/athanor-system-nvidia"), None, "another image");
        assert_eq!(moved(&found, "registry.example/previous/"), None);
        assert_eq!(moved(&found, "athanor-system"), None);
        let third = r#""registry.example/third/athanor-system":[{"type":"sigstoreSigned","keyPaths":[],"signedIdentity":{"type":"matchRepository"}}]"#;
        let two = SHIPPED.replace(r#""registry.example/owner/relaxed":[{"type":"insecureAcceptAnything"}]"#, third);
        assert_eq!(moved(&scopes(two.as_bytes()), "registry.example/previous/athanor-system"), None, "ambiguous");
    }

    #[test]
    fn a_local_file_or_a_missing_registries_entry_is_not_the_shipped_policy() {
        let dir = std::env::temp_dir().join(format!("athanor-update-policy-{}", std::process::id()));
        let paths = PolicyPaths { etc_policy: dir.join("etc/policy.json"), etc_registries: dir.join("etc/registries.d/athanor.yaml"), shipped: dir.join("usr") };
        for sub in ["etc/registries.d", "usr/registries.d"] {
            std::fs::create_dir_all(dir.join(sub)).expect("mkdir");
        }
        std::fs::write(dir.join("usr/policy.json"), SHIPPED).expect("write");
        std::fs::write(dir.join("usr/registries.d/athanor.yaml"), "docker: {}\n").expect("write");
        std::os::unix::fs::symlink(dir.join("usr/policy.json"), &paths.etc_policy).expect("symlink");
        assert!(!in_force(&paths).info.shipped, "the registries.d entry is missing");

        std::os::unix::fs::symlink(dir.join("usr/registries.d/athanor.yaml"), &paths.etc_registries).expect("symlink");
        let linked = in_force(&paths);
        assert!(linked.info.shipped);
        assert_eq!(linked.scopes.len(), 1);

        std::fs::remove_file(&paths.etc_policy).expect("unlink");
        std::fs::write(&paths.etc_policy, r#"{"default":[{"type":"insecureAcceptAnything"}]}"#).expect("write");
        let shadowed = in_force(&paths);
        assert!(!shadowed.info.shipped && shadowed.scopes.is_empty());
        assert_ne!(shadowed.info.sha256, linked.info.sha256);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
