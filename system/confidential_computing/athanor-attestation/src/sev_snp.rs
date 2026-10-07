use anyhow::{anyhow, Context, Result};
use log::info;
use nix::ioctl_readwrite_bad;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub const SEV_GUEST_DEVICE: &str = "/dev/sev-guest";
pub const SEV_ALT_DEVICE: &str = "/dev/sev";

// AMD SEV-SNP ioctl magic 'S', command index 0x0
// SNP_GET_REPORT = _IOWR('S', 0x0, struct snp_guest_request_ioctl)
const SNP_GET_REPORT_MAGIC: u8 = b'S';
const SNP_GET_REPORT_CMD: u8 = 0x00;

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct SnpReportReq {
    pub user_data: [u8; 64],
    pub vmpl: u32,
    pub rsvd: [u8; 28],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct SnpReportResp {
    pub status: u32,
    pub report_size: u32,
    pub reserved: [u8; 24],
    pub data: [u8; 4000],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct SnpGuestRequestIoctl {
    pub msg_version: u8,
    pub req_data: u64,
    pub resp_data: u64,
    pub exitinfo2: u64,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct SnpAttestationReport {
    pub version: u32,
    pub guest_svn: u32,
    pub policy: u64,
    pub family_id: [u8; 16],
    pub image_id: [u8; 16],
    pub vmpl: u32,
    pub signature_algo: u32,
    pub platform_version: u64,
    pub platform_info: u64,
    pub flags: u32,
    pub reserved0: u32,
    pub report_data: [u8; 64],
    pub measurement: [u8; 48],
    pub host_data: [u8; 32],
    pub id_key_digest: [u8; 48],
    pub author_key_digest: [u8; 48],
    pub report_id: [u8; 32],
    pub report_id_ma: [u8; 32],
    pub cpuid_fam: u32,
    pub cpuid_mod: u32,
    pub cpuid_step: u32,
    pub reserved1: u32,
    pub chip_id: [u8; 64],
    pub committed_svn: u64,
    pub committed_version: u64,
    pub launch_mitigation_vector: u64,
    pub reserved2: [u8; 168],
    pub signature: [u8; 512],
}

// Define the bad ioctl macro for SNP_GET_REPORT
ioctl_readwrite_bad!(
    snp_get_report_ioctl,
    nix::request_code_readwrite!(SNP_GET_REPORT_MAGIC, SNP_GET_REPORT_CMD, std::mem::size_of::<SnpGuestRequestIoctl>()),
    SnpGuestRequestIoctl
);

pub fn is_sev_snp_available() -> bool {
    Path::new(SEV_GUEST_DEVICE).exists() || Path::new(SEV_ALT_DEVICE).exists()
}

pub fn get_sev_snp_report(nonce: &[u8; 64]) -> Result<SnpAttestationReport> {
    let dev_path = if Path::new(SEV_GUEST_DEVICE).exists() {
        SEV_GUEST_DEVICE
    } else if Path::new(SEV_ALT_DEVICE).exists() {
        SEV_ALT_DEVICE
    } else {
        return Err(anyhow!("AMD SEV hardware device node (/dev/sev-guest or /dev/sev) not found"));
    };

    info!("Opening AMD SEV-SNP hardware device at {}", dev_path);
    let file = File::open(dev_path)
        .with_context(|| format!("Failed to open AMD SEV-SNP device {}", dev_path))?;
    let fd = file.as_raw_fd();

    let mut req = SnpReportReq {
        user_data: *nonce,
        vmpl: 0,
        rsvd: [0u8; 28],
    };

    let mut resp = SnpReportResp {
        status: 0,
        report_size: 0,
        reserved: [0u8; 24],
        data: [0u8; 4000],
    };

    let mut ioctl_data = SnpGuestRequestIoctl {
        msg_version: 1,
        req_data: &mut req as *mut _ as u64,
        resp_data: &mut resp as *mut _ as u64,
        exitinfo2: 0,
    };

    info!("Issuing SNP_GET_REPORT ioctl to hardware...");
    // SAFETY: FFI call to C library or raw pointer dereference is bounds-checked and validated according to enclave specifications.
    unsafe {
        snp_get_report_ioctl(fd, &mut ioctl_data)
            .map_err(|e| anyhow!("SNP_GET_REPORT ioctl failed: {}", e))?;
    }

    let status = resp.status;
    let report_size = resp.report_size;

    if status != 0 {
        return Err(anyhow!("SEV-SNP firmware returned error status: {}", status));
    }

    if (report_size as usize) < std::mem::size_of::<SnpAttestationReport>() {
        return Err(anyhow!(
            "SEV-SNP report size too small: expected {}, got {}",
            std::mem::size_of::<SnpAttestationReport>(),
            report_size
        ));
    }

    let report_ptr = resp.data.as_ptr() as *const SnpAttestationReport;
    // SAFETY: FFI call to C library or raw pointer dereference is bounds-checked and validated according to enclave specifications.
    let report = unsafe { *report_ptr };

    let version = report.version;
    let vmpl = report.vmpl;

    info!("Successfully extracted AMD SEV-SNP hardware report!");
    info!("Report Version: {}, VMPL: {}", version, vmpl);
    info!("Measurement (SHA-384): {}", hex::encode(report.measurement));

    Ok(report)
}
