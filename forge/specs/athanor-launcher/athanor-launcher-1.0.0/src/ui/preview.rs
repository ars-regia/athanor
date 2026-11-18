//! The preview pane (LA6): it draws the card athanor-preview hands over at each change, in a
//! column beside the results. Every text is set as plain text.

use std::collections::HashMap;
use std::path::Path;

use athanor_preview::Card;
use athanor_sal::{IconTheme, Image, Kind, Rgba, Ui, WidgetId, Wrap, FALLBACK_ICON};
use gio::prelude::*;

/// The square the picture is drawn in, as tall as the GTK pane's picture.
const PICTURE: f32 = 220.0;
/// The icon of a card without a picture, as the GTK pane's.
const ICON: u16 = 96;

/// Themed icons as images, kept by their names, size and scale until the look changes.
pub struct Icons {
    theme: IconTheme,
    /// What a symbolic icon is tinted with: the panel's text.
    ink: Rgba,
    cache: HashMap<(String, u16, u16), Option<Image>>,
}

impl Icons {
    pub fn new(theme: &str, ink: Rgba) -> Icons {
        Icons {
            theme: IconTheme::system(theme),
            ink,
            cache: HashMap::new(),
        }
    }

    /// Forgets the images, tinted with the old ink.
    pub fn reset(&mut self, ink: Rgba) {
        self.ink = ink;
        self.cache.clear();
    }

    /// The first of `icon`'s names the theme has, at `size` logical pixels and `scale`, or the
    /// theme's fallback, as GTK draws a themed icon. `None` when neither is found or the file
    /// does not read; only a themed icon has names (LA9).
    pub fn image(&mut self, icon: &gio::Icon, size: u16, scale: u16) -> Option<Image> {
        let names: Vec<String> = icon
            .downcast_ref::<gio::ThemedIcon>()
            .map(|themed| themed.names().iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        let key = (names.join("\n"), size, scale);
        if let Some(image) = self.cache.get(&key) {
            return image.clone();
        }
        let path = names
            .iter()
            .map(String::as_str)
            .chain([FALLBACK_ICON])
            .find_map(|name| self.theme.lookup(name, size, scale));
        let image = path.and_then(|path| {
            match Image::from_file(&path, size.saturating_mul(scale.max(1))) {
                Ok(image) if symbolic(&path) => Some(image.tinted(self.ink)),
                Ok(image) => Some(image),
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "the icon does not read");
                    None
                }
            }
        });
        self.cache.insert(key, image.clone());
        image
    }
}

/// A symbolic icon takes the colour of the text, as GTK recolours it.
fn symbolic(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.ends_with("-symbolic"))
}

/// A label of the pane: plain text that wraps at the pane's width.
fn label(ui: &mut Ui, pane: WidgetId, text: &str) {
    if text.is_empty() {
        return;
    }
    let label = ui.add(pane, Kind::Label(text.to_owned()));
    ui.set_wrap(label, Some(Wrap::default()));
}

/// Fills `pane` with `card`: the picture, or else the icon, then the title, the body and the
/// facts, each only when it has something to show, as the GTK pane hides an empty label.
pub fn draw(ui: &mut Ui, pane: WidgetId, card: &Card, icons: &mut Icons, scale: u16) {
    ui.remove_children(pane);
    if let Some(picture) = &card.picture {
        match Image::from_premultiplied_rgba8(picture.width, picture.height, picture.rgba.to_vec())
        {
            Ok(image) => {
                ui.image(pane, image, PICTURE, "");
            }
            Err(err) => tracing::warn!(error = %err, "the preview's picture is not an image"),
        }
    } else if let Some(image) = card
        .icon
        .as_ref()
        .and_then(|icon| icons.image(icon, ICON, scale))
    {
        ui.image(pane, image, f32::from(ICON), "");
    }
    label(ui, pane, &card.title);
    label(ui, pane, &card.body);
    label(ui, pane, &card.facts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_sal::Theme;

    #[test]
    fn an_empty_text_adds_no_label_and_a_card_drawn_again_replaces_the_last() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let pane = ui.add(Ui::ROOT, Kind::Column);
        let mut icons = Icons::new("Adwaita", Rgba([0, 0, 0, 255]));
        let card = Card {
            title: "Files".into(),
            body: String::new(),
            facts: "2 KB".into(),
            ..Card::default()
        };
        draw(&mut ui, pane, &card, &mut icons, 1);
        draw(&mut ui, pane, &card, &mut icons, 1);
        let labels: Vec<String> = ui
            .widget(pane)
            .children
            .iter()
            .filter_map(|&id| match &ui.widget(id).kind {
                Kind::Label(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, ["Files", "2 KB"]);
    }

    #[test]
    fn a_symbolic_icon_is_told_by_its_file_name() {
        assert!(symbolic(Path::new(
            "/usr/share/icons/Adwaita/symbolic/apps/a-symbolic.svg"
        )));
        assert!(!symbolic(Path::new(
            "/usr/share/icons/hicolor/48x48/apps/a.png"
        )));
    }
}
