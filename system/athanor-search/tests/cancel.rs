//! Alone in its file because it puts a stub on PATH, and PATH is process-global.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use athanor_search::engine::{Engine, DEADLINE, DEBOUNCE};
use athanor_search::item::Group;

mod common;
use common::{live_qalc, wait_until};

/// Puts a `qalc` ahead on PATH that outlives `DEADLINE` on `1+1` unless it is killed,
/// and runs the real one for everything else.
struct SlowFirstQalc(std::ffi::OsString, std::path::PathBuf);

impl SlowFirstQalc {
    fn install() -> Self {
        let real = gio::glib::find_program_in_path("qalc").expect("qalc is on PATH");
        let dir = std::env::temp_dir().join(format!("athanor-slow-qalc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("stub dir");
        let stub = dir.join("qalc");
        // `sleep` under the name qalc: `exec` makes the killed pid the sleeper itself, and `live_qalc` sees it.
        let sleeper = dir.join("slow/qalc");
        std::fs::create_dir_all(sleeper.parent().expect("parent")).expect("sleeper dir");
        let sleep = gio::glib::find_program_in_path("sleep").expect("sleep is on PATH");
        std::os::unix::fs::symlink(sleep, &sleeper).expect("sleeper");
        let script = format!(
            "#!/bin/sh\ncase \"$*\" in *1+1*) exec {} 30;; esac\nexec {} \"$@\"\n",
            sleeper.display(),
            real.display()
        );
        std::fs::write(&stub, script).expect("stub");
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).expect("path"));
        Self(old, dir)
    }
}

impl Drop for SlowFirstQalc {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.0);
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

#[test]
fn typing_cancels_the_previous_generation() {
    let _slow = SlowFirstQalc::install();
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let seen: Rc<RefCell<Vec<String>>> = Rc::default();
                let record = seen.clone();
                let state = std::env::temp_dir()
                    .join(format!("athanor-engine-{}/usage.json", std::process::id()));
                let engine = Engine::new(state, None, Vec::new(), move |rows| {
                    for section in rows.sections.iter().filter(|s| s.group == Group::Calc) {
                        record
                            .borrow_mut()
                            .extend(section.hits.iter().map(|h| h.title.clone()));
                    }
                });
                engine.query("1+1");
                gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(15)).await;
                let first = live_qalc();
                assert_eq!(first.len(), 1, "one qalc for the first query: {first:?}");
                engine.query("2+2");
                let killed = wait_until(DEADLINE / 2, || {
                    live_qalc().iter().all(|pid| !first.contains(pid))
                })
                .await;
                assert!(killed, "the first qalc was killed");
                gio::glib::timeout_future(Duration::from_millis(900)).await;
                assert_eq!(
                    *seen.borrow(),
                    ["4"],
                    "only the second query's answer was shown"
                );
                assert!(live_qalc().is_empty());
            })
        })
        .expect("a fresh context is free");
}
