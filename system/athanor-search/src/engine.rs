//! The life of a query (LA4). Each keystroke opens a generation and aborts the tasks of
//! the previous one, which kills its `qalc` and cancels its bus calls. Applications,
//! settings, windows, the command and the web entry answer in the same call; the
//! calculator, files and providers start after `DEBOUNCE` and are dropped at `DEADLINE`.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gio::glib;

use crate::apps::{self, Catalog};
use crate::board::{Board, Rows};
use crate::files::{within, Files};
use crate::item::{Group, Hit};
use crate::providers::{self, Provider};
use crate::rank::Ranker;
use crate::usage::Usage;
use crate::windows::{self, WindowEntry};
use crate::{calc, command, web};

/// Estimates (open doubt 3); the first measurement on the dev VM sets them.
pub const DEBOUNCE: Duration = Duration::from_millis(120);
pub const DEADLINE: Duration = Duration::from_secs(1);

/// What any source sees of the query: its length, and what providers get of its words.
const MAX_QUERY_CHARS: usize = 256;
const MAX_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;

/// The query as providers get it: at most `MAX_TERMS` words of `MAX_TERM_CHARS` each.
fn provider_query(text: &str) -> String {
    let terms: Vec<String> = text
        .split_whitespace()
        .take(MAX_TERMS)
        .map(|term| term.chars().take(MAX_TERM_CHARS).collect())
        .collect();
    terms.join(" ")
}

struct Inner {
    board: RefCell<Board>,
    query: RefCell<String>,
    catalog: RefCell<Rc<Catalog>>,
    windows: RefCell<Rc<Vec<WindowEntry>>>,
    usage: RefCell<Usage>,
    usage_path: PathBuf,
    files: Option<Rc<Files>>,
    providers: Rc<Vec<Provider>>,
    /// The debounce and the slow sources of the current generation; aborting drops them.
    tasks: RefCell<Vec<glib::JoinHandle<()>>>,
    listener: Box<dyn Fn(&Rows)>,
}

#[derive(Clone)]
pub struct Engine(Rc<Inner>);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Engine {
    pub fn new(
        usage_path: PathBuf,
        files: Option<Files>,
        providers: Vec<Provider>,
        listener: impl Fn(&Rows) + 'static,
    ) -> Engine {
        Engine(Rc::new(Inner {
            board: RefCell::default(),
            query: RefCell::default(),
            catalog: RefCell::default(),
            windows: RefCell::default(),
            usage: RefCell::new(Usage::load(&usage_path)),
            usage_path,
            files: files.map(Rc::new),
            providers: Rc::new(providers),
            tasks: RefCell::default(),
            listener: Box::new(listener),
        }))
    }

    pub fn set_catalog(&self, catalog: Catalog) {
        self.0.catalog.replace(Rc::new(catalog));
    }

    pub fn set_windows(&self, windows: Vec<WindowEntry>) {
        self.0.windows.replace(Rc::new(windows));
    }

    /// The library writes usage too (plan 3b): read it again before a query session.
    pub fn reload_usage(&self) {
        self.0.usage.replace(Usage::load(&self.0.usage_path));
    }

    pub fn current(&self) -> String {
        self.0.query.borrow().clone()
    }

    /// Records that the user chose `key` for the current query, and saves.
    pub fn record(&self, key: &str) {
        if key.is_empty() {
            return;
        }
        let query = self.current();
        let mut usage = self.0.usage.borrow_mut();
        usage.record(&query, key, now());
        if let Err(err) = usage.save(&self.0.usage_path) {
            tracing::warn!("usage was not saved to {}: {err}", self.0.usage_path.display());
        }
    }

    fn notify(&self) {
        let rows = self.0.board.borrow().rows();
        (self.0.listener)(&rows);
    }

    fn put(&self, generation: u64, group: Group, source: &str, title: &str, hits: Vec<Hit>) {
        if self.0.board.borrow_mut().put(generation, group, source, title, hits) {
            self.notify();
        }
    }

    pub fn query(&self, text: &str) {
        let old = std::mem::take(&mut *self.0.tasks.borrow_mut());
        for task in old {
            task.abort();
        }
        // A command is judged on the whole line (it refuses an over-long one); only the
        // search sources get the cut.
        let whole = text;
        let text: String = text.chars().take(MAX_QUERY_CHARS).collect();
        let text = text.as_str();
        self.0.query.replace(text.to_owned());
        let generation = self.0.board.borrow_mut().start();
        if text.trim().is_empty() {
            self.notify();
            return;
        }
        // A query that starts with `>` is a command, whether it parses or not: a refused one
        // must never fall through to the web entry or to a provider (LA2).
        match command::hit(whole) {
            Some(Ok(hit)) => {
                self.put(generation, Group::Command, "", "", vec![hit]);
                return;
            }
            Some(Err(why)) => {
                if self.0.board.borrow_mut().refuse(generation, why) {
                    self.notify();
                }
                return;
            }
            None => {}
        }
        let now = now();
        let mut ranker = Ranker::new(text);
        let catalog = self.0.catalog.borrow().clone();
        let windows = self.0.windows.borrow().clone();
        {
            let usage = self.0.usage.borrow();
            let mut board = self.0.board.borrow_mut();
            board.put(generation, Group::Apps, "", "", apps::search(&catalog.apps, Group::Apps, text, &mut ranker, &usage, now));
            board.put(generation, Group::Settings, "", "", apps::search(&catalog.settings, Group::Settings, text, &mut ranker, &usage, now));
            board.put(generation, Group::Windows, "", "", windows::search(&windows, text, &mut ranker, &usage, now));
            board.put(generation, Group::Web, "", "", web::hit(text).into_iter().collect());
        }

        let weak = self.weak();
        let text = text.to_owned();
        // On the thread-default context, like every other source here. Registered before
        // the listener runs: a `query` it makes aborts this generation's debounce.
        let handle = glib::spawn_future_local(async move {
            glib::timeout_future(DEBOUNCE).await;
            if let Some(engine) = Engine::upgrade(&weak) {
                engine.start_slow(generation, text);
            }
        });
        self.0.tasks.borrow_mut().push(handle);
        self.notify();
    }

    /// Tasks hold the engine weakly: a dropped engine stops them at their next step.
    fn weak(&self) -> Weak<Inner> {
        Rc::downgrade(&self.0)
    }

    fn upgrade(weak: &Weak<Inner>) -> Option<Engine> {
        weak.upgrade().map(Engine)
    }

    fn spawn(&self, task: impl Future<Output = ()> + 'static) {
        let handle = glib::spawn_future_local(task);
        self.0.tasks.borrow_mut().push(handle);
    }

    fn start_slow(&self, generation: u64, text: String) {
        if calc::wanted(&text) {
            let (weak, text) = (self.weak(), text.clone());
            self.spawn(async move {
                // Files and providers carry their own deadline and warning.
                match within(DEADLINE, calc::evaluate(&text)).await {
                    Some(answer) => {
                        if let Some(engine) = Engine::upgrade(&weak) {
                            engine.put(generation, Group::Calc, "", "", answer.iter().map(calc::hit).collect());
                        }
                    }
                    None => tracing::warn!("the calculator missed the deadline of {} ms", DEADLINE.as_millis()),
                }
            });
        }
        if let Some(files) = self.0.files.clone() {
            let (weak, text) = (self.weak(), text.clone());
            let usage_path = self.0.usage_path.clone();
            self.spawn(async move {
                let usage_now = now();
                let answer = {
                    let usage = Usage::load(&usage_path);
                    files.search(&text, &usage, usage_now).await
                };
                let Some(engine) = Engine::upgrade(&weak) else { return };
                if let Some(answer) = answer {
                    if engine.0.board.borrow_mut().set_indexing(generation, answer.indexing) {
                        engine.put(generation, Group::Files, "", "", answer.hits);
                    }
                }
            });
        }
        let terms = provider_query(&text);
        for provider in self.0.providers.iter().cloned() {
            let (weak, terms) = (self.weak(), terms.clone());
            self.spawn(async move {
                let hits = providers::search(&provider, &terms).await;
                let Some(engine) = Engine::upgrade(&weak) else { return };
                if let Some(hits) = hits {
                    engine.put(generation, Group::Providers, &provider.bus_name, &provider.name, hits);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_get_a_bounded_query() {
        let words = vec!["x".repeat(100); 20].join("  ");
        let bounded = provider_query(&words);
        let terms: Vec<_> = bounded.split(' ').collect();
        assert_eq!(terms.len(), MAX_TERMS);
        assert!(terms.iter().all(|t| t.len() == MAX_TERM_CHARS));
    }
}
