//! Frame timing for the shell bench (doc_shell_standard.md, ST9). With `ATHANOR_SHELL_BENCH`
//! set to a non-empty value, every frame a watched surface presents is logged as one line
//! of JSON carrying the presentation time GTK's frame clock got from the compositor, in
//! microseconds of `CLOCK_MONOTONIC`, the clock the bench stamps its input with. A layer
//! surface also logs where it lies on its output, which Wayland never tells a client and
//! the bench needs to aim its pointer. Without the variable nothing is connected.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, LayerShell};

pub const VARIABLE: &str = "ATHANOR_SHELL_BENCH";
/// Presentation feedback arrives after the frame is painted: pending frames are looked at
/// this often, and dropped after `FLUSHES` looks (the compositor discarded them, or the
/// surface went away).
const FLUSH: Duration = Duration::from_millis(250);
const FLUSHES: u8 = 8;

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os(VARIABLE).is_some_and(|value| !value.is_empty()))
}

/// Logs the frames `native` presents, under the name `surface`. `native` is a widget with
/// its own surface (a window or a popover).
pub fn watch(native: &impl IsA<gtk4::Widget>, surface: &'static str) {
    if enabled() {
        attach(native.upcast_ref(), surface, None);
    }
}

/// As [`watch`], and logs where the layer surface `window` lies on its output.
pub fn watch_layer(window: &gtk4::ApplicationWindow, surface: &'static str) {
    if enabled() {
        attach(window.upcast_ref(), surface, Some(window.downgrade()));
    }
}

type Pending = Rc<RefCell<Vec<(i64, u8)>>>;

/// Follows the frame clock of each realization of `widget`: a popover is realized again
/// every time it is shown, and must not count its frames twice.
fn attach(widget: &gtk4::Widget, surface: &'static str, layer: Option<glib::WeakRef<gtk4::ApplicationWindow>>) {
    let current: Rc<RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>> = Rc::default();
    let slot = current.clone();
    widget.connect_realize(move |widget| {
        if let Some(clock) = widget.frame_clock() {
            let id = follow(&clock, surface, layer.clone());
            if let Some((old, old_id)) = slot.replace(Some((clock, id))) {
                old.disconnect(old_id);
            }
        }
    });
    widget.connect_unrealize(move |_| {
        if let Some((clock, id)) = current.take() {
            clock.disconnect(id);
        }
    });
}

fn follow(
    clock: &gdk::FrameClock,
    surface: &'static str,
    layer: Option<glib::WeakRef<gtk4::ApplicationWindow>>,
) -> glib::SignalHandlerId {
    let pending: Pending = Rc::default();
    let scheduled = Rc::new(Cell::new(false));
    let placed = Cell::new(None::<[i32; 6]>);
    clock.connect_after_paint(move |clock| {
        pending.borrow_mut().push((clock.frame_counter(), 0));
        if let Some(rect) = layer.as_ref().and_then(glib::WeakRef::upgrade).and_then(|w| geometry(&w)) {
            if placed.replace(Some(rect)) != Some(rect) {
                tracing::info!("{}", surface_line(surface, rect));
            }
        }
        schedule(clock, surface, &pending, &scheduled);
    })
}

fn schedule(clock: &gdk::FrameClock, surface: &'static str, pending: &Pending, scheduled: &Rc<Cell<bool>>) {
    if scheduled.replace(true) {
        return;
    }
    let (clock, pending, scheduled) = (clock.clone(), pending.clone(), scheduled.clone());
    glib::timeout_add_local_once(FLUSH, move || {
        scheduled.set(false);
        let done = settle(&mut pending.borrow_mut(), clock.history_start(), |frame| {
            clock.timings(frame).filter(gdk::FrameTimings::is_complete)
        });
        for timings in done {
            tracing::info!(
                "{}",
                frame_line(
                    surface,
                    timings.frame_counter(),
                    timings.presentation_time(),
                    timings.predicted_presentation_time(),
                    timings.refresh_interval(),
                )
            );
        }
        if !pending.borrow().is_empty() {
            schedule(&clock, surface, &pending, &scheduled);
        }
    });
}

/// Takes the complete frames out of `pending`, oldest first; drops the frames GDK no longer
/// remembers and those that had their last flush; counts a flush on the rest.
fn settle<T>(pending: &mut Vec<(i64, u8)>, history_start: i64, complete: impl Fn(i64) -> Option<T>) -> Vec<T> {
    let mut done = Vec::new();
    pending.retain_mut(|(frame, flushes)| {
        if *frame < history_start {
            return false;
        }
        if let Some(timings) = complete(*frame) {
            done.push(timings);
            return false;
        }
        *flushes += 1;
        *flushes < FLUSHES
    });
    done
}

/// Where `window` lies on its output: x, y, width, height, output width, output height.
fn geometry(window: &gtk4::ApplicationWindow) -> Option<[i32; 6]> {
    let output = LayerShell::monitor(window)?.geometry();
    let (w, h) = (window.width(), window.height());
    let anchored = |edge| window.is_anchor(edge).then(|| window.margin(edge));
    Some([
        origin(anchored(Edge::Left), anchored(Edge::Right), output.width(), w),
        origin(anchored(Edge::Top), anchored(Edge::Bottom), output.height(), h),
        w,
        h,
        output.width(),
        output.height(),
    ])
}

/// The origin of a layer surface along one axis: `start` and `end` are the margins of the
/// edges it is anchored to.
fn origin(start: Option<i32>, end: Option<i32>, output: i32, len: i32) -> i32 {
    match (start, end) {
        (Some(margin), _) => margin,
        (None, Some(margin)) => output - len - margin,
        (None, None) => (output - len) / 2,
    }
}

fn frame_line(surface: &str, frame: i64, presented: i64, predicted: i64, refresh: i64) -> String {
    format!(
        r#"{{"bench":"frame","surface":"{surface}","frame":{frame},"presented_us":{presented},"predicted_us":{predicted},"refresh_us":{refresh}}}"#
    )
}

fn surface_line(surface: &str, [x, y, w, h, ow, oh]: [i32; 6]) -> String {
    format!(
        r#"{{"bench":"surface","surface":"{surface}","x":{x},"y":{y},"w":{w},"h":{h},"output_w":{ow},"output_h":{oh}}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_one_line_of_json() {
        assert_eq!(
            frame_line("dock", 42, 1_000_016, 1_000_000, 16_666),
            r#"{"bench":"frame","surface":"dock","frame":42,"presented_us":1000016,"predicted_us":1000000,"refresh_us":16666}"#
        );
    }

    #[test]
    fn a_placement_is_one_line_of_json() {
        assert_eq!(
            surface_line("bar", [0, 1032, 1920, 48, 1920, 1080]),
            r#"{"bench":"surface","surface":"bar","x":0,"y":1032,"w":1920,"h":48,"output_w":1920,"output_h":1080}"#
        );
    }

    #[test]
    fn the_origin_follows_the_anchors() {
        // Anchored at the start: its margin. At the end only: from the far side.
        assert_eq!(origin(Some(8), None, 1920, 600), 8);
        assert_eq!(origin(None, Some(8), 1080, 64), 1080 - 64 - 8);
        // Both: stretched, so the start margin. Neither: centred.
        assert_eq!(origin(Some(0), Some(0), 1920, 1920), 0);
        assert_eq!(origin(None, None, 1920, 600), 660);
    }

    #[test]
    fn a_frame_is_dropped_after_its_last_flush() {
        let mut pending = vec![(1, 0), (2, FLUSHES - 1)];
        // Neither is complete: the first waits, the second has had its last chance.
        settle(&mut pending, 0, |_| None::<i64>);
        assert_eq!(pending, vec![(1, 1)]);
        // Complete frames leave and are returned; frames older than the history are lost.
        let mut pending = vec![(1, 0), (5, 0), (6, 0)];
        let done = settle(&mut pending, 2, |frame| (frame == 5).then_some(frame));
        assert_eq!(done, vec![5]);
        assert_eq!(pending, vec![(6, 1)]);
    }
}
