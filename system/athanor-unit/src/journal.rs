//! Log lines for the journal. On a stream systemd connected to the journal each line starts
//! with `<N>`, its syslog priority, which journald reads (SyslogLevelPrefix=, on by default), so
//! `journalctl --user -u <unit> -p err` finds what went wrong. On any other stream, a terminal, a
//! pipe or a file, the lines are tracing's own, coloured only on a terminal.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, IsTerminal};
use std::os::fd::AsFd;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

struct SyslogPrefix;

impl<S, N> FormatEvent<S, N> for SyslogPrefix
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        write!(writer, "<{}>", priority(*event.metadata().level()))?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn priority(level: Level) -> u8 {
    match level {
        Level::ERROR => 3,
        Level::WARN => 4,
        Level::INFO => 6,
        _ => 7,
    }
}

/// The stream a program logs to.
#[derive(Clone, Copy, Debug)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Logs to stderr what `RUST_LOG` names, info when it names nothing.
pub fn init() {
    with_filter(default_filter(), Stream::Stderr).init();
}

/// `init` for a program that logs to stdout.
pub fn init_stdout() {
    with_filter(default_filter(), Stream::Stdout).init();
}

fn default_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// The subscriber that logs what `filter` lets through to `stream`: with the priority prefix
/// when the stream is the journal, tracing's own lines otherwise.
pub fn with_filter(filter: EnvFilter, stream: Stream) -> Box<dyn Subscriber + Send + Sync> {
    match stream {
        Stream::Stdout => {
            let out = io::stdout();
            build(filter, io::stdout, is_journal(&out), out.is_terminal())
        }
        Stream::Stderr => {
            let err = io::stderr();
            build(filter, io::stderr, is_journal(&err), err.is_terminal())
        }
    }
}

/// The journal subscriber with the default filter, writing to `writer`.
#[cfg(test)]
pub(crate) fn subscriber<W>(writer: W) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    build(default_filter(), writer, true, false)
}

fn build<W>(filter: EnvFilter, writer: W, journal: bool, ansi: bool) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let fmt = tracing_subscriber::fmt().with_env_filter(filter).with_writer(writer);
    if journal {
        // journald stores the bytes as they come: colour codes would land in the log.
        Box::new(fmt.with_ansi(false).event_format(SyslogPrefix).finish())
    } else {
        Box::new(fmt.with_ansi(ansi).finish())
    }
}

/// Whether `stream` is the one systemd connected to the journal: `JOURNAL_STREAM` holds its
/// `device:inode` (systemd.exec(5)), compared with the descriptor's own, as sd-daemon advises.
/// A descriptor the program inherited from a parent with the journal counts as well; under
/// systemd-cat the variable names stderr only.
pub fn is_journal(stream: impl AsFd) -> bool {
    names_stream(std::env::var_os("JOURNAL_STREAM").as_deref(), stream)
}

fn names_stream(value: Option<&OsStr>, stream: impl AsFd) -> bool {
    let Some((device, inode)) = value.and_then(OsStr::to_str).and_then(|v| v.split_once(':')) else {
        return false;
    };
    let Ok(stat) = nix::sys::stat::fstat(stream) else {
        return false;
    };
    device.parse() == Ok(stat.st_dev) && inode.parse() == Ok(stat.st_ino)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn lines_carry_no_terminal_colours() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);
        let make = move || Sink(Arc::clone(&sink));
        tracing::subscriber::with_default(subscriber(make), || {
            tracing::info!(preset = "float", "default layout picked");
        });
        let text = String::from_utf8(buffer.lock().expect("lock").clone()).expect("utf-8");
        assert_eq!(text, "<6>default layout picked preset=\"float\"\n");
    }

    #[test]
    fn a_stream_that_is_not_the_journal_gets_tracing_lines() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);
        let make = move || Sink(Arc::clone(&sink));
        tracing::subscriber::with_default(build(EnvFilter::new("info"), make, false, false), || {
            tracing::warn!("default layout picked");
        });
        let text = String::from_utf8(buffer.lock().expect("lock").clone()).expect("utf-8");
        assert!(!text.starts_with('<'), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(text.contains(" WARN ") && text.ends_with("default layout picked\n"), "{text}");
    }

    #[test]
    fn journal_stream_names_the_descriptor_by_device_and_inode() {
        let dir = std::env::temp_dir();
        let one = tempfile_in(&dir, "one");
        let two = tempfile_in(&dir, "two");
        let stat = nix::sys::stat::fstat(&one).expect("fstat");
        let value = format!("{}:{}", stat.st_dev, stat.st_ino);
        assert!(names_stream(Some(OsStr::new(&value)), &one));
        assert!(!names_stream(Some(OsStr::new(&value)), &two));
        assert!(!names_stream(None, &one));
        assert!(!names_stream(Some(OsStr::new("garbage")), &one));
        assert!(!names_stream(Some(OsStr::new(&format!("{}:x", stat.st_dev))), &one));
    }

    fn tempfile_in(dir: &std::path::Path, name: &str) -> std::fs::File {
        let path = dir.join(format!("athanor-unit-journal-{}-{name}", std::process::id()));
        let file = std::fs::File::create(&path).expect("create");
        std::fs::remove_file(&path).expect("remove");
        file
    }

    #[test]
    fn levels_map_to_syslog_priorities() {
        assert_eq!(priority(Level::ERROR), 3);
        assert_eq!(priority(Level::WARN), 4);
        assert_eq!(priority(Level::INFO), 6);
        assert_eq!(priority(Level::DEBUG), 7);
        assert_eq!(priority(Level::TRACE), 7);
    }
}
