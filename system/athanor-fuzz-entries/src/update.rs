use base64::Engine as _;
use p256::ecdsa::VerifyingKey;
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::Setup;

fn keys() -> &'static [VerifyingKey] {
    static KEYS: OnceLock<Vec<VerifyingKey>> = OnceLock::new();
    KEYS.get_or_init(|| {
        [
            include_str!("../../../forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/real/k1.pub"),
            include_str!("../../../forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/made/a.pub"),
        ]
        .iter()
        .filter_map(|pem| crate::sigobj::load_key(pem))
        .collect()
    })
}

/// `crate::sigobj::claims` over a signature object built from `data`. The first byte picks the shape:
/// even, the rest is `manifest.json` as is, with no blobs; odd, the rest is one length byte,
/// that many bytes of signature, then the payload, laid out as a well-formed single-layer
/// object (descriptor digest, size and media type right), so the fuzzer reaches the
/// signature and payload checks instead of stopping at the digest.
pub fn claims(data: &[u8]) -> Setup {
    if keys().len() != 2 {
        return Err(std::io::Error::other("the P-256 keys of the test vectors do not load"));
    }
    let Some((&shape, rest)) = data.split_first() else {
        return Ok(());
    };
    let dir = std::env::temp_dir().join(format!("athanor-fuzz-claims-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let manifest = dir.join("manifest.json");
    let mut blob: Option<PathBuf> = None;
    if shape % 2 == 0 {
        std::fs::write(&manifest, rest)?;
    } else {
        let Some((&len, rest)) = rest.split_first() else {
            return Ok(());
        };
        let (signature, payload) = rest.split_at(usize::from(len).min(rest.len()));
        let digest = hex::encode(Sha256::digest(payload));
        let manifest_json = serde_json::json!({
            "schemaVersion": 2,
            "layers": [{
                "mediaType": crate::sigobj::LAYER_MEDIA_TYPE,
                "digest": format!("sha256:{digest}"),
                "size": payload.len(),
                "annotations": {
                    crate::sigobj::SIGNATURE_ANNOTATION:
                        base64::engine::general_purpose::STANDARD.encode(signature),
                },
            }],
        });
        std::fs::write(&manifest, manifest_json.to_string())?;
        let path = dir.join(&digest);
        std::fs::write(&path, payload)?;
        blob = Some(path);
    }
    let _ = crate::sigobj::claims(&dir, keys());
    match blob {
        Some(path) => std::fs::remove_file(path),
        None => Ok(()),
    }
}

/// `crate::tools::parse_status`, with the incompatible-deployment fallback answering both ways.
pub fn status(data: &[u8]) -> Setup {
    let Ok(json) = std::str::from_utf8(data) else {
        return Ok(());
    };
    let _ = crate::tools::parse_status(json, || None);
    let _ = crate::tools::parse_status(json, || crate::tools::parse_local(json));
    Ok(())
}

/// `crate::tools::parse_local`.
pub fn local(data: &[u8]) -> Setup {
    if let Ok(json) = std::str::from_utf8(data) {
        let _ = crate::tools::parse_local(json);
    }
    Ok(())
}
