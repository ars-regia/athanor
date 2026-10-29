//! The binary ends cleanly on SIGTERM and SIGINT: it exits 0 and leaves nothing in the runtime
//! directory, neither the Wayland socket nor its lock.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn wait_for<T>(what: &str, mut ready: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(value) = ready() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// The compositor, killed when a failed wait unwinds the test.
struct Killed(Child);

impl std::ops::Deref for Killed {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Killed {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for Killed {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("runtime dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn stops_cleanly_on(signal: &str) {
    let runtime =
        std::env::temp_dir().join(format!("athanor-sigterm-{}-{signal}", std::process::id()));
    std::fs::create_dir_all(&runtime).expect("runtime dir");
    let mut child = Killed(Command::new(env!("CARGO_BIN_EXE_athanor-compositor"))
        .arg("--headless")
        .env("XDG_RUNTIME_DIR", &runtime)
        .env_remove("NOTIFY_SOCKET")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the compositor starts"));
    // The log line announced right before the loop runs, with the X11 display (if any) up: the
    // signal reaches a compositor at steady state, not one still starting.
    let log = child.stdout.take().expect("stdout");
    let (listening, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(log).lines().map_while(Result::ok) {
            if line.contains("listening") && listening.send(()).is_err() {
                break;
            }
        }
    });
    wait_for("the compositor to listen", || heard.try_recv().ok());
    let sent = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill");
    assert!(sent.success());
    let status = wait_for("the exit", || child.try_wait().expect("try_wait"));
    assert_eq!(status.code(), Some(0), "{status}");
    assert_eq!(entries(&runtime), Vec::<String>::new());
    std::fs::remove_dir(&runtime).expect("cleanup");
}

#[test]
fn sigterm_ends_the_compositor_cleanly() {
    stops_cleanly_on("TERM");
}

#[test]
fn sigint_ends_the_compositor_cleanly() {
    stops_cleanly_on("INT");
}
