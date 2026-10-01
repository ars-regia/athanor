//! The calculator (LA2): one `qalc` per query, started after the debounce and killed when
//! the query changes. It never touches the network: `update exchange rates 0` keeps it
//! on the rates on disk, which `athanor-launcher-rates.timer` refreshes once a day.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use athanor_unit::text;
use gio::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// qalc's answer is a few lines; anything longer is not an answer.
const MAX_OUTPUT: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calc {
    /// The expression as qalc read it, for the preview.
    pub expression: String,
    pub result: String,
}

/// Text worth a `qalc` process: it holds a digit and is not a command. "rel" would come
/// back as `re(liter) = 1 L`.
pub fn wanted(query: &str) -> bool {
    let query = query.trim_start();
    !query.starts_with('>') && query.chars().any(|c| c.is_ascii_digit())
}

/// The last line of qalc's answer, split at its `=` or `≈`; nothing when it is a warning,
/// an error, has no separator, or only repeats the query.
pub fn parse(stdout: &str, query: &str) -> Option<Calc> {
    let line = stdout
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    if line.starts_with("warning:") || line.starts_with("error:") {
        return None;
    }
    let (expression, result) = [" = ", " ≈ "]
        .iter()
        .filter_map(|separator| line.rsplit_once(separator))
        .max_by_key(|(expression, _)| expression.len())?;
    let (expression, result) = (expression.trim(), result.trim());
    if result.is_empty() || result == query.trim() {
        return None;
    }
    Some(Calc {
        expression: text::line(expression, text::TITLE_CHARS),
        result: text::line(result, text::TITLE_CHARS),
    })
}

/// Kills the process when the query it answers is dropped. `force_exit` does nothing to a
/// process that already exited.
struct Running(gio::Subprocess);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.force_exit();
    }
}

/// qalc writes its settings and history under `$XDG_CONFIG_HOME/qalculate` on every run
/// (measured with qalc 5.7.0). The launcher runs under Landlock with writes allowed only
/// in its own state and cache, so qalc gets a config directory there. `HOME` stays: the
/// rates are read from `$XDG_DATA_HOME/qalculate`, where the timer writes them.
pub fn config_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = gio::glib::user_cache_dir().join("athanor/launcher/qalc");
        if let Err(err) = private_config(&home) {
            tracing::warn!("qalc config directory {}: {err}", home.display());
        }
        home
    })
}

/// The directory (0700) with qalc's history sent to /dev/null: no query is kept on disk.
fn private_config(home: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, symlink};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(home)?;
    let history = home.join("qalculate/qalc.history");
    if let Some(parent) = history.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    }
    if std::fs::read_link(&history).is_ok_and(|to| to == Path::new("/dev/null")) {
        return Ok(());
    }
    match std::fs::remove_file(&history) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }
    symlink("/dev/null", &history)
}

/// Reads the stream to its end; nothing when it is longer than `limit` or fails.
async fn read_bounded(stream: &gio::InputStream, limit: usize) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    loop {
        match stream
            .read_bytes_future(1024, gio::glib::Priority::DEFAULT)
            .await
        {
            Ok(bytes) if bytes.is_empty() => return Some(output),
            Ok(bytes) => {
                output.extend_from_slice(&bytes);
                if output.len() > limit {
                    return None;
                }
            }
            Err(err) => {
                tracing::warn!("qalc did not answer: {err}");
                return None;
            }
        }
    }
}

pub async fn evaluate(query: &str) -> Option<Calc> {
    // Without STDIN_PIPE or STDIN_INHERIT, GIO gives the child /dev/null as stdin: qalc
    // never waits for input (GIO has no STDIN_SILENCE flag).
    let launcher = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    );
    launcher.setenv("XDG_CONFIG_HOME", config_home(), true);
    // The query comes after `--`: it is an expression even when it starts with `-`.
    let argv = [
        "qalc", "-s", "color 0", "-s", "update exchange rates 0", "--", query,
    ]
    .map(std::ffi::OsStr::new);
    let process = match launcher.spawn(&argv) {
        Ok(process) => Running(process),
        Err(err) => {
            tracing::warn!("qalc cannot start: {err}");
            return None;
        }
    };
    let Some(stdout) = process.0.stdout_pipe() else {
        tracing::warn!("qalc has no output pipe");
        return None;
    };
    let output = read_bounded(&stdout, MAX_OUTPUT).await?;
    if let Err(err) = process.0.wait_future().await {
        tracing::warn!("qalc did not finish: {err}");
        return None;
    }
    if !process.0.is_successful() {
        return None;
    }
    parse(&String::from_utf8_lossy(&output), query)
}

pub fn hit(calc: &Calc) -> Hit {
    Hit {
        group: Group::Calc,
        key: String::new(),
        title: calc.result.clone(),
        subtitle: calc.expression.clone(),
        icon: Some(gio::ThemedIcon::new("accessories-calculator").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Copy { text: calc.result.clone() },
    }
}

/// The rates on disk: their date and the currencies they price, EUR being the base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rates {
    pub date: String,
    pub codes: BTreeSet<String>,
}

impl Rates {
    /// The user's copy, refreshed by the timer, then the one the package ships.
    pub fn paths() -> Vec<PathBuf> {
        vec![
            gio::glib::user_data_dir().join("qalculate/eurofxref-daily.xml"),
            PathBuf::from("/usr/share/qalculate/eurofxref-daily.xml"),
        ]
    }

    pub fn read(paths: &[PathBuf]) -> Option<Rates> {
        let text = paths.iter().find_map(|path| std::fs::read_to_string(path).ok())?;
        let attribute = |name: &str| {
            let marker = format!("{name}='");
            text.match_indices(&marker)
                .filter_map(|(at, _)| text[at + marker.len()..].split('\'').next())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let date = attribute("time").into_iter().next()?;
        let mut codes: BTreeSet<String> = attribute("currency").into_iter().collect();
        codes.insert("EUR".to_owned());
        Some(Rates { date, codes })
    }

    /// The calculation names a currency: a code the rates price or a currency symbol.
    pub fn involves(&self, calc: &Calc) -> bool {
        let words = format!("{} {}", calc.expression, calc.result);
        words
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| self.codes.contains(word))
            || words.chars().any(|c| "€$£¥₹₩₽₺₪₫₴₦₱฿".contains(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| context.block_on(future))
            .expect("a fresh context is free")
    }

    #[test]
    fn dropping_the_guard_kills_the_process() {
        let process = gio::Subprocess::newv(
            &[std::ffi::OsStr::new("sleep"), std::ffi::OsStr::new("30")],
            gio::SubprocessFlags::NONE,
        )
        .expect("sleep starts");
        drop(Running(process.clone()));
        block_on(process.wait_future()).expect("reaped");
        assert!(process.has_signaled() && process.term_sig() == 9);
    }

    #[test]
    fn output_over_the_cap_is_refused() {
        let stream = |len: usize| {
            gio::MemoryInputStream::from_bytes(&gio::glib::Bytes::from_owned(vec![b'a'; len]))
                .upcast::<gio::InputStream>()
        };
        assert_eq!(block_on(read_bounded(&stream(100), 4096)).map(|o| o.len()), Some(100));
        assert!(block_on(read_bounded(&stream(5000), 4096)).is_none());
    }

    #[test]
    fn the_history_goes_to_dev_null() {
        let dir = std::env::temp_dir().join(format!("athanor-search-cfg-{}", std::process::id()));
        let history = dir.join("qalculate/qalc.history");
        std::fs::create_dir_all(history.parent().expect("parent")).expect("dir");
        std::fs::write(&history, "old query 123").expect("write");
        private_config(&dir).expect("prepared");
        private_config(&dir).expect("prepared twice");
        assert_eq!(std::fs::read_link(&history).expect("link"), Path::new("/dev/null"));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn only_text_with_a_digit_is_sent_to_qalc() {
        assert!(wanted("2+2*3"));
        assert!(wanted("100 USD to EUR"));
        assert!(wanted("sqrt(2)"));
        assert!(!wanted("rel"));
        assert!(!wanted("firefox"));
        assert!(!wanted("> 2+2"));
    }

    #[test]
    fn the_expression_and_the_result_are_split() {
        assert_eq!(
            parse("2 + (2 × 3) = 8\n", "2+2*3"),
            Some(Calc { expression: "2 + (2 × 3)".into(), result: "8".into() })
        );
        assert_eq!(
            parse("warning: It has been 430 days since the exchange rates last were updated.\n100 USD ≈ 85,80744809 €\n", "100 USD to EUR"),
            Some(Calc { expression: "100 USD".into(), result: "85,80744809 €".into() })
        );
    }

    #[test]
    fn a_result_equal_to_the_query_or_empty_output_is_nothing() {
        assert_eq!(parse("2 = 2\n", "2"), None);
        assert_eq!(parse("", "2+"), None);
        assert_eq!(parse("error: something\n", "2+"), None);
        assert_eq!(parse("just text\n", "x1"), None);
    }

    #[test]
    fn the_rates_file_gives_its_date_and_currencies() {
        let dir = std::env::temp_dir().join(format!("athanor-search-rates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("eurofxref-daily.xml");
        std::fs::write(&file, "<Cube><Cube time='2025-07-28'><Cube currency='USD' rate='1.1'/><Cube currency='CHF' rate='0.9'/></Cube></Cube>").expect("write");
        let rates = Rates::read(&[dir.join("absent.xml"), file]).expect("rates");
        assert_eq!(rates.date, "2025-07-28");
        assert!(rates.codes.contains("USD") && rates.codes.contains("EUR"));
        assert!(rates.involves(&Calc { expression: "100 USD".into(), result: "85,8 €".into() }));
        assert!(rates.involves(&Calc { expression: "10 CHF".into(), result: "11 CHF".into() }));
        assert!(!rates.involves(&Calc { expression: "2 + (2 × 3)".into(), result: "8".into() }));
        assert!(Rates::read(&[dir.join("absent.xml")]).is_none());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
