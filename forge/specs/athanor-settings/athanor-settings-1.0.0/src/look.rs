//! Settings' colours and measures: the toolkit's theme (`athanor_style::sal`,
//! doc_visual_language.md VL10) for the appearance's variant and accent, as the layout
//! chooser's.

use athanor_sal::{Rgba, TextStyle, Theme, TitleBarStyle, Ui, WidgetId};
use athanor_style::appearance::Appearance;
use athanor_style::sal;

/// Calmo's font and its `size.body` and `size.small`.
const FONT_FAMILY: &str = "Inter";
const BODY: f32 = 13.0;
const SMALL: f32 = 11.5;

#[derive(Clone)]
pub struct Look {
    pub theme: Theme,
    /// The secondary lines' ink: Calmo's `ink2`.
    secondary: Rgba,
}

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
    theme.margin = 18.0;
    theme.section_gap = 18.0;
    theme.gap = 8.0;
    Look {
        theme,
        secondary: Rgba(palette.ink2),
    }
}

impl Look {
    /// A page's title: Calmo's `title-3`.
    pub fn title(&self, ui: &mut Ui, label: WidgetId) {
        let style = TextStyle {
            scale: sal::size::TITLE_3.scale,
            weight: sal::size::TITLE_3.weight,
            ..TextStyle::default()
        };
        ui.set_text_style(label, style);
    }

    /// A group's heading, 11.5 px semi-bold in `ink2`.
    pub fn heading(&self, ui: &mut Ui, label: WidgetId) {
        self.small(ui, label, 600);
    }

    /// A secondary line, 11.5 px in `ink2`.
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
}
