//! The shell's models of system services (doc_control_center.md, CC3). Each model speaks
//! D-Bus through `zbus` on a Tokio runtime of its own thread and publishes a typed state on a
//! `tokio::sync::watch` channel, so a GTK process awaits it from GLib's main context and no
//! reply is ever decoded on the thread that draws. The crate links no toolkit (doc_shell.md,
//! SH4): a headless program can use it without GLib.
//!
//! Every model takes the runtime's [`tokio::runtime::Handle`] as an argument and never looks
//! for an ambient one, so code running outside the runtime fails to compile instead of
//! aborting at run time.

#[cfg(feature = "audio")]
pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod media;
pub mod mirror;
pub mod network;
pub mod notifications;
pub mod props;
pub mod runtime;
#[cfg(any(test, feature = "testbus"))]
pub mod testbus;

pub use runtime::{Bus, Buses, Runtime};

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::{fs, io};

    /// The sources of the crate's models, without the test helpers.
    fn sources(dir: &Path, out: &mut Vec<(String, String)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                sources(&path, out)?;
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path.file_name().is_some_and(|name| name != "testbus.rs")
            {
                out.push((path.display().to_string(), fs::read_to_string(&path)?));
            }
        }
        Ok(())
    }

    /// CC3: a model takes the runtime's handle. An ambient lookup, or a spawn that relies on
    /// one, aborts the process when a GTK callback reaches it outside the runtime.
    #[test]
    fn no_model_looks_up_the_ambient_runtime() {
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        )
        .expect("src");
        assert!(files.len() >= 3, "the scan found the crate's sources");
        for (file, text) in files {
            // Tests may use the runtime they run on; only the code before them is a model.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
                assert!(
                    !line.contains("Handle::current") && !line.contains("tokio::spawn"),
                    "{file}: {line}"
                );
            }
        }
    }
}
