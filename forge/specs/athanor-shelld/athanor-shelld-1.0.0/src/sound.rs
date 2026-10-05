//! The sound of a notification (NC7): a name in the freedesktop Sound Theme, resolved to a
//! file under the data directories, played by one short `pw-play` process (spike N1).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::process::Command;

use crate::store::Urgency;

/// The theme every lookup ends in. The specification names no source for the user's
/// choice, so this is the one theme the image ships.
pub const THEME: &str = "freedesktop";
const NORMAL: &str = "message-new-instant";
const CRITICAL: &str = "dialog-warning";
const EXTENSIONS: [&str; 3] = ["oga", "ogg", "wav"];
/// A sound is about a second long; this only ends a player that hangs.
const PLAY_LIMIT: Duration = Duration::from_secs(10);
/// Bounds a loop of `Inherits=` that a theme could make.
const MAX_THEMES: usize = 16;

/// `$XDG_DATA_DIRS`, or the specification's default. The daemon's Landlock rules read only
/// `/usr` and `/etc`, so a directory elsewhere (the user's own) resolves nothing.
#[must_use]
pub fn data_dirs() -> Vec<PathBuf> {
    let var = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let var = if var.is_empty() {
        "/usr/local/share:/usr/share"
    } else {
        &var
    };
    var.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// The name to look up: the application's hint, else the urgency's own.
#[must_use]
pub fn name_for(urgency: Urgency, sound_name: Option<&str>) -> &str {
    match (sound_name, urgency) {
        (Some(name), _) if !name.is_empty() => name,
        (_, Urgency::Critical) => CRITICAL,
        _ => NORMAL,
    }
}

/// The file for `name` in `theme`, then in the themes it inherits, then in `freedesktop`.
#[must_use]
pub fn resolve(data_dirs: &[PathBuf], theme: &str, name: &str) -> Option<PathBuf> {
    if !is_plain(name) || !is_plain(theme) {
        return None;
    }
    let mut chain = vec![theme.to_owned()];
    let mut next = 0;
    while next < chain.len() && chain.len() < MAX_THEMES {
        for parent in inherits(data_dirs, &chain[next].clone()) {
            if chain.len() >= MAX_THEMES {
                break;
            }
            if is_plain(&parent) && !chain.contains(&parent) {
                chain.push(parent);
            }
        }
        next += 1;
    }
    if !chain.iter().any(|t| t == THEME) {
        chain.push(THEME.to_owned());
    }
    for theme in chain.iter().filter(|t| is_plain(t)) {
        for dir in data_dirs {
            let stereo = dir.join("sounds").join(theme).join("stereo");
            for extension in EXTENSIONS {
                let file = stereo.join(format!("{name}.{extension}"));
                if file.is_file() {
                    return Some(file);
                }
            }
        }
    }
    None
}

/// A name that stays inside the `sounds` directory it is joined to.
fn is_plain(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.starts_with('.')
}

/// The `Inherits=` of the first `index.theme` of `theme`.
fn inherits(data_dirs: &[PathBuf], theme: &str) -> Vec<String> {
    for dir in data_dirs {
        let index = dir.join("sounds").join(theme).join("index.theme");
        if let Ok(text) = std::fs::read_to_string(index) {
            return text
                .lines()
                .find_map(|line| line.strip_prefix("Inherits="))
                .map(|list| list.split(',').map(|t| t.trim().to_owned()).collect())
                .unwrap_or_default();
        }
    }
    Vec::new()
}

/// The sound for a notification: its hinted name when the theme has it, else the urgency's.
#[must_use]
pub fn sound_for(data_dirs: &[PathBuf], urgency: Urgency, hint: Option<&str>) -> Option<PathBuf> {
    resolve(data_dirs, THEME, name_for(urgency, hint))
        .or_else(|| resolve(data_dirs, THEME, name_for(urgency, None)))
}

/// The task that plays, and what it plays.
struct Playing {
    id: u64,
    critical: bool,
    task: tokio::task::AbortHandle,
}

/// Frees the slot when the playing task ends, however it ends: an aborted task never reaches
/// the end of its body. A newer sound's slot is not this one's to free.
struct Release {
    slot: Arc<Mutex<Option<Playing>>>,
    id: u64,
}

impl Drop for Release {
    fn drop(&mut self) {
        let mut slot = lock(&self.slot);
        if slot.as_ref().is_some_and(|playing| playing.id == self.id) {
            *slot = None;
        }
    }
}

/// A poisoned lock means a panic, and panic = "abort" means there is none: take the guard.
fn lock(slot: &Mutex<Option<Playing>>) -> MutexGuard<'_, Option<Playing>> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Plays one sound at a time: a sound requested while another still plays is dropped, unless
/// it is critical and the one playing is not, which it cuts.
#[derive(Clone)]
pub struct Player {
    /// The data directories whose `sounds/` hold the theme.
    dirs: Vec<PathBuf>,
    program: PathBuf,
    /// The sound playing, if any.
    slot: Arc<Mutex<Option<Playing>>>,
    warned: Arc<AtomicBool>,
}

impl Default for Player {
    /// Resolves nothing, so it plays nothing: for a test that must stay silent.
    fn default() -> Player {
        Player::with_program(Vec::new(), "pw-play")
    }
}

impl Player {
    /// The real player over the session's data directories (`data_dirs()`).
    #[must_use]
    pub fn new(dirs: Vec<PathBuf>) -> Player {
        Player::with_program(dirs, "pw-play")
    }

    /// A player that runs `program` as `pw-play`: the tests' stand-in for the real one.
    #[must_use]
    pub fn with_program(dirs: Vec<PathBuf>, program: impl Into<PathBuf>) -> Player {
        Player {
            dirs,
            program: program.into(),
            slot: Arc::default(),
            warned: Arc::default(),
        }
    }

    /// The sound for a notification, see `sound_for`.
    #[must_use]
    pub fn sound_for(&self, urgency: Urgency, hint: Option<&str>) -> Option<PathBuf> {
        sound_for(&self.dirs, urgency, hint)
    }

    /// Plays `path` with `pw-play`, on the runtime; `Notify` never waits for it.
    pub fn play(&self, path: &Path, critical: bool) {
        let mut command = Command::new(&self.program);
        command
            .args(["--media-role", "Notification", "--"])
            .arg(path);
        self.run(command, PLAY_LIMIT, critical);
    }

    /// Whether a sound is playing now.
    #[must_use]
    pub fn playing(&self) -> bool {
        lock(&self.slot).is_some()
    }

    /// A failure is logged the first time only: a missing player would otherwise fill the
    /// journal with one line per notification.
    fn warn_once(&self, reason: &dyn std::fmt::Display) {
        if !self.warned.swap(true, Ordering::AcqRel) {
            tracing::warn!(%reason, "cannot play a notification sound");
        }
    }

    /// Starts `command` unless a sound is playing, except that a critical sound cuts one that
    /// is not. The child is killed after `limit` and reaped either way; a spawn failure is
    /// logged once for the process.
    pub fn run(&self, mut command: Command, limit: Duration, critical: bool) -> bool {
        let mut slot = lock(&self.slot);
        if let Some(playing) = slot.as_ref() {
            if !critical || playing.critical {
                return false;
            }
        }
        // The three standard streams are inherited, not opened on /dev/null: the daemon's
        // Landlock ruleset grants no access to /dev, and the player's own errors reach the
        // journal that way.
        command.kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                self.warn_once(&err);
                return false;
            }
        };
        if let Some(cut) = slot.take() {
            // The task's drop kills its child, and its guard finds the slot no longer its own.
            cut.task.abort();
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (release, player) = (
            Release {
                slot: Arc::clone(&self.slot),
                id,
            },
            self.clone(),
        );
        let task = tokio::spawn(async move {
            let _release = release;
            match tokio::time::timeout(limit, child.wait()).await {
                Ok(Ok(status)) if !status.success() => player.warn_once(&status),
                Ok(_) => {}
                Err(_) => {
                    // Killed and reaped; a hung player is a failure like any other.
                    let _ = child.kill().await;
                    player.warn_once(&"the player did not finish in time");
                }
            }
        });
        *slot = Some(Playing {
            id,
            critical,
            task: task.abort_handle(),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-sound-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn put(dir: &Path, theme: &str, file: &str, text: &str) {
        let path = dir.join("sounds").join(theme).join(file);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    #[test]
    fn a_theme_falls_back_through_inherits_then_to_freedesktop() {
        let dir = tree("inherit");
        let dirs = [dir.clone()];
        put(
            &dir,
            "mine",
            "index.theme",
            "[Sound Theme]\nInherits=base, other\n",
        );
        put(&dir, "base", "stereo/ding.oga", "");
        put(&dir, THEME, "stereo/plain.oga", "");
        put(&dir, "mine", "stereo/own.oga", "");
        assert_eq!(
            resolve(&dirs, "mine", "own"),
            Some(dir.join("sounds/mine/stereo/own.oga"))
        );
        assert_eq!(
            resolve(&dirs, "mine", "ding"),
            Some(dir.join("sounds/base/stereo/ding.oga"))
        );
        assert_eq!(
            resolve(&dirs, "mine", "plain"),
            Some(dir.join("sounds/freedesktop/stereo/plain.oga"))
        );
        assert_eq!(resolve(&dirs, "mine", "absent"), None);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_loop_of_inherits_ends_and_an_escaping_parent_is_not_followed() {
        let dir = tree("loop");
        put(&dir, "a", "index.theme", "Inherits=b\n");
        put(&dir, "b", "index.theme", "Inherits=a,../x\n");
        // What following `../x` would reach: <dir>/x/stereo/none.oga.
        fs::create_dir_all(dir.join("x/stereo")).expect("mkdir");
        fs::write(dir.join("x/stereo/none.oga"), "").expect("write");
        assert_eq!(resolve(std::slice::from_ref(&dir), "a", "none"), None);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn more_inherited_themes_than_the_cap_are_not_all_followed() {
        let dir = tree("cap");
        let list: Vec<String> = (0..40).map(|n| format!("t{n}")).collect();
        put(
            &dir,
            "a",
            "index.theme",
            &format!("Inherits={}\n", list.join(",")),
        );
        put(&dir, "t30", "stereo/deep.oga", "");
        assert_eq!(resolve(std::slice::from_ref(&dir), "a", "deep"), None);
        put(&dir, "t3", "stereo/near.oga", "");
        assert!(resolve(std::slice::from_ref(&dir), "a", "near").is_some());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn wav_is_found_when_oga_is_absent_and_oga_wins_when_both_exist() {
        let dir = tree("ext");
        put(&dir, THEME, "stereo/a.wav", "");
        put(&dir, THEME, "stereo/b.wav", "");
        put(&dir, THEME, "stereo/b.oga", "");
        let dirs = [dir.clone()];
        assert_eq!(
            resolve(&dirs, THEME, "a"),
            Some(dir.join("sounds/freedesktop/stereo/a.wav"))
        );
        assert_eq!(
            resolve(&dirs, THEME, "b"),
            Some(dir.join("sounds/freedesktop/stereo/b.oga"))
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Puts a real file at the path each escaping name would reach, so a missing guard finds it.
    #[test]
    fn a_name_or_theme_that_leaves_the_sounds_directory_is_refused() {
        let dir = tree("refuse");
        put(&dir, THEME, "stereo/x.oga", "");
        let stereo = dir.join("sounds/freedesktop/stereo");
        for target in [
            "sounds/freedesktop/outside.oga",
            "sounds/freedesktop/freedesktop/stereo/x.oga",
            "sounds/freedesktop/stereo/.x.oga",
            "sounds/freedesktop/stereo/a/b.oga",
            "sounds/freedesktop/stereo/.oga",
            "x/stereo/x.oga",
        ] {
            let path = dir.join(target);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, "").expect("write");
        }
        assert!(stereo.join("x.oga").is_file());
        let dirs = [dir.clone()];
        for name in ["../outside", "../freedesktop/stereo/x", ".x", "a/b", ""] {
            assert_eq!(resolve(&dirs, THEME, name), None, "{name}");
        }
        // A theme `../x` would reach <dir>/x/stereo/x.oga.
        assert_eq!(resolve(&dirs, "../x", "x"), None);
        assert_eq!(resolve(&dirs, "", "x"), None);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn the_name_is_the_hint_else_the_urgencys() {
        assert_eq!(name_for(Urgency::Normal, None), "message-new-instant");
        assert_eq!(name_for(Urgency::Low, None), "message-new-instant");
        assert_eq!(name_for(Urgency::Critical, None), "dialog-warning");
        assert_eq!(name_for(Urgency::Critical, Some("bell")), "bell");
        assert_eq!(name_for(Urgency::Normal, Some("")), "message-new-instant");
    }

    #[test]
    fn an_unknown_hint_falls_back_to_the_urgencys_sound() {
        let dir = tree("hint");
        put(&dir, THEME, "stereo/message-new-instant.oga", "");
        put(&dir, THEME, "stereo/bell.oga", "");
        let dirs = [dir.clone()];
        let bell = Some(dir.join("sounds/freedesktop/stereo/bell.oga"));
        assert_eq!(sound_for(&dirs, Urgency::Normal, Some("bell")), bell);
        assert_eq!(
            sound_for(&dirs, Urgency::Normal, Some("nope")),
            Some(dir.join("sounds/freedesktop/stereo/message-new-instant.oga"))
        );
        assert_eq!(sound_for(&dirs, Urgency::Critical, None), None);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    fn marker(path: &Path) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("sleep 0.3; echo x >> {}", path.display()));
        command
    }

    #[tokio::test]
    async fn a_sound_requested_while_one_plays_is_dropped_not_queued() {
        let dir = tree("busy");
        let log = dir.join("log");
        let player = Player::default();
        assert!(player.run(marker(&log), Duration::from_secs(5), false));
        for _ in 0..5 {
            assert!(!player.run(marker(&log), Duration::from_secs(5), false));
        }
        tokio::time::sleep(Duration::from_millis(900)).await;
        assert_eq!(
            fs::read_to_string(&log).expect("log"),
            "x\n",
            "one process ran, none queued"
        );
        assert!(
            player.run(marker(&log), Duration::from_secs(5), false),
            "free again"
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    fn hang() -> Command {
        let mut command = Command::new("sleep");
        command.arg("30");
        command
    }

    #[tokio::test]
    async fn a_critical_sound_cuts_a_normal_one_and_the_slot_stays_held() {
        let dir = tree("preempt");
        let log = dir.join("log");
        let player = Player::default();
        assert!(player.run(hang(), Duration::from_secs(30), false));
        assert!(
            player.run(marker(&log), Duration::from_secs(5), true),
            "preempts"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            player.playing(),
            "the aborted task must not free the new sound's slot"
        );
        assert!(
            !player.run(marker(&log), Duration::from_secs(5), false),
            "normal dropped"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(fs::read_to_string(&log).expect("log"), "x\n");
        assert!(!player.playing());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_normal_or_critical_sound_during_a_critical_one_is_dropped() {
        let player = Player::default();
        assert!(player.run(hang(), Duration::from_secs(30), true));
        assert!(!player.run(hang(), Duration::from_secs(30), false));
        assert!(!player.run(hang(), Duration::from_secs(30), true));
        assert!(player.playing());
    }

    #[tokio::test]
    async fn the_slot_is_free_after_a_preemption_ends() {
        let player = Player::default();
        assert!(player.run(hang(), Duration::from_secs(30), false));
        assert!(player.run(hang(), Duration::from_millis(100), true));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !player.playing(),
            "released after the critical sound's own end"
        );
        assert!(player.run(hang(), Duration::from_millis(100), false));
    }

    #[tokio::test]
    async fn a_player_that_hangs_is_killed_at_the_limit() {
        let player = Player::default();
        let mut hang = Command::new("sleep");
        hang.arg("30");
        assert!(player.run(hang, Duration::from_millis(100), false));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!player.playing(), "reaped and released");
    }

    #[tokio::test]
    async fn a_player_that_fails_is_reported_once_and_frees_the_slot() {
        let player = Player::default();
        let mut fail = Command::new("sh");
        fail.args(["-c", "exit 3"]);
        assert!(player.run(fail, Duration::from_secs(5), false));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!player.playing());
        assert!(player.warned.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn a_missing_player_is_not_a_crash_and_frees_the_slot() {
        let player = Player::default();
        assert!(!player.run(
            Command::new("/nonexistent/pw-play"),
            Duration::from_secs(1),
            false
        ));
        assert!(!player.playing());
        assert!(player.warned.load(Ordering::Acquire));
    }
}
