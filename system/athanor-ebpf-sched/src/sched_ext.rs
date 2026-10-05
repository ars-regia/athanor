#![allow(unsafe_code)]
use aya::maps::HashMap as BpfHashMap;
use aya::programs::Extension;
use aya::Ebpf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[repr(C)]
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct AiSchedParam {
    pub pid: u32,
    pub target_core: u32,       // Core ID assigned by AI Predictor DAG (P-Core vs E-Core)
    pub core_type: u8,          // 0 = P-Core, 1 = E-Core, 2 = NPU-Core
    pub _pad: [u8; 3],          // Padding for 4-byte alignment
    pub cpu_weight: u32,
    pub slice_us: u64,
    pub sched_class: u32,       // 0: RealtimeNpu, 1: InteractiveUi, 2: BatchCompute, 3: IdleBackground
    pub latency_target_us: u64,
    pub flags: u32,
}

unsafe impl aya::Pod for AiSchedParam {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SchedClass {
    RealtimeNpu = 0,     // Ultra-low latency NPU/AI tasks
    InteractiveUi = 1,   // Compositor / UI frame tasks
    BatchCompute = 2,    // Compilation / Heavy background compute
    IdleBackground = 3,  // Low priority background tasks
}

impl From<u32> for SchedClass {
    fn from(val: u32) -> Self {
        match val {
            0 => SchedClass::RealtimeNpu,
            1 => SchedClass::InteractiveUi,
            2 => SchedClass::BatchCompute,
            _ => SchedClass::IdleBackground,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskSchedPolicy {
    pub pid: u32,
    pub class: SchedClass,
    pub cpu_weight: u32,       // 1 to 10000 (cgroup v2 weight)
    pub slice_us: u64,         // Scheduling time slice in microseconds
    pub latency_target_us: u64,// Zero-latency target requirement
}

/// Safe thread-safe interface exposing `AI_SCHED_MAP` for daemons and scheduler controllers
#[derive(Clone)]
pub struct AiSchedMap {
    ebpf: Arc<Mutex<Ebpf>>,
}

impl AiSchedMap {
    pub fn new(ebpf: Arc<Mutex<Ebpf>>) -> Self {
        Self { ebpf }
    }

    pub async fn update_policy(&self, pid: u32, value: AiSchedParam) -> anyhow::Result<()> {
        let mut ebpf = self.ebpf.lock().await;
        if let Some(map) = ebpf.map_mut("AI_SCHED_MAP") {
            let mut bpf_map = BpfHashMap::<_, u32, AiSchedParam>::try_from(map)
                .map_err(|e| anyhow::anyhow!("Failed to cast AI_SCHED_MAP: {}", e))?;
            bpf_map.insert(pid, value, 0)
                .map_err(|e| anyhow::anyhow!("Failed to insert PID {} into eBPF AI_SCHED_MAP: {}", pid, e))?;
            info!("⚡ [eBPF Map] AI_SCHED_MAP updated for PID {} -> weight={}, slice={}us", pid, value.cpu_weight, value.slice_us);
            Ok(())
        } else {
            Err(anyhow::anyhow!("AI_SCHED_MAP not found in eBPF"))
        }
    }

    pub async fn is_bpf_active(&self) -> bool {
        let ebpf = self.ebpf.lock().await;
        ebpf.map("AI_SCHED_MAP").is_some()
    }

    pub async fn get_policy(&self, pid: u32) -> anyhow::Result<Option<AiSchedParam>> {
        let mut ebpf = self.ebpf.lock().await;
        if let Some(map) = ebpf.map_mut("AI_SCHED_MAP") {
            let bpf_map = BpfHashMap::<_, u32, AiSchedParam>::try_from(map)
                .map_err(|e| anyhow::anyhow!("Failed to cast AI_SCHED_MAP: {}", e))?;
            Ok(bpf_map.get(&pid, 0).ok())
        } else {
            Err(anyhow::anyhow!("AI_SCHED_MAP not found in eBPF"))
        }
    }

    pub async fn remove_policy(&self, pid: u32) -> anyhow::Result<()> {
        let mut ebpf = self.ebpf.lock().await;
        if let Some(map) = ebpf.map_mut("AI_SCHED_MAP") {
            let mut bpf_map = BpfHashMap::<_, u32, AiSchedParam>::try_from(map)
                .map_err(|e| anyhow::anyhow!("Failed to cast AI_SCHED_MAP: {}", e))?;
            let _ = bpf_map.remove(&pid);
            Ok(())
        } else {
            Err(anyhow::anyhow!("AI_SCHED_MAP not found in eBPF"))
        }
    }

    pub async fn list_policies(&self) -> anyhow::Result<Vec<(u32, AiSchedParam)>> {
        let mut ebpf = self.ebpf.lock().await;
        if let Some(map) = ebpf.map_mut("AI_SCHED_MAP") {
            let bpf_map = BpfHashMap::<_, u32, AiSchedParam>::try_from(map)
                .map_err(|e| anyhow::anyhow!("Failed to cast AI_SCHED_MAP: {}", e))?;
            let keys = bpf_map.keys().collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("Failed to collect keys from AI_SCHED_MAP: {}", e))?;
            let mut results = Vec::new();
            for k in keys {
                if let Ok(val) = bpf_map.get(&k, 0) {
                    results.push((k, val));
                }
            }
            Ok(results)
        } else {
            Err(anyhow::anyhow!("AI_SCHED_MAP not found in eBPF"))
        }
    }
}

pub struct SchedExtController {
    sched_ext_enabled: bool,
    sched_map: AiSchedMap,
}

/// Critical process PIDs that AI agent scheduling must never deprioritize or manipulate
const PROTECTED_PIDS: &[u32] = &[
    0, // Kernel idle process
    1, // Init / systemd / system-oracle process
];

const EBPF_BYTECODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/athanor-ebpf-sched-bpf"));

impl SchedExtController {
    pub async fn new() -> anyhow::Result<Self> {
        info!("==========================================================================");
        info!("🧠 Initializing User-Space eBPF Scheduler Loader (`aya`) & sched_ext...");
        info!("==========================================================================");

        let mut loaded_ebpf = None;

        if !EBPF_BYTECODE.is_empty() {
            info!("⚡ Loading embedded eBPF bytecode (0 external runtime dependencies)...");
            match Ebpf::load(EBPF_BYTECODE) {
                Ok(bpf) => {
                    info!("✅ Successfully loaded embedded eBPF object bytecode ({} bytes)", EBPF_BYTECODE.len());
                    loaded_ebpf = Some(bpf);
                }
                Err(e) => {
                    warn!("⚠️ Failed to parse embedded eBPF bytecode: {}", e);
                }
            }
        }

        if loaded_ebpf.is_none() {
            let candidate_paths = [
                "target/bpfel-unknown-none/release/athanor-ebpf-sched-bpf",
                "target/bpfel-unknown-none/debug/athanor-ebpf-sched-bpf",
                "/usr/lib/athanor/ebpf/sched_ext.bpf.o",
                "system/ebpf/target/bpfel-unknown-none/release/ebpf-core",
            ];

            for path in candidate_paths {
                if std::path::Path::new(path).exists() {
                    info!("🔍 Found candidate BPF bytecode object at: {}", path);
                    match Ebpf::load_file(path) {
                        Ok(bpf) => {
                            info!("✅ Successfully loaded eBPF object file from {}", path);
                            loaded_ebpf = Some(bpf);
                            break;
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to parse BPF object file {}: {}", path, e);
                        }
                    }
                }
            }
        }


        let is_sysfs_sched_ext = std::path::Path::new("/sys/kernel/sched_ext").exists();

        let (sched_map, sched_ext_enabled) = if let Some(mut ebpf) = loaded_ebpf {
            let map_present = ebpf.map_mut("AI_SCHED_MAP").is_some();
            if map_present {
                info!("✅ `AI_SCHED_MAP` eBPF HashMap detected in BPF object.");
            } else {
                anyhow::bail!("Map `AI_SCHED_MAP` missing in BPF bytecode.");
            }

            let mut attached = false;
            if is_sysfs_sched_ext {
                info!("⚡ Kernel `sched_ext` sysfs interface available.");
                if let Some(prog) = ebpf.program_mut("scx_enqueue") {
                    if let Ok(struct_ops) = <&mut Extension>::try_from(prog) {
                        if let Err(e) = struct_ops.attach() {
                            anyhow::bail!("Failed to physically attach `scx_enqueue` to Kernel: {}", e);
                        } else {
                            info!("✅ Attached `scx_enqueue` sched_ext eBPF program to kernel.");
                            attached = true;
                        }
                    } else {
                        anyhow::bail!("`scx_enqueue` is not a valid StructOps program.");
                    }
                }
            } else {
                anyhow::bail!("sysfs path `/sys/kernel/sched_ext` absent. Kernel standard CFS/EEVDF fallback is NOT allowed in Zero-Trust.");
            }

            let ebpf_arc = Arc::new(Mutex::new(ebpf));
            (AiSchedMap::new(ebpf_arc), attached)
        } else {
            anyhow::bail!("BPF bytecode object not found or load failed.");
        };

        Ok(Self {
            sched_ext_enabled,
            sched_map,
        })
    }

    pub fn sched_map(&self) -> &AiSchedMap {
        &self.sched_map
    }

    pub fn is_sched_ext_enabled(&self) -> bool {
        self.sched_ext_enabled
    }

    /// Apply zero-latency task priority decision directly into kernel sched_ext BPF maps or fallback map.
    /// Validates safety boundaries to prevent AI manipulation of PID 1 or out-of-range slice values.
    pub async fn apply_task_policy(&self, policy: &TaskSchedPolicy) -> anyhow::Result<()> {
        // 1. PID Protection Check (PID 1 / Kernel Idle protection)
        if PROTECTED_PIDS.contains(&policy.pid) {
            let msg = format!(
                "⛔ [AI Confinement Violation] Refused to modify scheduling metrics for protected PID {} (init or idle).",
                policy.pid
            );
            warn!("{}", msg);
            anyhow::bail!(msg);
        }

        // 2. CPU Weight Boundary Check (cgroup v2 range 1..=10000)
        if policy.cpu_weight < 1 || policy.cpu_weight > 10000 {
            let msg = format!(
                "⛔ [AI Confinement Violation] Invalid cpu_weight {} for PID {}. Weight must be between 1 and 10000.",
                policy.cpu_weight, policy.pid
            );
            warn!("{}", msg);
            anyhow::bail!(msg);
        }

        // 3. Time Slice Boundary Check (100us to 100,000us max slice)
        if policy.slice_us < 100 || policy.slice_us > 100_000 {
            let msg = format!(
                "⛔ [AI Confinement Violation] Invalid time slice {}us for PID {}. Slice must be between 100us and 100,000us.",
                policy.slice_us, policy.pid
            );
            warn!("{}", msg);
            anyhow::bail!(msg);
        }

        let map_val = AiSchedParam {
            pid: policy.pid,
            target_core: 0,
            core_type: 0,
            _pad: [0; 3],
            cpu_weight: policy.cpu_weight,
            slice_us: policy.slice_us,
            sched_class: policy.class as u32,
            latency_target_us: policy.latency_target_us,
            flags: 1,
        };

        // Update AI_SCHED_MAP map safely
        self.sched_map.update_policy(policy.pid, map_val).await?;

        info!(
            "⚡ [sched_ext] Policy applied for PID {} ('{:?}'): Weight={}, Slice={}us, TargetLatency={}us (Mode: {})",
            policy.pid, policy.class, policy.cpu_weight, policy.slice_us, policy.latency_target_us,
            if self.sched_ext_enabled { "Kernel sched_ext" } else { "Unknown" }
        );

        Ok(())
    }
}



