//! The life of a query on a real main loop, with the real `qalc` (the build image has it).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use athanor_search::engine::{Engine, DEBOUNCE};
use athanor_search::item::Group;

/// The pids of this process's children named qalc that have not exited.
fn live_qalc() -> Vec<u32> {
    let mut pids = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").expect("tasks").flatten() {
        let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        for pid in children.split_whitespace().filter_map(|p| p.parse::<u32>().ok()) {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // "pid (comm) state ..."
            let after = stat.rsplit_once(") ").map(|(_, rest)| rest).unwrap_or("Z");
            if stat.contains("(qalc)") && !after.starts_with('Z') {
                pids.push(pid);
            }
        }
    }
    pids
}

#[test]
fn typing_cancels_the_previous_generation() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let seen: Rc<RefCell<Vec<String>>> = Rc::default();
                let record = seen.clone();
                let state = std::env::temp_dir().join(format!("athanor-engine-{}/usage.json", std::process::id()));
                let engine = Engine::new(state, None, Vec::new(), move |rows| {
                    for section in rows.sections.iter().filter(|s| s.group == Group::Calc) {
                        record.borrow_mut().extend(section.hits.iter().map(|h| h.title.clone()));
                    }
                });
                engine.query("1+1");
                gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(15)).await;
                let first = live_qalc();
                assert_eq!(first.len(), 1, "one qalc for the first query: {first:?}");
                engine.query("2+2");
                gio::glib::timeout_future(Duration::from_millis(30)).await;
                assert!(live_qalc().iter().all(|pid| !first.contains(pid)), "the first qalc was killed");
                gio::glib::timeout_future(Duration::from_millis(900)).await;
                assert_eq!(*seen.borrow(), ["4"], "only the second query's answer was shown");
                assert!(live_qalc().is_empty());
            })
        })
        .expect("a fresh context is free");
}

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
