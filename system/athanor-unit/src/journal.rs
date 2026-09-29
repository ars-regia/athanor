//! Log lines for the journal. Each line starts with `<N>`, its syslog priority, which
//! journald reads from a service's stderr (SyslogLevelPrefix=, on by default), so
//! `journalctl --user -u <unit> -p err` finds what went wrong.

use std::fmt;
use std::io;

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

pub fn init() {
    subscriber(io::stderr).init();
}

/// The journal subscriber, writing to `writer`.
fn subscriber<W>(writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(writer)
        // journald stores the bytes as they come: colour codes would land in the log.
        .with_ansi(false)
        .event_format(SyslogPrefix)
        .finish()
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
    fn levels_map_to_syslog_priorities() {
        assert_eq!(priority(Level::ERROR), 3);
        assert_eq!(priority(Level::WARN), 4);
        assert_eq!(priority(Level::INFO), 6);
        assert_eq!(priority(Level::DEBUG), 7);
        assert_eq!(priority(Level::TRACE), 7);
    }
}
