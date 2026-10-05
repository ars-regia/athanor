use aya::programs::TracePoint;
use aya::Ebpf;
use aya::maps::perf::{PerfEvent, PerfEventArray};
use aya::util::online_cpus;
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;
use tokio::signal;
use tokio::task;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    info!("Athanor Sysmon eBPF (Ring-0 Analytics) starting...");

    // Load the compiled eBPF bytecode.
    let bpf_path = "target/bpfel-unknown-none/release/athanor-sysmon-ebpf";
    let bpf = Ebpf::load_file(bpf_path).map_err(|e| format!("Failed to load eBPF bytecode from '{bpf_path}': {e}"))?;
    let bpf: &'static mut Ebpf = Box::leak(Box::new(bpf));
    
    // Attach to the tracepoint
    let program: &mut TracePoint = bpf.program_mut("sched_process_exec").ok_or("program 'sched_process_exec' not found")?.try_into()?;
    program.load()?;
    program.attach("sched", "sched_process_exec")?;
    info!("eBPF hooks attached to sched:sched_process_exec.");
    
    // Read events from the kernel via PerfEventArray
    if let Some(events_map) = bpf.map_mut("EVENTS") {
        let mut perf_array = PerfEventArray::try_from(events_map)?;
        for cpu_id in online_cpus().map_err(|(s, e)| format!("{s}: {e}"))? {
            let mut buf = AsyncFd::with_interest(perf_array.open(cpu_id, None)?, Interest::READABLE)?;
            task::spawn(async move {
                loop {
                    let mut guard = match buf.readable_mut().await {
                        Ok(guard) => guard,
                        Err(e) => {
                            warn!("Error waiting for perf events on CPU {}: {}", cpu_id, e);
                            break;
                        }
                    };
                    guard.get_inner_mut().for_each(|event| match event {
                        PerfEvent::Sample { head, tail } => {
                            info!("Received event from kernel on CPU {} ({} bytes)", cpu_id, head.len() + tail.len());
                        }
                        PerfEvent::Lost { count } => {
                            warn!("Lost {} events from kernel on CPU {}", count, cpu_id);
                        }
                    });
                    guard.clear_ready();
                }
            });
        }
        info!("Awaiting events...");
    } else {
        warn!("EVENTS map not found in the eBPF program, running without events hook.");
    }

    // Wait for SIGINT
    signal::ctrl_c().await?;
    info!("Exiting...");

    Ok(())
}
