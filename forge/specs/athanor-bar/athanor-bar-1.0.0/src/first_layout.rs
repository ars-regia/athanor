//! SH10, the first-session layout: on a user's first session the bar picks the preset the
//! outputs suit and writes it to the user's layout file, once. The translator did this
//! until the switch of stage 2; the marker path is the same, so a desktop upgraded from it
//! never picks again.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use athanor_compositor_client::outputs;
use athanor_layout::first_session::{self, Outcome};
use athanor_layout::loader::{self, Paths};
use athanor_layout::placement::Output;
use gtk4::gdk;

/// True when nothing is left to do in this process.
fn attempt(paths: &Paths, marker: &Path, outputs: &[Output]) -> bool {
    match first_session::run(&loader::resolve(paths), &paths.user_file, marker, outputs) {
        Ok(Outcome::NotYet) => false,
        Ok(Outcome::Wrote(preset)) => {
            tracing::info!(preset = preset.id(), "first session: default layout picked");
            true
        }
        Ok(Outcome::AlreadyRan | Outcome::LeftToPolicyOrUser) => true,
        Err(err) => {
            tracing::error!(error = %err, "cannot record the first-session layout; the vendor layout applies");
            true
        }
    }
}

/// Picks now if an output is sized, otherwise on the first output change that sizes one.
/// The bar's own watch on the layout directory then draws the layout the pick wrote.
pub fn arm(display: &gdk::Display, paths: Paths, marker: PathBuf) {
    if attempt(&paths, &marker, &outputs::current(display)) {
        return;
    }
    let done = Cell::new(false);
    outputs::watch(display, move |current| {
        if !done.get() && attempt(&paths, &marker, &current) {
            done.set(true);
        }
    });
}
