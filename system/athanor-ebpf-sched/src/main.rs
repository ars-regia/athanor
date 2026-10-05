#![allow(unsafe_code)]
#![allow(clippy::all)]
#![allow(clippy::pedantic)]

pub mod ai_bridge;
pub mod bpf_trace;
pub mod cgroup_manager;
pub mod dbus_interface;
pub mod sched_ext;

use ai_bridge::AiDaemonBridge;
use bpf_trace::BpfExecTracer;
use cgroup_manager::CgroupManager;
use dbus_interface::SchedExtDbusInterface;
use sched_ext::{SchedClass, SchedExtController, TaskSchedPolicy};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("==========================================================================");
    info!("🧠 Athanor eBPF AI Kernel Scheduler (`athanor-ebpf-sched`) Starting...");
    info!("   Bridging NPU/AI predictions with Ring-0 `sched_ext` & cgroup priority");
    info!("==========================================================================");

    // BPF map memory lock limit expansion
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    unsafe {
        libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim);
    }

    // Initialize core components
    let mut tracer = BpfExecTracer::new().await;
    let ai_bridge = AiDaemonBridge::new().await;
    let sched_controller = Arc::new(SchedExtController::new().await?);
    let cgroup_mgr = CgroupManager::new();

    // Register DBus interface for remote AI_SCHED_MAP manipulation
    let dbus_iface = SchedExtDbusInterface::new(sched_controller.clone());
    let _dbus_conn = match zbus::connection::Builder::session() {
        Ok(builder) => match builder.name("os.athanor.SchedExt") {
            Ok(builder) => match builder.serve_at("/os/athanor/SchedExt", dbus_iface) {
                Ok(builder) => match builder.build().await {
                    Ok(conn) => {
                        info!("✅ Registered `os.athanor.SchedExt` DBus service interface.");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!("⚠️ DBus connection build failed: {}. Operating without external DBus interface.", e);
                        None
                    }
                },
                Err(e) => {
                    warn!("⚠️ DBus serve_at failed: {}", e);
                    None
                }
            },
            Err(e) => {
                warn!("⚠️ DBus name reservation failed: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("⚠️ DBus session builder failed: {}", e);
            None
        }
    };

    let mut interval = tokio::time::interval(Duration::from_millis(1500));

    info!("🚀 eBPF sys_execve -> AI Prediction -> sched_ext zero-latency loop running.");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // 1. Trace new process exec events captured by eBPF sys_execve tracepoint
                let events = tracer.poll_exec_events().await;

                for event in events {
                    info!(
                        "⚡ [sys_execve] Captured new process: PID={}, Comm='{}', Path='{}'",
                        event.pid, event.comm, event.filename
                    );

                    // 2. Query AI / NPU daemon for optimal task scheduling weights
                    let ai_prediction = ai_bridge.predict_task_priority(event.pid, &event.comm, &event.filename).await;

                    info!(
                        "🧠 [AI NPU Inference] Process '{}' (PID {}) -> Class: {:?}, Weight: {}, Heuristic Score: {:.2}",
                        ai_prediction.binary_name,
                        ai_prediction.pid,
                        ai_prediction.recommended_sched_class,
                        ai_prediction.recommended_weight,
                        ai_prediction.heuristic_score
                    );

                    // 3. Apply zero-latency kernel sched_ext task policy
                    let latency_target = match ai_prediction.recommended_sched_class {
                        SchedClass::RealtimeNpu => 100,      // 100us sub-millisecond target
                        SchedClass::InteractiveUi => 500,   // 500us target
                        SchedClass::BatchCompute => 5000,    // 5ms target
                        SchedClass::IdleBackground => 20000, // 20ms target
                    };

                    let policy = TaskSchedPolicy {
                        pid: ai_prediction.pid,
                        class: ai_prediction.recommended_sched_class,
                        cpu_weight: ai_prediction.recommended_weight,
                        slice_us: ai_prediction.recommended_slice_us,
                        latency_target_us: latency_target,
                    };

                    if let Err(err) = sched_controller.apply_task_policy(&policy).await {
                        warn!("Failed to apply sched_ext policy for PID {}: {}", event.pid, err);
                    }

                    // 4. Update cgroup v2 cpu.weight for zero latency task execution
                    let is_realtime = matches!(
                        ai_prediction.recommended_sched_class,
                        SchedClass::RealtimeNpu | SchedClass::InteractiveUi
                    );

                    if let Err(err) = cgroup_mgr.update_process_cgroup(
                        event.pid,
                        ai_prediction.recommended_weight,
                        is_realtime
                    ) {
                        warn!("Failed to update cgroup for PID {}: {}", event.pid, err);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received. Terminating `athanor-ebpf-sched` gracefully.");
                break;
            }
        }
    }

    Ok(())
}



