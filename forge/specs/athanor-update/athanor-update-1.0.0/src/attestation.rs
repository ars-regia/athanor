//! The release attestation of an image, as `skopeo copy … dir:` stores
//! `<repo>:sha256-<hex>.att` (docs/architecture/doc_update_trust.md, UT13; ADR-0127 points 1
//! to 4). Each layer is a DSSE envelope of an in-toto Statement v1, and an object of more than
//! `MAX_LAYERS` layers is not read. An envelope counts only when
//! its signature, base64 ASN.1 DER ECDSA P-256 over the DSSE pre-authentication encoding,
//! verifies with a key of the policy in force, and its payload type is the in-toto one. The
//! image is of the security class only when a counted envelope with the release predicate
//! type names the image alone as its subject and says `security`, and no counted envelope for
//! that subject says otherwise. Everything else, an error included, is the feature class.
//! Nothing is cached: every call reads and verifies again.
use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// The predicate type of the release attestation (ADR-0127, point 1).
pub const PREDICATE_TYPE: &str =
    "https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b";
pub const LAYER_MEDIA_TYPE: &str = "application/vnd.dsse.envelope.v1+json";
pub const PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
/// The in-toto statement type of ADR-0127, point 2.
pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
const MAX_MANIFEST: u64 = 1 << 20;
const MAX_PAYLOAD: u64 = 64 << 10;
/// Layers past this many make the object of the feature class unread.
const MAX_LAYERS: usize = 64;
/// Bounds of the advisory ids a security verdict carries: ADR-0127 gives them no format.
const MAX_ADVISORIES: usize = 1024;
const MAX_ADVISORY_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// The advisories of every envelope that says `security`, sorted and unique.
    Security {
        advisories: Vec<String>,
    },
    Feature,
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
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "payloadType")]
    payload_type: String,
    payload: String,
    signatures: Vec<EnvelopeSignature>,
}

#[derive(Deserialize)]
struct EnvelopeSignature {
    sig: String,
}

/// Read twice: first with `P = IgnoredAny` to learn what the statement is about, then with
/// `P = Predicate`. Typed members refuse a duplicate key, where a `serde_json::Value` keeps
/// the last one silently.
#[derive(Deserialize)]
struct Statement<P> {
    #[serde(rename = "_type")]
    kind: String,
    subject: Vec<Subject>,
    #[serde(rename = "predicateType")]
    predicate_type: String,
    predicate: P,
}

#[derive(Deserialize)]
struct Subject {
    name: String,
    digest: SubjectDigest,
}

#[derive(Deserialize)]
struct SubjectDigest {
    sha256: String,
}

#[derive(Deserialize)]
struct Predicate {
    class: String,
    advisories: Vec<String>,
}

/// The DSSE pre-authentication encoding: `"DSSEv1" SP len(type) SP type SP len(body) SP body`.
fn pae(payload_type: &str, body: &[u8]) -> Vec<u8> {
    let mut encoding = format!(
        "DSSEv1 {} {payload_type} {} ",
        payload_type.len(),
        body.len()
    )
    .into_bytes();
    encoding.extend_from_slice(body);
    encoding
}

fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > cap {
        return None;
    }
    std::fs::read(path).ok()
}

fn sha256_hex(digest: &str) -> Option<&str> {
    digest.strip_prefix("sha256:").filter(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The statement of one layer, when a key of `keys` signed its envelope; `None` for any layer
/// that is not a well-formed, verified in-toto envelope, which is then not read further.
fn verified_statement(dir: &Path, layer: &Descriptor, keys: &[VerifyingKey]) -> Option<Vec<u8>> {
    if layer.media_type != LAYER_MEDIA_TYPE || layer.size > MAX_PAYLOAD {
        return None;
    }
    let hex_digest = sha256_hex(&layer.digest)?;
    let blob = read_capped(&dir.join(hex_digest), MAX_PAYLOAD)?;
    if blob.len() as u64 != layer.size || hex::encode(Sha256::digest(&blob)) != hex_digest {
        return None;
    }
    let envelope: Envelope = serde_json::from_slice(&blob).ok()?;
    if envelope.payload_type != PAYLOAD_TYPE {
        return None;
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    let body = base64.decode(&envelope.payload).ok()?;
    let encoding = pae(&envelope.payload_type, &body);
    let mut signatures = envelope
        .signatures
        .iter()
        .filter_map(|signature| base64.decode(&signature.sig).ok())
        .filter_map(|der| Signature::from_der(&der).ok());
    signatures
        .any(|signature| {
            keys.iter()
                .any(|key| key.verify(&encoding, &signature).is_ok())
        })
        .then_some(body)
}

/// What a verified statement says of `(repository, hex)`: `None` when it is about something
/// else; the feature class when it is about the image and cannot be read as a release
/// statement of the image alone.
fn verdict(statement: &[u8], repository: &str, hex: &str) -> Option<Class> {
    let Ok(about) = serde_json::from_slice::<Statement<serde::de::IgnoredAny>>(statement) else {
        // Signed by a key of the policy, about an image nobody can tell: it may be this one.
        return Some(Class::Feature);
    };
    if about.predicate_type != PREDICATE_TYPE {
        return None;
    }
    if !about.subject.iter().any(|subject| subject.name == repository && subject.digest.sha256 == hex) {
        return None;
    }
    if about.kind != STATEMENT_TYPE || about.subject.len() != 1 {
        return Some(Class::Feature);
    }
    // doc_update_trust.md item 18: the security class comes "with advisory ids", so a
    // security statement that names none is of the feature class.
    let within_bounds = |advisories: &[String]| !advisories.is_empty() && advisories.len() <= MAX_ADVISORIES && advisories.iter().all(|id| !id.is_empty() && id.len() <= MAX_ADVISORY_LEN);
    match serde_json::from_slice::<Statement<Predicate>>(statement) {
        Ok(Statement { predicate, .. }) if predicate.class == "security" && within_bounds(&predicate.advisories) => Some(Class::Security { advisories: predicate.advisories }),
        _ => Some(Class::Feature),
    }
}

/// The class of `digest` in `repository` by the attestation object in `dir`, verified with
/// `keys`, the keys the policy in force names for the repository.
#[cfg_attr(not(test), allow(dead_code))] // the check classifies its candidate with it from UT13's classification on
pub fn class_of(dir: &Path, keys: &[VerifyingKey], repository: &str, digest: &str) -> Class {
    let Some(hex) = sha256_hex(digest) else {
        return Class::Feature;
    };
    let Some(manifest) = read_capped(&dir.join("manifest.json"), MAX_MANIFEST) else {
        return Class::Feature;
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&manifest) else {
        return Class::Feature;
    };
    if manifest.layers.len() > MAX_LAYERS {
        return Class::Feature;
    }
    let mut advisories = BTreeSet::new();
    let mut security = false;
    for statement in manifest
        .layers
        .iter()
        .filter_map(|layer| verified_statement(dir, layer, keys))
    {
        match verdict(&statement, repository, hex) {
            Some(Class::Security { advisories: said }) => {
                security = true;
                advisories.extend(said);
            }
            Some(Class::Feature) => return Class::Feature,
            None => {}
        }
    }
    if security && advisories.len() <= MAX_ADVISORIES {
        Class::Security {
            advisories: advisories.into_iter().collect(),
        }
    } else {
        Class::Feature
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sigobj::load_key;
    use std::path::PathBuf;

    const REPO: &str = "registry.example/owner/athanor-system";

    fn vectors(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/made")
            .join(name)
    }

    fn key(name: &str) -> VerifyingKey {
        load_key(&std::fs::read_to_string(vectors(&format!("{name}.pub"))).expect("key file"))
            .expect("a P-256 public key")
    }

    fn digest() -> String {
        std::fs::read_to_string(vectors("image-digest"))
            .expect("digest")
            .trim()
            .to_owned()
    }

    fn class(object: &str, keys: &[VerifyingKey]) -> Class {
        class_of(&vectors(object), keys, REPO, &digest())
    }

    #[test]
    fn a_signed_security_statement_is_security() {
        assert_eq!(
            class("att-security", &[key("a")]),
            Class::Security {
                advisories: vec!["CVE-2026-0001".into()]
            }
        );
        assert_eq!(
            class("att-security", &[key("b"), key("a")]),
            Class::Security {
                advisories: vec!["CVE-2026-0001".into()]
            },
            "every key is tried"
        );
        assert_eq!(class("att-feature", &[key("a")]), Class::Feature);
    }

    #[test]
    fn an_invalid_signature_is_feature() {
        for object in [
            "att-bad-signature",
            "att-malformed-der",
            "att-truncated-pae",
            "att-tampered-payload",
        ] {
            assert_eq!(class(object, &[key("a")]), Class::Feature, "{object}");
        }
    }

    #[test]
    fn a_statement_without_the_class_field_is_feature() {
        assert_eq!(class("att-no-class", &[key("a")]), Class::Feature);
    }

    #[test]
    fn a_statement_for_another_digest_or_repository_is_feature() {
        for object in ["att-other-digest", "att-other-repo"] {
            assert_eq!(class(object, &[key("a")]), Class::Feature, "{object}");
        }
        assert_eq!(
            class_of(
                &vectors("att-security"),
                &[key("a")],
                &format!("{REPO}-nvidia"),
                &digest()
            ),
            Class::Feature
        );
        assert_eq!(
            class_of(
                &vectors("att-security"),
                &[key("a")],
                REPO,
                "sha256:../../etc"
            ),
            Class::Feature
        );
    }

    #[test]
    fn another_predicate_type_is_feature() {
        assert_eq!(class("att-wrong-type", &[key("a")]), Class::Feature);
    }

    #[test]
    fn another_payload_type_is_feature_even_when_signed() {
        assert_eq!(class("att-wrong-payload-type", &[key("a")]), Class::Feature);
    }

    #[test]
    fn a_key_outside_the_policy_is_feature() {
        assert_eq!(class("att-unknown-key", &[key("a")]), Class::Feature);
        assert_eq!(class("att-security", &[]), Class::Feature);
    }

    #[test]
    fn a_missing_directory_is_feature() {
        assert_eq!(class("att-absent", &[key("a")]), Class::Feature);
        assert_eq!(
            class("bundle", &[key("a")]),
            Class::Feature,
            "not an attestation manifest"
        );
        assert_eq!(
            class("good", &[key("a")]),
            Class::Feature,
            "a signature object has no envelope"
        );
    }

    #[test]
    fn envelopes_that_disagree_are_feature() {
        assert_eq!(class("att-mixed", &[key("a"), key("b")]), Class::Feature);
        assert_eq!(class("att-mixed", &[key("a")]), Class::Feature);
        assert_eq!(
            class("att-mixed", &[key("b")]),
            Class::Security {
                advisories: vec!["CVE-2026-0001".into()]
            },
            "only verified envelopes count"
        );
    }

    #[test]
    fn an_oversized_layer_is_skipped_unread() {
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vectors("att-oversized/manifest.json")).expect("manifest"),
        )
        .expect("json");
        assert!(manifest["layers"][0]["size"].as_u64().expect("size") > MAX_PAYLOAD);
        assert_eq!(class("att-oversized", &[key("a")]), Class::Feature);
    }

    #[test]
    fn the_pre_authentication_encoding_is_the_dsse_one() {
        assert_eq!(
            pae(PAYLOAD_TYPE, b"{}"),
            b"DSSEv1 28 application/vnd.in-toto+json 2 {}".to_vec()
        );
        assert_eq!(pae("", b""), b"DSSEv1 0  0 ".to_vec());
    }

    /// A copy of `object` whose manifest lists its first layer `count` times.
    fn repeated(object: &str, count: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "athanor-update-att-{}-{object}-{count}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clean");
        }
        std::fs::create_dir_all(&dir).expect("mkdir");
        for entry in std::fs::read_dir(vectors(object)).expect("vectors") {
            let entry = entry.expect("entry");
            std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy");
        }
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).expect("manifest"))
                .expect("json");
        manifest["layers"] = serde_json::Value::Array(vec![manifest["layers"][0].clone(); count]);
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).expect("write");
        dir
    }

    #[test]
    fn an_object_of_more_than_max_layers_is_feature() {
        let security = Class::Security {
            advisories: vec!["CVE-2026-0001".into()],
        };
        assert_eq!(
            class_of(
                &repeated("att-security", MAX_LAYERS),
                &[key("a")],
                REPO,
                &digest()
            ),
            security
        );
        assert_eq!(
            class_of(
                &repeated("att-security", MAX_LAYERS + 1),
                &[key("a")],
                REPO,
                &digest()
            ),
            Class::Feature
        );
    }

    #[test]
    fn a_statement_type_other_than_in_toto_v1_is_feature() {
        assert_eq!(class("att-statement-v01", &[key("a")]), Class::Feature);
    }

    #[test]
    fn advisories_out_of_bounds_are_feature() {
        assert_eq!(class("att-long-advisory", &[key("a")]), Class::Feature);
        assert_eq!(class("att-many-advisories", &[key("a")]), Class::Feature);
    }

    #[test]
    fn a_security_statement_with_no_advisory_is_feature() {
        // doc_update_trust.md item 18: security class only "with advisory ids".
        assert_eq!(
            class("att-empty-advisories", &[key("a")]),
            Class::Feature
        );
    }

    #[test]
    fn a_statement_with_two_subjects_is_feature_even_when_one_matches() {
        assert_eq!(class("att-two-subjects", &[key("a")]), Class::Feature);
    }

    #[test]
    fn a_duplicate_class_member_is_feature() {
        assert_eq!(class("att-duplicate-class", &[key("a")]), Class::Feature);
    }

    #[test]
    fn base64_that_is_not_canonical_is_feature() {
        for object in ["att-unpadded", "att-trailing-bits"] {
            assert_eq!(class(object, &[key("a")]), Class::Feature, "{object}");
        }
    }

    #[test]
    fn a_verified_statement_of_another_predicate_type_does_not_count_either_way() {
        let security = Class::Security {
            advisories: vec!["CVE-2026-0001".into()],
        };
        assert_eq!(class("att-foreign-feature", &[key("a")]), security);
        assert_eq!(class("att-foreign-security", &[key("a")]), Class::Feature);
    }
}
