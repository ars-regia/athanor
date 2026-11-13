//! A tile of the control center (doc_visual_language.md VL7 and VL10): a toggle of its own,
//! apart from any group, that shows its icon in a disc, its title and a second line. The disc
//! takes the accent while the tile is on, and is the only part that does; the tile itself keeps
//! the controls' fill. AT-SPI reads it as GTK's ToggleButton: a toggle button named by its title,
//! described by its second line, pressed while on.

use std::num::NonZeroU32;
use std::sync::Arc;

use parley::Layout;

use crate::icon::Image;
use crate::text::{Brush, TextContext, TextStyle};
use crate::theme::Rgba;
use crate::widget::{Kind, Rect, Ui, WidgetId};
use crate::wrap::{self, Wrap};

/// A tile's height, its icon's disc and the glyph in it, and its corner radius: a 2×1 tile of
/// VL7, `size.icon.cc_disc` and `size.icon.cc_glyph` of the tokens.
pub(crate) const HEIGHT: f32 = 64.0;
pub(crate) const DISC: f32 = 36.0;
pub(crate) const GLYPH: f32 = 20.0;
pub(crate) const RADIUS: f32 = 18.0;
/// The second line's size against the theme's font: the `caption` step of VL6.
const CAPTION: f32 = 0.82;

/// A tile: see [`Ui::tile`].
pub struct Tile {
    /// What it is: its name for assistive technologies.
    pub title: String,
    /// The state it is in, under the title: its description for assistive technologies.
    pub subtitle: String,
    /// The glyph in the disc; it is drawn in the ink of the disc, whatever its colour.
    pub image: Image,
    /// Whether the tile is on.
    pub active: bool,
    /// The second line, laid out by the last layout; boxed, so that a tile does not make every
    /// widget as large as a layout.
    pub(crate) second: Option<Box<Layout<Brush>>>,
    /// The width the last place gave the tile, which the second line ends in an ellipsis at;
    /// `None` before the first. [`Ui::layout`] records it.
    pub(crate) placed: Option<f32>,
    /// The glyph in the disc's inks, off and on, made by a layout from `image` and the theme's
    /// inks: kept between frames, so that the GPU's atlas holds them and a frame tints nothing.
    pub(crate) glyphs: Option<Glyphs>,
}

/// A tile's glyph tinted in the disc's two inks, and what it was tinted from.
pub(crate) struct Glyphs {
    from: Image,
    inks: [Rgba; 2],
    /// Off, then on.
    pub(crate) tinted: [Image; 2],
}

impl Tile {
    /// The glyph as the disc draws it: in the theme's text colour while off, in the colour on
    /// the accent while on. `None` before the first layout.
    pub(crate) fn glyph(&self) -> Option<&Image> {
        self.glyphs.as_ref().map(|g| &g.tinted[usize::from(self.active)])
    }

    /// Tints the glyph again when its image or the inks changed since the last time.
    fn tint(&mut self, inks: [Rgba; 2]) {
        let same = self.glyphs.as_ref().is_some_and(|g| {
            g.inks == inks && Arc::ptr_eq(g.from.pixmap(), self.image.pixmap())
        });
        if !same {
            self.glyphs = Some(Glyphs {
                from: self.image.clone(),
                inks,
                tinted: inks.map(|ink| self.image.tinted(ink)),
            });
        }
    }
}

impl Ui {
    /// Adds a tile titled `title`, with `subtitle` under it and `image` in its disc, on when
    /// `active`. Activating it turns it on or off and raises [`crate::Event::Toggled`]; set the
    /// second line with [`Ui::set_tile_subtitle`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn tile(
        &mut self,
        parent: WidgetId,
        title: &str,
        subtitle: &str,
        image: Image,
        active: bool,
    ) -> WidgetId {
        let tile = Tile {
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            image,
            active,
            second: None,
            placed: None,
            glyphs: None,
        };
        self.add(parent, Kind::Tile(tile))
    }

    /// Shows `text` as the tile's second line. Whether it changed: `false` for the same text
    /// and for a widget that is not a tile. Call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_tile_subtitle(&mut self, id: WidgetId, text: &str) -> bool {
        match &mut self.widgets[id].kind {
            Kind::Tile(tile) if tile.subtitle != text => {
                tile.subtitle = text.to_owned();
                true
            }
            _ => false,
        }
    }
}

/// The tile's size: its padding around the disc and its title, and at least its height. The
/// second line, as GTK's tile ellipsizes it, asks for no width: it ends in an ellipsis at the
/// width the tile is placed at, so a long one, a network's name, never widens the tile. The title
/// takes the widget's style at the `heading` weight; the layouts are kept for drawing, the
/// title's in the widget's `text`.
pub(crate) fn measure(
    tile: &mut Tile,
    text: &mut TextContext,
    title: &mut Option<Layout<Brush>>,
    (size, pad, gap, rtl): (f32, f32, f32, bool),
    style: &TextStyle,
    inks: [Rgba; 2],
) -> (f32, f32) {
    tile.tint(inks);
    let heading = TextStyle {
        weight: 700,
        ..style.clone()
    };
    let caption = TextStyle {
        scale: style.scale * CAPTION,
        weight: 400,
        ..style.clone()
    };
    let first = text.layout(&tile.title, size, rtl, &heading);
    let chrome = 2.0 * pad + DISC + gap;
    let second = match tile.placed {
        Some(placed) => {
            let one = Wrap {
                lines: NonZeroU32::new(1),
                ..Wrap::default()
            };
            let width = (placed - chrome).max(0.0);
            wrap::layout(text, &tile.subtitle, (size, rtl), &caption, &one, width)
        }
        None => text.layout(&tile.subtitle, size, rtl, &caption),
    };
    let w = chrome + first.width();
    let h = (2.0 * pad + first.height() + second.height()).max(HEIGHT);
    *title = Some(first);
    tile.second = Some(Box::new(second));
    (w, h)
}

/// Where the tile draws its disc and the start of its lines: the disc at the start edge inside
/// the padding, the lines after it, mirrored in right-to-left.
pub(crate) fn disc(r: Rect, pad: f32, rtl: bool) -> Rect {
    let x = if rtl {
        r.x + r.w - pad - DISC
    } else {
        r.x + pad
    };
    Rect {
        x,
        y: r.y + (r.h - DISC) / 2.0,
        w: DISC,
        h: DISC,
    }
}

/// The x of a line `width` wide, after the disc in reading order.
pub(crate) fn line_x(r: Rect, pad: f32, gap: f32, rtl: bool, width: f32) -> f32 {
    if rtl {
        r.x + r.w - pad - DISC - gap - width
    } else {
        r.x + pad + DISC + gap
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use crate::widget::{Event, Kind, Ui};

    fn glyph() -> crate::icon::Image {
        match crate::icon::Image::from_rgba8(1, 1, vec![0, 0, 0, 255]) {
            Ok(image) => image,
            Err(err) => unreachable!("a 1×1 image is valid: {err}"),
        }
    }

    #[test]
    fn a_tile_turns_on_and_off_by_itself_and_keeps_its_height() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        let wifi = ui.tile(row, "Wi-Fi", "On", glyph(), true);
        let dark = ui.tile(row, "Dark mode", "Off", glyph(), false);
        assert!(ui.widget(wifi).is_focusable());
        assert_eq!(ui.activate(dark), Some(Event::Toggled(dark)));
        assert!(
            ui.is_active(dark) && ui.is_active(wifi),
            "no group: both on"
        );
        assert_eq!(ui.activate(wifi), Some(Event::Toggled(wifi)));
        assert!(!ui.is_active(wifi));
        assert_eq!(ui.set_active(wifi, false), None, "already off");
        assert_eq!(ui.set_active(wifi, true), Some(Event::Toggled(wifi)));
        assert!(ui.set_tile_subtitle(wifi, "Connecting"));
        assert!(!ui.set_tile_subtitle(wifi, "Connecting"));
        ui.layout();
        assert!(ui.widget(wifi).rect.h >= super::HEIGHT);
    }

    #[test]
    fn a_tile_tints_its_glyph_once_until_the_image_or_the_theme_changes() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let wifi = ui.tile(Ui::ROOT, "Wi-Fi", "On", glyph(), true);
        let tinted = |ui: &Ui| match &ui.widget(wifi).kind {
            Kind::Tile(tile) => tile.glyph().map(|g| std::sync::Arc::clone(g.pixmap())),
            _ => None,
        };
        assert!(tinted(&ui).is_none(), "before a layout");
        ui.layout();
        let first = tinted(&ui).expect("a layout tints it");
        let ink = ui.theme.on_accent.0;
        let pixel = first.data()[0];
        assert_eq!([pixel.r, pixel.g, pixel.b], [ink[0], ink[1], ink[2]], "on: on the accent");
        ui.layout();
        let again = tinted(&ui).expect("still tinted");
        assert!(std::sync::Arc::ptr_eq(&first, &again), "the same theme and image: kept");
        ui.theme.on_accent = crate::Rgba([1, 2, 3, 255]);
        ui.layout();
        let other = tinted(&ui).expect("tinted again");
        assert!(!std::sync::Arc::ptr_eq(&first, &other), "a new theme: tinted again");
        if let Kind::Tile(tile) = &mut ui.widgets[wifi].kind {
            tile.image = glyph();
        }
        ui.layout();
        let image = tinted(&ui).expect("tinted again");
        assert!(!std::sync::Arc::ptr_eq(&other, &image), "a new image: tinted again");
    }

    /// GTK's tile ellipsizes its second line at the end: a network's long name ends in an
    /// ellipsis inside the tile and leaves the row as wide as a short one does.
    #[test]
    fn a_long_second_line_ends_in_an_ellipsis_and_never_widens_the_row() {
        let long = "A network whose name runs on for sixty characters or nearly";
        let lay = |subtitle: &str| {
            let mut ui = Ui::new("t", Theme::default(), false);
            let row = ui.add(Ui::ROOT, Kind::Row);
            let wifi = ui.tile(row, "Wi-Fi", subtitle, glyph(), true);
            ui.tile(row, "Bluetooth", "Off", glyph(), false);
            ui.layout();
            (ui, wifi)
        };
        let (short, _) = lay("Home");
        let (ui, wifi) = lay(long);
        assert_eq!(ui.size(), short.size(), "the long name widens nothing");
        let r = ui.widget(wifi).rect;
        let room = r.w - 2.0 * ui.theme.padding - super::DISC - ui.theme.gap;
        let Kind::Tile(tile) = &ui.widget(wifi).kind else {
            unreachable!("a tile");
        };
        assert_eq!(tile.placed, Some(r.w), "the layout recorded its place");
        let second = tile.second.as_deref().expect("laid out");
        assert_eq!(second.len(), 1, "one line");
        assert!(second.width() <= room + 0.5, "{} in {room}", second.width());
        assert!(second.width() > 0.0, "the start of the name and the ellipsis");
    }

    /// The second line stays the tile's description over a tooltip, as the GTK panel sets it;
    /// without one, the tooltip describes it.
    #[test]
    fn the_second_line_describes_the_tile_over_a_tooltip() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let wifi = ui.tile(Ui::ROOT, "Wi-Fi", "Home", glyph(), true);
        ui.set_tooltip(wifi, Some("Wi-Fi"));
        ui.layout();
        let described = |ui: &Ui| {
            crate::a11y::node(ui, wifi, 1.0)
                .description()
                .map(str::to_owned)
        };
        assert_eq!(described(&ui).as_deref(), Some("Home"));
        assert!(ui.set_tile_subtitle(wifi, ""));
        assert_eq!(described(&ui).as_deref(), Some("Wi-Fi"));
    }

    #[test]
    fn the_disc_and_the_lines_follow_the_reading_order() {
        let r = crate::widget::Rect {
            x: 10.0,
            y: 0.0,
            w: 200.0,
            h: 64.0,
        };
        let ltr = super::disc(r, 12.0, false);
        let rtl = super::disc(r, 12.0, true);
        assert_eq!((ltr.x, ltr.y), (22.0, 14.0));
        assert_eq!(rtl.x + rtl.w, 198.0);
        assert_eq!(super::line_x(r, 12.0, 6.0, false, 50.0), 64.0);
        assert_eq!(super::line_x(r, 12.0, 6.0, true, 50.0), 106.0);
    }
}
