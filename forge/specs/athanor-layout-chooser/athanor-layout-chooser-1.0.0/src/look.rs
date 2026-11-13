//! The chooser's colours and measures: the toolkit's theme (`athanor_style::sal`,
//! doc_visual_language.md VL10) for the appearance's variant and accent, with the measures of
//! the GTK chooser's Calmo rules (`window.athanor-layout`, `.layout-group-title`,
//! `button.layout-choice`, `.layout-note` and `.layout-status` in
//! athanor-style's `calmo/templates/surfaces.css.in`).

use athanor_sal::{Rgba, TextStyle, Theme, TitleBarStyle, Ui, WidgetId};
use athanor_style::appearance::Appearance;
use athanor_style::sal;

/// Calmo's font, `[font] family` in athanor-style's `calmo/tokens.toml`, which the GTK surfaces
/// set through `window.athanor-surface`.
const FONT_FAMILY: &str = "Inter";
/// Calmo's `size.body`, the GTK surfaces' text size.
const BODY: f32 = 13.0;
/// Calmo's `size.small`: the headings, the notes and the status line.
const SMALL: f32 = 11.5;

/// The chooser's theme and the inks of its secondary text.
#[derive(Clone)]
pub struct Look {
    pub theme: Theme,
    /// The headings', notes' and status line's ink: Calmo's `ink2`.
    secondary: Rgba,
}

/// The look of `appearance`: its variant's colours, with its accent when it has one.
pub fn look(appearance: &Appearance) -> Look {
    let palette = sal::palette(appearance);
    let mut theme = Theme::default();
    theme.background = Rgba(palette.surf);
    theme.surface = Rgba(palette.surf2);
    theme.text = Rgba(palette.ink);
    theme.accent = Rgba(palette.acc);
    theme.on_accent = Rgba(palette.acc_ink);
    theme.border = Rgba(palette.line);
    theme.title_bar = Rgba(palette.bar);
    theme.focus = Rgba(palette.acc_fg);
    theme.font_family = Some(FONT_FAMILY.into());
    theme.font_size = BODY;
    theme.title_bar_style = TitleBarStyle::Bare;
    // The height of the GTK chooser's title bar in its goldens: 39 px at scale 1 from the top
    // of the window to its bottom border, which it includes.
    theme.title_bar_height = Some(39.0);
    // `button.layout-choice`: 14 px across, at least 34 px tall, a 1 px `line` border and a
    // radius of 8.
    theme.padding = 14.0;
    theme.control_min_height = 34.0;
    theme.control_border = 1.0;
    theme.radius = 8.0;
    // The GTK chooser's margins and spacings: 24 around, 18 between the groups, 8 inside one.
    theme.margin = 24.0;
    theme.section_gap = 18.0;
    theme.gap = 8.0;
    Look {
        theme,
        secondary: Rgba(palette.ink2),
    }
}

impl Look {
    /// A group's heading: `.layout-group-title`, 11.5 px semi-bold in `ink2`.
    pub fn heading(&self, ui: &mut Ui, label: WidgetId) {
        self.small(ui, label, 600);
    }

    /// A note under a group, or the status line: `.layout-note` and `.layout-status`, 11.5 px
    /// in `ink2`.
    pub fn note(&self, ui: &mut Ui, label: WidgetId) {
        self.small(ui, label, 400);
    }

    fn small(&self, ui: &mut Ui, label: WidgetId, weight: u16) {
        let style = TextStyle {
            scale: SMALL / BODY,
            weight,
            ..TextStyle::default()
        };
        ui.set_text_style(label, style);
        ui.set_ink(label, Some(self.secondary));
    }

    /// A dialog's message: GTK's alert dialog sets it as a heading, bold and larger.
    pub fn message(&self, ui: &mut Ui, label: WidgetId) {
        let style = TextStyle {
            scale: sal::size::TITLE_4.scale,
            weight: sal::size::TITLE_4.weight,
            ..TextStyle::default()
        };
        ui.set_text_style(label, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dark_variant_draws_dark_and_the_accent_is_the_users() {
        let light = look(&Appearance::default()).theme;
        let dark = look(&Appearance {
            dark: true,
            ..Appearance::default()
        })
        .theme;
        assert_eq!(light.background, Rgba(sal::LIGHT.surf));
        assert_eq!(dark.background, Rgba(sal::DARK.surf));
        assert_ne!(light.text, dark.text);
        assert_eq!(light.accent, Rgba(sal::palette(&Appearance::default()).acc));
    }
}
