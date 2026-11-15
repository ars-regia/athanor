//! SH10, the first-session layout: on a user's first session the bar picks the preset the
//! outputs suit and writes it to the user's layout file, once (`start::pick_first_layout`).

use std::cell::Cell;
use std::path::PathBuf;

use athanor_bar::start::pick_first_layout;
use athanor_sulphur_client::outputs;
use athanor_layout::loader::Paths;
use gtk4::gdk;

/// Picks now if an output is sized, otherwise on the first output change that sizes one.
/// The bar's own watch on the layout directory then draws the layout the pick wrote.
pub fn arm(display: &gdk::Display, paths: Paths, marker: PathBuf) {
    if pick_first_layout(&paths, &marker, &outputs::current(display)) {
        return;
    }
    let done = Cell::new(false);
    outputs::watch(display, move |current| {
        if !done.get() && pick_first_layout(&paths, &marker, &current) {
            done.set(true);
        }
    });
}
