//! The layout of the Athanor desktop (doc_shell.md, SH6-SH8, SH10): the versioned,
//! layered document that names a preset and two knobs, and what follows from it.
//!
//! This crate is the permanent part of stage 1c. It knows no toolkit: the translator
//! that renders the layout for cosmic-panel and the chooser window both link it, and so
//! will the shell that one day reads the document itself.

pub mod apply;
pub mod cosmic;
pub mod document;
pub mod first_session;
pub mod loader;
pub mod placement;
pub mod preset;
pub mod user;

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// A fresh directory for one test. Every call gets its own, even when two modules'
    /// tests pick the same name and run in parallel.
    pub fn scratch(name: &str) -> PathBuf {
        let call = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "athanor-layout-{}-{call}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create the scratch directory");
        dir
    }
}
