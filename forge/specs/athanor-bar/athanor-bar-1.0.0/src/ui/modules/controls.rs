//! What the bar's popovers hold the pages of athanor-controls with (doc_bar.md, BR3): the theme
//! and the look of a page, and the wake that follows a page's model.

use athanor_apps::sal::Icons;
use athanor_controls::sal::widgets::Look;
use athanor_controls::sal::Turn;
use athanor_sal::{Image, Rgba, TextStyle, Theme};
use athanor_style::sal;

use super::{Reply, Wake, Wakeup, ICON};

/// The room between a page's radio rows, for the GTK popover's 30 px apart (measured on the
/// goldens of bar-battery at scale 1).
const RADIO_GAP: f32 = 2.25;

/// The least width of a page's sliders: under GTK's, so that, as in the GTK popover, the widest
/// row of switches and labels sets the popover's width and the sliders take it.
const SLIDER_WIDTH: f32 = 160.0;

/// The room between a page row's icon and its name, as the GTK popover's row box: 8 px,
/// measured on the goldens of bar-network at scale 1.
const ROW_GAP: f32 = 8.0;

/// The popover's theme for a page: flat buttons on the popover's fill, as the GTK popover's
/// device rows, where the bar's own buttons sit on a chip.
pub fn theme(popover: &Theme) -> Theme {
    let mut theme = popover.clone();
    theme.surface = popover.island;
    theme.field_width = SLIDER_WIDTH;
    theme
}

/// The popover's theme for a page of rows, the network's and Bluetooth's: flat buttons on the
/// popover's fill, a row's icon [`ROW_GAP`] from its name, and the theme's field width for the
/// password prompt's entry.
pub fn rows(popover: &Theme) -> Theme {
    let mut theme = popover.clone();
    theme.surface = popover.island;
    theme.gap = ROW_GAP;
    theme
}

/// What a page is drawn with in a popover at `scale`: the bar's icons at a module's size, in
/// their right-to-left variant when `rtl`, and secondary lines in the caption's size and
/// `secondary`, the popovers' ink2, as calmo's `bar-popover-note`.
pub fn look(icons: &mut Icons, scale: u16, rtl: bool, secondary: Rgba) -> Look<'_> {
    Look {
        icon: Box::new(move |name| image(icons, name, scale, rtl)),
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

/// The icon `name` at a module's size and `scale`, in its right-to-left variant when `rtl`.
pub fn image(icons: &mut Icons, name: &str, scale: u16, rtl: bool) -> Option<Image> {
    let name = icons.reading(name, ICON, scale, rtl);
    icons.image(&name, ICON, scale)
}

/// What the bar does after a page's `turn`: a popover keeps its size once open, so a page that
/// built its widgets again, or whose widgets set in place changed the layout's size
/// (`resized`, run only then), is built again at the new size; else `stay` says.
pub fn after_turn(turn: Turn, resized: impl FnOnce() -> bool, stay: impl FnOnce() -> Reply) -> Reply {
    match turn {
        Turn::Rebuild => Reply::Reopen,
        Turn::Updated if resized() => Reply::Reopen,
        Turn::Updated | Turn::Unchanged => stay(),
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

#[cfg(test)]
mod tests {
    use athanor_layout::preset::Preset;
    use athanor_style::appearance::Appearance;

    use super::*;
    use crate::ui::look;

    #[test]
    fn a_page_set_in_place_reopens_the_popover_only_when_its_size_changed() {
        let stay = || Reply::Close;
        assert_eq!(after_turn(Turn::Updated, || true, stay), Reply::Reopen);
        assert_eq!(after_turn(Turn::Updated, || false, stay), Reply::Close);
        assert_eq!(after_turn(Turn::Rebuild, || false, stay), Reply::Reopen);
        assert_eq!(after_turn(Turn::Unchanged, || panic!("no turn, no layout"), stay), Reply::Close);
    }

    #[test]
    fn a_page_draws_its_buttons_on_the_popover_fill() {
        let popover = look::look(&Appearance::default(), Preset::Float).popover;
        for page in [theme(&popover), rows(&popover)] {
            assert_eq!(page.surface, popover.island);
            assert_eq!(page.island, popover.island, "the frame is the popover's");
        }
        assert_eq!(rows(&popover).gap, ROW_GAP);
        assert_eq!(rows(&popover).field_width, popover.field_width, "the entry keeps its width");
        assert_eq!(theme(&popover).field_width, SLIDER_WIDTH);
    }
}
