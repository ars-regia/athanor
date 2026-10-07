use anyhow::{anyhow, Context, Result};
use log::info;
use nix::ioctl_readwrite_bad;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub const TDX_GUEST_DEVICE: &str = "/dev/tdx_guest";
pub const TDX_ALT_DEVICE: &str = "/dev/tdx-guest";

// Intel TDX ioctl magic 'T', command index 1 (TDX_CMD_GET_REPORT0)
const TDX_GET_REPORT_MAGIC: u8 = b'T';
const TDX_GET_REPORT_CMD: u8 = 1;

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct TdReportReq {
    pub reportdata: [u8; 64],
    pub tdreport: [u8; 1024],
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct TdInfo {
    pub attributes: [u8; 8],
    pub xfam: [u8; 8],
    pub mrtd: [u8; 48],
    pub mrconfigid: [u8; 48],
    pub mrowner: [u8; 48],
    pub mrownerconfig: [u8; 48],
    pub rtmr0: [u8; 48],
    pub rtmr1: [u8; 48],
    pub rtmr2: [u8; 48],
    pub rtmr3: [u8; 48],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct TdReport {
    pub report_mac_struct: [u8; 256],
    pub tee_tcb_info: [u8; 238],
    pub reserved: [u8; 18],
    pub td_info: TdInfo,
}

// Define the bad ioctl macro for TDX_CMD_GET_REPORT0
ioctl_readwrite_bad!(
    tdx_get_report_ioctl,
    nix::request_code_readwrite!(TDX_GET_REPORT_MAGIC, TDX_GET_REPORT_CMD, std::mem::size_of::<TdReportReq>()),
    TdReportReq
);

pub fn is_tdx_available() -> bool {
    Path::new(TDX_GUEST_DEVICE).exists() || Path::new(TDX_ALT_DEVICE).exists()
}

pub fn get_tdx_report(nonce: &[u8; 64]) -> Result<TdReport> {
    let dev_path = if Path::new(TDX_GUEST_DEVICE).exists() {
        TDX_GUEST_DEVICE
    } else if Path::new(TDX_ALT_DEVICE).exists() {
        TDX_ALT_DEVICE
    } else {
        return Err(anyhow!("Intel TDX hardware device node (/dev/tdx_guest or /dev/tdx-guest) not found"));
    };

    info!("Opening Intel TDX hardware device at {}", dev_path);
    let file = File::open(dev_path)
        .with_context(|| format!("Failed to open Intel TDX device {}", dev_path))?;
    let fd = file.as_raw_fd();

    let mut req = TdReportReq {
        reportdata: *nonce,
        tdreport: [0u8; 1024],
    };

    info!("Issuing TDX_CMD_GET_REPORT0 ioctl to hardware...");
    // SAFETY: FFI call to C library or raw pointer dereference is bounds-checked and validated according to enclave specifications.
    unsafe {
        tdx_get_report_ioctl(fd, &mut req)
            .map_err(|e| anyhow!("TDX_CMD_GET_REPORT0 ioctl failed: {}", e))?;
    }

    let report_ptr = req.tdreport.as_ptr() as *const TdReport;
    // SAFETY: FFI call to C library or raw pointer dereference is bounds-checked and validated according to enclave specifications.
    let report = unsafe { *report_ptr };

    info!("Successfully extracted Intel TDX hardware report!");
    info!("TDX MRTD (SHA-384): {}", hex::encode(report.td_info.mrtd));

    Ok(report)
}
