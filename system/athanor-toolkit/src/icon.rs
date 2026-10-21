//! Images and the icon theme: an [`Image`] is a decoded PNG or a rasterised SVG, and an
//! [`IconTheme`] finds the file of a desktop entry's icon by the freedesktop icon theme
//! specification, as GTK's `IconTheme` does for the GTK dock.

use crate::render::Error;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::{PixelMetadata, Pixmap};

/// The icon an entry gets when the theme has none of its name.
pub const FALLBACK_ICON: &str = "application-x-executable-symbolic";

/// The extensions an icon file may have, in the specification's order of preference.
const EXTENSIONS: [&str; 2] = ["png", "svg"];

/// A decoded image, cheap to clone.
#[derive(Clone, Debug)]
pub struct Image {
    pixmap: Arc<Pixmap>,
}

impl Image {
    /// Reads a PNG, or rasterises an SVG so that its longer side is `px` pixels (a PNG keeps its
    /// own size, and is scaled when drawn).
    pub fn from_file(path: &Path, px: u16) -> Result<Self, Error> {
        let pixmap = match path.extension().and_then(|e| e.to_str()) {
            Some("png") => Pixmap::from_png(BufReader::new(File::open(path)?))?,
            Some("svg") => rasterise(&fs::read(path)?, px)?,
            _ => return Err(format!("{}: not a PNG or an SVG", path.display()).into()),
        };
        Ok(Self {
            pixmap: Arc::new(pixmap),
        })
    }

    /// The width in pixels.
    pub fn width(&self) -> u16 {
        self.pixmap.width()
    }

    /// The height in pixels.
    pub fn height(&self) -> u16 {
        self.pixmap.height()
    }

    pub(crate) fn pixmap(&self) -> &Arc<Pixmap> {
        &self.pixmap
    }
}

fn rasterise(svg: &[u8], px: u16) -> Result<Pixmap, Error> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())?;
    let size = tree.size();
    let scale = f32::from(px) / size.width().max(size.height());
    let dimension = |side: f32| (side * scale).ceil().max(1.0) as u32;
    let (w, h) = (dimension(size.width()), dimension(size.height()));
    let mut target = tiny_skia::Pixmap::new(w, h).ok_or("an SVG of no area")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut target.as_mut(),
    );
    // tiny-skia's pixels are premultiplied RGBA8, as vello_cpu's.
    Ok(Pixmap::from_parts(
        target.take(),
        u16::try_from(w)?,
        u16::try_from(h)?,
        PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, true),
    ))
}

/// How a directory of a theme covers sizes.
#[derive(Clone, Copy, PartialEq)]
enum Coverage {
    Fixed,
    Scalable,
    Threshold,
}

/// A subdirectory of a theme, from its `index.theme`.
struct Directory {
    path: String,
    size: i64,
    scale: i64,
    min: i64,
    max: i64,
    threshold: i64,
    coverage: Coverage,
}

impl Directory {
    /// The range of sizes the directory serves at its own scale.
    fn range(&self) -> (i64, i64) {
        match self.coverage {
            Coverage::Fixed => (self.size, self.size),
            Coverage::Scalable => (self.min, self.max),
            Coverage::Threshold => (self.size - self.threshold, self.size + self.threshold),
        }
    }

    fn matches(&self, size: i64, scale: i64) -> bool {
        let (min, max) = self.range();
        self.scale == scale && (min..=max).contains(&size)
    }

    /// How far the directory is from `size` at `scale`, in physical pixels.
    fn distance(&self, size: i64, scale: i64) -> i64 {
        let (min, max) = self.range();
        let wanted = size * scale;
        let (min, max) = (min * self.scale, max * self.scale);
        if wanted < min {
            min - wanted
        } else if wanted > max {
            wanted - max
        } else {
            0
        }
    }
}

/// A theme's `index.theme`: what it inherits and its directories.
struct Index {
    inherits: Vec<String>,
    directories: Vec<Directory>,
}

impl Index {
    /// Parses the `Icon Theme` group and the groups of the directories it lists; what the
    /// lookup does not use is ignored.
    fn parse(text: &str) -> Self {
        let mut groups: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                groups.push((name, Vec::new()));
            } else if let (Some((_, keys)), Some((key, value))) =
                (groups.last_mut(), line.split_once('='))
            {
                keys.push((key.trim(), value.trim()));
            }
        }
        fn get<'a>(keys: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
            keys.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
        }
        let list = |keys: &[(&str, &str)], name: &str| -> Vec<String> {
            get(keys, name)
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let theme = groups
            .iter()
            .find(|(name, _)| *name == "Icon Theme")
            .map(|(_, keys)| keys.as_slice())
            .unwrap_or_default();
        let directories = list(theme, "Directories")
            .into_iter()
            .filter_map(|path| {
                let keys = &groups.iter().find(|(name, _)| *name == path)?.1;
                let number = |name: &str| get(keys, name).and_then(|v| v.parse::<i64>().ok());
                let size = number("Size")?;
                Some(Directory {
                    size,
                    scale: number("Scale").unwrap_or(1),
                    min: number("MinSize").unwrap_or(size),
                    max: number("MaxSize").unwrap_or(size),
                    threshold: number("Threshold").unwrap_or(2),
                    coverage: match get(keys, "Type") {
                        Some("Fixed") => Coverage::Fixed,
                        Some("Scalable") => Coverage::Scalable,
                        _ => Coverage::Threshold,
                    },
                    path,
                })
            })
            .collect();
        Self {
            inherits: list(theme, "Inherits"),
            directories,
        }
    }
}

/// The icon themes under some base directories, looked up by name.
pub struct IconTheme {
    roots: Vec<PathBuf>,
    theme: String,
}

impl IconTheme {
    /// The theme `theme` under the base directories `roots`, searched in order.
    pub fn new(roots: Vec<PathBuf>, theme: &str) -> Self {
        Self {
            roots,
            theme: theme.to_owned(),
        }
    }

    /// The theme `theme` under the base directories of the icon theme specification: the user's
    /// `~/.icons` and `$XDG_DATA_HOME/icons`, `icons` in each of `$XDG_DATA_DIRS`, and
    /// `/usr/share/pixmaps` for icons that belong to no theme.
    pub fn system(theme: &str) -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let home = var("HOME").map(PathBuf::from);
        let data_home = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let data_dirs =
            var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut roots: Vec<PathBuf> = home.iter().map(|h| h.join(".icons")).collect();
        roots.extend(data_home.map(|d| d.join("icons")));
        roots.extend(std::env::split_paths(&data_dirs).map(|d| d.join("icons")));
        roots.push("/usr/share/pixmaps".into());
        Self::new(roots, theme)
    }

    /// The file of the icon `icon` of a desktop entry (an absolute path or a theme's name) at
    /// `size` logical pixels and `scale`; the theme's [`FALLBACK_ICON`] when the entry's is
    /// not found; `None` when neither is.
    pub fn resolve(&self, icon: &str, size: u16, scale: u16) -> Option<PathBuf> {
        let path = Path::new(icon);
        if path.is_absolute() && path.is_file() {
            return Some(path.to_owned());
        }
        self.lookup(icon, size, scale)
            .or_else(|| self.lookup(FALLBACK_ICON, size, scale))
    }

    /// [`IconTheme::resolve`], read as an image of `size` times `scale` pixels.
    pub fn image(&self, icon: &str, size: u16, scale: u16) -> Result<Image, Error> {
        let path = self
            .resolve(icon, size, scale)
            .ok_or_else(|| format!("no icon {icon:?}, nor {FALLBACK_ICON:?}, in the theme"))?;
        Image::from_file(&path, size.saturating_mul(scale))
    }

    /// The file of `name` by the specification's lookup: the theme, then what it inherits and
    /// `hicolor`, each by the directory that matches `size` or else the closest; then an icon
    /// that belongs to no theme.
    pub fn lookup(&self, name: &str, size: u16, scale: u16) -> Option<PathBuf> {
        let (size, scale) = (i64::from(size), i64::from(scale.max(1)));
        self.chain()
            .iter()
            .find_map(|theme| self.lookup_in(theme, name, size, scale))
            .or_else(|| self.roots.iter().find_map(|root| file_in(root, name)))
    }

    /// The theme, what it inherits (depth first, once each) and `hicolor` last.
    fn chain(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut chain = Vec::new();
        let mut pending = vec![self.theme.clone()];
        while let Some(theme) = pending.pop() {
            if !seen.insert(theme.clone()) {
                continue;
            }
            if let Some(index) = self.index(&theme) {
                pending.extend(index.inherits.into_iter().rev());
            }
            chain.push(theme);
        }
        if seen.insert("hicolor".to_owned()) {
            chain.push("hicolor".to_owned());
        }
        chain
    }

    /// The `index.theme` of `theme`, from the first base directory that has one.
    fn index(&self, theme: &str) -> Option<Index> {
        self.roots
            .iter()
            .find_map(|root| fs::read_to_string(root.join(theme).join("index.theme")).ok())
            .map(|text| Index::parse(&text))
    }

    fn lookup_in(&self, theme: &str, name: &str, size: i64, scale: i64) -> Option<PathBuf> {
        let index = self.index(theme)?;
        let found = |dir: &Directory| {
            self.roots
                .iter()
                .find_map(|root| file_in(&root.join(theme).join(&dir.path), name))
        };
        index
            .directories
            .iter()
            .filter(|dir| dir.matches(size, scale))
            .find_map(found)
            .or_else(|| {
                index
                    .directories
                    .iter()
                    .filter_map(|dir| Some((dir.distance(size, scale), found(dir)?)))
                    .min_by_key(|(distance, _)| *distance)
                    .map(|(_, file)| file)
            })
    }
}

/// `name` with one of the icon extensions in `dir`, if it is there.
fn file_in(dir: &Path, name: &str) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("{name}.{extension}")))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> IconTheme {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/icons");
        IconTheme::new(vec![root], "fixture")
    }

    fn tail(path: Option<PathBuf>) -> String {
        let path = path.unwrap_or_default();
        let parts: Vec<_> = path.iter().rev().take(3).collect();
        parts
            .iter()
            .rev()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    #[test]
    fn a_themed_name_takes_the_directory_that_matches_the_size() {
        let theme = fixture();
        assert_eq!(
            tail(theme.resolve("themed", 16, 1)),
            "16x16/apps/themed.png"
        );
        // 48 is outside the fixed 16x16 directory and inside the scalable one.
        assert_eq!(
            tail(theme.resolve("themed", 48, 1)),
            "scalable/apps/themed.svg"
        );
        // The parent theme's icon, by its own directory.
        assert_eq!(
            tail(theme.resolve("inherited", 48, 1)),
            "48x48/apps/inherited.png"
        );
    }

    #[test]
    fn a_name_the_theme_lacks_falls_back_to_the_generic_executable_icon() {
        let theme = fixture();
        assert_eq!(
            tail(theme.resolve("no-such-icon", 32, 1)),
            "scalable/apps/application-x-executable-symbolic.svg"
        );
    }

    #[test]
    fn an_icon_of_a_theme_with_no_fallback_is_missing() {
        let theme = IconTheme::new(vec![PathBuf::from("/nonexistent")], "fixture");
        assert_eq!(theme.resolve("themed", 16, 1), None);
        assert!(theme.image("themed", 16, 1).is_err());
    }

    #[test]
    fn png_and_svg_icons_are_read_as_images() -> Result<(), Error> {
        let theme = fixture();
        let png = theme.image("themed", 16, 1)?;
        assert_eq!((png.width(), png.height()), (16, 16));
        // The SVG is rasterised at the requested size: 48 logical pixels at scale 2.
        let svg = theme.image("themed", 48, 2)?;
        assert_eq!((svg.width(), svg.height()), (96, 96));
        let pixel = svg.pixmap().sample(48, 48);
        assert_eq!((pixel.r, pixel.g, pixel.b, pixel.a), (0xcc, 0, 0, 0xff));
        Ok(())
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;
    use crate::{render, Kind, Theme, Ui};

    #[test]
    fn an_image_widget_draws_its_pixels_and_is_named() -> Result<(), Error> {
        let theme = IconTheme::new(
            vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/icons")],
            "fixture",
        );
        let mut ui = Ui::new("Image", Theme::default(), false);
        let id = ui.add(
            Ui::ROOT,
            Kind::Image {
                image: theme.image("themed", 32, 1)?,
                size: 32.0,
                description: "Themed".into(),
            },
        );
        let pixmap = render::render(&mut ui, 1.0)?;
        let r = ui.widget(id).rect;
        let pixel = pixmap.sample((r.x + r.w / 2.0) as u16, (r.y + r.h / 2.0) as u16);
        assert_eq!((pixel.r, pixel.g, pixel.b, pixel.a), (0xcc, 0, 0, 0xff));
        Ok(())
    }
}
