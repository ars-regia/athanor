//! The privacy prompt: how the portal asks the user, and how it reads the answer.
//!
//! The prompt is `athanor-shell-rs --privacy-prompt`, and its answer is its exit status.
//! Only one status grants, [`GRANTED_EXIT_CODE`], which the prompt returns from its Allow
//! button and from nothing else. Every other outcome denies: the Deny button, a closed
//! window, a crash, a signal, a prompt that would not start, a prompt that stayed open
//! past [`PROMPT_TIMEOUT`]. Status 0 is one of them, because it is what a process exits
//! with when its window is closed, and what a second instance of a single-instance GTK
//! application exits with after forwarding its request to the first.

use std::collections::HashSet;
use std::process::{ExitStatus, Stdio};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;
use tracing::warn;

/// The exit status of the Allow button. It must equal `EXIT_GRANTED` in
/// `athanor-shell-rs` (`src/ui/prompts/privacy.rs`); the two programs are in different
/// workspaces and cannot share the constant.
pub const GRANTED_EXIT_CODE: i32 = 100;

/// How long a prompt may stay on screen before the request is denied.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(not(test))]
const PROMPT_PROGRAM: &str = "athanor-shell-rs";
/// Under test the prompt is never the real one: on a developer's machine it would open a window.
#[cfg(test)]
const PROMPT_PROGRAM: &str = "/nonexistent/athanor-shell-rs";

/// The longest application id shown, in characters. Longer ones are cut, not refused.
const MAX_APP_ID_CHARS: usize = 128;

/// What the user is asked to allow. The prompt shows the name; the portal decides the
/// set, so a caller cannot ask about a resource this list does not name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resource {
    Camera,
    Microphone,
    Location,
}

impl Resource {
    fn prompt_name(self) -> &'static str {
        match self {
            Resource::Camera => "Camera",
            Resource::Microphone => "Microphone",
            Resource::Location => "Location",
        }
    }
}

/// Why a request was denied. Logged, and asserted in the tests.
#[derive(Debug, PartialEq, Eq)]
pub enum Denial {
    /// The application id was empty once cleaned.
    NoApplication,
    /// The same application is already being asked about the same resource.
    AlreadyAsking,
    /// The prompt could not be started.
    Spawn,
    /// The prompt could not be waited for.
    Wait,
    /// The prompt ended with a status other than [`GRANTED_EXIT_CODE`]; `None` when a
    /// signal ended it.
    Status(Option<i32>),
    /// The prompt was still open at [`PROMPT_TIMEOUT`] and was killed.
    TimedOut,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Granted,
    Denied(Denial),
}

/// Asks the user whether `app_id` may use `resource`. Fails closed: anything that is not
/// a click on Allow is a denial.
pub async fn ask(resource: Resource, app_id: &str) -> Verdict {
    let Some(app_id) = clean_app_id(app_id) else {
        return Verdict::Denied(Denial::NoApplication);
    };
    let Some(_asking) = Asking::enter(resource, &app_id) else {
        return Verdict::Denied(Denial::AlreadyAsking);
    };
    let mut prompt = Command::new(PROMPT_PROGRAM);
    prompt
        .arg("--privacy-prompt")
        .arg(format!("{}:{}", resource.prompt_name(), app_id));
    run(prompt, PROMPT_TIMEOUT).await
}

/// Runs `prompt` to the end or to `limit`, and reads its status.
async fn run(mut prompt: Command, limit: Duration) -> Verdict {
    prompt
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true);
    let mut child = match prompt.spawn() {
        Ok(child) => child,
        Err(err) => {
            warn!("cannot start the privacy prompt: {err}");
            return Verdict::Denied(Denial::Spawn);
        }
    };
    match timeout(limit, child.wait()).await {
        Ok(Ok(status)) => verdict_of(status),
        Ok(Err(err)) => {
            warn!("cannot wait for the privacy prompt: {err}");
            Verdict::Denied(Denial::Wait)
        }
        Err(_elapsed) => {
            if let Err(err) = child.kill().await {
                warn!("cannot stop the privacy prompt after its timeout: {err}");
            }
            Verdict::Denied(Denial::TimedOut)
        }
    }
}

fn verdict_of(status: ExitStatus) -> Verdict {
    if status.code() == Some(GRANTED_EXIT_CODE) {
        Verdict::Granted
    } else {
        Verdict::Denied(Denial::Status(status.code()))
    }
}

/// The application id as it goes to the prompt: control characters, text-direction
/// controls and zero-width characters removed, and cut to [`MAX_APP_ID_CHARS`]. `None`
/// when nothing is left. The id comes from the caller of a D-Bus method and is shown to
/// the user as the name of the application asking.
fn clean_app_id(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_format(*c))
        .take(MAX_APP_ID_CHARS)
        .collect();
    let cleaned = cleaned.trim().to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
    )
}

/// The requests being asked about right now. A second request for the same application
/// and resource is denied instead of opening a second prompt.
static ASKING: LazyLock<Mutex<HashSet<(Resource, String)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct Asking {
    key: (Resource, String),
}

impl Asking {
    fn enter(resource: Resource, app_id: &str) -> Option<Asking> {
        let key = (resource, app_id.to_owned());
        let mut asking = ASKING.lock().unwrap_or_else(PoisonError::into_inner);
        // Not `then_some(Asking { key })`: that builds the guard even for a duplicate, and
        // dropping it would take this lock again and remove the first request's entry.
        if asking.insert(key.clone()) {
            Some(Asking { key })
        } else {
            None
        }
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        ASKING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;

    use super::*;

    fn exit(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn only_the_allow_status_grants() {
        assert_eq!(verdict_of(exit(GRANTED_EXIT_CODE)), Verdict::Granted);
        // 0 is a closed window, or a second instance that forwarded its request.
        assert_eq!(verdict_of(exit(0)), Verdict::Denied(Denial::Status(Some(0))));
        assert_eq!(verdict_of(exit(1)), Verdict::Denied(Denial::Status(Some(1))));
        assert_eq!(verdict_of(exit(101)), Verdict::Denied(Denial::Status(Some(101))));
        // Ended by SIGKILL: no exit code at all.
        assert_eq!(
            verdict_of(ExitStatus::from_raw(9)),
            Verdict::Denied(Denial::Status(None))
        );
    }

    #[tokio::test]
    async fn a_prompt_is_read_by_its_status() {
        assert_eq!(
            run(shell("exit 100"), Duration::from_secs(10)).await,
            Verdict::Granted
        );
        assert_eq!(
            run(shell("exit 0"), Duration::from_secs(10)).await,
            Verdict::Denied(Denial::Status(Some(0)))
        );
        assert_eq!(
            run(shell("exit 1"), Duration::from_secs(10)).await,
            Verdict::Denied(Denial::Status(Some(1)))
        );
        assert_eq!(
            run(shell("kill -9 $$"), Duration::from_secs(10)).await,
            Verdict::Denied(Denial::Status(None))
        );
    }

    #[tokio::test]
    async fn output_on_stdout_cannot_grant() {
        assert_eq!(
            run(shell("echo granted; exit 0"), Duration::from_secs(10)).await,
            Verdict::Denied(Denial::Status(Some(0)))
        );
    }

    #[tokio::test]
    async fn a_prompt_that_will_not_start_denies() {
        assert_eq!(
            run(Command::new("/nonexistent/athanor-prompt"), Duration::from_secs(10)).await,
            Verdict::Denied(Denial::Spawn)
        );
    }

    #[tokio::test]
    async fn a_prompt_left_open_is_denied_and_killed() {
        let pid_file = std::env::temp_dir().join(format!("athanor-prompt-pid-{}", std::process::id()));
        let script = format!("echo $$ > {}; exec sleep 60", pid_file.display());
        let verdict = run(shell(&script), Duration::from_millis(500)).await;
        assert_eq!(verdict, Verdict::Denied(Denial::TimedOut));

        let pid = std::fs::read_to_string(&pid_file).expect("the prompt wrote its pid");
        std::fs::remove_file(&pid_file).ok();
        let proc = format!("/proc/{}", pid.trim());
        assert!(!Path::new(&proc).exists(), "the prompt is still running");
    }

    #[test]
    fn the_application_id_is_cleaned() {
        assert_eq!(
            clean_app_id("org.mozilla.firefox").as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            clean_app_id("  evil\napp\u{202E}gnp.exe\u{200B} ").as_deref(),
            Some("evilappgnp.exe")
        );
        assert_eq!(clean_app_id("").as_deref(), None);
        assert_eq!(clean_app_id(" \n\t\u{200B}\u{202E}").as_deref(), None);
        let long = "a".repeat(MAX_APP_ID_CHARS + 50);
        assert_eq!(clean_app_id(&long).map(|id| id.chars().count()), Some(MAX_APP_ID_CHARS));
    }

    #[test]
    fn a_second_request_for_the_same_thing_is_not_asked() {
        let first = Asking::enter(Resource::Microphone, "test.same.app");
        assert!(first.is_some());
        assert!(Asking::enter(Resource::Microphone, "test.same.app").is_none());
        // Another resource, or another application, is a different question.
        assert!(Asking::enter(Resource::Camera, "test.same.app").is_some());
        assert!(Asking::enter(Resource::Microphone, "test.other.app").is_some());
        // Once the first prompt is over, the question can be asked again.
        drop(first);
        assert!(Asking::enter(Resource::Microphone, "test.same.app").is_some());
    }

    #[tokio::test]
    async fn an_empty_application_is_denied_without_a_prompt() {
        assert_eq!(
            ask(Resource::Microphone, " \n").await,
            Verdict::Denied(Denial::NoApplication)
        );
    }
}
