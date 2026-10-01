//! Files (LA2) from localsearch's index, by name and by content, with the excerpt. With
//! localsearch absent or stopped the group does not appear and nothing is shown as an
//! error (LA10); while it indexes, `Answer::indexing` asks for one row that says so.
//! Everything localsearch returns is untrusted: counts and lengths are bounded, text is
//! cleaned by `athanor_unit::text`, and only `file://` URIs become an `Open` action.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use athanor_unit::text;
use gio::prelude::*;
use tracker::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::{Ranker, Tier};
use crate::usage::Usage;

pub const MAX_HITS: usize = 20;
/// Rows read from the cursor, whatever the query's own LIMIT says.
const MAX_ROWS: usize = 40;
/// The query as typed is bounded before it reaches the store.
const MAX_QUERY_CHARS: usize = 256;
/// Longer URIs are dropped, not cut: a cut URI opens another file.
const MAX_URI_BYTES: usize = 4096;
/// The per-source budget of LA4: a source that misses it is dropped for that query.
const DEADLINE: Duration = Duration::from_secs(1);
const SERVICE: &str = "org.freedesktop.LocalSearch3";
const MINER_PATH: &str = "/org/freedesktop/Tracker3/Miner/Files";
const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const MINER: &str = "org.freedesktop.Tracker3.Miner";
const QUERY: &str = include_str!("files.rq");

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub uri: String,
    pub name: String,
    pub snippet: Option<String>,
    pub modified: Option<String>,
}

#[derive(Debug)]
pub struct Answer {
    pub hits: Vec<Hit>,
    pub indexing: bool,
}

pub struct Files {
    connection: RefCell<Option<tracker::SparqlConnection>>,
    /// Tests run against a local store, which has no miner to ask about indexing.
    local: bool,
    /// A loss of the connection is logged once, not on every keystroke while it lasts.
    warned: Cell<bool>,
}

impl Files {
    pub fn new() -> Files {
        Files { connection: RefCell::new(None), local: false, warned: Cell::new(false) }
    }

    pub fn with_connection(connection: tracker::SparqlConnection) -> Files {
        Files { connection: RefCell::new(Some(connection)), local: true, warned: Cell::new(false) }
    }

    /// The connection, after asking the bus on every search whether localsearch owns its
    /// name: a call to an absent name would start it (LA10). Without an owner the cached
    /// connection is dropped too.
    async fn connection(&self) -> Option<tracker::SparqlConnection> {
        let bus = if self.local {
            None
        } else {
            let bus = gio::bus_get_future(gio::BusType::Session).await.ok();
            if !name_has_owner(bus.as_ref()).await {
                tracing::debug!("localsearch is not running");
                self.connection.replace(None);
                return None;
            }
            bus
        };
        let cached = self.connection.borrow().clone();
        if cached.is_some() {
            return cached;
        }
        match tracker::SparqlConnection::bus_new_future(SERVICE, None, bus.as_ref()).await {
            Ok(connection) => {
                self.connection.replace(Some(connection.clone()));
                self.warned.set(false);
                Some(connection)
            }
            Err(err) => {
                tracing::debug!("localsearch is not reachable: {err}");
                None
            }
        }
    }

    /// Logs a lost connection once and drops it, so the next search opens a new one.
    fn lost(&self, why: &str) {
        self.connection.replace(None);
        if !self.warned.replace(true) {
            tracing::warn!("the file search failed: {why}");
        }
    }

    pub async fn search(&self, query: &str, usage: &Usage, now: u64) -> Option<Answer> {
        let query: String = query.trim().chars().take(MAX_QUERY_CHARS).collect();
        if query.is_empty() {
            return None;
        }
        self.search_within(DEADLINE, &query, usage, now).await
    }

    async fn search_within(&self, deadline: Duration, query: &str, usage: &Usage, now: u64) -> Option<Answer> {
        let work = async {
            let connection = self.connection().await?;
            // The progress question runs while the query does.
            let progress = (!self.local).then(|| gio::glib::spawn_future_local(indexing()));
            let rows = run(&connection, query).await;
            let indexing = match progress {
                Some(progress) => progress.await.unwrap_or(false),
                None => false,
            };
            Some((rows, indexing))
        };
        let Some(done) = within(deadline, work).await else {
            self.lost("the deadline passed");
            return None;
        };
        let (rows, indexing) = done?;
        match rows {
            Ok(rows) => Some(Answer { hits: rows_to_hits(rows, query, &mut Ranker::new(query), usage, now), indexing }),
            Err(err) => {
                self.lost(&err.to_string());
                None
            }
        }
    }
}

/// `future`'s output, or `None` when `deadline` passes first; the future is then dropped,
/// which cancels the bus call it was waiting on.
pub(crate) async fn within<T>(deadline: Duration, future: impl Future<Output = T>) -> Option<T> {
    let cancellable = gio::Cancellable::new();
    let source = {
        let cancellable = cancellable.clone();
        gio::glib::timeout_source_new(deadline, None, gio::glib::Priority::DEFAULT, move || {
            cancellable.cancel();
            gio::glib::ControlFlow::Break
        })
    };
    source.attach(Some(&gio::glib::MainContext::ref_thread_default()));
    let done = gio::CancellableFuture::new(future, cancellable).await.ok();
    source.destroy();
    done
}

async fn name_has_owner(bus: Option<&gio::DBusConnection>) -> bool {
    let Some(bus) = bus else { return false };
    let reply = bus
        .call_future(
            Some(BUS),
            BUS_PATH,
            BUS,
            "NameHasOwner",
            Some(&(SERVICE,).to_variant()),
            Some(gio::glib::VariantTy::new("(b)").unwrap_or(gio::glib::VariantTy::ANY)),
            gio::DBusCallFlags::NO_AUTO_START,
            1000,
        )
        .await;
    matches!(reply.ok().and_then(|reply| reply.get::<(bool,)>()), Some((true,)))
}

impl Default for Files {
    fn default() -> Files {
        Files::new()
    }
}

/// At most `max` characters of a value read from the store, so a huge one is never copied whole.
fn bounded(value: Option<gio::glib::GString>, max: usize) -> Option<String> {
    value.map(|value| value.chars().take(max).collect())
}

async fn run(connection: &tracker::SparqlConnection, query: &str) -> Result<Vec<Row>, gio::glib::Error> {
    let statement = connection.query_statement(QUERY, None::<&gio::Cancellable>)?;
    statement.bind_string("name", query);
    statement.bind_string("match", query);
    let cursor = statement.execute_future().await?;
    let mut rows = Vec::new();
    while rows.len() < MAX_ROWS && cursor.next_future().await? {
        let (Some(uri), Some(name)) = (cursor.string(0), cursor.string(1)) else { continue };
        if uri.len() > MAX_URI_BYTES {
            continue;
        }
        rows.push(Row {
            uri: uri.to_string(),
            name: bounded(Some(name), text::NAME_CHARS * 4).unwrap_or_default(),
            snippet: bounded(cursor.string(2), text::SUMMARY_CHARS * 4).filter(|s| !s.is_empty()),
            modified: bounded(cursor.string(3), 64),
        });
    }
    cursor.close();
    Ok(rows)
}

/// The miner reports progress below 1 while it indexes. A miner that does not answer is
/// not indexing as far as the launcher can tell.
async fn indexing() -> bool {
    let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return false };
    let reply = bus
        .call_future(
            Some(SERVICE),
            MINER_PATH,
            MINER,
            "GetProgress",
            None,
            Some(gio::glib::VariantTy::new("(d)").unwrap_or(gio::glib::VariantTy::ANY)),
            gio::DBusCallFlags::NO_AUTO_START,
            500,
        )
        .await;
    match reply.ok().and_then(|reply| reply.get::<(f64,)>()) {
        Some((progress,)) => progress < 1.0,
        None => false,
    }
}

/// A local `file:///` URI without control characters, raw or percent-encoded (%00-%1F,
/// %7F): the only kind that becomes `Action::Open`.
fn is_file_uri(uri: &str) -> bool {
    let bytes = uri.as_bytes();
    let encoded_control = bytes.windows(3).any(|w| {
        w[0] == b'%'
            && matches!(u8::from_str_radix(std::str::from_utf8(&w[1..]).unwrap_or("zz"), 16), Ok(b) if b < 0x20 || b == 0x7f)
    });
    uri.len() <= MAX_URI_BYTES && uri.starts_with("file:///") && !uri.chars().any(char::is_control) && !encoded_control
}

/// One row per file, its excerpt kept when it was also found by content; a name match
/// ranks by LA3, a content-only match sits at the scattered tier.
pub fn rows_to_hits(rows: Vec<Row>, query: &str, ranker: &mut Ranker, usage: &Usage, now: u64) -> Vec<Hit> {
    let mut merged: Vec<Row> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for row in rows {
        if !is_file_uri(&row.uri) {
            continue;
        }
        match seen.get(&row.uri) {
            Some(&at) => {
                if merged[at].snippet.is_none() {
                    merged[at].snippet = row.snippet;
                }
            }
            None => {
                seen.insert(row.uri.clone(), merged.len());
                merged.push(row);
            }
        }
    }
    let mut hits: Vec<Hit> = merged
        .into_iter()
        .map(|row| {
            let title = text::line(&row.name, text::NAME_CHARS);
            let (tier, score) = ranker.score(&title, &[]).unwrap_or((Tier::Scattered, 0));
            let (bonus, learned) = usage.bonus(query, &row.uri, now);
            let subtitle = match &row.snippet {
                Some(snippet) => text::line(snippet, text::SUMMARY_CHARS),
                None => folder(&row.uri),
            };
            Hit {
                group: Group::Files,
                title,
                subtitle,
                icon: None,
                tier,
                score: score.saturating_add(bonus),
                learned,
                action: Action::Open { uri: row.uri.clone() },
                key: row.uri,
            }
        })
        .collect();
    // Rows arrive newest first and the sort is stable: equal scores keep that order.
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
    hits.truncate(MAX_HITS);
    hits
}

/// The parent folder, with the home directory shown as `~`.
fn folder(uri: &str) -> String {
    let Some(parent) = gio::File::for_uri(uri).parent().and_then(|parent| parent.path()) else {
        return String::new();
    };
    let home = gio::glib::home_dir();
    let shown = match parent.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => parent.display().to_string(),
    };
    text::line(&shown, text::SUMMARY_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(uri: &str, name: &str, snippet: Option<&str>) -> Row {
        Row { uri: uri.into(), name: name.into(), snippet: snippet.map(Into::into), modified: None }
    }

    #[test]
    fn a_file_found_twice_keeps_one_row_with_its_excerpt() {
        let hits = rows_to_hits(
            vec![
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", None),
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", Some("la relazione finale")),
            ],
            "rel",
            &mut Ranker::new("rel"),
            &Usage::default(),
            0,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subtitle, "la relazione finale");
        assert_eq!(hits[0].action, Action::Open { uri: "file:///h/Relazione_Q3.odt".into() });
        assert_eq!(hits[0].key, "file:///h/Relazione_Q3.odt");
    }

    #[test]
    fn a_content_match_ranks_below_a_name_match() {
        let hits = rows_to_hits(
            vec![
                row("file:///h/verbale.pdf", "verbale.pdf", Some("la relazione finale")),
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", None),
            ],
            "rel",
            &mut Ranker::new("rel"),
            &Usage::default(),
            0,
        );
        let mut sorted = hits.clone();
        sorted.sort_by_key(|hit| std::cmp::Reverse(hit.score));
        assert_eq!(sorted[0].title, "Relazione_Q3.odt");
        assert_eq!(sorted[1].tier, Tier::Scattered);
    }

    #[test]
    fn names_and_excerpts_are_plain_text_and_bounded() {
        let hits = rows_to_hits(
            vec![row("file:///h/x", "Evil\u{202e}fdp.exe", Some(&"word ".repeat(1000)))],
            "evil",
            &mut Ranker::new("evil"),
            &Usage::default(),
            0,
        );
        assert!(!hits[0].title.contains('\u{202e}'));
        assert!(hits[0].subtitle.chars().count() <= text::SUMMARY_CHARS);
    }

    #[test]
    fn at_most_twenty_files() {
        let rows = (0..40).map(|i| row(&format!("file:///h/rel{i}"), &format!("rel{i}"), None)).collect();
        assert_eq!(rows_to_hits(rows, "rel", &mut Ranker::new("rel"), &Usage::default(), 0).len(), MAX_HITS);
    }

    #[test]
    fn the_best_name_match_survives_the_cut_even_when_older() {
        let mut rows: Vec<Row> = (0..30).map(|i| row(&format!("file:///h/n{i}"), &format!("notes{i}"), Some("la relazione"))).collect();
        rows.push(row("file:///h/Relazione", "Relazione", None));
        let hits = rows_to_hits(rows, "relazione", &mut Ranker::new("relazione"), &Usage::default(), 0);
        assert_eq!(hits.len(), MAX_HITS);
        assert_eq!(hits[0].title, "Relazione");
    }

    #[test]
    fn a_future_that_misses_its_deadline_is_dropped() {
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let slow = gio::glib::timeout_future(Duration::from_secs(30));
                    let started = std::time::Instant::now();
                    assert!(within(Duration::from_millis(50), slow).await.is_none());
                    assert!(started.elapsed() < Duration::from_secs(5));
                    assert_eq!(within(Duration::from_secs(5), async { 7 }).await, Some(7));
                })
            })
            .expect("a fresh context is free");
    }

    #[test]
    fn without_a_running_localsearch_the_answer_is_none_and_quick() {
        // Whatever the session bus is (absent, or present without the name) the answer is
        // None; the rig has no service file for the name, so activation is not exercised.
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let started = std::time::Instant::now();
                    assert!(Files::new().search("rel", &Usage::default(), 0).await.is_none());
                    assert!(started.elapsed() < Duration::from_secs(3));
                })
            })
            .expect("a fresh context is free");
    }

    #[test]
    fn a_cached_connection_is_dropped_when_localsearch_leaves() {
        // The rig cannot make a name leave a bus, but its session bus (or the lack of one)
        // has no owner for it, which is the state after localsearch stops.
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let ontology = tracker::functions::sparql_get_ontology_nepomuk().expect("ontology");
                    let connection = tracker::SparqlConnection::new(
                        tracker::SparqlConnectionFlags::NONE,
                        None::<&gio::File>,
                        Some(&ontology),
                        None::<&gio::Cancellable>,
                    )
                    .expect("an in-memory store");
                    let files = Files::new();
                    files.connection.replace(Some(connection));
                    let started = std::time::Instant::now();
                    assert!(files.search("rel", &Usage::default(), 0).await.is_none());
                    assert!(files.connection.borrow().is_none());
                    assert!(started.elapsed() < Duration::from_secs(3));
                })
            })
            .expect("a fresh context is free");
    }

    #[test]
    fn only_plain_file_uris_can_be_opened() {
        let long = format!("file:///{}", "a".repeat(MAX_URI_BYTES));
        let hits = rows_to_hits(
            vec![
                row("https://example.org/rel", "rel-web", None),
                row("x-scheme:rel", "rel-x", None),
                row("file:///h/rel\nx", "rel-newline", None),
                row("file:///h/rel%0Ax", "rel-encoded-newline", None),
                row("file:///h/rel%00", "rel-encoded-nul", None),
                row("file:///h/rel%7f", "rel-encoded-del", None),
                row("file://host/rel", "rel-remote", None),
                row(&long, "rel-long", None),
                row("file:///h/rel-ok", "rel-ok", None),
            ],
            "rel",
            &mut Ranker::new("rel"),
            &Usage::default(),
            0,
        );
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["rel-ok"]);
    }

    #[test]
    fn the_query_runs_against_a_nepomuk_store() {
        let ontology = tracker::functions::sparql_get_ontology_nepomuk().expect("the Nepomuk ontology ships with tinysparql");
        let connection = tracker::SparqlConnection::new(
            tracker::SparqlConnectionFlags::NONE,
            None::<&gio::File>,
            Some(&ontology),
            None::<&gio::Cancellable>,
        )
        .expect("an in-memory store");
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    connection.update_future(FIXTURE).await.expect("fixture");
                    let files = Files::with_connection(connection.clone());
                    let answer = files.search("rel", &Usage::default(), 0).await.expect("an answer");
                    let names: Vec<_> = answer.hits.iter().map(|h| h.title.as_str()).collect();
                    assert!(names.contains(&"Relazione_Q3.odt") && names.contains(&"verbale.pdf"), "{names:?}");
                    assert!(!answer.indexing);
                })
            })
            .expect("a fresh context is free");
    }

    const FIXTURE: &str = r#"INSERT DATA {
      <file:///f/Relazione_Q3.odt> a nfo:FileDataObject ; nie:url "file:///f/Relazione_Q3.odt" ;
        nfo:fileName "Relazione_Q3.odt" ; nfo:fileLastModified "2026-09-30T18:02:00Z" .
      <urn:c1> a nfo:Document ; nie:isStoredAs <file:///f/Relazione_Q3.odt> ;
        nie:plainTextContent "Il verbale della riunione finale" .
      <file:///f/verbale.pdf> a nfo:FileDataObject ; nie:url "file:///f/verbale.pdf" ; nfo:fileName "verbale.pdf" .
      <urn:c2> a nfo:Document ; nie:isStoredAs <file:///f/verbale.pdf> ;
        nie:plainTextContent "contiene la relazione finale del progetto" .
    }"#;
}
