//! `athanor-backup`: hourly read-only btrfs snapshots of `/var/home`, their retention,
//! and restoring a file or directory from one into its owner's home.
//!
//! Every snapshot is the whole `/var/home` subvolume, taken read-only into the
//! `/var/home/.snapshots` subvolume (root, 0700; created by `init`), and named after
//! its UTC creation time. A snapshot shares its blocks with the live home until files
//! change, so it protects against deletion and overwriting, not against a failed disk.
//! A restore never touches the live files: it copies from the snapshot into
//! `~/Ripristinati/<snapshot>/`, as the home's owner.

mod retention;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{NaiveDateTime, Utc};
use nix::fcntl::{openat2, OFlag, OpenHow, ResolveFlag};
use nix::sys::stat::fstatat;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Permissions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

const HOME_ROOT: &str = "/var/home";
const SNAPSHOT_DIR: &str = "/var/home/.snapshots";
/// The directory of a home that restored copies land in.
const RESTORE_DIR: &str = "Ripristinati";
/// Snapshot names: their UTC creation time, which sorts like the time itself.
const ID_FORMAT: &str = "%Y%m%dT%H%M%SZ";
/// The inode number of the root directory of every btrfs subvolume.
const BTRFS_SUBVOLUME_ROOT_INO: u64 = 256;

const USAGE: &str =
    "usage: athanor-backup init | create | prune | list | restore <snapshot> <path>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("athanor-backup: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if !matches!(
        args.as_slice(),
        ["init" | "create" | "prune" | "list"] | ["restore", _, _]
    ) {
        bail!("{USAGE}");
    }
    if !nix::unistd::geteuid().is_root() {
        bail!(
            "snapshots belong to root: run it as `sudo athanor-backup {}`",
            args.join(" ")
        );
    }
    match args.as_slice() {
        ["init"] => init(),
        ["create"] => create(),
        ["prune"] => prune(),
        ["list"] => list(),
        ["restore", id, path] => restore(id, Path::new(path)),
        _ => bail!("{USAGE}"),
    }
}

/// Creates the snapshot subvolume if it is missing, and makes it root's alone.
///
/// tmpfiles.d cannot do this: its `v` line creates a subvolume only when `/` is one
/// itself, and the root of a composefs image is an overlay, so it would leave a plain
/// directory that `create` refuses.
fn init() -> Result<()> {
    ensure_subvolume(Path::new(HOME_ROOT))?;
    let dir = Path::new(SNAPSHOT_DIR);
    match fs::symlink_metadata(dir) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            btrfs(&[
                OsStr::new("subvolume"),
                OsStr::new("create"),
                dir.as_os_str(),
            ])?;
        }
        Err(err) => return Err(err).with_context(|| format!("cannot inspect {SNAPSHOT_DIR}")),
    }
    ensure_subvolume(dir)?;
    fs::set_permissions(dir, Permissions::from_mode(0o700))
        .with_context(|| format!("cannot restrict {SNAPSHOT_DIR} to root"))
}

/// Takes a read-only snapshot of `/var/home`.
fn create() -> Result<()> {
    ensure_subvolume(Path::new(HOME_ROOT))?;
    ensure_subvolume(Path::new(SNAPSHOT_DIR))?;
    let id = Utc::now().format(ID_FORMAT).to_string();
    let target = Path::new(SNAPSHOT_DIR).join(&id);
    btrfs(&[
        OsStr::new("subvolume"),
        OsStr::new("snapshot"),
        OsStr::new("-r"),
        OsStr::new(HOME_ROOT),
        target.as_os_str(),
    ])?;
    println!("created {id}");
    Ok(())
}

/// Deletes the snapshots [`retention::keep`] does not keep.
fn prune() -> Result<()> {
    let snapshots = snapshots()?;
    let mut failed = Vec::new();
    for id in prunable(&snapshots, Utc::now().naive_utc()) {
        let path = Path::new(SNAPSHOT_DIR).join(id);
        match btrfs(&[
            OsStr::new("subvolume"),
            OsStr::new("delete"),
            path.as_os_str(),
        ]) {
            Ok(()) => println!("deleted {id}"),
            Err(err) => {
                eprintln!("athanor-backup: {err:#}");
                failed.push(id);
            }
        }
    }
    if !failed.is_empty() {
        bail!("could not delete {}", failed.join(", "));
    }
    Ok(())
}

/// The names of the snapshots, given newest first, that a prune at `now` deletes.
///
/// A snapshot dated after `now` was taken while the clock ran ahead. It is neither
/// deleted nor counted: counted, it would hold the newest hour, day and week of the
/// retention for good and push the real snapshots out. Once the clock passes its date
/// it is an ordinary snapshot again.
fn prunable(snapshots: &[(NaiveDateTime, String)], now: NaiveDateTime) -> Vec<&str> {
    let past: Vec<_> = snapshots.iter().filter(|(time, _)| *time <= now).collect();
    let times: Vec<_> = past.iter().map(|(time, _)| *time).collect();
    past.iter()
        .zip(retention::keep(&times))
        .filter(|(_, keep)| !keep)
        .map(|((_, id), _)| id.as_str())
        .collect()
}

fn list() -> Result<()> {
    for (_, id) in snapshots()? {
        println!("{id}");
    }
    Ok(())
}

/// Copies `path`, as it was in snapshot `id`, into `~/Ripristinati/<id>/` of the home it
/// belongs to, keeping its place in the tree below the home.
///
/// Two rules keep a root process from being steered by what a user put in their home.
/// The path is resolved inside the snapshot with `openat2` beneath the snapshot's root
/// and without following any symlink, so a link in the snapshot cannot point the copy
/// at another user's files or at the system. The copy itself runs as the home's owner,
/// from a directory descriptor this process opened, so it can only read what that user
/// could read and only write where that user can write: nothing in the home is ever
/// written by root, and `--update=none-fail` refuses to replace a file already there.
fn restore(id: &str, path: &Path) -> Result<()> {
    parse_id(id)
        .ok_or_else(|| anyhow!("{id} is not a snapshot name (see `athanor-backup list`)"))?;
    let relative = relative_to_home_root(path)?;
    let mut components = relative.components();
    let home_name = components
        .next()
        .map(|c| c.as_os_str())
        .ok_or_else(|| anyhow!("{} names no home", path.display()))?;
    let home = Path::new(HOME_ROOT).join(home_name);
    let home_meta = fs::symlink_metadata(&home)
        .with_context(|| format!("{} is not a home today", home.display()))?;
    if !home_meta.is_dir() || home_meta.uid() == 0 {
        bail!("{} is not a user's home", home.display());
    }

    let snapshot = File::options()
        .read(true)
        .custom_flags((OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW).bits())
        .open(Path::new(SNAPSHOT_DIR).join(id))
        .with_context(|| format!("snapshot {id} does not exist"))?;
    let name = relative
        .file_name()
        .ok_or_else(|| anyhow!("{} names no file", path.display()))?;
    let parent = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Without O_CLOEXEC on purpose: the descriptor is how the copy, running as the
    // user, reaches the snapshot it could not open by path. This process exits right
    // after the copy, which closes it.
    let parent_fd = openat2(
        snapshot.as_raw_fd(),
        parent,
        OpenHow::new()
            .flags(OFlag::O_PATH | OFlag::O_DIRECTORY)
            .resolve(
                ResolveFlag::RESOLVE_BENEATH
                    | ResolveFlag::RESOLVE_NO_SYMLINKS
                    | ResolveFlag::RESOLVE_NO_MAGICLINKS
                    | ResolveFlag::RESOLVE_NO_XDEV,
            ),
    )
    .with_context(|| {
        format!(
            "{} is not a directory in snapshot {id} (symlinks are not followed)",
            parent.display()
        )
    })?;
    fstatat(
        Some(parent_fd),
        name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .with_context(|| format!("{} is not in snapshot {id}", path.display()))?;

    let below_home = relative
        .parent()
        .and_then(|parent| parent.strip_prefix(home_name).ok())
        .unwrap_or(Path::new(""));
    let destination = home.join(RESTORE_DIR).join(id).join(below_home);
    let mut source = OsString::from(format!("/proc/self/fd/{parent_fd}/"));
    source.push(name);

    let (uid, gid) = (home_meta.uid(), home_meta.gid());
    as_user(
        uid,
        gid,
        &[
            OsStr::new("mkdir"),
            OsStr::new("-p"),
            OsStr::new("--"),
            destination.as_os_str(),
        ],
    )?;
    as_user(
        uid,
        gid,
        &[
            OsStr::new("cp"),
            OsStr::new("-a"),
            OsStr::new("--reflink=auto"),
            OsStr::new("--update=none-fail"),
            OsStr::new("--"),
            &source,
            destination.as_os_str(),
        ],
    )?;
    println!("restored into {}", destination.join(name).display());
    Ok(())
}

/// `path`, given under `/var/home` or `/home` (a link to it), relative to `/var/home`.
/// Only plain names are accepted below the root: no `..`, no `.`.
fn relative_to_home_root(path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(HOME_ROOT)
        .or_else(|_| path.strip_prefix("/home"))
        .map_err(|_| anyhow!("{} is not under /home or /var/home", path.display()))?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        bail!(
            "{} must name something inside a home, without `.` or `..`",
            path.display()
        );
    }
    Ok(relative.to_path_buf())
}

/// The creation time a snapshot name stands for, if `name` is one.
fn parse_id(name: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(name, ID_FORMAT)
        .ok()
        .filter(|time| time.format(ID_FORMAT).to_string() == name)
}

/// The snapshots, newest first. Entries not named like a snapshot are left alone.
fn snapshots() -> Result<Vec<(NaiveDateTime, String)>> {
    let entries = match fs::read_dir(SNAPSHOT_DIR) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {SNAPSHOT_DIR}")),
    };
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {SNAPSHOT_DIR}"))?;
        if let Some(name) = entry.file_name().to_str() {
            if let Some(time) = parse_id(name) {
                snapshots.push((time, name.to_owned()));
            }
        }
    }
    snapshots.sort_unstable_by_key(|snapshot| std::cmp::Reverse(snapshot.0));
    Ok(snapshots)
}

/// Fails unless `path` is the root of a btrfs subvolume, not a link to one.
fn ensure_subvolume(path: &Path) -> Result<()> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("{} is missing", path.display()))?;
    if !meta.is_dir() || meta.ino() != BTRFS_SUBVOLUME_ROOT_INO {
        bail!("{} is not a btrfs subvolume", path.display());
    }
    Ok(())
}

fn btrfs(args: &[&OsStr]) -> Result<()> {
    let status = Command::new("btrfs")
        .arg("-q")
        .args(args)
        .status()
        .context("cannot run btrfs")?;
    if !status.success() {
        bail!("btrfs {} failed: {status}", display_args(args));
    }
    Ok(())
}

/// Runs `command` as `uid`:`gid` with the user's groups, no capabilities and no way to
/// regain privileges, in a clean environment.
fn as_user(uid: u32, gid: u32, command: &[&OsStr]) -> Result<()> {
    let status = Command::new("setpriv")
        .args(["--reuid", &uid.to_string(), "--regid", &gid.to_string()])
        .args([
            "--init-groups",
            "--no-new-privs",
            "--inh-caps=-all",
            "--bounding-set=-all",
            "--",
        ])
        .args(command)
        .env_clear()
        .env("PATH", "/usr/bin")
        .current_dir("/")
        .status()
        .context("cannot run setpriv")?;
    if !status.success() {
        bail!("{} failed: {status}", display_args(command));
    }
    Ok(())
}

fn display_args(args: &[&OsStr]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_round_trip_and_nothing_else_parses() {
        let id = "20260924T081500Z";
        assert!(parse_id(id).is_some());
        for name in [
            "",
            "latest",
            "2026-09-24T08:15:00Z",
            "20260924T081500",
            "20260924T081500Z/..",
            "../20260924T081500Z",
        ] {
            assert!(
                parse_id(name).is_none(),
                "{name} must not be a snapshot name"
            );
        }
    }

    #[test]
    fn a_prune_deletes_the_older_snapshot_of_an_hour_and_ignores_future_ones() {
        let snapshots: Vec<_> = ["20270101T000000Z", "20260924T081500Z", "20260924T080000Z"]
            .into_iter()
            .map(|id| (parse_id(id).expect("valid name"), id.to_owned()))
            .collect();
        let now = parse_id("20260924T090000Z").expect("valid name");
        assert_eq!(prunable(&snapshots, now), ["20260924T080000Z"]);
        // Once the clock reaches its date it counts like any other and is still kept.
        let later = parse_id("20270101T000000Z").expect("valid name");
        assert_eq!(prunable(&snapshots, later), ["20260924T080000Z"]);
    }

    #[test]
    fn paths_resolve_under_either_home_root() {
        for root in ["/var/home", "/home"] {
            assert_eq!(
                relative_to_home_root(Path::new(&format!("{root}/tester/Documenti/a.txt")))
                    .expect("valid path"),
                PathBuf::from("tester/Documenti/a.txt")
            );
        }
        assert_eq!(
            relative_to_home_root(Path::new("/home/tester")).expect("a whole home"),
            PathBuf::from("tester")
        );
    }

    #[test]
    fn paths_outside_a_home_or_with_parent_steps_are_refused() {
        for path in [
            "/etc/shadow",
            "/var/home",
            "/home/",
            "/home/tester/../other",
            "tester/a",
            "/var/homeother/a",
            "/home/../etc",
        ] {
            assert!(
                relative_to_home_root(Path::new(path)).is_err(),
                "{path} must be refused"
            );
        }
    }

    #[test]
    fn a_non_root_caller_is_told_to_use_sudo_and_a_bad_command_gets_the_usage() {
        let err = run(&["frobnicate".to_owned()]).expect_err("unknown command");
        assert!(err.to_string().starts_with("usage:"));
        if !nix::unistd::geteuid().is_root() {
            let err = run(&["list".to_owned()]).expect_err("not root");
            assert!(err.to_string().contains("sudo athanor-backup list"));
        }
    }
}
