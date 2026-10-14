//! The greeter's account list (doc_lock_and_prompts.md, LP19): whether it is shown, which
//! accounts it holds, in what order and under which names. Free of GTK, so that every rule
//! is tested on fixture files; the window only draws what `load` returns.
//!
//! Everything here runs once, before the window exists and while the process has one
//! thread: getpwent and uselocale are not thread-safe, and the read of lastlog2 must not
//! run on GTK's main loop.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, CStr, CString};
use std::ops::RangeInclusive;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

/// Where the greeter reads what LP19 needs. A struct so that the tests point it at fixtures.
pub struct Sources<'a> {
    /// The root-owned switch of L21: `user-list = false` hides the list.
    pub greeter_toml: &'a str,
    /// `UID_MIN` and `UID_MAX`, as first run reads them (FR1).
    pub login_defs: &'a str,
    /// The login shells an account must have to be listed, as getusershell(3) reads them.
    pub shells: &'a str,
    /// pam_lastlog2's database: the order of D20.
    pub lastlog2: &'a str,
}

/// The files of the running system.
pub const SYSTEM: Sources<'static> = Sources {
    greeter_toml: "/etc/athanor/greeter.toml",
    login_defs: "/etc/login.defs",
    shells: "/etc/shells",
    lastlog2: "/var/lib/lastlog/lastlog2.db",
};

/// The uid range when /etc/login.defs gives none, or one out of bounds.
const DEFAULT_UIDS: RangeInclusive<u32> = 1000..=60000;
/// The lowest `UID_MIN` the greeter accepts: below it, system accounts would be listed.
const LOWEST_UID_MIN: u32 = 1000;
/// A lastlog2 database larger than this is not opened (D22).
pub const LASTLOG2_MAX_BYTES: u64 = 1024 * 1024;
/// The PAM service whose lastlog2 records order the list: a sign-in at this greeter.
const GREETD_SERVICE: &str = "greetd";
/// The longest name "Other user" sends to greetd.
pub const MAX_TYPED_NAME_BYTES: usize = 256;
/// What passwd(5) runs for an empty shell field.
const EMPTY_SHELL: &str = "/bin/sh";

/// One account record, as NSS gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub uid: u32,
    pub gecos: String,
    pub shell: String,
}

/// One card of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The name greetd is asked for.
    pub user: String,
    /// The first comma-separated field of GECOS, trimmed, or the user name when it is empty.
    pub full_name: String,
    /// Another listed account has the same full name, so the card shows the user name too.
    pub shows_user: bool,
}

impl Account {
    /// The initial drawn on the accent.
    pub fn initial(&self) -> String {
        self.full_name
            .chars()
            .next()
            .map(|first| first.to_uppercase().collect())
            .unwrap_or_default()
    }

    /// The name a screen reader reads for the card: "Ada Lovelace, ada".
    pub fn accessible_name(&self) -> String {
        format!("{}, {}", self.full_name, self.user)
    }
}

/// The accounts to list, in order; empty when the list is hidden or nothing is listed, and
/// then the greeter shows "Other user" alone. `notes` receives one line for each fallback
/// taken, for the journal.
pub fn load(
    sources: &Sources,
    entries: impl FnOnce() -> Vec<Entry>,
    collate: impl Fn(&str, &str) -> Ordering,
    notes: &mut Vec<String>,
) -> Vec<Account> {
    match list_shown(std::fs::read_to_string(sources.greeter_toml)) {
        Ok(true) => {}
        // No account is read and the database stays unopened: nothing of them is shown.
        Ok(false) => return Vec::new(),
        Err(reason) => {
            notes.push(format!(
                "{}: {reason}; the account list is hidden",
                sources.greeter_toml
            ));
            return Vec::new();
        }
    }
    let uids = uid_range(
        std::fs::read_to_string(sources.login_defs).ok().as_deref(),
        notes,
    );
    let shells = login_shells(std::fs::read_to_string(sources.shells).ok().as_deref());
    let mut accounts = listed(entries(), &uids, &shells);
    if accounts.is_empty() {
        notes.push(format!(
            "no account with a uid from {} to {} and a login shell: only \"Other user\" is shown",
            uids.start(),
            uids.end()
        ));
        return accounts;
    }
    let sign_ins = greetd_sign_ins(Path::new(sources.lastlog2)).unwrap_or_else(|reason| {
        notes.push(format!(
            "{}: {reason}; the accounts follow the name order",
            sources.lastlog2
        ));
        HashMap::new()
    });
    order(&mut accounts, &sign_ins, collate);
    accounts
}

/// The list of the running system, each fallback taken logged once: what the greeter shows.
pub fn load_system() -> Vec<Account> {
    let mut notes = Vec::new();
    let locale = collation_locale(std::fs::read_to_string("/etc/locale.conf").ok().as_deref())
        .unwrap_or_else(|| "C".to_string());
    let collator = Collator::new(&locale);
    if collator.is_none() {
        notes.push(format!(
            "no collation for the locale {locale}: the names are sorted by their bytes"
        ));
    }
    let accounts = load(
        &SYSTEM,
        nss_entries,
        |a, b| match &collator {
            Some(collator) => collator.compare(a, b),
            None => a.cmp(b),
        },
        &mut notes,
    );
    for note in notes {
        tracing::warn!("{note}");
    }
    accounts
}

/// Whether /etc/athanor/greeter.toml lets the list show, from the result of reading it. A
/// missing file is the default, shown; a file that cannot be read or parsed, or whose
/// `user-list` is not a boolean, is an error, and the caller hides the list: the switch
/// fails closed.
pub fn list_shown(read: std::io::Result<String>) -> Result<bool, String> {
    let text = match read {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(format!("cannot be read: {err}")),
    };
    let table: toml::Table = text
        .parse()
        .map_err(|err: toml::de::Error| format!("cannot be parsed: {}", err.message()))?;
    match table.get("user-list") {
        None => Ok(true),
        Some(toml::Value::Boolean(shown)) => Ok(*shown),
        Some(other) => Err(format!(
            "user-list is a {}, not a boolean",
            other.type_str()
        )),
    }
}

/// The uids of the accounts a person signs in to, from the text of /etc/login.defs: its
/// `UID_MIN` to `UID_MAX`, or 1000 to 60000 when either is missing or the file is
/// unreadable (None), and when `UID_MIN` is below 1000 or `UID_MAX` below `UID_MIN`, which
/// adds a note naming the values ignored.
pub fn uid_range(login_defs: Option<&str>, notes: &mut Vec<String>) -> RangeInclusive<u32> {
    let value = |key: &str| {
        login_defs?
            .lines()
            // shadow-utils reads the file top to bottom, so a later line wins.
            .rev()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                (words.next() == Some(key)).then(|| words.next()?.parse::<u32>().ok())?
            })
    };
    match (value("UID_MIN"), value("UID_MAX")) {
        (Some(min), Some(max)) if min >= LOWEST_UID_MIN && max >= min => min..=max,
        (Some(min), Some(max)) => {
            notes.push(format!(
                "/etc/login.defs: UID_MIN {min} and UID_MAX {max} ignored, the list takes \
                 uids from {} to {}",
                DEFAULT_UIDS.start(),
                DEFAULT_UIDS.end()
            ));
            DEFAULT_UIDS
        }
        _ => DEFAULT_UIDS,
    }
}

/// The login shells of /etc/shells as getusershell(3) of glibc reads them: on each line, the
/// word that starts at the first `/` before any `#`. An unreadable file (None) gives glibc's
/// own defaults.
pub fn login_shells(shells: Option<&str>) -> HashSet<String> {
    let Some(text) = shells else {
        return ["/bin/sh", "/bin/csh"].map(str::to_string).into();
    };
    text.lines()
        .filter_map(|line| {
            let line = &line[..line.find('#').unwrap_or(line.len())];
            let path = &line[line.find('/')?..];
            path.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

/// The accounts the list holds, in no order yet: once per user name, the first record
/// winning, with a uid in `uids` and a login shell in `shells`, an empty shell counting as
/// /bin/sh. Locked and expired accounts are listed: NSS's passwd record says nothing of it.
pub fn listed(
    entries: Vec<Entry>,
    uids: &RangeInclusive<u32>,
    shells: &HashSet<String>,
) -> Vec<Account> {
    let mut seen = HashSet::new();
    let mut accounts: Vec<Account> = entries
        .into_iter()
        .filter(|entry| seen.insert(entry.name.clone()))
        .filter(|entry| {
            let shell = if entry.shell.is_empty() {
                EMPTY_SHELL
            } else {
                entry.shell.as_str()
            };
            uids.contains(&entry.uid) && shells.contains(shell)
        })
        .map(|entry| {
            let full_name = entry.gecos.split(',').next().unwrap_or_default().trim();
            Account {
                full_name: if full_name.is_empty() {
                    entry.name.clone()
                } else {
                    full_name.to_string()
                },
                user: entry.name,
                shows_user: false,
            }
        })
        .collect();
    let mut names: HashMap<String, usize> = HashMap::new();
    for account in &accounts {
        *names.entry(account.full_name.clone()).or_default() += 1;
    }
    for account in &mut accounts {
        account.shows_user = names
            .get(&account.full_name)
            .is_some_and(|count| *count > 1);
    }
    accounts
}

/// D20: the latest sign-in at the greeter first, then the accounts with none by full name
/// in `collate`'s order, then by user name.
pub fn order(
    accounts: &mut [Account],
    sign_ins: &HashMap<String, i64>,
    collate: impl Fn(&str, &str) -> Ordering,
) {
    accounts.sort_by(|a, b| {
        // None sorts before Some, so comparing b to a puts the newest first and None last.
        sign_ins
            .get(&b.user)
            .cmp(&sign_ins.get(&a.user))
            .then_with(|| collate(&a.full_name, &b.full_name))
            .then_with(|| a.user.cmp(&b.user))
    });
}

/// The time of each account's latest sign-in at the greeter, from pam_lastlog2's database:
/// only the records whose service is greetd. The file is refused above
/// LASTLOG2_MAX_BYTES before it is opened, opened read-only, and never waited on: a
/// database locked by a writer, or beside a hot journal, is an error, not a delay.
pub fn greetd_sign_ins(path: &Path) -> Result<HashMap<String, i64>, String> {
    let size = std::fs::metadata(path)
        .map_err(|err| format!("cannot be read: {err}"))?
        .len();
    if size > LASTLOG2_MAX_BYTES {
        return Err(format!(
            "{size} bytes, more than the {LASTLOG2_MAX_BYTES} the greeter reads"
        ));
    }
    let unreadable = |err: rusqlite::Error| format!("cannot be read as a lastlog2 database: {err}");
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(unreadable)?;
    // rusqlite waits five seconds on a lock by default.
    db.busy_timeout(Duration::ZERO).map_err(unreadable)?;
    let mut records = db
        .prepare("SELECT Name, Time, Service FROM Lastlog2")
        .map_err(unreadable)?;
    let rows = records
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(unreadable)?;
    let mut sign_ins = HashMap::new();
    for row in rows {
        let (name, time, service) = row.map_err(unreadable)?;
        if let (Some(time), Some(GREETD_SERVICE)) = (time, service.as_deref()) {
            sign_ins.insert(name, time);
        }
    }
    Ok(sign_ins)
}

/// Whether "Other user" sends `name` to greetd: not empty, at most MAX_TYPED_NAME_BYTES
/// bytes, no control character. Nothing else is checked, and nothing is looked up.
pub fn typed_name_is_valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_TYPED_NAME_BYTES && !name.chars().any(char::is_control)
}

/// Every account NSS enumerates, in its order: getpwent(3) over the sources of
/// nsswitch.conf. A record without a name, or whose name is not UTF-8, is left out: greetd
/// takes the name as a JSON string, and a lossy copy would ask for another account.
///
/// getpwent keeps its position and its record in process-global state and is not
/// thread-safe: main.rs calls this once, while the process has one thread
/// (sandbox::ensure_single_threaded), and nothing else in the process enumerates accounts.
#[allow(unsafe_code)]
pub fn nss_entries() -> Vec<Entry> {
    let mut entries = Vec::new();
    // SAFETY: no other thread exists to race on getpwent's state (see above).
    unsafe { libc::setpwent() };
    loop {
        // SAFETY: as for setpwent; the record it returns is read before the next call.
        let record = unsafe { libc::getpwent() };
        // SAFETY: getpwent returns null at the end, else a valid record that lives until
        // the next call, and this iteration copies everything it needs from it.
        let Some(record) = (unsafe { record.as_ref() }) else {
            break;
        };
        let text = |field: *const c_char| {
            // SAFETY: a non-null string field of a getpwent record is NUL-terminated and
            // lives as long as the record.
            (!field.is_null()).then(|| unsafe { CStr::from_ptr(field) })
        };
        let lossy = |field| {
            text(field)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        if let Some(name) = text(record.pw_name).and_then(|name| name.to_str().ok()) {
            entries.push(Entry {
                name: name.to_string(),
                uid: record.pw_uid,
                gecos: lossy(record.pw_gecos),
                shell: lossy(record.pw_shell),
            });
        }
    }
    // SAFETY: as for setpwent.
    unsafe { libc::endpwent() };
    entries
}

/// The locale whose collation orders the names: LC_ALL, LC_COLLATE or LANG of the
/// environment, the first that is set, else LC_COLLATE or LANG of /etc/locale.conf.
pub fn collation_locale(locale_conf: Option<&str>) -> Option<String> {
    ["LC_ALL", "LC_COLLATE", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .or_else(|| {
            let lines: Vec<&str> = locale_conf?.lines().map(str::trim).collect();
            ["LC_COLLATE=", "LANG="].iter().find_map(|key| {
                lines.iter().find_map(|line| {
                    let value = line.strip_prefix(key)?.trim_matches('"');
                    (!value.is_empty()).then(|| value.to_string())
                })
            })
        })
}

/// The collation of one locale, compared through strcoll(3) without changing the process
/// locale: the greeter calls no setlocale (i18n.rs).
pub struct Collator(libc::locale_t);

impl Collator {
    /// The collation of `locale`, or None when the locale is not installed.
    #[allow(unsafe_code)]
    pub fn new(locale: &str) -> Option<Collator> {
        let name = CString::new(locale).ok()?;
        // strcoll reads the strings in the character set of LC_CTYPE: both come from the
        // locale, or a UTF-8 collation would compare the bytes of the C locale's ASCII.
        let categories = libc::LC_COLLATE_MASK | libc::LC_CTYPE_MASK;
        // SAFETY: `name` is a NUL-terminated string that outlives the call, and a null base
        // asks for a new locale object, which Drop frees.
        let handle = unsafe { libc::newlocale(categories, name.as_ptr(), std::ptr::null_mut()) };
        // `then`, not `then_some`: a Collator built for a null handle would be dropped, and
        // freelocale(NULL) is undefined.
        (!handle.is_null()).then(|| Collator(handle))
    }

    /// `a` against `b` in this collation; by bytes when either holds a NUL.
    #[allow(unsafe_code)]
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        let (Ok(left), Ok(right)) = (CString::new(a), CString::new(b)) else {
            return a.cmp(b);
        };
        // SAFETY: self.0 is a live locale object, owned by self; uselocale sets the locale
        // of the calling thread only, and the previous one is restored below.
        let previous = unsafe { libc::uselocale(self.0) };
        // SAFETY: two NUL-terminated strings that outlive the call.
        let order = unsafe { libc::strcoll(left.as_ptr(), right.as_ptr()) };
        // SAFETY: `previous` is the thread's locale before the call above (or null, which
        // only queries), so the thread leaves as it came.
        unsafe { libc::uselocale(previous) };
        order.cmp(&0)
    }
}

impl Drop for Collator {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: self.0 came from newlocale, is in use by no thread (compare restores the
        // previous locale) and is freed once.
        unsafe { libc::freelocale(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A directory of this test's own, named after the case.
    fn scratch(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "athanor-greeter-accounts-{}-{case}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// passwd(5) lines as NSS would give them, in order.
    fn passwd(text: &str) -> Vec<Entry> {
        text.lines()
            .map(|line| {
                let fields: Vec<&str> = line.split(':').collect();
                assert_eq!(fields.len(), 7, "a passwd line: {line}");
                Entry {
                    name: fields[0].to_string(),
                    uid: fields[2].parse().expect("a uid"),
                    gecos: fields[4].to_string(),
                    shell: fields[6].to_string(),
                }
            })
            .collect()
    }

    /// The image's /etc/shells.
    const IMAGE_SHELLS: &str = "/bin/sh\n/bin/bash\n/usr/bin/sh\n/usr/bin/bash\n";
    const LOGIN_DEFS: &str =
        "# comment\nUID_MIN                  1000\nUID_MAX                 60000\n";

    fn users(accounts: &[Account]) -> Vec<&str> {
        accounts
            .iter()
            .map(|account| account.user.as_str())
            .collect()
    }

    fn list(text: &str) -> Vec<Account> {
        let mut notes = Vec::new();
        let uids = uid_range(Some(LOGIN_DEFS), &mut notes);
        assert!(notes.is_empty(), "{notes:?}");
        listed(passwd(text), &uids, &login_shells(Some(IMAGE_SHELLS)))
    }

    #[test]
    fn only_accounts_in_the_uid_range_with_a_login_shell_are_listed() {
        let accounts = list(
            "systemd-oom:x:999:999:OOM:/:/bin/bash\n\
             ada:x:1000:1000:Ada Lovelace:/home/ada:/bin/bash\n\
             svc:x:1001:1001::/:/usr/sbin/nologin\n\
             nope:x:1002:1002::/:/bin/false\n\
             max:x:60000:60000:Max:/home/max:/usr/bin/bash\n\
             over:x:60001:60001::/:/bin/bash\n\
             runner:x:63893:63893::/:/bin/bash\n\
             nobody:x:65534:65534::/:/bin/bash",
        );
        assert_eq!(users(&accounts), ["ada", "max"]);
    }

    #[test]
    fn a_name_in_two_sources_is_listed_once_and_the_first_record_wins() {
        // /etc/passwd first, then /usr/lib/passwd, as nss-altfiles enumerates them.
        let accounts = list(
            "ada:x:1000:1000:Ada Lovelace:/home/ada:/bin/bash\n\
             svc:x:1001:1001::/:/usr/sbin/nologin\n\
             ada:x:1000:1000:Ada of the image:/home/ada:/bin/bash\n\
             svc:x:1001:1001::/:/bin/bash",
        );
        assert_eq!(accounts.len(), 1, "{accounts:?}");
        assert_eq!(accounts[0].full_name, "Ada Lovelace");
    }

    #[test]
    fn an_empty_shell_field_counts_as_bin_sh_and_is_listed() {
        assert_eq!(users(&list("ada:x:1000:1000::/home/ada:")), ["ada"]);
        // Without /bin/sh among the login shells it is not.
        let mut notes = Vec::new();
        let shells = login_shells(Some("/bin/bash\n"));
        let uids = uid_range(Some(LOGIN_DEFS), &mut notes);
        assert!(listed(passwd("ada:x:1000:1000::/home/ada:"), &uids, &shells).is_empty());
    }

    #[test]
    fn locked_and_expired_accounts_are_listed() {
        // A lock or an expiry lives in the shadow record, which the greeter never reads;
        // the old in-passwd conventions ("!", "*") do not hide an account either (L16).
        let accounts = list(
            "ada:!:1000:1000:Ada:/home/ada:/bin/bash\n\
             bea:*:1001:1001:Bea:/home/bea:/bin/bash\n\
             cy:!!:1002:1002:Cy:/home/cy:/bin/bash",
        );
        assert_eq!(users(&accounts), ["ada", "bea", "cy"]);
    }

    #[test]
    fn the_uid_range_falls_back_to_1000_to_60000() {
        let range = |text: Option<&str>| {
            let mut notes = Vec::new();
            let range = uid_range(text, &mut notes);
            (range, notes)
        };
        assert_eq!(
            range(Some("UID_MIN 2000\nUID_MAX 3000\n")),
            (2000..=3000, vec![])
        );
        // A later line wins, as shadow-utils reads the file.
        assert_eq!(
            range(Some("UID_MIN 2000\nUID_MAX 3000\nUID_MAX 4000\n")).0,
            2000..=4000
        );
        // Missing or unreadable: the defaults, without a note.
        assert_eq!(range(None), (1000..=60000, vec![]));
        assert_eq!(range(Some("UID_MAX 3000\n")), (1000..=60000, vec![]));
        assert_eq!(range(Some("UID_MIN 2000\n")), (1000..=60000, vec![]));
        assert_eq!(
            range(Some("UID_MIN x\nUID_MAX 3000\n")),
            (1000..=60000, vec![])
        );
        // Out of bounds: the defaults, and one note naming what was ignored.
        for (text, ignored) in [
            ("UID_MIN 0\nUID_MAX 60000\n", "UID_MIN 0 and UID_MAX 60000"),
            (
                "UID_MIN 999\nUID_MAX 60000\n",
                "UID_MIN 999 and UID_MAX 60000",
            ),
            (
                "UID_MIN 5000\nUID_MAX 4000\n",
                "UID_MIN 5000 and UID_MAX 4000",
            ),
        ] {
            let (range, notes) = range(Some(text));
            assert_eq!(range, 1000..=60000, "{text}");
            assert_eq!(notes.len(), 1, "{notes:?}");
            assert!(notes[0].contains(ignored), "{notes:?}");
        }
        // A one-account range is in bounds.
        assert_eq!(range(Some("UID_MIN 1000\nUID_MAX 1000\n")).0, 1000..=1000);
    }

    #[test]
    fn login_shells_are_read_as_getusershell_reads_them() {
        let shells = login_shells(Some(
            "# /etc/shells: valid login shells\n\n  /bin/bash  # the default\n/usr/bin/zsh\nnot-a-path\n#/bin/csh\n",
        ));
        let mut sorted: Vec<&str> = shells.iter().map(String::as_str).collect();
        sorted.sort();
        assert_eq!(sorted, ["/bin/bash", "/usr/bin/zsh"]);
        let mut defaults: Vec<String> = login_shells(None).into_iter().collect();
        defaults.sort();
        assert_eq!(defaults, ["/bin/csh", "/bin/sh"]);
    }

    #[test]
    fn the_card_shows_the_first_gecos_field_and_the_user_name_only_when_needed() {
        let accounts = list(
            "ada:x:1000:1000: Ada Lovelace ,Room 1,555:/home/ada:/bin/bash\n\
             bea:x:1001:1001:,Room 2:/home/bea:/bin/bash\n\
             sam1:x:1002:1002:Sam Smith:/home/sam1:/bin/bash\n\
             sam2:x:1003:1003:Sam Smith,x:/home/sam2:/bin/bash",
        );
        let cards: Vec<(&str, &str, bool)> = accounts
            .iter()
            .map(|a| (a.full_name.as_str(), a.user.as_str(), a.shows_user))
            .collect();
        assert_eq!(
            cards,
            [
                ("Ada Lovelace", "ada", false),
                ("bea", "bea", false),
                ("Sam Smith", "sam1", true),
                ("Sam Smith", "sam2", true),
            ]
        );
        assert_eq!(accounts[0].initial(), "A");
        assert_eq!(accounts[0].accessible_name(), "Ada Lovelace, ada");
    }

    #[test]
    fn the_list_switch_fails_closed() {
        let read = |text: &str| list_shown(Ok(text.to_string()));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            list_shown(Err(missing)),
            Ok(true),
            "a missing file is the default"
        );
        assert_eq!(read(""), Ok(true));
        assert_eq!(read("other = 1\n"), Ok(true));
        assert_eq!(read("user-list = true\n"), Ok(true));
        assert_eq!(read("user-list = false\n"), Ok(false));
        for hidden in [
            "user-list = \"no\"\n",
            "user-list = 0\n",
            "user-list = [false]\n",
        ] {
            let reason = read(hidden).expect_err(hidden);
            assert!(reason.contains("not a boolean"), "{reason}");
        }
        assert!(read("user-list = \n").is_err(), "malformed");
        assert!(
            read("user-list = false\nuser-list = true\n").is_err(),
            "a duplicate key"
        );
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(list_shown(Err(denied)).is_err(), "unreadable");
    }

    /// The fixture files for `load`, in a directory of the case's own.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(case: &str) -> Self {
            let dir = scratch(case);
            std::fs::write(dir.join("login.defs"), LOGIN_DEFS).expect("login.defs");
            std::fs::write(dir.join("shells"), IMAGE_SHELLS).expect("shells");
            Fixture { dir }
        }

        fn path(&self, name: &str) -> String {
            self.dir
                .join(name)
                .to_str()
                .expect("a UTF-8 path")
                .to_string()
        }

        /// `load` over the fixture, in `collate`'s order.
        fn load_with(
            &self,
            entries: impl FnOnce() -> Vec<Entry>,
            collate: impl Fn(&str, &str) -> Ordering,
        ) -> (Vec<Account>, Vec<String>) {
            let (toml, defs, shells, lastlog) = (
                self.path("greeter.toml"),
                self.path("login.defs"),
                self.path("shells"),
                self.path("lastlog2.db"),
            );
            let sources = Sources {
                greeter_toml: &toml,
                login_defs: &defs,
                shells: &shells,
                lastlog2: &lastlog,
            };
            let mut notes = Vec::new();
            let accounts = load(&sources, entries, collate, &mut notes);
            (accounts, notes)
        }

        /// `load` over the fixture, in `locale`'s collation.
        fn load(&self, entries: Vec<Entry>, locale: &str) -> (Vec<Account>, Vec<String>) {
            let collator = Collator::new(locale).expect("the locale is installed");
            self.load_with(|| entries, |a, b| collator.compare(a, b))
        }

        /// A lastlog2 database named `name` with pam_lastlog2's table and `records`.
        fn database(&self, name: &str, records: &[(&str, i64, &str)]) -> Connection {
            let db = Connection::open(self.dir.join(name)).expect("a database");
            db.execute_batch(
                "CREATE TABLE Lastlog2(Name TEXT PRIMARY KEY, Time INTEGER, TTY TEXT, \
                 RemoteHost TEXT, Service TEXT)",
            )
            .expect("the table");
            for (name, time, service) in records {
                db.execute(
                    "INSERT INTO Lastlog2 VALUES (?1, ?2, 'tty1', '', ?3)",
                    rusqlite::params![name, time, service],
                )
                .expect("a record");
            }
            db
        }

        /// The lastlog2 database `load` reads.
        fn lastlog(&self, records: &[(&str, i64, &str)]) -> Connection {
            self.database("lastlog2.db", records)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const ROSTER: &str = "zoe:x:1000:1000:Zoe:/home/zoe:/bin/bash\n\
         emile:x:1001:1001:Émile:/home/emile:/bin/bash\n\
         dan:x:1002:1002::/home/dan:/bin/bash\n\
         cy:x:1003:1003:Cy:/home/cy:/bin/bash\n\
         bea:x:1004:1004:Bea:/home/bea:/bin/bash\n\
         ada:x:1005:1005:Ada:/home/ada:/bin/bash\n\
         sam2:x:1006:1006:Sam:/home/sam2:/bin/bash\n\
         sam1:x:1007:1007:Sam:/home/sam1:/bin/bash";

    /// The name order of ROSTER: by full name in the locale's collation, where case and
    /// accents come second (bytes would put "Zoe" before "dan" and "Émile" last), then by
    /// user name.
    const NAME_ORDER: [&str; 8] = ["ada", "bea", "cy", "dan", "emile", "sam1", "sam2", "zoe"];

    /// The records of the order fixture: bea and ada signed in at the greeter, cy last came
    /// in over SSH, and a name that is not listed signed in last of all.
    const RECORDS: [(&str, i64, &str); 4] = [
        ("ada", 1_700_000_100, "greetd"),
        ("bea", 1_700_000_300, "greetd"),
        ("cy", 1_700_000_500, "sshd"),
        ("ghost", 1_700_000_900, "greetd"),
    ];

    /// RECORDS applied: the greeter's sign-ins newest first, then the name order.
    const RECORD_ORDER: [&str; 8] = ["bea", "ada", "cy", "dan", "emile", "sam1", "sam2", "zoe"];

    #[test]
    fn the_latest_sign_in_at_the_greeter_comes_first_then_the_names_in_the_locales_order() {
        for locale in ["en_US.UTF-8", "it_IT.UTF-8"] {
            let fixture = Fixture::new(&format!("order-{locale}"));
            drop(fixture.lastlog(&RECORDS));
            let (accounts, notes) = fixture.load(passwd(ROSTER), locale);
            assert_eq!(users(&accounts), RECORD_ORDER, "{locale}");
            assert!(notes.is_empty(), "{notes:?}");
            let (accounts, _) =
                Fixture::new(&format!("names-{locale}")).load(passwd(ROSTER), locale);
            assert_eq!(users(&accounts), NAME_ORDER, "{locale}, no database");
        }
    }

    #[test]
    fn a_database_that_cannot_be_read_gives_the_name_order_and_one_note() {
        type Spoil = fn(&Fixture);
        let cases: [(&str, Spoil); 4] = [
            ("missing", |_| {}),
            ("a directory", |f| {
                std::fs::create_dir(f.dir.join("lastlog2.db")).expect("a directory")
            }),
            ("not SQLite", |f| {
                std::fs::write(f.dir.join("lastlog2.db"), vec![0x5a; 8192]).expect("noise")
            }),
            ("another table", |f| {
                Connection::open(f.dir.join("lastlog2.db"))
                    .and_then(|db| db.execute_batch("CREATE TABLE Other(x)"))
                    .expect("a database without Lastlog2")
            }),
        ];
        for (case, spoil) in cases {
            let fixture = Fixture::new(&format!("spoilt-{}", case.replace(' ', "-")));
            spoil(&fixture);
            let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
            assert_eq!(users(&accounts), NAME_ORDER, "{case}");
            assert_eq!(notes.len(), 1, "{case}: {notes:?}");
            assert!(notes[0].contains("name order"), "{case}: {notes:?}");
        }
    }

    #[test]
    fn a_database_no_one_may_read_gives_the_name_order() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new("mode-0000");
        drop(fixture.lastlog(&RECORDS));
        let path = fixture.dir.join("lastlog2.db");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        // Root reads a file of mode 0000 anyway: the case holds only where the open fails.
        if std::fs::File::open(&path).is_ok() {
            eprintln!("skipped: this user reads a file of mode 0000");
            return;
        }
        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert_eq!(users(&accounts), NAME_ORDER);
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn a_database_over_one_mebibyte_is_not_opened() {
        let fixture = Fixture::new("large");
        drop(fixture.database("small.db", &RECORDS));
        let db = fixture.lastlog(&RECORDS);
        // Padding to just over the bound: the records are intact, so only the bound stops
        // them.
        db.execute_batch("CREATE TABLE Padding(x BLOB)")
            .expect("padding table");
        db.execute(
            "INSERT INTO Padding VALUES (zeroblob(?1))",
            [LASTLOG2_MAX_BYTES as i64],
        )
        .expect("padding");
        drop(db);
        let size = std::fs::metadata(fixture.dir.join("lastlog2.db"))
            .expect("the size")
            .len();
        assert!(size > LASTLOG2_MAX_BYTES, "{size}");

        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert_eq!(users(&accounts), NAME_ORDER);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("bytes"), "{notes:?}");

        // The same records below the bound are read: the order fell to the bound alone.
        std::fs::rename(
            fixture.dir.join("small.db"),
            fixture.dir.join("lastlog2.db"),
        )
        .expect("the small database in place");
        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert_eq!(users(&accounts), RECORD_ORDER, "{notes:?}");
    }

    #[test]
    fn a_database_beside_a_hot_journal_is_refused_not_rolled_back() {
        // A writer that stopped mid-transaction leaves its journal behind. Opened for
        // writing, SQLite would roll the database back and read it; opened read-only, as
        // the greeter opens it, it refuses. The records are the ones a rollback restores, so
        // a greeter that opened the file for writing would order by them.
        let source = Fixture::new("hot-source");
        let writer = source.lastlog(&RECORDS);
        writer
            .execute_batch(
                "PRAGMA cache_size = 1; CREATE TABLE Padding(x BLOB); BEGIN; \
                 UPDATE Lastlog2 SET Time = 1 WHERE Name = 'ada'",
            )
            .expect("a transaction");
        for _ in 0..50 {
            writer
                .execute("INSERT INTO Padding VALUES (zeroblob(3000))", [])
                .expect("a page that spills");
        }
        let fixture = Fixture::new("hot");
        for name in ["lastlog2.db", "lastlog2.db-journal"] {
            std::fs::copy(source.dir.join(name), fixture.dir.join(name)).expect(name);
        }
        writer.execute_batch("ROLLBACK").expect("the rollback");
        let before = std::fs::read(fixture.dir.join("lastlog2.db")).expect("the database");

        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert_eq!(users(&accounts), NAME_ORDER);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            fixture.dir.join("lastlog2.db-journal").exists(),
            "the journal stays"
        );
        let after = std::fs::read(fixture.dir.join("lastlog2.db")).expect("the database");
        assert!(before == after, "the database is not written");
    }

    #[test]
    fn a_database_locked_by_a_writer_is_not_waited_on() {
        let fixture = Fixture::new("locked");
        let writer = fixture.lastlog(&RECORDS);
        writer
            .execute_batch("BEGIN EXCLUSIVE")
            .expect("the exclusive lock");
        let started = std::time::Instant::now();
        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        let waited = started.elapsed();
        assert_eq!(users(&accounts), NAME_ORDER);
        assert_eq!(notes.len(), 1, "{notes:?}");
        // rusqlite's own default would wait five seconds.
        assert!(waited < Duration::from_secs(1), "waited {waited:?}");
        writer.execute_batch("COMMIT").expect("the lock goes");
    }

    #[test]
    fn with_the_list_hidden_no_account_is_read_and_the_database_is_not_opened() {
        for (case, toml, note) in [
            ("off", "user-list = false\n", false),
            ("malformed", "user-list = [\n", true),
            ("not-boolean", "user-list = \"false\"\n", true),
        ] {
            let fixture = Fixture::new(&format!("hidden-{case}"));
            std::fs::write(fixture.dir.join("greeter.toml"), toml).expect("greeter.toml");
            // No database: one opened would leave a note of its own.
            let read = std::cell::Cell::new(false);
            let (accounts, notes) = fixture.load_with(
                || {
                    read.set(true);
                    passwd(ROSTER)
                },
                |a: &str, b: &str| a.cmp(b),
            );
            assert!(accounts.is_empty(), "{case}");
            assert!(!read.get(), "{case}: the accounts were read");
            assert_eq!(notes.len(), usize::from(note), "{case}: {notes:?}");
            if note {
                assert!(notes[0].contains("hidden"), "{case}: {notes:?}");
            }
        }
        // A directory where the file should be cannot be read: hidden, one note.
        let fixture = Fixture::new("hidden-unreadable");
        std::fs::create_dir(fixture.dir.join("greeter.toml")).expect("a directory");
        let (accounts, notes) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert!(accounts.is_empty());
        assert_eq!(notes.len(), 1, "{notes:?}");
        // With the list on, the same accounts are listed.
        let fixture = Fixture::new("shown");
        std::fs::write(fixture.dir.join("greeter.toml"), "user-list = true\n").expect("on");
        let (accounts, _) = fixture.load(passwd(ROSTER), "en_US.UTF-8");
        assert_eq!(users(&accounts), NAME_ORDER);
    }

    #[test]
    fn with_no_account_to_list_only_other_user_is_shown_with_one_note() {
        let fixture = Fixture::new("empty");
        drop(fixture.lastlog(&RECORDS));
        let (accounts, notes) = fixture.load(
            passwd("svc:x:1001:1001::/:/usr/sbin/nologin\nroot:x:0:0::/root:/bin/bash"),
            "en_US.UTF-8",
        );
        assert!(accounts.is_empty());
        assert_eq!(notes.len(), 1, "{notes:?}");
        let (accounts, notes) = fixture.load(Vec::new(), "en_US.UTF-8");
        assert!(accounts.is_empty());
        assert_eq!(notes.len(), 1, "NSS answered nothing: {notes:?}");
    }

    #[test]
    fn a_typed_name_is_refused_only_when_empty_too_long_or_with_a_control_character() {
        let longest = "a".repeat(MAX_TYPED_NAME_BYTES);
        for name in ["ada", "nobody-here", "Ada Lovelace", "ä", longest.as_str()] {
            assert!(typed_name_is_valid(name), "{name}");
        }
        let too_long = "a".repeat(MAX_TYPED_NAME_BYTES + 1);
        for name in [
            "",
            too_long.as_str(),
            "ada\n",
            "a\u{7f}b",
            "\u{1b}[2J",
            "a\0b",
        ] {
            assert!(!typed_name_is_valid(name), "{name:?}");
        }
        // Bytes, not characters.
        assert!(!typed_name_is_valid(&"ä".repeat(129)));
    }

    #[test]
    fn the_collation_is_the_locales_and_bytes_without_one() {
        let collator = Collator::new("en_US.UTF-8").expect("the locale is installed");
        assert_eq!(collator.compare("dan", "Zoe"), Ordering::Less);
        assert_eq!(collator.compare("Émile", "Zoe"), Ordering::Less);
        assert_eq!(collator.compare("Zoe", "Zoe"), Ordering::Equal);
        assert!(Collator::new("xx_NOWHERE.UTF-8").is_none());
    }

    #[test]
    #[allow(unsafe_code)]
    fn a_comparison_leaves_the_threads_locale_as_it_found_it() {
        let collator = Collator::new("en_US.UTF-8").expect("the locale is installed");
        // SAFETY: a null locale only asks for the thread's current one.
        let before = unsafe { libc::uselocale(std::ptr::null_mut()) };
        collator.compare("dan", "Zoe");
        // SAFETY: as above.
        let after = unsafe { libc::uselocale(std::ptr::null_mut()) };
        assert_eq!(after, before);
        assert_ne!(after, collator.0);
    }

    #[test]
    fn the_collation_locale_comes_from_locale_conf_when_the_environment_names_none() {
        // The environment of the test run decides first; only its absence is testable here.
        if ["LC_ALL", "LC_COLLATE", "LANG"]
            .iter()
            .any(|key| std::env::var(key).is_ok_and(|value| !value.is_empty()))
        {
            eprintln!("skipped: the environment names a locale");
            return;
        }
        assert_eq!(
            collation_locale(Some("LANG=\"it_IT.UTF-8\"\nLC_COLLATE=de_DE.UTF-8\n")),
            Some("de_DE.UTF-8".to_string())
        );
        assert_eq!(
            collation_locale(Some("LANG=it_IT.UTF-8\n")),
            Some("it_IT.UTF-8".to_string())
        );
        assert_eq!(collation_locale(None), None);
    }
}
