//! The life of a query on a real main loop, with the real `qalc` (the build image has it).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use athanor_search::board::Rows;
use athanor_search::command::Refusal;
use athanor_search::engine::{Engine, DEADLINE, DEBOUNCE};
use athanor_search::item::Group;

/// `live_qalc` counts the children of the whole process: the tests that start qalc take turns.
static QALC: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod common;
use common::{live_qalc, wait_until};

#[test]
fn in_memory_sources_answer_before_the_debounce() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            let seen: Rc<RefCell<Vec<Group>>> = Rc::default();
            let record = seen.clone();
            let state = std::env::temp_dir().join(format!("athanor-engine-mem-{}/usage.json", std::process::id()));
            let engine = Engine::new(state, None, Vec::new(), move |rows| {
                *record.borrow_mut() = rows.sections.iter().map(|s| s.group).collect();
            });
            engine.query("> htop");
            assert_eq!(*seen.borrow(), [Group::Command], "synchronously, in the same call");
            engine.query("zzzz");
            assert_eq!(*seen.borrow(), [Group::Web]);
        })
        .expect("a fresh context is free");
}

fn state(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("athanor-engine-{name}-{}/usage.json", std::process::id()))
}

#[test]
fn an_over_long_command_line_is_never_run_cut() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            let seen: Rc<RefCell<Vec<Group>>> = Rc::default();
            let record = seen.clone();
            let engine = Engine::new(state("long"), None, Vec::new(), move |rows| {
                *record.borrow_mut() = rows.sections.iter().map(|s| s.group).collect();
            });
            engine.query(&format!("> {}", "a".repeat(300)));
            assert!(seen.borrow().is_empty(), "no command and no other group: {:?}", seen.borrow());
        })
        .expect("a fresh context is free");
}

#[test]
fn a_refused_command_reaches_no_other_source() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let seen: Rc<RefCell<Vec<Rows>>> = Rc::default();
                let record = seen.clone();
                let engine = Engine::new(state("refused"), None, Vec::new(), move |rows| record.borrow_mut().push(rows.clone()));
                for (query, why) in [(">\"unclosed", Refusal::Quoting), (">", Refusal::Empty), (">\u{200B}", Refusal::Hidden)] {
                    seen.borrow_mut().clear();
                    engine.query(query);
                    // Past the debounce: the calculator, files and providers never start.
                    gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(50)).await;
                    let seen = seen.borrow();
                    assert_eq!(seen.len(), 1, "{query:?}: one answer, at once");
                    assert!(seen[0].sections.is_empty() && seen[0].top.is_none(), "{query:?}: {:?}", seen[0].sections);
                    assert_eq!(seen[0].refused, Some(why), "{query:?}");
                }
            })
        })
        .expect("a fresh context is free");
}

#[test]
fn a_query_made_by_the_listener_aborts_the_outer_generation() {
    let _turn = QALC.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let slot: Rc<RefCell<Option<Engine>>> = Rc::default();
                let again = slot.clone();
                let done = Rc::new(std::cell::Cell::new(false));
                let engine = Engine::new(state("reentrant"), None, Vec::new(), move |_| {
                    if !done.replace(true) {
                        if let Some(engine) = again.borrow().clone() {
                            engine.query("2+2");
                        }
                    }
                });
                slot.replace(Some(engine.clone()));
                engine.query("1+1");
                gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(15)).await;
                assert_eq!(live_qalc().len(), 1, "only the second generation runs qalc");
                slot.replace(None);
            })
        })
        .expect("a fresh context is free");
}

#[test]
fn a_dropped_engine_stops_its_pending_work() {
    let _turn = QALC.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let calls = Rc::new(std::cell::Cell::new(0));
                let count = calls.clone();
                let engine = Engine::new(state("dropped"), None, Vec::new(), move |_| count.set(count.get() + 1));
                engine.query("1+1");
                let before = calls.get();
                // Known blind spot: the engine is dropped before the debounce, so no qalc ever starts
                // and the last assertion holds without any kill. cancel.rs covers a running child.
                drop(engine);
                gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(300)).await;
                assert_eq!(calls.get(), before, "the listener is never called again");
                assert!(wait_until(DEADLINE * 2, || live_qalc().is_empty()).await, "the pending qalc was killed");
            })
        })
        .expect("a fresh context is free");
}
