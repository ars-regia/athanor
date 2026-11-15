//! What the bar's popovers hold the pages of athanor-controls with (doc_bar.md, BR3): the look
//! of a page's secondary lines and icons, and the wake that follows a page's model.

use athanor_apps::sal::Icons;
use athanor_controls::sal::widgets::Look;
use athanor_sal::{Rgba, TextStyle};
use athanor_style::appearance::Appearance;
use athanor_style::sal;

use super::{Wake, Wakeup, ICON};

/// The room between a page's radio rows, for GTK's 28 px apart: a radio is its 15.75 px line
/// of Inter at 13 px and the bar's button padding of 8 tall.
const RADIO_GAP: f32 = 4.25;

/// What a page is drawn with in a popover at `scale`: the bar's icons at a module's size, and
/// secondary lines in the caption's size and the appearance's ink2, as calmo's
/// `bar-popover-note`.
pub fn look(icons: &mut Icons, scale: u16) -> Look<'_> {
    let secondary = Rgba(sal::palette(&Appearance::read()).ink2);
    Look {
        icon: Box::new(move |name| icons.image(name, ICON, scale)),
        icon_size: f32::from(ICON),
        caption: TextStyle {
            scale: sal::size::CAPTION.scale,
            weight: sal::size::CAPTION.weight,
            features: Vec::new(),
        },
        secondary,
        radio_gap: Some(RADIO_GAP),
    }
}

/// What follows a page's model for the bar: each state wakes the bar, so that an open popover
/// shows it.
pub fn waker<T>(wake: &Wake) -> impl Fn(&T) + 'static {
    let wake = wake.clone();
    move |_| {
        let wake = wake.clone();
        // ponytail: the page applies the same state in its own follower, woken by the same send
        // at GLib's default priority; an idle source runs after it. A follower of the page's
        // model that called the bar back would need athanor-controls to take a callback.
        glib::idle_add_local_once(move || wake(Wakeup::Changed));
    }
}
