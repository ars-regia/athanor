
use anyhow::Result;
use log::{error, info};
use std::sync::Arc;

use athanor_cvm_attestation::config::AttestationConfig;
use athanor_cvm_attestation::cvm_manager::{run_cvm_dbus_service, CvmManager};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("============================================================");
    info!("Athanor OS Level 16 Confidential Virtual Machine (CVM) Manager");
    info!("Hardware Enclave Attestation Daemon (SEV-SNP / TDX / Keylime)");
    info!("============================================================");

    let config = AttestationConfig::load_or_default();

    let cvm_manager = Arc::new(CvmManager::new(config));

    // Connect and run CVM Manager on D-Bus interface org.athanor.AttestationAlarm
    if let Err(e) = run_cvm_dbus_service(cvm_manager.clone()).await {
        error!("FATAL: Confidential Computing CVM Manager D-Bus error: {}", e);
        error!("Halting boot process. Key release for /var/home REFUSED.");
        std::process::exit(1);
    }

    Ok(())
}
