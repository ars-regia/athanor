//! Images and the icon theme: an [`Image`] is a decoded PNG or a rasterised SVG, and an
//! [`IconTheme`] finds the file of a desktop entry's icon by the freedesktop icon theme
//! specification, as GTK's `IconTheme` does for the GTK dock.

use crate::render::Error;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::ops::RangeInclusive;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::{PixelMetadata, Pixmap};

/// The icon an entry gets when the theme has none of its name.
pub const FALLBACK_ICON: &str = "application-x-executable-symbolic";

/// The extensions an icon file may have, in the specification's order of preference.
const EXTENSIONS: [&str; 2] = ["png", "svg"];

/// The longest side of an image, in pixels: a PNG whose header names a larger one is refused
/// before it is decoded, and an SVG is never rasterised larger. An icon is drawn far smaller.
pub const MAX_IMAGE_SIDE: u16 = 1024;

/// The largest PNG or SVG file read, in bytes.
const MAX_IMAGE_FILE: u64 = 4 << 20;

/// The largest `index.theme` read, in bytes.
const MAX_INDEX_FILE: u64 = 1 << 20;

/// The bounds of a directory's `Size`, `MinSize` and `MaxSize`, `Scale` and `Threshold`. They keep
/// the lookup's arithmetic far from overflow: a directory with a value outside them is skipped.
const SIZES: RangeInclusive<i64> = 1..=4096;
const SCALES: RangeInclusive<i64> = 1..=16;
const THRESHOLDS: RangeInclusive<i64> = 0..=4096;

/// The most directories read from an `index.theme`, which bounds the parse and the files each
/// lookup stats. hicolor lists about a hundred.
const MAX_DIRECTORIES: usize = 256;

/// `O_NONBLOCK` of Linux on x86_64 and aarch64; libc is not a dependency of the toolkit.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("O_NONBLOCK is defined for x86_64 and aarch64 only: add this architecture's value");

/// The bytes of the regular file `path`, refused when it is larger than `cap` bytes. The
/// errors do not name the path, which the caller adds.
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, Error> {
    // A FIFO or a device would block the open or never end: only a regular file is opened.
    if !fs::metadata(path)?.is_file() {
        return Err("not a regular file".into());
    }
    // Opened without blocking, should a FIFO take the file's place after the check, and checked
    // again on what was opened.
    let file = File::options()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err("not a regular file".into());
    }
    let len = metadata.len();
    if len > cap {
        return Err(format!("{len} bytes, more than {cap}").into());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len)?);
    // The file may grow after the check: never read past the cap.
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(format!("more than {cap} bytes").into());
    }
    Ok(bytes)
}

/// The width and height a PNG's header names, from its first chunk, IHDR, which the format
/// requires right after the signature.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let chunk = bytes.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    // The chunk's length, its type, then the width and the height, each four bytes.
    if chunk.get(4..8)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| Some(u32::from_be_bytes(chunk.get(at..at + 4)?.try_into().ok()?));
    Some((word(8)?, word(12)?))
}

/// Decodes a PNG once its header names a size within [`MAX_IMAGE_SIDE`]: the decoder allocates
/// the whole image from the header before it reads a pixel.
fn decode_png(bytes: &[u8]) -> Result<Pixmap, Error> {
    let (width, height) = png_size(bytes).ok_or("not a PNG")?;
    let side = u32::from(MAX_IMAGE_SIDE);
    if width > side || height > side {
        return Err(format!("a PNG of {width}x{height} pixels, over {side} on a side").into());
    }
    Ok(Pixmap::from_png(Cursor::new(bytes))?)
}

/// Whether `name` names an icon or a theme and nothing else: not empty, not `.` or `..`, and
/// without a `/`. Names are not paths.
fn is_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// Whether `dir`, a theme's subdirectory, stays inside the theme: relative and without `..`.
fn is_subdirectory(dir: &str) -> bool {
    Path::new(dir)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// A decoded image, cheap to clone.
#[derive(Clone, Debug)]
pub struct Image {
    pub(crate) pixmap: Arc<Pixmap>,
}

impl Image {
    /// Reads a PNG, or rasterises an SVG so that its longer side is `px` pixels (a PNG keeps its
    /// own size, and is scaled when drawn). Neither may be larger than [`MAX_IMAGE_SIDE`] on a
    /// side, and `px` is held to it.
    pub fn from_file(path: &Path, px: u16) -> Result<Self, Error> {
        // The path is printed escaped: a theme's file name may carry control characters.
        let pixmap = match icon_extension(path) {
            Some("png") => read_capped(path, MAX_IMAGE_FILE).and_then(|bytes| decode_png(&bytes)),
            Some("svg") => {
                read_capped(path, MAX_IMAGE_FILE).and_then(|bytes| rasterise(&bytes, px))
            }
            _ => Err("not a PNG or an SVG".into()),
        }
        .map_err(|error| format!("{path:?}: {error}"))?;
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
    let options = usvg::Options {
        // An icon is drawn from its own file alone. usvg's default resolver reads any path an
        // <image> or a <feImage> names (/dev/zero or a FIFO included) and decodes data: URLs;
        // icons need neither.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(svg, &options)?;
    let size = tree.size();
    let scale = f32::from(px.min(MAX_IMAGE_SIDE)) / size.width().max(size.height());
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
    /// Parses the `Icon Theme` group and the groups of the directories it lists in
    /// `Directories` and `ScaledDirectories`; what the lookup does not use is ignored.
    fn parse(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let theme = groups
            .iter()
            .find(|(name, _)| *name == "Icon Theme")
            .map(|(_, keys)| keys.as_slice())
            .unwrap_or_default();
        // ScaledDirectories lists the directories of a scale above 1 (hicolor's @2 ones), kept
        // apart for implementations that predate them.
        let directories = list(theme, "Directories")
            .into_iter()
            .chain(list(theme, "ScaledDirectories"))
            .filter(|path| is_subdirectory(path))
            .take(MAX_DIRECTORIES)
            .filter_map(|path| {
                let keys = &groups.iter().find(|(name, _)| *name == path)?.1;
                // A value out of its bounds, or not a number, skips the directory; one that is
                // absent takes its default, but Size, which the specification requires.
                let number =
                    |name: &str, default: i64, bounds: RangeInclusive<i64>| match get(keys, name) {
                        None => Some(default),
                        Some(value) => value.parse().ok().filter(|n| bounds.contains(n)),
                    };
                let size = get(keys, "Size")?
                    .parse()
                    .ok()
                    .filter(|n| SIZES.contains(n))?;
                Some(Directory {
                    size,
                    scale: number("Scale", 1, SCALES)?,
                    min: number("MinSize", size, SIZES)?,
                    max: number("MaxSize", size, SIZES)?,
                    threshold: number("Threshold", 2, THRESHOLDS)?,
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
            inherits: list(theme, "Inherits")
                .into_iter()
                .filter(|name| is_name(name))
                .collect(),
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

    /// [`IconTheme::resolve`], read as an image of `size` times `scale` pixels. A file that
    /// cannot be read as an image gets [`FALLBACK_ICON`] instead, as GTK draws a fallback for a
    /// broken icon.
    pub fn image(&self, icon: &str, size: u16, scale: u16) -> Result<Image, Error> {
        let px = size.saturating_mul(scale);
        if let Some(path) = self.resolve(icon, size, scale) {
            match Image::from_file(&path, px) {
                Ok(image) => return Ok(image),
                // The error names the path, escaped.
                Err(error) => {
                    eprintln!("athanor-toolkit: icon {icon:?}: {error}; drawing {FALLBACK_ICON}")
                }
            }
        }
        let path = self
            .lookup(FALLBACK_ICON, size, scale)
            .ok_or_else(|| format!("no icon {icon:?}, nor {FALLBACK_ICON:?}, in the theme"))?;
        Image::from_file(&path, px)
    }

    /// The file of `name` by the specification's lookup: the theme, then what it inherits and
    /// `hicolor`, each by the directory that matches `size` or else the closest; then an icon
    /// that belongs to no theme.
    pub fn lookup(&self, name: &str, size: u16, scale: u16) -> Option<PathBuf> {
        if !is_name(name) {
            return None;
        }
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
            if !is_name(&theme) || !seen.insert(theme.clone()) {
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
            .find_map(|root| {
                read_capped(&root.join(theme).join("index.theme"), MAX_INDEX_FILE).ok()
            })
            .and_then(|bytes| String::from_utf8(bytes).ok())
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

/// The icon extension of `path`, in lower case, whatever its case in the name.
fn icon_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    EXTENSIONS
        .into_iter()
        .find(|known| known.eq_ignore_ascii_case(extension))
}

/// `name` with one of the icon extensions in `dir`, if it is there; `name` itself when it
/// already carries one, as an `Icon=foo.png` written against the specification does.
fn file_in(dir: &Path, name: &str) -> Option<PathBuf> {
    if icon_extension(Path::new(name)).is_some() {
        let path = dir.join(name);
        return path.is_file().then_some(path);
    }
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

    /// A fresh directory holding `files`, each a path relative to it and its bytes.
    pub(super) fn scratch(test: &str, files: &[(&str, &[u8])]) -> Result<PathBuf, Error> {
        let dir = std::env::temp_dir().join(format!(
            "athanor-toolkit-icon-{}-{test}",
            std::process::id()
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        for (path, bytes) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().ok_or("a file with no directory")?)?;
            fs::write(path, bytes)?;
        }
        Ok(dir)
    }

    const PNG: &[u8] = include_bytes!("../tests/icons/fixture/16x16/apps/themed.png");

    #[test]
    fn a_png_whose_header_names_a_huge_image_is_refused_before_it_is_decoded() -> Result<(), Error>
    {
        // The signature, then IHDR: 65535 by 65535 pixels of 8-bit RGBA, 16 GiB decoded.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&[
            0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff, 8, 6, 0, 0, 0, 0, 0, 0, 0,
        ]);
        let dir = scratch("huge-png", &[("huge.png", &png)])?;
        let error = Image::from_file(&dir.join("huge.png"), 16)
            .err()
            .ok_or("a 65535x65535 PNG was decoded")?;
        assert!(error.to_string().contains("over 1024 on a side"), "{error}");
        Ok(())
    }

    #[test]
    fn files_over_their_cap_are_not_read() -> Result<(), Error> {
        let mut big = PNG.to_vec();
        big.resize(4 << 20 | 1, 0);
        let index = format!(
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=16\nType=Fixed\n#{}\n",
            "x".repeat(1 << 20)
        );
        let dir = scratch(
            "caps",
            &[
                ("big.png", &big),
                ("t/index.theme", index.as_bytes()),
                ("t/apps/icon.png", PNG),
            ],
        )?;
        let error = Image::from_file(&dir.join("big.png"), 16)
            .err()
            .ok_or("a PNG over 4 MiB was read")?;
        assert!(error.to_string().contains("more than"), "{error}");
        // An index.theme over 1 MiB is not read: the theme has no directories.
        assert_eq!(IconTheme::new(vec![dir], "t").resolve("icon", 16, 1), None);
        Ok(())
    }

    #[test]
    fn an_svg_reads_no_file_it_names() -> Result<(), Error> {
        let red = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/icons/fixture/scalable/apps/themed.svg");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16"><image xlink:href="{}" width="16" height="16"/></svg>"#,
            red.display()
        );
        let dir = scratch("svg-href", &[("icon.svg", svg.as_bytes())])?;
        let image = Image::from_file(&dir.join("icon.svg"), 16)?;
        // The red square it names is not drawn.
        assert_eq!(image.pixmap().sample(8, 8).a, 0);
        Ok(())
    }

    #[test]
    fn an_svg_naming_dev_zero_renders_at_once() -> Result<(), Error> {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16"><filter id="f"><feImage xlink:href="/dev/zero"/></filter><image xlink:href="/dev/zero" width="16" height="16"/><rect width="16" height="16" filter="url(#f)"/></svg>"#;
        let dir = scratch("svg-dev-zero", &[("icon.svg", svg)])?;
        let started = std::time::Instant::now();
        Image::from_file(&dir.join("icon.svg"), 16)?;
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        Ok(())
    }

    #[test]
    fn a_directory_with_values_out_of_bounds_is_skipped() -> Result<(), Error> {
        let index = b"[Icon Theme]\nDirectories=huge,scaled,apps\n\
            [huge]\nSize=9223372036854775807\nType=Threshold\n\
            [scaled]\nSize=16\nScale=9223372036854775807\n\
            [apps]\nSize=16\nType=Fixed\n";
        let dir = scratch(
            "bounds",
            &[
                ("t/index.theme", index),
                ("t/huge/icon.png", PNG),
                ("t/scaled/icon.png", PNG),
                ("t/apps/icon.png", PNG),
            ],
        )?;
        let theme = IconTheme::new(vec![dir], "t");
        assert_eq!(tail(theme.resolve("icon", 16, 1)), "t/apps/icon.png");
        assert_eq!(tail(theme.resolve("icon", 512, 16)), "t/apps/icon.png");
        Ok(())
    }

    #[test]
    fn an_error_prints_the_path_escaped() -> Result<(), Error> {
        let dir = scratch("escape", &[("red\x1b[31m.png", b"not a png")])?;
        let error = match Image::from_file(&dir.join("red\x1b[31m.png"), 16) {
            Ok(_) => return Err("a file that is not a PNG was decoded".into()),
            Err(error) => error.to_string(),
        };
        assert!(!error.contains('\x1b'), "{error:?}");
        assert!(error.contains("red\\u{1b}[31m.png"), "{error}");
        Ok(())
    }

    #[test]
    fn names_and_directories_are_not_paths() -> Result<(), Error> {
        let outside = scratch("paths-outside", &[("escaped.png", PNG)])?;
        let index = format!(
            "[Icon Theme]\nInherits=x/evil\nDirectories=apps,../outside,{abs}\n\
             [apps]\nSize=16\nType=Fixed\n\
             [../outside]\nSize=16\nType=Fixed\n\
             [{abs}]\nSize=16\nType=Fixed\n",
            abs = outside.display()
        );
        let evil = b"[Icon Theme]\nDirectories=apps\n[apps]\nSize=16\nType=Fixed\n";
        let dir = scratch(
            "paths",
            &[
                ("t/index.theme", index.as_bytes()),
                ("t/apps/ok.png", PNG),
                ("outside/escaped.png", PNG),
                ("x/evil/index.theme", evil),
                ("x/evil/apps/evil.png", PNG),
            ],
        )?;
        let theme = IconTheme::new(vec![dir.clone()], "t");
        assert_eq!(tail(theme.resolve("ok", 16, 1)), "t/apps/ok.png");
        assert_eq!(theme.resolve("escaped", 16, 1), None);
        assert_eq!(theme.resolve("evil", 16, 1), None);
        assert_eq!(theme.resolve("apps/ok", 16, 1), None);
        assert_eq!(theme.resolve("../t/apps/ok", 16, 1), None);
        assert_eq!(
            IconTheme::new(vec![dir], "x/evil").resolve("evil", 16, 1),
            None
        );
        Ok(())
    }

    /// A theme `t` of `index`, with an empty icon `icon` in each of `dirs`.
    fn theme_of(test: &str, index: &str, dirs: &[&str]) -> Result<IconTheme, Error> {
        let mut files = vec![("t/index.theme".to_owned(), index.as_bytes())];
        files.extend(dirs.iter().map(|d| (format!("t/{d}/icon.png"), PNG)));
        let files: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        Ok(IconTheme::new(vec![scratch(test, &files)?], "t"))
    }

    #[test]
    fn a_threshold_directory_serves_sizes_near_its_own() -> Result<(), Error> {
        let theme = theme_of(
            "threshold",
            "[Icon Theme]\nDirectories=38,32\n\
             [38]\nSize=38\nType=Fixed\n\
             [32]\nSize=32\nType=Threshold\nThreshold=4\n",
            &["38", "32"],
        )?;
        // 36 is within 32 plus 4, though 38 is closer.
        assert_eq!(tail(theme.resolve("icon", 36, 1)), "t/32/icon.png");
        assert_eq!(tail(theme.resolve("icon", 37, 1)), "t/38/icon.png");
        Ok(())
    }

    #[test]
    fn a_scale_of_2_takes_a_scaled_directory() -> Result<(), Error> {
        let theme = theme_of(
            "scaled",
            "[Icon Theme]\nDirectories=48\nScaledDirectories=48@2\n\
             [48]\nSize=48\nType=Fixed\n\
             [48@2]\nSize=48\nScale=2\nType=Fixed\n",
            &["48", "48@2"],
        )?;
        assert_eq!(tail(theme.resolve("icon", 48, 2)), "t/48@2/icon.png");
        assert_eq!(tail(theme.resolve("icon", 48, 1)), "t/48/icon.png");
        Ok(())
    }

    #[test]
    fn without_a_match_the_closest_directory_wins() -> Result<(), Error> {
        let theme = theme_of(
            "closest",
            "[Icon Theme]\nDirectories=48,16,24\n\
             [48]\nSize=48\nType=Fixed\n\
             [16]\nSize=16\nType=Fixed\n\
             [24]\nSize=24\nType=Fixed\n",
            &["48", "16", "24"],
        )?;
        assert_eq!(tail(theme.resolve("icon", 30, 1)), "t/24/icon.png");
        assert_eq!(tail(theme.resolve("icon", 40, 1)), "t/48/icon.png");
        Ok(())
    }

    #[test]
    fn an_inherits_cycle_ends() -> Result<(), Error> {
        let index = |parent: &str| {
            format!(
                "[Icon Theme]\nInherits={parent}\nDirectories=apps\n[apps]\nSize=16\nType=Fixed\n"
            )
        };
        let (a, b) = (index("b"), index("a"));
        let dir = scratch(
            "cycle",
            &[
                ("a/index.theme", a.as_bytes()),
                ("b/index.theme", b.as_bytes()),
                ("b/apps/icon.png", PNG),
            ],
        )?;
        let theme = IconTheme::new(vec![dir], "a");
        assert_eq!(theme.chain(), ["a", "b", "hicolor"]);
        assert_eq!(tail(theme.resolve("icon", 16, 1)), "b/apps/icon.png");
        assert_eq!(theme.resolve("missing", 16, 1), None);
        Ok(())
    }

    #[test]
    fn a_malformed_index_keeps_what_it_can() -> Result<(), Error> {
        let theme = theme_of(
            "malformed",
            "\u{feff}[Icon Theme]\nDirectories= apps , ,nogroup,bad\nno equals sign\n=\n\
             [apps]\nSize=16\nType=Fixed\n\
             [bad]\nSize=sixteen\n\
             [unterminated\nSize=1\n",
            &["apps", "nogroup", "bad"],
        )?;
        assert_eq!(tail(theme.resolve("icon", 16, 1)), "t/apps/icon.png");
        assert_eq!(tail(theme.resolve("icon", 300, 1)), "t/apps/icon.png");
        let garbage = theme_of("garbage", "\0\u{1}[[]]=\n[", &["apps"])?;
        assert_eq!(garbage.resolve("icon", 16, 1), None);
        Ok(())
    }

    #[test]
    fn an_icon_that_cannot_be_decoded_gets_the_fallback() -> Result<(), Error> {
        let index = "[Icon Theme]\nDirectories=apps\n[apps]\nSize=16\nType=Fixed\n";
        let fallback = format!("t/apps/{FALLBACK_ICON}.png");
        let dir = scratch(
            "undecodable",
            &[
                ("t/index.theme", index.as_bytes()),
                ("t/apps/broken.png", b"not a png"),
                ("t/apps/upper.PNG", PNG),
                ("t/apps/named.png", PNG),
                (&fallback, PNG),
                ("other.xpm", b"/* XPM */"),
            ],
        )?;
        let theme = IconTheme::new(vec![dir.clone()], "t");
        assert_eq!(tail(theme.resolve("broken", 16, 1)), "t/apps/broken.png");
        assert_eq!(theme.image("broken", 16, 1)?.width(), 16);
        let xpm = dir.join("other.xpm");
        assert!(Image::from_file(&xpm, 16).is_err());
        assert_eq!(theme.image(&xpm.to_string_lossy(), 16, 1)?.width(), 16);
        // An extension in any case, and an Icon= that carries one, name the file itself.
        assert_eq!(tail(theme.resolve("upper.PNG", 16, 1)), "t/apps/upper.PNG");
        assert_eq!(
            Image::from_file(&dir.join("t/apps/upper.PNG"), 16)?.width(),
            16
        );
        assert_eq!(tail(theme.resolve("named.png", 16, 1)), "t/apps/named.png");
        Ok(())
    }

    #[test]
    fn a_directory_without_a_size_is_skipped() -> Result<(), Error> {
        let theme = theme_of(
            "no-size",
            "[Icon Theme]\nDirectories=nosize,apps\n\
             [nosize]\nType=Scalable\nMinSize=1\nMaxSize=512\n\
             [apps]\nSize=16\nType=Fixed\n",
            &["nosize", "apps"],
        )?;
        assert_eq!(tail(theme.resolve("icon", 48, 1)), "t/apps/icon.png");
        Ok(())
    }

    #[test]
    fn at_most_256_directories_are_read() -> Result<(), Error> {
        let names: Vec<String> = (0..300).map(|n| format!("d{n}")).collect();
        let mut index = format!("[Icon Theme]\nDirectories={}\n", names.join(","));
        for name in &names {
            index.push_str(&format!("[{name}]\nSize=16\nType=Fixed\n"));
        }
        let dir = scratch(
            "many",
            &[
                ("t/index.theme", index.as_bytes()),
                ("t/d255/last.png", PNG),
                ("t/d256/beyond.png", PNG),
            ],
        )?;
        let theme = IconTheme::new(vec![dir], "t");
        assert_eq!(tail(theme.resolve("last", 16, 1)), "t/d255/last.png");
        assert_eq!(theme.resolve("beyond", 16, 1), None);
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

    #[test]
    fn an_image_node_is_named_or_hidden() {
        let image = Image {
            pixmap: Arc::new(Pixmap::new(4, 4)),
        };
        let mut ui = Ui::new("Image", Theme::default(), false);
        let named = ui.add(
            Ui::ROOT,
            Kind::Image {
                image: image.clone(),
                size: 16.0,
                description: "Files".into(),
            },
        );
        let decorative = ui.add(
            Ui::ROOT,
            Kind::Image {
                image,
                size: 16.0,
                description: String::new(),
            },
        );
        ui.layout();
        let node = crate::a11y::node(&ui, named, 1.0);
        assert_eq!(node.role(), accesskit::Role::Image);
        assert_eq!(node.label(), Some("Files"));
        assert!(!node.is_hidden());
        let node = crate::a11y::node(&ui, decorative, 1.0);
        assert!(node.is_hidden());
        assert_eq!(node.label(), None);
    }

    #[test]
    fn a_wide_image_keeps_its_aspect_ratio() -> Result<(), Error> {
        let red = [0xcc, 0, 0, 0xff].repeat(32 * 16);
        let metadata = PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, false);
        let image = Image {
            pixmap: Arc::new(Pixmap::from_parts(red, 32, 16, metadata)),
        };
        let mut ui = Ui::new("Image", Theme::default(), false);
        let id = ui.add(
            Ui::ROOT,
            Kind::Image {
                image,
                size: 32.0,
                description: String::new(),
            },
        );
        let pixmap = render::render(&mut ui, 1.0)?;
        let r = ui.widget(id).rect;
        let x = (r.x + r.w / 2.0) as u16;
        let at = |y: f32| {
            let p = pixmap.sample(x, (r.y + y) as u16);
            (p.r, p.g, p.b)
        };
        // 32 by 16, centred in 32 by 32: rows 8 to 24 are the image, the rest the background.
        assert_eq!(at(16.0), (0xcc, 0, 0));
        assert_ne!(at(2.0), (0xcc, 0, 0));
        assert_ne!(at(30.0), (0xcc, 0, 0));
        Ok(())
    }
}
