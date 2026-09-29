//! Verification of a cosign "simple signing" attachment as `skopeo copy … dir:` stores it
//! (docs/architecture/doc_update_trust.md, UT5). Four rules, each with a test below:
//! every key is tried against every layer; the signed bytes are the payload blob exactly
//! as stored, checked against its descriptor; the signature is base64 ASN.1 DER, read with
//! `from_der`; `critical.type` must be the cosign type and an unknown member is a refusal.
use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey as _;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::path::Path;

pub const LAYER_MEDIA_TYPE: &str = "application/vnd.dev.cosign.simplesigning.v1+json";
pub const SIGNATURE_ANNOTATION: &str = "dev.cosignproject.cosign/signature";
pub const PAYLOAD_TYPE: &str = "cosign container image signature";
const MAX_MANIFEST: u64 = 1 << 20;
const MAX_PAYLOAD: u64 = 64 << 10;

/// What one verified signature layer states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub manifest_digest: String,
    pub repository: String,
    /// One of the given keys verifies the layer. A claim that is not backed is reported
    /// only so the caller can tell `key-not-in-policy` from `no-signature`.
    pub backed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unreadable,
    Malformed,
}

#[derive(Deserialize)]
struct Manifest {
    layers: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Descriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: u64,
    #[serde(default)]
    annotations: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    critical: Critical,
    #[allow(dead_code)]
    #[serde(default)]
    optional: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Critical {
    identity: Identity,
    image: Image,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    #[serde(rename = "docker-reference")]
    docker_reference: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Image {
    #[serde(rename = "docker-manifest-digest")]
    docker_manifest_digest: String,
}

/// Parses one PEM public key as `cosign generate-key-pair` and `skopeo generate-sigstore-key` write it.
pub fn load_key(pem: &str) -> Option<VerifyingKey> {
    VerifyingKey::from_public_key_pem(pem).ok()
}

/// `registry/path:tag` or `registry/path@digest` without the tag or digest.
pub fn repository_of(reference: &str) -> &str {
    let reference = reference.split('@').next().unwrap_or(reference);
    match reference.rfind(':') {
        Some(colon) if !reference[colon..].contains('/') => &reference[..colon],
        _ => reference,
    }
}

fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > cap {
        return None;
    }
    std::fs::read(path).ok()
}

/// One layer: the descriptor first, then the signature over the stored bytes, then the JSON.
fn claim_of(dir: &Path, layer: &Descriptor, keys: &[VerifyingKey]) -> Option<Claim> {
    if layer.media_type != LAYER_MEDIA_TYPE {
        return None;
    }
    let hex_digest = layer.digest.strip_prefix("sha256:")?;
    if hex_digest.len() != 64 || !hex_digest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let blob = read_capped(&dir.join(hex_digest), MAX_PAYLOAD)?;
    if blob.len() as u64 != layer.size || hex::encode(Sha256::digest(&blob)) != hex_digest {
        return None;
    }
    let der = base64::engine::general_purpose::STANDARD.decode(layer.annotations.get(SIGNATURE_ANNOTATION)?).ok()?;
    let signature = Signature::from_der(&der).ok()?;
    let backed = keys.iter().any(|key| key.verify(&blob, &signature).is_ok());
    let payload: Payload = serde_json::from_slice(&blob).ok()?;
    if payload.critical.kind != PAYLOAD_TYPE {
        return None;
    }
    Some(Claim {
        manifest_digest: payload.critical.image.docker_manifest_digest,
        repository: repository_of(&payload.critical.identity.docker_reference).to_owned(),
        backed,
    })
}

/// The claims of the well-formed layers of the signature object in `dir`, each marked with
/// whether one of `keys` backs it: every key against every layer.
///
/// # Errors
/// The manifest is missing, oversized or not JSON with a `layers` list.
pub fn claims(dir: &Path, keys: &[VerifyingKey]) -> Result<Vec<Claim>, Error> {
    let manifest = read_capped(&dir.join("manifest.json"), MAX_MANIFEST).ok_or(Error::Unreadable)?;
    let manifest: Manifest = serde_json::from_slice(&manifest).map_err(|_| Error::Malformed)?;
    Ok(manifest.layers.iter().filter_map(|layer| claim_of(dir, layer, keys)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vectors(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors").join(name)
    }

    fn key(file: &str) -> VerifyingKey {
        load_key(&std::fs::read_to_string(vectors(file)).expect("key file")).expect("a P-256 public key")
    }

    fn backed(object: &str, keys: &[VerifyingKey]) -> Vec<Claim> {
        claims(&vectors(object), keys).expect("readable object").into_iter().filter(|claim| claim.backed).collect()
    }

    #[test]
    fn what_containers_image_wrote_verifies_with_either_key_of_its_two_layers() {
        // Layer 0 was pushed with k2 and layer 1 with k1: a verifier that reads only
        // layers[0] fails with k1.
        for file in ["real/k1.pub", "real/k2.pub"] {
            let found = backed("real", &[key(file)]);
            assert_eq!(found.len(), 1, "{file}");
            assert_eq!(found[0].repository, "localhost:5000/spike/athanor-system");
            assert_eq!(found[0].manifest_digest, "sha256:08d9f3ab2f3fd065175df48841e6434914170493578a3f59d6f6fc5dcdb971f9");
        }
        assert_eq!(backed("real", &[key("real/k1.pub"), key("real/k2.pub")]).len(), 2);
    }

    #[test]
    fn a_key_that_signed_nothing_backs_nothing_but_the_claim_is_still_reported() {
        let all = claims(&vectors("real"), &[key("made/a.pub")]).expect("readable object");
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|claim| !claim.backed));
        assert!(backed("real", &[]).is_empty());
    }

    #[test]
    fn every_key_is_tried_against_every_layer() {
        let digest = std::fs::read_to_string(vectors("made/image-digest")).expect("digest").trim().to_owned();
        for file in ["made/a.pub", "made/b.pub"] {
            let found = backed("made/good", &[key("real/k1.pub"), key(file)]);
            assert_eq!(found.len(), 1, "{file}");
            assert_eq!(found[0], Claim { manifest_digest: digest.clone(), repository: "registry.example/owner/athanor-system".into(), backed: true });
        }
    }

    #[test]
    fn the_signed_bytes_are_the_stored_blob_not_a_reserialised_value() {
        assert!(backed("made/reserialised", &[key("made/a.pub")]).is_empty());
    }

    #[test]
    fn a_blob_that_does_not_match_its_descriptor_is_no_claim_at_all() {
        assert!(claims(&vectors("made/descriptor-mismatch"), &[key("made/a.pub")]).expect("readable").is_empty());
    }

    #[test]
    fn a_raw_signature_is_refused_because_only_der_is_read() {
        assert!(claims(&vectors("made/raw-signature"), &[key("made/a.pub")]).expect("readable").is_empty());
    }

    #[test]
    fn another_type_or_an_unknown_critical_member_is_refused_even_when_signed() {
        for object in ["made/wrong-type", "made/extra-field"] {
            assert!(claims(&vectors(object), &[key("made/a.pub")]).expect("readable").is_empty(), "{object}");
        }
    }

    #[test]
    fn a_signature_made_for_another_repository_says_so() {
        let found = backed("made/other-repo", &[key("made/a.pub")]);
        assert_eq!(found[0].repository, "registry.example/owner/athanor-system-nvidia");
    }

    #[test]
    fn a_cosign_3_bundle_is_not_a_signature_object() {
        assert_eq!(claims(&vectors("made/bundle"), &[key("made/a.pub")]), Err(Error::Malformed));
        assert_eq!(claims(&vectors("made/absent"), &[]), Err(Error::Unreadable));
    }

    #[test]
    fn the_repository_is_the_reference_without_its_tag_or_digest() {
        assert_eq!(repository_of("localhost:5000/a/b:v1"), "localhost:5000/a/b");
        assert_eq!(repository_of("localhost:5000/a/b"), "localhost:5000/a/b");
        assert_eq!(repository_of("registry.example/o/n@sha256:abc"), "registry.example/o/n");
        assert_eq!(repository_of("registry.example/o/n:1@sha256:abc"), "registry.example/o/n");
    }
}
