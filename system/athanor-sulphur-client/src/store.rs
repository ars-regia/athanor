//! The Athanor preference store, `$XDG_CONFIG_HOME/athanor/sulphur/` (ADR-0150;
//! `doc_own_stack.md` decision 38): one TOML file per family, each with its own `schema`.
//!
//! Every change is a read, a modification and a write under an advisory `flock` on `.lock` in
//! the directory, taken without blocking and retried for at most `LOCK_BOUND`, so two writers
//! in two processes never lose each other's change and a holder that hangs never hangs
//! another writer (point 4). The file is replaced whole through `athanor_layout::atomic`, so a
//! reader never sees half of it. A writer merges its change into the file as an untyped
//! `toml::Table`, so a key or a table a newer image added survives the rewrite (point 3).
//!
//! A writer refuses, and writes nothing, when its file does not parse, has no `schema` or a
//! newer one, is larger than `LIMIT`, is not a regular file, or is a symbolic link, which its
//! rename would replace with a file.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use toml::{Table, Value};

/// The schema of every file this client writes (ADR-0150 point 3).
pub const SCHEMA: i64 = 1;

/// The largest file read or written: Sulphur's bound on the same files (`src/xkb_config.rs`).
pub const LIMIT: u64 = 64 * 1024;

/// How long a writer waits for the lock before it gives up (ADR-0150 point 4).
pub const LOCK_BOUND: Duration = Duration::from_secs(1);

/// Between two tries of the lock.
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// The directory under the configuration root.
const RELATIVE: &str = "athanor/sulphur";

/// The lock every writer takes, in the directory.
const LOCK: &str = ".lock";

/// Why a file of the store was not read or not written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("neither XDG_CONFIG_HOME nor HOME is an absolute path")]
    NoDirectory,
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}: not a regular file", path.display())]
    NotRegular { path: PathBuf },
    #[error("{}: a symbolic link, which a rewrite would replace", path.display())]
    Link { path: PathBuf },
    #[error("{}: larger than {LIMIT} bytes", path.display())]
    TooLarge { path: PathBuf },
    #[error("{}: not TOML: {reason}", path.display())]
    Malformed { path: PathBuf, reason: String },
    #[error("{}: {found}, not schema {SCHEMA}", path.display())]
    Schema { path: PathBuf, found: String },
    #[error("{}: schema {found}, newer than {SCHEMA}", path.display())]
    Newer { path: PathBuf, found: i64 },
    #[error("{}: the lock stayed held for {LOCK_BOUND:?}", path.display())]
    Locked { path: PathBuf },
    #[error("{}: {reason}", path.display())]
    Refused { path: PathBuf, reason: String },
}

impl Error {
    fn io(path: &Path, source: io::Error) -> Error {
        Error::Io {
            path: path.to_owned(),
            source,
        }
    }
}

/// The store's directory.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `$XDG_CONFIG_HOME/athanor/sulphur`, else `$HOME/.config/athanor/sulphur`.
    pub fn from_env() -> Result<Store, Error> {
        athanor_layout::loader::config_home()
            .map(|home| Store::at(home.join(RELATIVE)))
            .ok_or(Error::NoDirectory)
    }

    /// The store at `dir`.
    pub fn at(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes the store's lock, creating the directory and `.lock` if they are absent. Fails
    /// with `Error::Locked` once `LOCK_BOUND` has passed with the lock held elsewhere.
    pub fn lock(&self) -> Result<Locked<'_>, Error> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(&self.dir)
            .map_err(|err| Error::io(&self.dir, err))?;
        let path = self.dir.join(LOCK);
        let file = open(&path, true)?
            .ok_or_else(|| Error::io(&path, io::Error::from(io::ErrorKind::NotFound)))?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Locked {
                        store: self,
                        _lock: file,
                    })
                }
                Err(fs::TryLockError::WouldBlock) if start.elapsed() < LOCK_BOUND => {
                    std::thread::sleep(LOCK_RETRY);
                }
                Err(fs::TryLockError::WouldBlock) => return Err(Error::Locked { path }),
                Err(fs::TryLockError::Error(err)) => return Err(Error::io(&path, err)),
            }
        }
    }

    /// `change` merged into the file `name` under the lock (`Locked::update`).
    pub fn update(
        &self,
        name: &str,
        change: impl FnOnce(&Path, &mut Table) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.lock()?.update(name, change)
    }
}

/// The store with its lock held; the lock goes with it.
#[derive(Debug)]
pub struct Locked<'a> {
    store: &'a Store,
    _lock: File,
}

impl Locked<'_> {
    /// The table of the file `name`, its `schema` checked; `None` when it is absent.
    pub fn read(&self, name: &str) -> Result<Option<Table>, Error> {
        let path = self.store.dir.join(name);
        let Some(file) = open(&path, false)? else {
            return Ok(None);
        };
        let mut text = String::new();
        file.take(LIMIT.saturating_add(1))
            .read_to_string(&mut text)
            .map_err(|err| Error::io(&path, err))?;
        if text.len() as u64 > LIMIT {
            return Err(Error::TooLarge { path });
        }
        let table: Table = toml::from_str(&text).map_err(|err| Error::Malformed {
            path: path.clone(),
            reason: err.message().to_owned(),
        })?;
        match table.get("schema") {
            Some(Value::Integer(SCHEMA)) => Ok(Some(table)),
            Some(Value::Integer(found)) if *found > SCHEMA => Err(Error::Newer {
                path,
                found: *found,
            }),
            Some(Value::Integer(found)) => Err(Error::Schema {
                path,
                found: format!("schema {found}"),
            }),
            Some(other) => Err(Error::Schema {
                path,
                found: format!("a {} schema", other.type_str()),
            }),
            None => Err(Error::Schema {
                path,
                found: "no schema".to_owned(),
            }),
        }
    }

    /// Reads the file `name`, or starts it with `schema` alone, applies `change` and replaces
    /// the file whole. Every key and table `change` leaves alone keeps its value; comments and
    /// the order of the keys are not kept. Nothing is written when the read or `change` fails.
    pub fn update(
        &self,
        name: &str,
        change: impl FnOnce(&Path, &mut Table) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let path = self.store.dir.join(name);
        let mut table = self.read(name)?.unwrap_or_else(|| {
            let mut table = Table::new();
            table.insert("schema".to_owned(), Value::Integer(SCHEMA));
            table
        });
        change(&path, &mut table)?;
        let text = toml::to_string(&table).map_err(|err| Error::Refused {
            path: path.clone(),
            reason: err.to_string(),
        })?;
        if text.len() as u64 > LIMIT {
            return Err(Error::TooLarge { path });
        }
        athanor_layout::atomic::write_atomically(&path, &text).map_err(|err| Error::io(&path, err))
    }
}

/// Opens `path` without following a link and without blocking, so a FIFO or a device never
/// hangs the writer; `create` makes an absent file, mode 0600. `None` when it is absent and
/// not created.
fn open(path: &Path, create: bool) -> Result<Option<File>, Error> {
    let opened = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(path);
    let file = match opened {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
            return Err(Error::Link {
                path: path.to_owned(),
            })
        }
        Err(err) => return Err(Error::io(path, err)),
    };
    let metadata = file.metadata().map_err(|err| Error::io(path, err))?;
    if !metadata.is_file() {
        return Err(Error::NotRegular {
            path: path.to_owned(),
        });
    }
    Ok(Some(file))
}

/// The table at `key` of `table`, made if absent; a value of another type there is refused,
/// never replaced.
pub(crate) fn table_mut<'t>(
    path: &Path,
    table: &'t mut Table,
    key: &str,
) -> Result<&'t mut Table, Error> {
    match table
        .entry(key.to_owned())
        .or_insert_with(|| Value::Table(Table::new()))
    {
        Value::Table(inner) => Ok(inner),
        other => Err(Error::Refused {
            path: path.to_owned(),
            reason: format!("{key} is a {}, not a table", other.type_str()),
        }),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::BufRead;
    use std::process::{Child, Command, Stdio};

    /// A new, empty directory for one test.
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-store-{name}-{}", std::process::id()));
        let _absent = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn set(store: &Store, key: &str, value: i64) -> Result<(), Error> {
        store.update("f.toml", |_, table| {
            table.insert(key.to_owned(), Value::Integer(value));
            Ok(())
        })
    }

    /// Runs this test binary's `child` test in a process of its own, with `job` for it.
    pub(crate) fn spawn(job: &str) -> Child {
        Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "store::tests::child", "--ignored", "--nocapture"])
            .args(["--test-threads", "1"])
            .env("ATHANOR_STORE_CHILD", job)
            .stdout(Stdio::piped())
            .spawn()
            .expect("child")
    }

    /// What another process does in the tests below: `hold <dir>` takes the lock, says so and
    /// keeps it; `count <dir> <n>` adds one to `count` of `f.toml` `n` times, a lock each.
    #[test]
    #[ignore = "run by the tests below in a process of its own"]
    fn child() {
        let Ok(job) = std::env::var("ATHANOR_STORE_CHILD") else {
            return;
        };
        let words: Vec<&str> = job.split(' ').collect();
        match words.as_slice() {
            ["hold", dir] => {
                let store = Store::at(PathBuf::from(dir));
                let _locked = store.lock().expect("lock");
                println!("held");
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            ["count", dir, times] => {
                let store = Store::at(PathBuf::from(dir));
                for _ in 0..times.parse::<u32>().expect("times") {
                    store
                        .update("f.toml", |_, table| {
                            let count = table.get("count").and_then(Value::as_integer);
                            table.insert("count".into(), Value::Integer(count.unwrap_or(0) + 1));
                            Ok(())
                        })
                        .expect("update");
                }
            }
            other => panic!("unknown job {other:?}"),
        }
    }

    /// Waits until a `hold` child holds the lock.
    pub(crate) fn held(child: &mut Child) {
        let out = child.stdout.take().expect("stdout");
        let said = io::BufReader::new(out)
            .lines()
            .map(|line| line.expect("line"))
            // libtest prints the test's name before its output, on the same line.
            .any(|line| line.ends_with("held"));
        assert!(said, "the child never held the lock");
    }

    #[test]
    fn two_writers_in_two_processes_keep_both_changes() {
        let dir = scratch("two-writers");
        let job = format!("count {} 50", dir.display());
        let mut writers = [spawn(&job), spawn(&job)];
        for writer in &mut writers {
            assert!(writer.wait().expect("wait").success());
        }
        let store = Store::at(dir.clone());
        let table = store.lock().expect("lock").read("f.toml").expect("read");
        assert_eq!(
            table.expect("written").get("count"),
            Some(&Value::Integer(100))
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_lock_held_by_a_stopped_process_fails_the_writer_within_the_bound() {
        let dir = scratch("stopped-holder");
        let mut holder = spawn(&format!("hold {}", dir.display()));
        held(&mut holder);
        let stopped = Command::new("kill")
            .args(["-STOP", &holder.id().to_string()])
            .status()
            .expect("kill");
        assert!(stopped.success());
        let store = Store::at(dir.clone());
        let start = Instant::now();
        let refused = set(&store, "a", 1);
        let waited = start.elapsed();
        assert!(matches!(refused, Err(Error::Locked { .. })), "{refused:?}");
        assert!(waited >= LOCK_BOUND, "{waited:?}");
        assert!(waited < LOCK_BOUND * 3, "{waited:?}");
        assert!(!dir.join("f.toml").exists(), "nothing written");
        holder.kill().expect("kill the holder");
        holder.wait().expect("wait");
        set(&store, "a", 1).expect("the lock is free once its holder is gone");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_write_replaces_the_file_by_a_rename() {
        let dir = scratch("atomic");
        let store = Store::at(dir.clone());
        set(&store, "a", 1).expect("first");
        let mut old = File::open(dir.join("f.toml")).expect("open the first file");
        set(&store, "a", 2).expect("second");
        let mut text = String::new();
        old.read_to_string(&mut text).expect("read the old file");
        assert!(text.contains("a = 1"), "the old file is untouched: {text}");
        let now = fs::read_to_string(dir.join("f.toml")).expect("read");
        assert!(now.contains("a = 2") && now.contains("schema = 1"), "{now}");
        let mut names: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        names.sort();
        assert_eq!(names, [".lock", "f.toml"], "no temporary is left");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn unknown_keys_and_tables_are_kept() {
        let dir = scratch("unknown");
        fs::write(
            dir.join("f.toml"),
            "schema = 1\nlater = \"kept\"\n[future]\nx = [1, 2]\n",
        )
        .expect("seed");
        set(&Store::at(dir.clone()), "a", 1).expect("write");
        let table: Table =
            toml::from_str(&fs::read_to_string(dir.join("f.toml")).expect("read")).expect("toml");
        assert_eq!(table["later"].as_str(), Some("kept"));
        assert_eq!(table["future"]["x"], Value::Array(vec![1.into(), 2.into()]));
        assert_eq!(table["a"].as_integer(), Some(1));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Each file a writer refuses is left as it was.
    #[test]
    fn a_writer_refuses_what_it_cannot_keep() {
        let dir = scratch("refused");
        let store = Store::at(dir.clone());
        let file = dir.join("f.toml");
        type Expected = fn(&Error) -> bool;
        let cases: [(&str, Expected); 5] = [
            ("schema = 2\n", |e| {
                matches!(e, Error::Newer { found: 2, .. })
            }),
            ("a = 1\n", |e| matches!(e, Error::Schema { .. })),
            ("schema = \"1\"\n", |e| matches!(e, Error::Schema { .. })),
            ("schema = 0\n", |e| matches!(e, Error::Schema { .. })),
            ("schema = 1\n[a\n", |e| matches!(e, Error::Malformed { .. })),
        ];
        for (text, expected) in cases {
            fs::write(&file, text).expect("seed");
            let refused = set(&store, "b", 1).expect_err(text);
            assert!(expected(&refused), "{text}: {refused:?}");
            assert_eq!(fs::read_to_string(&file).expect("read"), text);
        }
        fs::write(&file, vec![b' '; LIMIT as usize + 1]).expect("large");
        assert!(matches!(set(&store, "b", 1), Err(Error::TooLarge { .. })));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_fifo_or_a_link_in_place_of_a_file_is_refused() {
        let dir = scratch("fifo-link");
        let store = Store::at(dir.clone());
        let file = dir.join("f.toml");
        let made = Command::new("mkfifo").arg(&file).status().expect("mkfifo");
        assert!(made.success());
        assert!(
            matches!(set(&store, "a", 1), Err(Error::NotRegular { .. })),
            "a FIFO, refused without blocking"
        );
        fs::remove_file(&file).expect("remove");
        let target = dir.join("target.toml");
        fs::write(&target, "schema = 1\n").expect("target");
        std::os::unix::fs::symlink(&target, &file).expect("link");
        assert!(matches!(set(&store, "a", 1), Err(Error::Link { .. })));
        assert_eq!(fs::read_to_string(&target).expect("read"), "schema = 1\n");
        assert!(fs::symlink_metadata(&file).expect("link").is_symlink());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_lock_file_that_is_not_a_regular_file_is_refused() {
        let dir = scratch("lock-link");
        std::os::unix::fs::symlink("/dev/null", dir.join(LOCK)).expect("link");
        assert!(matches!(
            Store::at(dir.clone()).lock(),
            Err(Error::Link { .. })
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_table_of_another_type_is_refused() {
        let mut table: Table = toml::from_str("schema = 1\nmouse = 3\n").expect("toml");
        assert!(table_mut(Path::new("f"), &mut table, "mouse").is_err());
        assert!(table_mut(Path::new("f"), &mut table, "touchpad").is_ok());
    }
}
