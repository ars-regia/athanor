//! Crash-loop protection (doc_shell.md SH8), on the policy of /usr/bin/athanor-cosmic-panel:
//! five failures within ten minutes on CLOCK_BOOTTIME, then the unit gives up until the
//! next session. CLOCK_BOOTTIME keeps counting across suspend, so the window means the ten
//! minutes it says.
//!
//! The record lives in the unit's runtime directory. Whether it survives a stop, not only a
//! restart, is the unit's own choice: `RuntimeDirectoryPreserve=restart` clears it on a full
//! stop, `=yes` keeps it until the login session itself ends and $XDG_RUNTIME_DIR goes with
//! it — a give-up must stay a give-up across a stop wherever the unit is meant to (SH8).

use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

pub const FAILURE_WINDOW_SECONDS: i64 = 600;
pub const GIVE_UP_AFTER: usize = 5;

/// Seconds on CLOCK_BOOTTIME.
pub fn boottime() -> io::Result<i64> {
    let now =
        nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME).map_err(io::Error::from)?;
    Ok(now.tv_sec())
}

/// The failure timestamps of `text` still inside the window at `now`.
pub fn recent_failures(text: &str, now: i64) -> Vec<i64> {
    text.lines()
        .filter_map(|line| line.trim().parse::<i64>().ok())
        .filter(|&stamp| stamp <= now && now - stamp < FAILURE_WINDOW_SECONDS)
        .collect()
}

fn read_record(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err),
    }
}

/// Whether the translator has failed often enough to stop trying.
pub fn given_up(path: &Path, now: i64) -> io::Result<bool> {
    Ok(recent_failures(&read_record(path)?, now).len() >= GIVE_UP_AFTER)
}

/// The unit's ExecStopPost: counts the run that just ended when systemd says it failed,
/// and ends the run, in one write. `SERVICE_RESULT` is `success` for a clean stop, and
/// absent outside systemd.
pub fn record_exit(path: &Path, now: i64, service_result: Option<&str>) -> io::Result<()> {
    let text = read_record(path)?;
    let failed = service_result.is_some_and(|result| result != "success");
    if !failed && !is_running(&text) {
        return Ok(());
    }
    let mut stamps = recent_failures(&text, now);
    if failed {
        stamps.push(now);
    }
    write_record(path, &stamps, false)
}

/// The unit's ExecStart, before anything else: marks the run as started, and counts the
/// previous run as a failure if its end was never recorded. systemd runs ExecStopPost in the
/// unit's cgroup, so a kill of the whole cgroup (`systemctl kill`, systemd-oomd) takes the
/// ExecStopPost down with the service and leaves the run marked as running.
pub fn record_start(path: &Path, now: i64) -> io::Result<()> {
    let text = read_record(path)?;
    let mut stamps = recent_failures(&text, now);
    if is_running(&text) {
        stamps.push(now);
    }
    write_record(path, &stamps, true)
}

/// The run state is a line of the failure record, not a file of its own, so every change
/// of state is a single atomic write. `recent_failures` skips the line.
const RUNNING: &str = "running";

fn is_running(text: &str) -> bool {
    text.lines().any(|line| line.trim() == RUNNING)
}

fn write_record(path: &Path, stamps: &[i64], running: bool) -> io::Result<()> {
    let mut text: String = stamps.iter().map(|stamp| format!("{stamp}\n")).collect();
    if running {
        text.push_str(RUNNING);
        text.push('\n');
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(path, &text)
}

/// Write to a hidden sibling, sync, rename: a reader sees the old record or the new one.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temporary = path.with_file_name(format!(".{}.athanor-tmp", name.to_string_lossy()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "athanor-unit-crash-loop-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn five_failures_inside_ten_minutes_give_up() {
        let file = scratch("five").join("failures");
        for now in 100..104 {
            record_exit(&file, now, Some("exit-code")).expect("record");
        }
        assert!(!given_up(&file, 104).expect("read"));
        record_exit(&file, 104, Some("signal")).expect("record");
        assert!(given_up(&file, 105).expect("read"));
        assert!(
            !given_up(&file, 100 + FAILURE_WINDOW_SECONDS).expect("read"),
            "the first failure has left the window"
        );
    }

    #[test]
    fn a_clean_stop_or_a_run_outside_systemd_is_not_a_failure() {
        let file = scratch("clean").join("failures");
        record_exit(&file, 10, Some("success")).expect("record");
        record_exit(&file, 11, None).expect("record");
        assert!(!file.exists());
    }

    #[test]
    fn a_run_whose_exit_was_never_recorded_counts_at_the_next_start() {
        let file = scratch("unrecorded").join("failures");
        let failures = |now| recent_failures(&read_record(&file).expect("read"), now);
        record_start(&file, 10).expect("first start");
        assert!(failures(11).is_empty(), "a first start is not a failure");
        // Killed with its cgroup: the ExecStopPost died with it and recorded nothing.
        record_start(&file, 20).expect("second start");
        assert_eq!(failures(21), [20]);
        // A failure the ExecStopPost did record is not counted again at the next start.
        record_exit(&file, 30, Some("signal")).expect("record");
        record_start(&file, 31).expect("third start");
        assert_eq!(failures(32), [20, 30]);
        // A clean stop leaves nothing to count.
        record_exit(&file, 40, Some("success")).expect("clean stop");
        record_start(&file, 41).expect("fourth start");
        assert_eq!(failures(42), [20, 30]);
    }

    #[test]
    fn the_run_state_lives_in_the_failure_record() {
        let dir = scratch("one-file");
        let file = dir.join("failures");
        let entries = || {
            let mut names: Vec<_> = fs::read_dir(&dir)
                .expect("list")
                .map(|entry| entry.expect("entry").file_name())
                .collect();
            names.sort();
            names
        };
        record_start(&file, 10).expect("start");
        assert_eq!(entries(), ["failures"]);
        assert_eq!(read_record(&file).expect("read"), "running\n");
        // One write both counts the failure and ends the run: no kill can land in between.
        record_exit(&file, 20, Some("signal")).expect("record");
        assert_eq!(read_record(&file).expect("read"), "20\n");
        record_start(&file, 21).expect("restart");
        assert_eq!(read_record(&file).expect("read"), "20\nrunning\n");
        assert_eq!(entries(), ["failures"]);
    }

    #[test]
    fn the_record_keeps_only_the_window() {
        assert_eq!(recent_failures("1\n2\nnot a number\n900\n", 1000), [900]);
        assert_eq!(recent_failures("2000\n", 1000), Vec::<i64>::new());
    }
}
