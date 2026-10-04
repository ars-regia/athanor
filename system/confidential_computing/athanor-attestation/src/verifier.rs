use anyhow::{anyhow, Context, Result};
use log::{error, info};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature as P256Signature, VerifyingKey as P256VerifyingKey};
use p384::ecdsa::{Signature as P384Signature, VerifyingKey as P384VerifyingKey};
use p384::pkcs8::DecodePublicKey;
use std::fs;

use crate::config::AttestationConfig;
use crate::sev_snp::SnpAttestationReport;
use crate::tdx::TdReport;

#[derive(Debug)]
pub enum VerifiedHardwareReport {
    SevSnp {
        measurement: [u8; 48],
        report_data: [u8; 64],
        policy: u64,
    },
    Tdx {
        mrtd: [u8; 48],
        report_data: [u8; 64],
        attributes: [u8; 8],
    },
}

pub struct AttestationVerifier {
    pub config: AttestationConfig,
}

impl AttestationVerifier {
    pub fn new(config: AttestationConfig) -> Self {
        Self { config }
    }

    /// Loads remote public key from file (PEM format)
    fn load_remote_pubkey_bytes(&self) -> Result<Vec<u8>> {
        let pubkey_path = &self.config.remote_pubkey_path;
        if !pubkey_path.exists() {
            return Err(anyhow!(
                "Remote public key file missing at {:?}. Attestation verification failed.",
                pubkey_path
            ));
        }

        let pem_str = fs::read_to_string(pubkey_path)
            .with_context(|| format!("Failed to read public key from {:?}", pubkey_path))?;

        info!("Loaded remote public key from {:?}", pubkey_path);
        Ok(pem_str.into_bytes())
    }

    /// Verifies AMD SEV-SNP report signature and measurement against remote public key
    pub fn verify_sev_snp_report(
        &self,
        report: &SnpAttestationReport,
        nonce: &[u8; 64],
    ) -> Result<VerifiedHardwareReport> {
        info!("Verifying AMD SEV-SNP hardware report cryptographically...");

        // 1. Verify nonce matching
        if report.report_data != *nonce {
            error!("Report Data mismatch! Hardware report nonce does not match requested challenge.");
            return Err(anyhow!("SEV-SNP report data mismatch"));
        }

        // 2. Validate expected launch measurement if configured
        if let Some(ref expected_hex) = self.config.expected_measurement_hex {
            let actual_hex = hex::encode(report.measurement);
            if !actual_hex.eq_ignore_ascii_case(expected_hex) {
                error!(
                    "Measurement mismatch! Hardware: {}, Expected: {}",
                    actual_hex, expected_hex
                );
                return Err(anyhow!("SEV-SNP measurement validation failed"));
            }
            info!("Hardware launch measurement matches expected hash!");
        }

        // 3. Signature verification against remote public key
        let pubkey_pem = self.load_remote_pubkey_bytes()?;
        self.verify_signature_p384_or_p256(&report.measurement, &report.signature[..144], &pubkey_pem)?;

        info!("AMD SEV-SNP Hardware Report verified successfully!");
        Ok(VerifiedHardwareReport::SevSnp {
            measurement: report.measurement,
            report_data: report.report_data,
            policy: report.policy,
        })
    }

    /// Verifies Intel TDX report and MRTD measurement against remote public key
    pub fn verify_tdx_report(
        &self,
        report: &TdReport,
        nonce: &[u8; 64],
    ) -> Result<VerifiedHardwareReport> {
        info!("Verifying Intel TDX hardware report cryptographically...");

        let mrtd = report.td_info.mrtd;
        let attributes = report.td_info.attributes;

        // Validate expected launch measurement if configured
        if let Some(ref expected_hex) = self.config.expected_measurement_hex {
            let actual_hex = hex::encode(mrtd);
            if !actual_hex.eq_ignore_ascii_case(expected_hex) {
                error!(
                    "TDX MRTD mismatch! Hardware: {}, Expected: {}",
                    actual_hex, expected_hex
                );
                return Err(anyhow!("TDX MRTD measurement validation failed"));
            }
            info!("Intel TDX MRTD matches expected hash!");
        }

        // Signature verification against remote public key
        let pubkey_pem = self.load_remote_pubkey_bytes()?;
        self.verify_signature_p384_or_p256(&mrtd, &report.report_mac_struct[..64], &pubkey_pem)?;

        info!("Intel TDX Hardware Report verified successfully!");
        Ok(VerifiedHardwareReport::Tdx {
            mrtd,
            report_data: *nonce,
            attributes,
        })
    }

    /// Generic signature verifier for ECDSA P-384 / P-256 against remote public key
    fn verify_signature_p384_or_p256(
        &self,
        message: &[u8],
        sig_bytes: &[u8],
        pubkey_pem: &[u8],
    ) -> Result<()> {
        let pem_str = std::str::from_utf8(pubkey_pem).context("Invalid UTF-8 in public key PEM")?;

        // 1. Attempt P-384 Verification (using PKCS#8 PEM or SEC1)
        if let Ok(vk) = P384VerifyingKey::from_public_key_pem(pem_str)
            .or_else(|_| P384VerifyingKey::from_sec1_bytes(pubkey_pem))
        {
            if let Ok(sig) = P384Signature::from_slice(sig_bytes) {
                if vk.verify(message, &sig).is_ok() {
                    info!("ECDSA P-384 signature verified successfully with remote public key.");
                    return Ok(());
                }
            }
        }

        // 2. Attempt P-256 Verification (using PKCS#8 PEM or SEC1)
        if let Ok(vk) = P256VerifyingKey::from_public_key_pem(pem_str)
            .or_else(|_| P256VerifyingKey::from_sec1_bytes(pubkey_pem))
        {
            if let Ok(sig) = P256Signature::from_slice(sig_bytes) {
                if vk.verify(message, &sig).is_ok() {
                    info!("ECDSA P-256 signature verified successfully with remote public key.");
                    return Ok(());
                }
            }
        }

        Err(anyhow!("Cryptographic verification failed: Signature does not match remote public key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p384::pkcs8::{EncodePublicKey, LineEnding};

    const MESSAGE: &[u8] = b"launch measurement";

    fn verifier() -> AttestationVerifier {
        AttestationVerifier::new(AttestationConfig::default())
    }

    fn p384_fixture() -> (Vec<u8>, String) {
        let key = p384::ecdsa::SigningKey::from_slice(&[0x17; 48]).unwrap();
        let signature: P384Signature = key.sign(MESSAGE);
        let pem = key.verifying_key().to_public_key_pem(LineEnding::LF).unwrap();
        (signature.to_bytes().to_vec(), pem)
    }

    fn p256_fixture() -> (Vec<u8>, String) {
        let key = p256::ecdsa::SigningKey::from_slice(&[0x29; 32]).unwrap();
        let signature: P256Signature = key.sign(MESSAGE);
        let pem = key.verifying_key().to_public_key_pem(LineEnding::LF).unwrap();
        (signature.to_bytes().to_vec(), pem)
    }

    #[test]
    fn a_p384_signature_verifies_against_its_pem_key() {
        let (signature, pem) = p384_fixture();
        verifier().verify_signature_p384_or_p256(MESSAGE, &signature, pem.as_bytes()).unwrap();
    }

    #[test]
    fn a_p256_signature_verifies_against_its_pem_key() {
        let (signature, pem) = p256_fixture();
        verifier().verify_signature_p384_or_p256(MESSAGE, &signature, pem.as_bytes()).unwrap();
    }

    #[test]
    fn a_signature_over_another_message_is_rejected() {
        for (signature, pem) in [p384_fixture(), p256_fixture()] {
            assert!(verifier()
                .verify_signature_p384_or_p256(b"another measurement", &signature, pem.as_bytes())
                .is_err());
        }
    }

    #[test]
    fn a_signature_is_rejected_against_a_key_of_the_other_curve() {
        let (p384_signature, p384_pem) = p384_fixture();
        let (p256_signature, p256_pem) = p256_fixture();
        assert!(verifier().verify_signature_p384_or_p256(MESSAGE, &p384_signature, p256_pem.as_bytes()).is_err());
        assert!(verifier().verify_signature_p384_or_p256(MESSAGE, &p256_signature, p384_pem.as_bytes()).is_err());
    }
}
