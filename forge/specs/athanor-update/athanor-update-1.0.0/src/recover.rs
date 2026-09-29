//! `athanor-update recover-key`: the out-of-band recovery of UT2. An administrator moves
//! one machine to a new signing key, with the old one removed, when the project has said
//! the old key is exposed. See forge/specs/athanor-update/RECOVERY.md.
//!
//! `begin NEW.pub` installs a local policy that names the new key alone, so the next
//! check accepts only images signed with it. `finish`, run after the machine has booted an
//! image whose shipped policy names the new key, gives `/etc/containers/policy.json` back
//! to the link into `/usr`. Between the two the state reads `policy-not-in-force`, which
//! is the truth: the policy in force is the administrator's.
use crate::policy::PolicyPaths;
use crate::sigobj;
use crate::store::Store;
use std::path::Path;

pub const RECOVERY_KEY: &str = "recovery.pub";

fn io(context: &str) -> impl Fn(std::io::Error) -> String + '_ {
    move |err| format!("{context}: {err}")
}

/// # Errors
/// A message for the administrator's terminal.
pub fn begin(paths: &PolicyPaths, etc_keys: &Path, new_key: &Path) -> Result<(), String> {
    let pem = std::fs::read_to_string(new_key).map_err(io("reading the new key"))?;
    sigobj::load_key(&pem).ok_or("the new key is not a PEM ECDSA P-256 public key")?;
    let shipped = std::fs::read(paths.shipped.join("policy.json")).map_err(io("reading the shipped policy"))?;
    let mut policy: serde_json::Value = serde_json::from_slice(&shipped).map_err(|err| format!("the shipped policy is not JSON: {err}"))?;
    let key_path = etc_keys.join(RECOVERY_KEY);
    let scopes: Vec<String> = crate::policy::scopes(&shipped).into_keys().collect();
    if scopes.is_empty() {
        return Err("the shipped policy has no signed repository: nothing to recover".into());
    }
    for scope in &scopes {
        for requirement in policy["transports"]["docker"][scope].as_array_mut().into_iter().flatten() {
            requirement["keyPaths"] = serde_json::json!([key_path]);
        }
    }
    std::fs::create_dir_all(etc_keys).map_err(io("creating the key directory"))?;
    Store::replace(etc_keys, RECOVERY_KEY, 0o644, pem.as_bytes()).map_err(io("installing the new key"))?;
    let (dir, name) = split(&paths.etc_policy)?;
    // The rename replaces the link into /usr with a regular file; the link target is untouched.
    Store::replace(dir, name, 0o644, policy.to_string().as_bytes()).map_err(io("installing the local policy"))
}

/// # Errors
/// A message for the administrator's terminal.
pub fn finish(paths: &PolicyPaths, etc_keys: &Path) -> Result<(), String> {
    let key_path = etc_keys.join(RECOVERY_KEY);
    let recovery = std::fs::read_to_string(&key_path).map_err(io("reading the recovery key (was `begin` run?)"))?;
    let shipped_file = paths.shipped.join("policy.json");
    let shipped = std::fs::read(&shipped_file).map_err(io("reading the shipped policy"))?;
    let scopes = crate::policy::scopes(&shipped);
    let named_everywhere = !scopes.is_empty()
        && scopes.values().all(|keys| keys.iter().any(|key| std::fs::read_to_string(key).is_ok_and(|pem| pem.trim() == recovery.trim())));
    if !named_everywhere {
        return Err("the booted image does not ship the new key in its policy yet: update and restart first".into());
    }
    let (dir, name) = split(&paths.etc_policy)?;
    let temporary = dir.join(format!(".{name}.link.{}", std::process::id()));
    match std::fs::remove_file(&temporary) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(io("clearing a leftover")(err)),
        _ => {}
    }
    std::os::unix::fs::symlink(&shipped_file, &temporary).map_err(io("creating the link"))?;
    std::fs::rename(&temporary, &paths.etc_policy).map_err(io("restoring the link"))?;
    std::fs::remove_file(&key_path).map_err(io("removing the recovery key"))
}

fn split(path: &Path) -> Result<(&Path, &str), String> {
    match (path.parent(), path.file_name().and_then(|name| name.to_str())) {
        (Some(dir), Some(name)) => Ok((dir, name)),
        _ => Err(format!("{} has no directory or name", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{Machine, REPO};
    use std::path::PathBuf;

    fn vector(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors").join(name)
    }

    #[test]
    fn begin_names_the_new_key_alone_and_finish_waits_for_an_image_that_ships_it() {
        let old = Machine::new("recover", &["real/k1.pub"]);
        let etc_keys = old.root.join("etc/athanor/keys");
        begin(&old.policy, &etc_keys, &vector("made/a.pub")).expect("begin");

        let local = crate::policy::in_force(&old.policy);
        assert!(!local.info.shipped, "the policy in force is the administrator's");
        assert_eq!(local.scopes[REPO], [etc_keys.join(RECOVERY_KEY)], "the old key is gone from the policy in force");
        assert!(std::fs::symlink_metadata(&old.policy.etc_policy).expect("meta").is_file());
        assert!(old.root.join("usr/policy.json").exists(), "the shipped policy is untouched");

        assert!(finish(&old.policy, &etc_keys).is_err(), "the booted image still ships the old key only");

        // The next image ships the new key: same machine, new /usr.
        let next = Machine::new("recover-next", &["made/a.pub"]);
        std::fs::copy(next.root.join("usr/policy.json"), old.root.join("usr/policy.json")).expect("new /usr");
        finish(&old.policy, &etc_keys).expect("finish");
        assert!(crate::policy::in_force(&old.policy).info.shipped);
        assert!(std::fs::symlink_metadata(&old.policy.etc_policy).expect("meta").is_symlink());
        assert!(!etc_keys.join(RECOVERY_KEY).exists());
    }

    #[test]
    fn begin_refuses_what_is_not_a_p256_public_key() {
        let machine = Machine::new("recover-bad", &["real/k1.pub"]);
        assert!(begin(&machine.policy, &machine.root.join("etc/athanor/keys"), &vector("made/image-digest")).is_err());
        assert!(crate::policy::in_force(&machine.policy).info.shipped, "nothing was touched");
    }
}
