use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationConfig {
    /// Path to the remote public key file (PEM format) used to verify hardware signatures
    pub remote_pubkey_path: PathBuf,
    /// Path where the decrypted / released key for /var/home will be stored securely (in tmpfs /run)
    pub key_output_path: PathBuf,
    /// Strict Zero-Trust mode: if true, requires physical CVM hardware (/dev/sev-guest or /dev/tdx_guest)
    pub strict_zero_trust: bool,
    /// Expected SHA-384 launch measurement (hex encoded, optional)
    pub expected_measurement_hex: Option<String>,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            remote_pubkey_path: PathBuf::from("/etc/athanor/attestation/remote_pubkey.pem"),
            key_output_path: PathBuf::from("/run/athanor/var_home.key"),
            strict_zero_trust: true,
            expected_measurement_hex: None,
        }
    }
}

impl AttestationConfig {
    pub fn load_or_default() -> Self {
        let env_path = std::env::var("ATHANOR_ATTESTATION_CONFIG").ok().map(PathBuf::from);
        let config_path = env_path.unwrap_or_else(|| PathBuf::from("/etc/athanor/attestation/config.json"));

        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(cfg) = serde_json::from_str::<AttestationConfig>(&content) {
                    return cfg;
                }
            }
        }
        Self::default()
    }
}
