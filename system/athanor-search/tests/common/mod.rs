//! Helpers shared by the tests that start a real `qalc`.

use std::time::{Duration, Instant};

/// The pids of this process's children named qalc that have not exited.
pub fn live_qalc() -> Vec<u32> {
    let mut pids = Vec::new();
    for task in std::fs::read_dir("/proc/self/task")
        .expect("tasks")
        .flatten()
    {
        let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        for pid in children
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
        {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // "pid (comm) state ..."
            let after = stat.rsplit_once(") ").map(|(_, rest)| rest).unwrap_or("Z");
            if stat.contains("(qalc)") && !after.starts_with('Z') {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Waits up to `bound` for a killed child to be gone: the kill and the reap take longer on a loaded runner.
/// The bound of the cancellation wait stays under `DEADLINE`, or the deadline would hide a missing abort.
pub async fn wait_until(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + bound;
    while Instant::now() < end {
        if done() {
            return true;
        }
        gio::glib::timeout_future(Duration::from_millis(10)).await;
    }
    done()
}
