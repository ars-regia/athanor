#![allow(clippy::all, warnings, unsafe_code)]

use anyhow::Context;
use aya::maps::{Array, HashMap};
use aya::programs::{Xdp, XdpMode};
use aya::{include_bytes_aligned, Ebpf};
use aya_log::EbpfLogger;
use clap::{Parser, ValueEnum};
use log::{info, warn};
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;
use tokio::signal;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum XdpAttachMode {
    Auto,
    Skb,
    Driver,
    Hw,
}

#[derive(Debug, Parser)]
#[command(author, version, about = "Athanor OS XDP Zero-Trust Firewall Loader", long_about = None)]
struct Opt {
    /// Network interface to attach the XDP program to
    #[clap(short, long, default_value = "eth0")]
    iface: String,

    /// XDP attachment mode (auto, skb, driver, hw)
    #[clap(short, long, value_enum, default_value = "auto")]
    mode: XdpAttachMode,

    /// List of IPv4 addresses to block
    #[clap(short, long)]
    block_ip: Vec<String>,

    /// List of destination ports to allow in zero-trust mode
    #[clap(short, long)]
    allow_port: Vec<u16>,

    /// Enable strict Zero-Trust mode (drop any traffic to ports not in allow-port list)
    #[clap(long)]
    zero_trust: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    // Bump memlock rlimit for eBPF map allocation on legacy kernel accounting
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: setrlimit is safe to call with RLIMIT_MEMLOCK to increase eBPF map memory limits
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        warn!("Failed to increase MEMLOCK rlimit, ret code: {}", ret);
    }

    #[cfg(debug_assertions)]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/ebpf-core"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/ebpf-core"
    ))?;

    // The logger owns the AYA_LOGS map: dropping it closes the map and breaks program loading.
    match EbpfLogger::init(&mut bpf) {
        Ok(logger) => {
            let mut logger = AsyncFd::with_interest(logger, Interest::READABLE)
                .context("Failed to register the eBPF logger with the runtime")?;
            tokio::spawn(async move {
                loop {
                    let mut guard = match logger.readable_mut().await {
                        Ok(guard) => guard,
                        Err(e) => {
                            warn!("eBPF logger stopped: {}", e);
                            break;
                        }
                    };
                    guard.get_inner_mut().flush();
                    guard.clear_ready();
                }
            });
        }
        Err(e) => warn!("Failed to initialize eBPF logger: {}", e),
    }

    // Initialize eBPF Firewall Maps
    if !opt.block_ip.is_empty() {
        let mut blocklist: HashMap<_, u32, u32> =
            HashMap::try_from(bpf.map_mut("BLOCKLIST_IPV4").context("BLOCKLIST_IPV4 map not found")?)?;

        for ip_str in &opt.block_ip {
            if let Ok(ip) = Ipv4Addr::from_str(ip_str) {
                let ip_u32 = u32::from_be_bytes(ip.octets());
                blocklist.insert(ip_u32, 1, 0)?;
                info!("Added IPv4 {} to XDP Firewall blocklist", ip_str);
            } else {
                warn!("Invalid IPv4 address provided for blocklist: {}", ip_str);
            }
        }
    }

    if !opt.allow_port.is_empty() {
        let mut allowed_ports: HashMap<_, u16, u32> =
            HashMap::try_from(bpf.map_mut("ALLOWED_PORTS").context("ALLOWED_PORTS map not found")?)?;

        for port in &opt.allow_port {
            allowed_ports.insert(*port, 1, 0)?;
            info!("Added Port {} to XDP Firewall allowed list", port);
        }
    }

    // Configure Zero-Trust Mode Flag
    let mut config_flags: Array<_, u32> =
        Array::try_from(bpf.map_mut("CONFIG_FLAGS").context("CONFIG_FLAGS map not found")?)?;
    let zero_trust_val: u32 = if opt.zero_trust { 1 } else { 0 };
    config_flags.set(0, zero_trust_val, 0)?;

    info!(
        "XDP Zero-Trust Firewall configured. Mode: {}, Zero-Trust: {}",
        if opt.zero_trust { "STRICT" } else { "PERMISSIVE" },
        opt.zero_trust
    );

    // Attach XDP Program
    let mode = match opt.mode {
        XdpAttachMode::Auto => XdpMode::Default,
        XdpAttachMode::Skb => XdpMode::Skb,
        XdpAttachMode::Driver => XdpMode::Driver,
        XdpAttachMode::Hw => XdpMode::Hardware,
    };

    let program: &mut Xdp = bpf
        .program_mut("xdp_firewall")
        .context("Program 'xdp_firewall' not found in eBPF object")?
        .try_into()?;
    program.load()?;
    program
        .attach(&opt.iface, mode)
        .context(format!("Failed to attach XDP firewall to interface {}", opt.iface))?;

    info!("XDP Zero-Trust Firewall attached to {}!", opt.iface);

    // Live Metrics Telemetry Loop
    let stats_map: Array<_, u64> =
        Array::try_from(bpf.map("FIREWALL_STATS").context("FIREWALL_STATS map not found")?)?;

    let mut interval = tokio::time::interval(Duration::from_secs(3));
    info!("XDP Zero-Trust Firewall active. Press Ctrl-C to detach and exit.");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let passed = stats_map.get(&0, 0).unwrap_or(0);
                let invalid_hdr = stats_map.get(&1, 0).unwrap_or(0);
                let land_attack = stats_map.get(&2, 0).unwrap_or(0);
                let anomalous_flags = stats_map.get(&3, 0).unwrap_or(0);
                let blocklist_ip = stats_map.get(&4, 0).unwrap_or(0);
                let unauthorized_port = stats_map.get(&5, 0).unwrap_or(0);

                info!(
                    "[XDP Telemetry] PASSED: {} | DROP (Hdr: {}, Land: {}, Scan: {}, BlockIP: {}, Port: {})",
                    passed, invalid_hdr, land_attack, anomalous_flags, blocklist_ip, unauthorized_port
                );
            }
            _ = signal::ctrl_c() => {
                info!("Received Ctrl-C. Detaching XDP Zero-Trust Firewall...");
                break;
            }
        }
    }

    Ok(())
}
