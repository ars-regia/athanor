#![allow(unsafe_code)]

pub mod config;
pub mod cvm_manager;
pub mod key_release;
pub mod sev_snp;
pub mod tdx;
pub mod verifier;

use anyhow::{anyhow, Result};

pub fn generate_hardware_nonce() -> Result<[u8; 64]> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut nonce = [0u8; 64];
    let rng = SystemRandom::new();
    rng.fill(&mut nonce)
        .map_err(|_| anyhow!("Cryptographic RNG failed to generate attestation nonce"))?;
    Ok(nonce)
}


