//! The requesting program, as the dialog names it (doc_lock_and_prompts.md, LP11): the
//! executable of `polkit.subject-pid` from `/proc/<pid>/exe`, and for a Flatpak's `bwrap` the
//! application's name from `/proc/<pid>/root/.flatpak-info`.
//!
//! It reads `/proc` only to describe the requesting program, never to decide anything:
//! `BeginAuthentication` carries no pidfd, so the pid may have been reused before it is read,
//! and the description is best effort (plan, section 4, T1). polkitd decides on its own subject
//! check and the root helper's report, never on this. Any failure, a dead pid among them, is
//! "Unknown program", never a guess.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use athanor_unit::text;
use nix::fcntl::OFlag;

/// The most `.flatpak-info` may hold; a larger file is refused (plan, section 3.1).
pub const FLATPAK_INFO_MAX: u64 = 16 * 1024;
/// One byte past the bound, so that reading that much tells a file over it.
const FLATPAK_INFO_READ: u64 = FLATPAK_INFO_MAX + 1;

/// The executable of a Flatpak's sandbox, as `/proc/<pid>/exe` names it.
const BWRAP: &str = "/usr/bin/bwrap";

/// What the dialog shows of the requesting program, made safe to show (T14).
#[derive(Debug, PartialEq, Eq)]
pub enum Program {
    /// The path of a native program's executable.
    Executable(String),
    /// A Flatpak's application name, from the `[Application]` group of its `.flatpak-info`.
    Flatpak(String),
}

/// The program that `details`' `polkit.subject-pid` names, read under `proc` (`/proc` but in
/// tests); `None` is "Unknown program".
#[must_use]
pub fn describe(details: &HashMap<String, String>, proc: &Path) -> Option<Program> {
    let pid: u32 = details.get("polkit.subject-pid")?.parse().ok()?;
    if pid == 0 {
        return None;
    }
    // The pid is a number, so the paths below never take a name from the details.
    let process = proc.join(pid.to_string());
    let exe = fs::read_link(process.join("exe")).ok()?;
    if exe != Path::new(BWRAP) {
        return Some(Program::Executable(text::line(
            &exe.to_string_lossy(),
            text::SUMMARY_CHARS,
        )));
    }
    let name = text::line(
        &flatpak_name(&process.join("root/.flatpak-info"))?,
        text::SUMMARY_CHARS,
    );
    (!name.is_empty()).then_some(Program::Flatpak(name))
}

/// The `[Application]` group's `name` of the `.flatpak-info` at `path`. The only path followed
/// from the subject's root, and read bounded: a link there is refused (`O_NOFOLLOW`), a FIFO
/// does not block the bus thread (`O_NONBLOCK`), a terminal does not become the agent's
/// (`O_NOCTTY`), the open file must be a regular one, and one over FLATPAK_INFO_MAX is refused.
fn flatpak_name(path: &Path) -> Option<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(
            (OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_NOCTTY | OFlag::O_CLOEXEC).bits(),
        )
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut keyfile = String::new();
    file.take(FLATPAK_INFO_READ)
        .read_to_string(&mut keyfile)
        .ok()?;
    if keyfile.len() as u64 > FLATPAK_INFO_MAX {
        return None;
    }
    application_name(&keyfile)
}

/// `name` in the `[Application]` group of a key file (the Desktop Entry syntax Flatpak writes).
fn application_name(keyfile: &str) -> Option<String> {
    let mut in_application = false;
    for line in keyfile.lines().map(str::trim) {
        if line.starts_with('[') {
            in_application = line == "[Application]";
        } else if in_application {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim_end() == "name" {
                    return Some(value.trim_start().to_owned());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    const INFO: &str = "[Application]\nname=org.example.Editor\nruntime=runtime/org.example.Platform/x86_64/1\n\n[Instance]\ninstance-id=1\n";

    /// A directory standing in for `/proc`, removed when the test ends.
    struct Proc(PathBuf);

    impl Proc {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "athanor-polkit-agent-{}-{test}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("fixture directory");
            Self(dir)
        }

        /// `<pid>/`, with its `exe` a link to `exe` when given.
        fn process(&self, pid: u32, exe: Option<&str>) -> PathBuf {
            let dir = self.0.join(pid.to_string());
            fs::create_dir_all(dir.join("root")).expect("process directory");
            if let Some(exe) = exe {
                symlink(exe, dir.join("exe")).expect("exe link");
            }
            dir
        }

        /// A Flatpak's `bwrap` whose `.flatpak-info` holds `info`.
        fn flatpak(&self, pid: u32, info: &str) -> PathBuf {
            let dir = self.process(pid, Some("/usr/bin/bwrap"));
            fs::write(dir.join("root/.flatpak-info"), info).expect(".flatpak-info");
            dir
        }

        fn describe(&self, pid: &str) -> Option<Program> {
            describe(&details(pid), &self.0)
        }
    }

    impl Drop for Proc {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn details(pid: &str) -> HashMap<String, String> {
        HashMap::from([("polkit.subject-pid".to_owned(), pid.to_owned())])
    }

    #[test]
    fn a_native_process_is_named_by_its_executable() {
        let proc = Proc::new("native");
        proc.process(4242, Some("/usr/bin/pkexec"));
        assert_eq!(
            proc.describe("4242"),
            Some(Program::Executable("/usr/bin/pkexec".to_owned()))
        );
    }

    #[test]
    fn this_process_is_named_from_the_real_proc() {
        let exe = std::env::current_exe().expect("current_exe");
        assert_eq!(
            describe(
                &details(&std::process::id().to_string()),
                Path::new("/proc")
            ),
            Some(Program::Executable(exe.to_string_lossy().into_owned()))
        );
    }

    #[test]
    fn a_flatpak_is_named_by_its_application() {
        let proc = Proc::new("flatpak");
        proc.flatpak(4242, INFO);
        assert_eq!(
            proc.describe("4242"),
            Some(Program::Flatpak("org.example.Editor".to_owned()))
        );
    }

    #[test]
    fn a_name_outside_the_application_group_is_not_read() {
        let proc = Proc::new("group");
        proc.flatpak(
            4242,
            "[Instance]\nname=org.example.Forged\n\n[Application]\nruntime=x\n",
        );
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn hidden_characters_are_removed_from_what_is_shown() {
        let proc = Proc::new("hidden");
        proc.flatpak(
            4242,
            "[Application]\nname=org.example.\u{202e}Editor\u{7}\n",
        );
        assert_eq!(
            proc.describe("4242"),
            Some(Program::Flatpak("org.example.Editor".to_owned()))
        );
    }

    #[test]
    fn a_dead_pid_is_an_unknown_program() {
        let proc = Proc::new("dead");
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn an_unreadable_executable_is_an_unknown_program() {
        let proc = Proc::new("unreadable");
        // A kernel thread's exe, or one the agent may not read: readlink fails.
        proc.process(4242, None);
        assert_eq!(proc.describe("4242"), None);
        // A file that is not a link: readlink fails as well.
        let dir = proc.process(4343, None);
        fs::write(dir.join("exe"), "").expect("exe file");
        assert_eq!(proc.describe("4343"), None);
    }

    #[test]
    fn a_missing_or_malformed_pid_is_an_unknown_program() {
        let proc = Proc::new("pid");
        proc.process(4242, Some("/usr/bin/pkexec"));
        assert_eq!(describe(&HashMap::new(), &proc.0), None);
        for pid in ["", "0", "-4242", "4242/../4242", "../4242", "4242 ", "x"] {
            assert_eq!(proc.describe(pid), None, "{pid:?}");
        }
    }

    #[test]
    fn bwrap_without_a_readable_flatpak_info_is_an_unknown_program() {
        let proc = Proc::new("no-info");
        proc.process(4242, Some("/usr/bin/bwrap"));
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn a_flatpak_info_that_is_a_link_is_refused() {
        let proc = Proc::new("link");
        let dir = proc.process(4242, Some("/usr/bin/bwrap"));
        let real = proc.0.join("info");
        fs::write(&real, INFO).expect("info");
        symlink(&real, dir.join("root/.flatpak-info")).expect("info link");
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn a_flatpak_info_that_is_a_fifo_is_refused_without_blocking() {
        let proc = Proc::new("fifo");
        let dir = proc.process(4242, Some("/usr/bin/bwrap"));
        nix::unistd::mkfifo(
            &dir.join("root/.flatpak-info"),
            nix::sys::stat::Mode::S_IRWXU,
        )
        .expect("fifo");
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn a_flatpak_info_that_is_a_directory_is_refused() {
        let proc = Proc::new("directory");
        let dir = proc.process(4242, Some("/usr/bin/bwrap"));
        fs::create_dir(dir.join("root/.flatpak-info")).expect("info directory");
        assert_eq!(proc.describe("4242"), None);
    }

    #[test]
    fn a_flatpak_info_over_the_bound_is_refused() {
        let at_bound = format!(
            "{INFO}{}",
            "#".repeat(FLATPAK_INFO_MAX as usize - INFO.len())
        );
        assert_eq!(at_bound.len() as u64, FLATPAK_INFO_MAX);
        let proc = Proc::new("bound");
        proc.flatpak(4242, &at_bound);
        assert_eq!(
            proc.describe("4242"),
            Some(Program::Flatpak("org.example.Editor".to_owned()))
        );
        proc.flatpak(4343, &format!("{at_bound}#"));
        assert_eq!(proc.describe("4343"), None);
    }
}
