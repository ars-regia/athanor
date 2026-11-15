//! The bar's colours and measures: the toolkit's theme (`athanor_style::sal`,
//! doc_visual_language.md VL10) for the appearance's variant and accent, with the bar's
//! measures of Calmo's stylesheet (`window.athanor-bar`, `button.bar-button`,
//! `popover.athanor-bar-popover`), which the GTK bar read.

use athanor_layout::preset::Preset;
use athanor_sal::{Rgba, Theme, Ui, WidgetId};
use athanor_style::appearance::Appearance;
use athanor_style::sal;

/// The bar's buttons, as `button.bar-button`: at least 32 px square, 8 px of padding across,
/// rounded by 8 px, 2 px apart in a group.
const BUTTON: f32 = 32.0;
const BUTTON_PADDING: f32 = 8.0;
const BUTTON_RADIUS: f32 = 8.0;
const GAP: f32 = 2.0;
/// A float group's island, as `.preset-float .bar-group`: rounded by 10 px.
const ISLAND_RADIUS: f32 = 10.0;
/// A popover's frame, as `popover.athanor-bar-popover > contents` (12 px of padding, a 1 px
/// line) with VL7's radius for the popovers, tiles of 12 px at a 12 px inset, and an arrow of
/// 10 px, which GTK's measures on the goldens.
const POPOVER_PADDING: f32 = sal::space::M;
const POPOVER_RADIUS: f32 = sal::radius::CARD + sal::space::M;
const POPOVER_BORDER: f32 = 1.0;
const POPOVER_ARROW: f32 = 10.0;

/// The themes of the bar's surfaces and of its popovers.
pub struct Look {
    pub bar: Theme,
    pub popover: Theme,
}

/// A group's padding, horizontal then vertical, as `.bar-group`: 2 px above and below but in
/// the minimal preset, 4 px at each end.
pub fn group_padding(preset: Preset) -> [f32; 2] {
    match preset {
        Preset::Minimal => [4.0, 0.0],
        Preset::Bar | Preset::Float => [4.0, 2.0],
    }
}

/// Calmo's family, the GTK sheet's `font-family`.
const FONT_FAMILY: &str = "Inter";

/// A float group's margin around its island: top, right, bottom, left, as
/// `.preset-float.edge-top .bar-group` (6 px 8 px 14 px) and its bottom twin; the inner side
/// leaves room for the shadow.
pub fn island_margin(top: bool) -> [f32; 4] {
    if top {
        [6.0, 8.0, 14.0, 8.0]
    } else {
        [14.0, 8.0, 6.0, 8.0]
    }
}

/// The look of `appearance` for `preset`: the bar's colour across the surface, or, for the
/// float preset, a transparent surface with each group on an island of that colour.
pub fn look(appearance: &Appearance, preset: Preset) -> Look {
    let palette = sal::palette(appearance);
    let mut bar = Theme::default();
    bar.background = if preset == Preset::Float {
        Rgba([0, 0, 0, 0])
    } else {
        Rgba(palette.bar)
    };
    bar.surface = Rgba(palette.chip);
    bar.text = Rgba(palette.ink);
    bar.accent = Rgba(palette.acc_fg);
    bar.on_accent = Rgba(palette.acc_ink);
    bar.border = Rgba(palette.line);
    bar.title_bar = Rgba(palette.bar);
    bar.focus = Rgba(palette.acc_fg);
    // The system font's size: the clock and the popovers follow the user's font (VL6), in
    // Calmo's family, as the GTK bar's sheet sets it.
    bar.font_size = appearance.font_size;
    bar.font_family = Some(FONT_FAMILY.into());
    bar.padding = BUTTON_PADDING;
    bar.margin = 0.0;
    bar.gap = GAP;
    bar.radius = BUTTON_RADIUS;
    bar.button_min = BUTTON;
    bar.island = Rgba(palette.bar);
    bar.island_radius = ISLAND_RADIUS;
    // The group's own padding is the inline's (`group_padding`).
    bar.island_padding = 0.0;

    // The popup's surface is transparent around the frame and its arrow.
    let mut popover = bar.clone();
    popover.background = Rgba([0, 0, 0, 0]);
    popover.island = Rgba(palette.surf);
    popover.island_radius = POPOVER_RADIUS;
    popover.island_padding = POPOVER_PADDING;
    popover.island_border = POPOVER_BORDER;
    popover.island_arrow = POPOVER_ARROW;
    Look { bar, popover }
}

/// A popover named `name` in `theme`, opening below the bar when `top`, and its frame: the
/// island the content goes in, whose arrow points at the button and whose margin on the edge
/// facing the bar leaves the arrow its room.
pub fn popover(name: &str, theme: Theme, rtl: bool, top: bool) -> (Ui, WidgetId) {
    let arrow = theme.island_arrow;
    let mut ui = Ui::new(name, theme, rtl);
    let margin = if top {
        [arrow, 0.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, arrow, 0.0]
    };
    let frame = ui.island(Ui::ROOT, true, margin);
    ui.set_arrow(frame);
    (ui, frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_and_its_popovers_take_calmos_family() {
        let look = look(&Appearance::default(), Preset::Float);
        for theme in [look.bar, look.popover] {
            assert_eq!(theme.font_family.as_deref(), Some("Inter"));
        }
    }

    #[test]
    fn only_the_float_preset_draws_on_a_transparent_surface() {
        let appearance = Appearance::default();
        let palette = sal::palette(&appearance);
        for preset in [Preset::Bar, Preset::Minimal] {
            assert_eq!(look(&appearance, preset).bar.background, Rgba(palette.bar));
        }
        let float = look(&appearance, Preset::Float);
        assert_eq!(float.bar.background.0[3], 0);
        assert_eq!(float.bar.island, Rgba(palette.bar));
    }

    #[test]
    fn the_buttons_keep_the_bar_buttons_measures() {
        let look = look(&Appearance::default(), Preset::Bar);
        assert_eq!(look.bar.button_min, 32.0);
        assert_eq!((look.bar.padding, look.bar.radius), (8.0, 8.0));
    }

    #[test]
    fn a_popover_is_a_framed_island_with_room_for_its_arrow_towards_the_bar() {
        let look = look(&Appearance::default(), Preset::Bar);
        assert_eq!(look.popover.background.0[3], 0);
        assert_eq!(
            look.popover.island,
            Rgba(sal::palette(&Appearance::default()).surf)
        );
        assert_eq!(look.popover.island_radius, 24.0);
        assert_eq!(
            (look.popover.island_padding, look.popover.island_border),
            (12.0, 1.0)
        );
        for (top, margin) in [(true, [10.0, 0.0, 0.0, 0.0]), (false, [0.0, 0.0, 10.0, 0.0])] {
            let (ui, frame) = popover("Power", look.popover.clone(), false, top);
            assert!(matches!(
                ui.widget(frame).kind,
                athanor_sal::Kind::Island { vertical: true, margin: m, .. } if m == margin
            ));
        }
    }

    #[test]
    fn the_minimal_preset_has_no_padding_above_and_below() {
        assert_eq!(group_padding(Preset::Minimal), [4.0, 0.0]);
        assert_eq!(group_padding(Preset::Bar), [4.0, 2.0]);
    }
}
