use std::io::Write;
use anyhow::{Context, Result};
use log::{error, info};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::verifier::VerifiedHardwareReport;

/// Secure memory container for the sensitive decryption key.
/// ZeroizeOnDrop guarantees the key bytes in RAM are wiped clean when dropped.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretDecryptionKey {
    pub key_data: [u8; 32], // 256-bit AES-GCM / LUKS decryption key
}

impl SecretDecryptionKey {
    pub fn new(raw: [u8; 32]) -> Self {
        Self { key_data: raw }
    }
}

pub struct KeyReleaseManager {
    output_path: std::path::PathBuf,
}

impl KeyReleaseManager {
    pub fn new(output_path: std::path::PathBuf) -> Self {
        Self { output_path }
    }

    /// Releases the decryption key for /var/home ONLY after successful hardware attestation verification
    pub fn release_var_home_key(&self, report: &VerifiedHardwareReport) -> Result<()> {
        info!("============================================================");
        info!("HARDWARE ATTESTATION SUCCESSFUL! Proceeding with Key Release.");
        info!("============================================================");

        let (ikm, _info_label): (&[u8], &[u8]) = match report {
            VerifiedHardwareReport::SevSnp { measurement, .. } => {
                info!("Hardware Tier: AMD SEV-SNP CVM");
                info!("Attestor Measurement: {}", hex::encode(measurement));
                (&measurement[..], b"athanor-sev-snp-luks-v1")
            }
            VerifiedHardwareReport::Tdx { mrtd, .. } => {
                info!("Hardware Tier: Intel TDX CVM");
                info!("Attestor MRTD: {}", hex::encode(mrtd));
                (&mrtd[..], b"athanor-tdx-luks-v1")
            }

        };

        
        // VITREOL: Never derive secrets from public measurements. 
        // We strictly invoke hardware-backed unsealing passing the measurement policy.
        let enclave_tool = match report {
            VerifiedHardwareReport::SevSnp { .. } => "sev-guest-unseal",
            VerifiedHardwareReport::Tdx { .. } => "tdx-guest-unseal",
        };

        info!("Invoking strict hardware unseal via {}", enclave_tool);
        let output = std::process::Command::new(enclave_tool)
            .arg("--pcr-policy")
            .arg(hex::encode(ikm))
            .output()
            .map_err(|e| anyhow::anyhow!("Hardware unseal binary missing or failed: {}", e))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Hardware unsealing cryptographically rejected by enclave!"));
        }

        if output.stdout.len() < 32 {
            return Err(anyhow::anyhow!("Hardware returned malformed key length"));
        }

        let mut key_buffer = [0u8; 32];
        key_buffer.copy_from_slice(&output.stdout[..32]);
        let secret_key = SecretDecryptionKey::new(key_buffer);
        key_buffer.zeroize();
        let mut raw_stdout = output.stdout;
        raw_stdout.zeroize();


        // Ensure parent directory /run/athanor exists
        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {:?}", parent))?;
        }

        info!("Writing decryption key securely to {:?}", self.output_path);

        // Create key file with strict Unix permissions: 0400 (Read-only by owner / root)
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o400)
            .open(&self.output_path)
            .with_context(|| format!("Failed to create secure key file at {:?}", self.output_path))?;

        file.write_all(&secret_key.key_data)
            .with_context(|| "Failed to write key data to output file")?;
        file.flush()?;

        info!("Successfully released decryption key for /var/home at {:?}", self.output_path);
        info!("Memory buffers scrubbed (Zeroize active). Zero-Trust hardware release COMPLETE.");

        Ok(())
    }

    /// Revokes key release and sanitizes any existing key files on attestation failure
    pub fn revoke_and_purge(&self) {
        error!("PERFORMING SECURITY PURGE: Revoking key release for /var/home.");
        if Path::new(&self.output_path).exists() {
            if let Err(e) = fs::remove_file(&self.output_path) {
                error!("Failed to remove key file at {:?}: {}", self.output_path, e);
            } else {
                info!("Purged existing key file at {:?}", self.output_path);
            }
        }
    }
}


