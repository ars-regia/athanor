//! Translations for Athanor's own programs.
//!
//! The format is gettext's: translators work on `.po` files with the tools they know,
//! and packages ship `.mo` catalogs under `/usr/share/locale`. The *reader* is ours, in
//! safe Rust with no dependency, for three reasons (doc_shell.md, SH13; decision of
//! 2026-09-19):
//!
//! - the greeter handles the password: no C FFI and no process-global
//!   `setlocale`/`textdomain` state in that process;
//! - glibc's gettext translates only when the system locale is installed, so it is
//!   silently English inside a sandbox or a test container that lacks the locale;
//! - it is toolkit-agnostic logic (SH4), usable by a surface and by a notifier alike.
//!
//! GTK's own strings keep going through the system's gettext, untouched.
//!
//! One answer comes from glibc instead: [`first_weekday`], the first day of the week of the
//! time locale, read through a locale object of its own, never through `setlocale`.

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};

/// Where packages install catalogs.
pub const LOCALE_DIR: &str = "/usr/share/locale";
/// A catalog file to load instead of looking one up. For the test rig, where German and
/// the right-to-left pseudo-language are catalogs of the test, not of the product.
pub const OVERRIDE_VARIABLE: &str = "ATHANOR_I18N_CATALOG";

const SYSTEM_LOCALE_FILE: &str = "/etc/locale.conf";
const MAGIC: u32 = 0x9504_12de;
const CONTEXT_SEPARATOR: char = '\u{4}';
/// Languages written right to left. The direction comes from the language of the
/// catalog in use, never from a process-wide locale.
const RTL_LANGUAGES: [&str; 4] = ["ar", "he", "fa", "ur"];
/// glibc's `_NL_TIME_FIRST_WEEKDAY`, `_NL_ITEM(LC_TIME, 104)` in `<langinfo.h>`, which the libc
/// crate does not export.
const NL_TIME_FIRST_WEEKDAY: libc::nl_item = (libc::LC_TIME << 16) | 104;
/// glibc's `_NL_TIME_WEEK_1STDAY`, `_NL_ITEM(LC_TIME, 102)`: the week's origin, a date written
/// as the number `YYYYMMDD`, from which [`NL_TIME_FIRST_WEEKDAY`] counts.
const NL_TIME_WEEK_1STDAY: libc::nl_item = (libc::LC_TIME << 16) | 102;
/// The week origins GTK's calendar knows: 30 November 1997, a Sunday, and 1 December 1997, a
/// Monday.
const ORIGIN_SUNDAY: u32 = 19_971_130;
const ORIGIN_MONDAY: u32 = 19_971_201;
/// The first weekday when the time locale cannot be read: Sunday, the C locale's, 0 as
/// [`first_weekday`] counts.
pub const DEFAULT_FIRST_WEEKDAY: u8 = 0;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a `.mo` file, or one cut short.
    Malformed(&'static str),
    /// A revision whose major number is not 0.
    UnsupportedRevision(u32),
    /// Only UTF-8 catalogs are read; `msgfmt` is told to write nothing else.
    NotUtf8,
    /// A `Plural-Forms` rule that is not in [`PluralRule`]'s table.
    UnknownPluralRule(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed catalog: {what}"),
            Self::UnsupportedRevision(revision) => {
                write!(f, "unsupported catalog revision {revision:#x}")
            }
            Self::NotUtf8 => write!(f, "the catalog is not UTF-8"),
            Self::UnknownPluralRule(rule) => write!(f, "unknown plural rule: {rule}"),
        }
    }
}

impl std::error::Error for Error {}

/// The plural rules of the languages Athanor ships or tests, by their canonical
/// `Plural-Forms` expression. A table, not an expression evaluator: a catalog is data
/// read in the process that handles the password, and an evaluator is a parser and an
/// interpreter to get wrong there. A new language adds one line and one test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluralRule {
    /// `nplurals=1; plural=0` (Japanese, Chinese, ...).
    One,
    /// `nplurals=2; plural=(n != 1)` (English, Italian, German, Spanish, ...).
    NotOne,
    /// `nplurals=2; plural=(n > 1)` (French, Brazilian Portuguese).
    MoreThanOne,
    /// Arabic's six forms.
    Arabic,
}

impl PluralRule {
    fn parse(header: &str) -> Result<Self, Error> {
        let compact: String = header.chars().filter(|c| !c.is_whitespace()).collect();
        let compact = compact.trim_end_matches(';');
        Ok(match compact {
            "nplurals=1;plural=0" => Self::One,
            "nplurals=2;plural=(n!=1)" | "nplurals=2;plural=n!=1" => Self::NotOne,
            "nplurals=2;plural=(n>1)" | "nplurals=2;plural=n>1" => Self::MoreThanOne,
            "nplurals=6;plural=(n==0?0:n==1?1:n==2?2:n%100>=3&&n%100<=10?3:n%100>=11?4:5)"
            | "nplurals=6;plural=n==0?0:n==1?1:n==2?2:n%100>=3&&n%100<=10?3:n%100>=11?4:5" => {
                Self::Arabic
            }
            other => return Err(Error::UnknownPluralRule(other.to_string())),
        })
    }

    /// The index of the form for `n`.
    pub fn index(self, n: u64) -> usize {
        match self {
            Self::One => 0,
            Self::NotOne => usize::from(n != 1),
            Self::MoreThanOne => usize::from(n > 1),
            Self::Arabic => match (n, n % 100) {
                (0, _) => 0,
                (1, _) => 1,
                (2, _) => 2,
                (_, 3..=10) => 3,
                (_, 11..) => 4,
                _ => 5,
            },
        }
    }
}

/// One language's translations. An empty catalog returns every message id unchanged,
/// which is English.
#[derive(Debug)]
pub struct Catalog {
    /// Key: `msgid`, or `msgctxt` + EOT + `msgid`. Value: the plural forms, one for a
    /// message without a plural.
    messages: HashMap<String, Vec<String>>,
    plural: PluralRule,
    language: Option<String>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self::empty()
    }
}

impl Catalog {
    pub fn empty() -> Self {
        Self {
            messages: HashMap::new(),
            plural: PluralRule::NotOne,
            language: None,
        }
    }

    /// Parses a `.mo` file of either byte order. Every offset is bounds-checked: a
    /// truncated or hostile file is an error, never a panic.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let word_at = |offset: usize, big_endian: bool| -> Result<u32, Error> {
            let raw: [u8; 4] = bytes
                .get(
                    offset
                        ..offset
                            .checked_add(4)
                            .ok_or(Error::Malformed("offset overflow"))?,
                )
                .and_then(|slice| slice.try_into().ok())
                .ok_or(Error::Malformed("truncated"))?;
            Ok(if big_endian {
                u32::from_be_bytes(raw)
            } else {
                u32::from_le_bytes(raw)
            })
        };
        let big_endian = match (word_at(0, false)?, word_at(0, true)?) {
            (MAGIC, _) => false,
            (_, MAGIC) => true,
            _ => return Err(Error::Malformed("not a .mo file")),
        };
        let word = |offset: usize| word_at(offset, big_endian).map(|value| value as usize);

        let revision = word_at(4, big_endian)?;
        if revision >> 16 != 0 {
            return Err(Error::UnsupportedRevision(revision));
        }
        let (count, originals, translations) = (word(8)?, word(12)?, word(16)?);

        let text = |table: usize, index: usize| -> Result<&str, Error> {
            let entry = index
                .checked_mul(8)
                .and_then(|offset| offset.checked_add(table))
                .ok_or(Error::Malformed("table overflow"))?;
            let (length, start) = (word(entry)?, word(entry + 4)?);
            let end = start
                .checked_add(length)
                .ok_or(Error::Malformed("string overflow"))?;
            let raw = bytes
                .get(start..end)
                .ok_or(Error::Malformed("string out of bounds"))?;
            std::str::from_utf8(raw).map_err(|_| Error::NotUtf8)
        };

        let mut catalog = Self::empty();
        for index in 0..count {
            let original = text(originals, index)?;
            let translation = text(translations, index)?;
            if original.is_empty() {
                catalog.read_header(translation)?;
                continue;
            }
            // A plural entry stores "msgid NUL msgid_plural"; the key is the singular.
            let key = original.split('\0').next().unwrap_or(original);
            catalog.messages.insert(
                key.to_string(),
                translation.split('\0').map(str::to_string).collect(),
            );
        }
        Ok(catalog)
    }

    fn read_header(&mut self, header: &str) -> Result<(), Error> {
        for line in header.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-type" => {
                    let charset = value.rsplit("charset=").next().unwrap_or("");
                    if !charset.eq_ignore_ascii_case("utf-8") {
                        return Err(Error::NotUtf8);
                    }
                }
                "plural-forms" => self.plural = PluralRule::parse(value)?,
                "language" if !value.is_empty() => self.language = Some(value.to_string()),
                _ => {}
            }
        }
        Ok(())
    }

    /// The catalog of `domain` for the first language of `languages` that has one under
    /// `locale_dir`. A catalog that exists and cannot be read is an error, not a reason
    /// to fall through silently to the next language.
    pub fn find(
        locale_dir: &Path,
        domain: &str,
        languages: &[String],
    ) -> Result<Self, (PathBuf, Error)> {
        for language in languages {
            let path = locale_dir
                .join(language)
                .join("LC_MESSAGES")
                .join(format!("{domain}.mo"));
            if let Ok(bytes) = std::fs::read(&path) {
                let mut catalog = Self::parse(&bytes).map_err(|error| (path, error))?;
                catalog.language.get_or_insert_with(|| language.clone());
                return Ok(catalog);
            }
        }
        Ok(Self::empty())
    }

    /// What a program calls at start: the override file when [`OVERRIDE_VARIABLE`] names
    /// one, otherwise the catalog for the language of the environment, of
    /// `/etc/locale.conf`, or none. The error says which file was unreadable; the caller
    /// logs it and carries on in English with [`Catalog::empty`].
    pub fn load(domain: &str) -> Result<Self, (PathBuf, Error)> {
        if let Some(path) = std::env::var_os(OVERRIDE_VARIABLE).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(path);
            let bytes = std::fs::read(&path)
                .map_err(|_| (path.clone(), Error::Malformed("unreadable override file")))?;
            return Self::parse(&bytes).map_err(|error| (path, error));
        }
        let system = std::fs::read_to_string(SYSTEM_LOCALE_FILE).ok();
        let languages = languages(|name| std::env::var(name).ok(), system.as_deref());
        Self::find(Path::new(LOCALE_DIR), domain, &languages)
    }

    pub fn tr<'a>(&'a self, msgid: &'a str) -> &'a str {
        self.form(msgid, 0).unwrap_or(msgid)
    }

    pub fn tr_n<'a>(&'a self, msgid: &'a str, msgid_plural: &'a str, n: u64) -> &'a str {
        match self.form(msgid, self.plural.index(n)) {
            Some(translated) => translated,
            None if n == 1 => msgid,
            None => msgid_plural,
        }
    }

    /// A message with a context (`msgctxt`), for a word that translates differently in
    /// two places.
    pub fn tr_c<'a>(&'a self, context: &str, msgid: &'a str) -> &'a str {
        let key = format!("{context}{CONTEXT_SEPARATOR}{msgid}");
        // The borrow of `key` ends here; the result borrows from `self` or `msgid`.
        match self.messages.get(&key).and_then(|forms| forms.first()) {
            Some(translated) if !translated.is_empty() => translated,
            _ => msgid,
        }
    }

    fn form(&self, msgid: &str, index: usize) -> Option<&str> {
        let forms = self.messages.get(msgid)?;
        forms
            .get(index)
            .map(String::as_str)
            .filter(|text| !text.is_empty())
    }

    /// The language of the catalog in use; `None` for the untranslated English.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Whether the language in use is written right to left.
    pub fn is_rtl(&self) -> bool {
        self.language()
            .map(primary_language)
            .is_some_and(|language| RTL_LANGUAGES.contains(&language))
    }
}

fn primary_language(locale: &str) -> &str {
    locale.split(['_', '.', '@']).next().unwrap_or(locale)
}

/// The catalog directories to try, most specific first: `it_IT.UTF-8@euro` gives
/// `["it_IT", "it"]`. The locale is the first non-empty of `LC_ALL`, `LC_MESSAGES`,
/// `LANG` in the environment, else `LANG` of `/etc/locale.conf`. `C` and `POSIX` mean
/// untranslated.
pub fn languages(
    env: impl Fn(&str) -> Option<String>,
    system_locale_conf: Option<&str>,
) -> Vec<String> {
    let from_env = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| env(name).filter(|value| !value.is_empty()));
    let from_system = || {
        system_locale_conf?.lines().find_map(|line| {
            let value = line.trim().strip_prefix("LANG=")?;
            Some(value.trim_matches('"').to_string())
        })
    };
    let Some(locale) = from_env.or_else(from_system) else {
        return Vec::new();
    };
    let base = locale.split(['.', '@']).next().unwrap_or("");
    if base.is_empty() || base == "C" || base == "POSIX" {
        return Vec::new();
    }
    let mut found = vec![base.to_string()];
    let primary = primary_language(base);
    if primary != base {
        found.push(primary.to_string());
    }
    found
}

/// The first day of the week of the time locale the environment names (`LC_ALL`, `LC_TIME`,
/// `LANG`), 0 for Sunday to 6 for Saturday, as GTK's calendar reads it. A locale glibc cannot
/// load, or an answer that is not a day, gives [`DEFAULT_FIRST_WEEKDAY`], with the reason on
/// standard error.
pub fn first_weekday() -> u8 {
    match first_weekday_of(c"") {
        Ok(day) => day,
        Err(error) => {
            eprintln!(
                "athanor-i18n: cannot read the first weekday of the time locale ({error}); Sunday is used"
            );
            DEFAULT_FIRST_WEEKDAY
        }
    }
}

/// Makes the calling thread format times in the time locale the environment names (`LC_ALL`,
/// `LC_TIME`, `LANG`), as a GTK program's start did for its whole process with `setlocale`:
/// GLib's `g_date_time_format` reads the names of the months and days, `%X` and `%p` through
/// `nl_langinfo`, which follows the thread's locale. Only the time category changes; the
/// thread's other categories stay "C", as the process's, and no process-global locale is
/// touched. Called once, on the thread that formats dates; on an error the thread keeps the
/// locale it had.
pub fn use_time_locale() -> std::io::Result<()> {
    use_time_locale_of(c"")
}

/// [`use_time_locale`] with the time locale `name`, `""` for the one the environment names.
fn use_time_locale_of(name: &CStr) -> std::io::Result<()> {
    // SAFETY: `name` is a NUL-terminated string that outlives the call, and a null base asks
    // newlocale for a new object whose other categories are "C", so no other object is changed.
    // A null result returns before any other call, with errno still newlocale's. uselocale
    // makes the object the calling thread's locale only; it is never freed, as the thread uses
    // it until the thread ends, and the locale uselocale returns is not freed either, as it may
    // be the global one. A failed uselocale leaves the thread's locale as it was, and the new
    // object, used by no thread, is freed. One block for the object's whole life, as in
    // `first_weekday_of`.
    #[allow(unsafe_code, clippy::multiple_unsafe_ops_per_block)]
    let used = unsafe {
        let locale = libc::newlocale(libc::LC_TIME_MASK, name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let used = !libc::uselocale(locale).is_null();
        if !used {
            libc::freelocale(locale);
        }
        used
    };
    if used {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The first weekday of the time locale `name`, `""` for the one the environment names.
fn first_weekday_of(name: &CStr) -> std::io::Result<u8> {
    // SAFETY: `name` is a NUL-terminated string that outlives the call, and a null base asks
    // newlocale for a new locale object, so no other object is changed or freed. A null result
    // returns before any other call, with errno still newlocale's. Otherwise the object stays
    // valid until the freelocale below, the only path out once it exists: nl_langinfo_l only
    // reads it, and the byte its answer points to, data of that object, is copied before the
    // object is freed, so no pointer into it outlives it. No process-global locale is touched.
    // One block for the six operations, by the maintainer's decision of 2026-11-15: the
    // object's whole life, from newlocale to freelocale, is read in one place. The week's
    // origin is a number, not a string: glibc returns it in the bits of the pointer, as a
    // `union { const char *string; unsigned int word; }` that GTK's calendar reads through
    // `word`, so it is read the same way and never dereferenced.
    #[allow(unsafe_code, clippy::multiple_unsafe_ops_per_block)]
    let answer = unsafe {
        let locale = libc::newlocale(libc::LC_TIME_MASK, name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let item = libc::nl_langinfo_l(NL_TIME_FIRST_WEEKDAY, locale);
        let day = if item.is_null() { None } else { Some(*item as u8) };
        let origin = LangInfo {
            string: libc::nl_langinfo_l(NL_TIME_WEEK_1STDAY, locale),
        }
        .word;
        libc::freelocale(locale);
        day.map(|day| (origin, day))
    };
    match answer {
        Some((origin, day)) => week_start(origin, day),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "glibc gave no first weekday",
        )),
    }
}

/// An answer of `nl_langinfo`, read as a string or as a number, as glibc's own union.
#[repr(C)]
union LangInfo {
    string: *const libc::c_char,
    word: libc::c_uint,
}

/// The first day of the week, 0 for Sunday, from glibc's week `origin` and the `day` that
/// counts from it, 1 for the origin itself, as GTK's calendar computes it.
fn week_start(origin: u32, day: u8) -> std::io::Result<u8> {
    let invalid = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
    let origin_day = match origin {
        ORIGIN_SUNDAY => 0,
        ORIGIN_MONDAY => 1,
        other => return Err(invalid(format!("unknown week origin {other}"))),
    };
    match day {
        1..=7 => Ok((origin_day + day - 1) % 7),
        other => Err(invalid(format!("glibc answered {other}, not a day from 1 to 7"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first weekday of an installed locale; a locale the system lacks skips its test with a
    /// line saying so (the rig's build image installs glibc-langpack-en and -it). The line goes
    /// to standard error directly, which the test harness does not capture, so a skip is seen.
    fn installed(name: &CStr) -> Option<u8> {
        match first_weekday_of(name) {
            Ok(day) => Some(day),
            Err(error) => {
                use std::io::Write as _;
                let line = format!("skipped: the locale {name:?} is not installed ({error})\n");
                assert!(std::io::stderr().write_all(line.as_bytes()).is_ok());
                None
            }
        }
    }

    #[test]
    fn italian_weeks_start_on_monday() {
        if let Some(day) = installed(c"it_IT.UTF-8") {
            assert_eq!(day, 1);
        }
    }

    #[test]
    fn american_weeks_start_on_sunday() {
        if let Some(day) = installed(c"en_US.UTF-8") {
            assert_eq!(day, 0);
        }
    }

    #[test]
    fn the_first_weekday_counts_from_the_weeks_origin() {
        // `week 7;19971130` with `first_weekday 2`, as it_IT: Monday.
        assert_eq!(week_start(ORIGIN_SUNDAY, 2).ok(), Some(1));
        // `week 7;19971201` with `first_weekday 1`: Monday too, which a Sunday origin read
        // for every locale would make Sunday.
        assert_eq!(week_start(ORIGIN_MONDAY, 1).ok(), Some(1));
        assert_eq!(week_start(ORIGIN_MONDAY, 7).ok(), Some(0));
        assert!(week_start(19_971_202, 1).is_err());
        assert!(week_start(ORIGIN_SUNDAY, 8).is_err());
    }

    #[test]
    fn the_c_locale_starts_on_sunday() {
        assert_eq!(first_weekday_of(c"C").ok(), Some(DEFAULT_FIRST_WEEKDAY));
    }

    #[test]
    fn a_missing_locale_is_an_error_not_sunday() {
        assert!(first_weekday_of(c"xx_NOWHERE.UTF-8").is_err());
    }

    #[test]
    fn a_thread_takes_a_time_locale_it_can_load() {
        // On a thread of its own: the locale stays with it.
        let outcome = std::thread::spawn(|| {
            (
                use_time_locale_of(c"xx_NOWHERE.UTF-8").is_err(),
                use_time_locale_of(c"C").is_ok(),
            )
        })
        .join();
        assert_eq!(outcome.ok(), Some((true, true)));
    }
}
