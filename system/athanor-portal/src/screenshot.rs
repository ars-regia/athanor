//! The Screenshot portal (doc_portal.md, PT8): which request may capture without a prompt, how
//! the outputs' pictures become one image in the layout's logical coordinates, the colour of a
//! pixel, and the files in flight beneath the unit's runtime directory, with the
//! `org.freedesktop.impl.portal.Screenshot` interface and its Request objects (PT4).
//!
//! The interactive overlay and the colour picker are drawn by the backend in Sal (PT2); until
//! they are built, an interactive `Screenshot` and every `PickColor` answer 2 with a journal
//! line, and the frontend's configuration routes the interface to no backend.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{self, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::Notify;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::ObjectServer;
use zbus::{ObjectPath, OwnedVariant as OwnedValue, Str};

/// The version of `org.freedesktop.impl.portal.Screenshot` served, xdg-desktop-portal 1.20.4's.
pub const VERSION: u32 = 2;
/// How long a file in flight is kept: the frontend and the recorder read it within that time.
pub const MAX_AGE: Duration = Duration::from_secs(3600);
const PREFIX: &str = "screenshot-";
const SUFFIX: &str = ".png";
/// The largest image composed: a side, and its RGBA bytes, so that a layout of absurd
/// coordinates is refused rather than allocated.
const MAX_SIDE: u32 = 16384;
const MAX_BYTES: u64 = 16384 * 8192 * 4;

/// What a `Screenshot` request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Captured at once, without a prompt.
    Silent,
    /// The overlay of PT8, where the user picks what is captured.
    Interactive,
}

/// The mode of a request: silent only once the frontend has checked the permission store and
/// recorded the grant (`permission_store_checked`), and never when `interactive` is set. A
/// request without the flag is interactive, so it never captures silently. `modal` is not
/// read: the overlay covers every output. An option of the wrong type refuses the request.
pub fn mode(options: &HashMap<String, OwnedValue>) -> Result<Mode, String> {
    let flag = |key: &str| match options.get(key).map(|value| value.downcast_ref::<bool>()) {
        None => Ok(false),
        Some(Ok(set)) => Ok(set),
        Some(Err(error)) => Err(format!("{key} is not a boolean: {error}")),
    };
    let checked = flag("permission_store_checked")?;
    let interactive = flag("interactive")?;
    Ok(if checked && !interactive {
        Mode::Silent
    } else {
        Mode::Interactive
    })
}

/// The layout of a picture's pixels, as wl_shm names them: the bytes of a little-endian 32-bit
/// word, so that [`Format::Argb8888`] holds blue, green, red and alpha in that order. The colour
/// is premultiplied by the alpha; an `X` format has no alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Argb8888,
    Xrgb8888,
    Abgr8888,
    Xbgr8888,
}

/// The picture of one output as the compositor copied it, and where the output lies in the
/// layout, in logical pixels.
#[derive(Debug, Clone)]
pub struct Shot {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The picture's `wl_output.transform`, 0 to 7: an output is copied as it is scanned out.
    pub transform: u32,
    /// `height` rows of `stride` bytes, of which the first `width` pixels are the picture.
    pub pixels_width: u32,
    pub pixels_height: u32,
    pub stride: u32,
    pub format: Format,
    pub bytes: Vec<u8>,
}

impl Shot {
    /// The pixel at column `x` and row `y` of the buffer, in straight RGBA.
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize)
            .saturating_mul(self.stride as usize)
            .saturating_add((x as usize).saturating_mul(4));
        let Some(&[b0, b1, b2, b3]) = self.bytes.get(at..at.saturating_add(4)) else {
            return [0; 4];
        };
        let (r, g, b, a) = match self.format {
            Format::Argb8888 => (b2, b1, b0, b3),
            Format::Xrgb8888 => (b2, b1, b0, 255),
            Format::Abgr8888 => (b0, b1, b2, b3),
            Format::Xbgr8888 => (b0, b1, b2, 255),
        };
        straight([r, g, b, a])
    }

    /// The buffer's size once its transform is undone: the output's size in buffer pixels.
    fn upright(&self) -> (u32, u32) {
        if self.transform & 1 == 1 {
            (self.pixels_height, self.pixels_width)
        } else {
            (self.pixels_width, self.pixels_height)
        }
    }

    /// The buffer pixel shown at column `u` and row `v` of the upright picture, as Weston maps a
    /// surface point through a buffer transform (`weston_transformed_coord`), `width` and
    /// `height` being the upright size.
    fn source(&self, u: u32, v: u32) -> (u32, u32) {
        let (width, height) = self.upright();
        let last_u = width.saturating_sub(1).saturating_sub(u);
        let last_v = height.saturating_sub(1).saturating_sub(v);
        match self.transform {
            1 => (v, last_u),
            2 => (last_u, last_v),
            3 => (last_v, u),
            4 => (last_u, v),
            5 => (v, u),
            6 => (u, last_v),
            7 => (last_v, last_u),
            _ => (u, v),
        }
    }
}

/// Undoes the premultiplication of a pixel's colour by its alpha.
fn straight([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    match a {
        0 => [0; 4],
        255 => [r, g, b, a],
        _ => {
            // `a` is 1 to 254 here, so the division always happens and the product never
            // overflows; the result is rounded to the nearest value.
            let alpha = u32::from(a);
            let undo = |c: u8| {
                u32::from(c)
                    .saturating_mul(255)
                    .saturating_add(alpha >> 1)
                    .checked_div(alpha)
                    .map_or(0, |c| c.min(255) as u8)
            };
            [undo(r), undo(g), undo(b), a]
        }
    }
}

/// An image of straight RGBA pixels, row after row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixel at column `x` and row `y`, transparent outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize)
            .saturating_mul(self.width as usize)
            .saturating_add(x as usize)
            .saturating_mul(4);
        match self.rgba.get(at..at.saturating_add(4)) {
            Some(&[r, g, b, a]) if x < self.width => [r, g, b, a],
            _ => [0; 4],
        }
    }
}

/// The outputs as one image, in the layout's logical coordinates at the highest output scale
/// (spike S6): the image covers the bounding box of the outputs, each output is scaled to its
/// place, and what no output covers is transparent. An output that overlaps another, as a
/// mirror does, is drawn over it in the order given.
// ponytail: nearest-neighbour scaling of the outputs below the highest scale, until spike S6
// measures whether a filtered scale reads better in a mixed-scale layout.
pub fn compose(shots: &[Shot]) -> Result<Image, String> {
    let mut scale = 0f64;
    let (mut left, mut top, mut right, mut bottom) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for shot in shots {
        if shot.transform > 7 {
            return Err(format!(
                "an output's transform {} is not one of wl_output's eight",
                shot.transform
            ));
        }
        let (width, height) = shot.upright();
        if shot.width <= 0 || shot.height <= 0 || width == 0 || height == 0 {
            return Err(format!(
                "an output of {}×{} logical pixels has no picture",
                shot.width, shot.height
            ));
        }
        if shot.bytes.len() < (shot.stride as usize).saturating_mul(shot.pixels_height as usize)
            || (shot.stride as usize) < (shot.pixels_width as usize).saturating_mul(4)
        {
            return Err("a picture is shorter than its size".to_owned());
        }
        scale = scale.max(f64::from(width) / f64::from(shot.width));
        left = left.min(i64::from(shot.x));
        top = top.min(i64::from(shot.y));
        right = right.max(i64::from(shot.x).saturating_add(i64::from(shot.width)));
        bottom = bottom.max(i64::from(shot.y).saturating_add(i64::from(shot.height)));
    }
    if shots.is_empty() {
        return Err("no output was captured".to_owned());
    }
    let side = |logical: i64| (logical as f64 * scale).round();
    let (width, height) = (
        side(right.saturating_sub(left)),
        side(bottom.saturating_sub(top)),
    );
    if width > f64::from(MAX_SIDE)
        || height > f64::from(MAX_SIDE)
        || width * height * 4.0 > MAX_BYTES as f64
    {
        return Err(format!(
            "a layout of {width}×{height} pixels is too large to compose"
        ));
    }
    let (width, height) = (width as u32, height as u32);
    // Bounded by MAX_BYTES above.
    let mut rgba = vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
    for shot in shots {
        let (upright_width, upright_height) = shot.upright();
        let place = |from: i32, to: i64, origin: i64, limit: u32| {
            let start =
                side(i64::from(from).saturating_sub(origin)).clamp(0.0, f64::from(limit)) as u32;
            let end = side(to.saturating_sub(origin)).clamp(0.0, f64::from(limit)) as u32;
            (start, end)
        };
        let (x0, x1) = place(
            shot.x,
            i64::from(shot.x).saturating_add(i64::from(shot.width)),
            left,
            width,
        );
        let (y0, y1) = place(
            shot.y,
            i64::from(shot.y).saturating_add(i64::from(shot.height)),
            top,
            height,
        );
        let along = |at: u32, start: u32, end: u32, size: u32| {
            let cells = f64::from(end.saturating_sub(start));
            let offset = f64::from(at.saturating_sub(start)) + 0.5;
            ((offset * f64::from(size) / cells) as u32).min(size.saturating_sub(1))
        };
        for y in y0..y1 {
            let v = along(y, y0, y1, upright_height);
            for x in x0..x1 {
                let u = along(x, x0, x1, upright_width);
                let (bx, by) = shot.source(u, v);
                let at = (y as usize)
                    .saturating_mul(width as usize)
                    .saturating_add(x as usize)
                    .saturating_mul(4);
                if let Some(cell) = rgba.get_mut(at..at.saturating_add(4)) {
                    cell.copy_from_slice(&shot.pixel(bx, by));
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// A pixel as the portal's `(ddd)`: sRGB, each component from 0 to 1.
pub fn color([r, g, b, _]: [u8; 4]) -> (f64, f64, f64) {
    let unit = |c: u8| f64::from(c) / 255.0;
    (unit(r), unit(g), unit(b))
}

/// A pixel as `#rrggbb`, the text PickColor copies to the clipboard.
pub fn hex([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The image as a PNG.
pub fn png(image: &Image, out: impl Write) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.rgba)?;
    writer.finish()
}

/// Writes the image as a new PNG beneath `dir`, the unit's runtime directory, readable by the
/// user alone, and returns its path. The name is new each time: the file is created, never
/// opened, so a file already there is never written through.
pub fn write(dir: &Path, image: &Image, now: SystemTime) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!(
        "{PREFIX}{}-{}-{n}{SUFFIX}",
        since.as_secs(),
        since.subsec_nanos()
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let mut out = BufWriter::new(file);
    let written = png(image, &mut out)
        .map_err(io::Error::other)
        .and_then(|()| out.flush());
    if let Err(error) = written {
        // The half-written file is not answered; its removal is best effort, and the sweep
        // removes it within the hour if this fails.
        if let Err(removal) = fs::remove_file(&path) {
            tracing::warn!(%removal, path = %path.display(), "a half-written screenshot stays until the sweep");
        }
        return Err(error);
    }
    Ok(path)
}

/// Removes the files in flight beneath `dir` older than [`MAX_AGE`] at `now`: the regular files
/// this module names, never a link, a directory or another name. Returns how many it removed;
/// a file it cannot read or remove is logged and left.
pub fn sweep(dir: &Path, now: SystemTime) -> usize {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, dir = %dir.display(), "the screenshots in flight were not swept");
            return 0;
        }
    };
    let mut removed = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, dir = %dir.display(), "an entry in flight was not read");
                continue;
            }
        };
        let name = entry.file_name();
        let name = name.as_bytes();
        if !name.starts_with(PREFIX.as_bytes()) || !name.ends_with(SUFFIX.as_bytes()) {
            continue;
        }
        let path = entry.path();
        let old = entry.metadata().is_ok_and(|meta| {
            meta.file_type().is_file()
                && meta.modified().is_ok_and(|modified| {
                    now.duration_since(modified).is_ok_and(|age| age > MAX_AGE)
                })
        });
        if !old {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed = removed.saturating_add(1),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "a screenshot in flight was not removed")
            }
        }
    }
    removed
}

/// `path` as a `file://` URI, every byte outside RFC 3986's unreserved set and `/`
/// percent-encoded.
pub fn uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// What the backend gives the interface: the frontend check of PT3 and the outputs' pictures.
pub trait Host: Send + Sync + 'static {
    /// `Ok` when the caller is the frontend, the owner of `org.freedesktop.portal.Desktop`.
    fn frontend(&self, header: &Header<'_>) -> impl Future<Output = Result<(), String>> + Send;
    /// Every output's picture, without the pointer.
    fn capture(&self) -> impl Future<Output = Result<Vec<Shot>, String>> + Send;
}

struct Shared<H: Host> {
    host: H,
    /// The unit's runtime directory, where the files in flight are written.
    dir: PathBuf,
}

impl<H: Host> Shared<H> {
    async fn frontend(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.host.frontend(header).await.map_err(|why| {
            tracing::warn!(%why, "a call that is not the frontend's is refused");
            fdo::Error::AccessDenied(why)
        })
    }
}

type Results = HashMap<String, OwnedValue>;

fn answer(response: u32) -> (u32, Results) {
    (response, Results::new())
}

/// `org.freedesktop.impl.portal.Screenshot`, served at the portal's object.
pub struct Screenshot<H: Host>(Arc<Shared<H>>);

impl<H: Host> Screenshot<H> {
    pub fn new(host: H, dir: PathBuf) -> Screenshot<H> {
        Screenshot(Arc::new(Shared { host, dir }))
    }

    /// Composes, sweeps and writes, off the runtime's thread: a layout of several outputs is
    /// tens of megabytes to scale and encode.
    async fn keep(&self, shots: Vec<Shot>) -> Result<PathBuf, String> {
        let dir = self.0.dir.clone();
        tokio::task::spawn_blocking(move || {
            let now = SystemTime::now();
            sweep(&dir, now);
            let image = compose(&shots)?;
            write(&dir, &image, now)
                .map_err(|error| format!("the screenshot was not written: {error}"))
        })
        .await
        .map_err(|error| format!("the screenshot's task ended: {error}"))?
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl<H: Host> Screenshot<H> {
    async fn screenshot(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        handle: ObjectPath<'_>,
        app_id: String,
        _parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, Results)> {
        self.0.frontend(&header).await?;
        match mode(&options) {
            Ok(Mode::Silent) => {}
            Ok(Mode::Interactive) => {
                tracing::warn!(%app_id, "an interactive Screenshot is refused: the overlay of PT8 is not built yet");
                return Ok(answer(2));
            }
            Err(why) => {
                tracing::warn!(%app_id, %why, "Screenshot refused");
                return Ok(answer(2));
            }
        }
        let closed = Arc::new(Notify::new());
        let request = Request {
            shared: self.0.clone(),
            closed: closed.clone(),
        };
        if !matches!(server.at(&handle, request).await, Ok(None)) {
            tracing::warn!(%handle, "Screenshot refused: the request's object path is taken");
            return Ok(answer(2));
        }
        let taken = tokio::select! {
            biased;
            () = closed.notified() => None,
            shots = self.0.host.capture() => Some(shots),
        };
        if let Err(error) = server.remove::<Request<H>, _>(&handle).await {
            tracing::error!(%error, %handle, "the request's object was not removed");
        }
        let Some(shots) = taken else {
            return Ok(answer(1));
        };
        let kept = match shots {
            Ok(shots) => self.keep(shots).await,
            Err(why) => Err(format!("the outputs were not captured: {why}")),
        };
        match kept {
            Ok(path) => {
                tracing::info!(%app_id, "a screenshot was taken without a prompt");
                let mut results = Results::new();
                results.insert("uri".into(), OwnedValue::from(Str::from(uri(&path))));
                Ok((0, results))
            }
            Err(why) => {
                tracing::error!(%app_id, %why, "Screenshot failed");
                Ok(answer(2))
            }
        }
    }

    async fn pick_color(
        &self,
        #[zbus(header)] header: Header<'_>,
        _handle: ObjectPath<'_>,
        app_id: String,
        _parent_window: String,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, Results)> {
        self.0.frontend(&header).await?;
        tracing::warn!(%app_id, "PickColor is refused: the colour picker of PT8 is not built yet");
        Ok(answer(2))
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        VERSION
    }
}

/// `org.freedesktop.impl.portal.Request` on a `Screenshot`'s handle until it answers.
pub struct Request<H: Host> {
    shared: Arc<Shared<H>>,
    closed: Arc<Notify>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl<H: Host> Request<H> {
    /// Ends the request: `Screenshot` answers 1 and writes nothing.
    async fn close(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.shared.frontend(&header).await?;
        self.closed.notify_one();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, MetadataExt};
    use std::sync::Mutex;

    use zbus::Connection;

    use super::*;
    use crate::testing::{members, Bus};

    const DESKTOP: &str = "/org/freedesktop/portal/desktop";
    const SCREENSHOT: &str = "org.freedesktop.impl.portal.Screenshot";

    fn options(pairs: &[(&str, OwnedValue)]) -> HashMap<String, OwnedValue> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.try_clone().expect("clone")))
            .collect()
    }

    #[test]
    fn only_a_checked_request_that_is_not_interactive_captures_silently() {
        let yes = || OwnedValue::from(true);
        let no = || OwnedValue::from(false);
        let checked = ("permission_store_checked", yes());
        assert_eq!(mode(&options(&[checked.clone_pair()])), Ok(Mode::Silent));
        assert_eq!(
            mode(&options(&[checked.clone_pair(), ("interactive", no())])),
            Ok(Mode::Silent)
        );
        assert_eq!(
            mode(&options(&[checked.clone_pair(), ("interactive", yes())])),
            Ok(Mode::Interactive)
        );
        // Without the flag, or with it false, a request is interactive: never a silent capture.
        assert_eq!(mode(&options(&[])), Ok(Mode::Interactive));
        assert_eq!(
            mode(&options(&[("interactive", no())])),
            Ok(Mode::Interactive)
        );
        assert_eq!(
            mode(&options(&[("permission_store_checked", no())])),
            Ok(Mode::Interactive)
        );
        assert_eq!(mode(&options(&[("modal", no())])), Ok(Mode::Interactive));
        // A flag of the wrong type refuses the request rather than guessing.
        assert!(mode(&options(&[(
            "permission_store_checked",
            OwnedValue::from(1u32)
        )]))
        .is_err());
        assert!(mode(&options(&[
            checked.clone_pair(),
            ("interactive", OwnedValue::from(Str::from("no")))
        ]))
        .is_err());
    }

    trait ClonePair {
        fn clone_pair(&self) -> (&'static str, OwnedValue);
    }

    impl ClonePair for (&'static str, OwnedValue) {
        fn clone_pair(&self) -> (&'static str, OwnedValue) {
            (self.0, self.1.try_clone().expect("clone"))
        }
    }

    /// A picture whose every pixel is `pixel(x, y)` in `format`'s byte order.
    fn picture(
        width: u32,
        height: u32,
        format: Format,
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> (u32, Vec<u8>) {
        let stride = width * 4 + 8;
        let mut bytes = vec![0xee; (stride * height) as usize];
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, a] = pixel(x, y);
                let word = match format {
                    Format::Argb8888 | Format::Xrgb8888 => [b, g, r, a],
                    Format::Abgr8888 | Format::Xbgr8888 => [r, g, b, a],
                };
                let at = (y * stride + x * 4) as usize;
                bytes[at..at + 4].copy_from_slice(&word);
            }
        }
        (stride, bytes)
    }

    fn shot(
        x: i32,
        y: i32,
        logical: (i32, i32),
        size: (u32, u32),
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> Shot {
        let (stride, bytes) = picture(size.0, size.1, Format::Xrgb8888, pixel);
        Shot {
            x,
            y,
            width: logical.0,
            height: logical.1,
            transform: 0,
            pixels_width: size.0,
            pixels_height: size.1,
            stride,
            format: Format::Xrgb8888,
            bytes,
        }
    }

    #[test]
    fn every_shm_format_reads_as_straight_rgba() {
        let colour = [200, 100, 50, 255];
        for format in [
            Format::Argb8888,
            Format::Xrgb8888,
            Format::Abgr8888,
            Format::Xbgr8888,
        ] {
            let (stride, bytes) = picture(1, 1, format, |_, _| colour);
            let one = Shot {
                format,
                stride,
                bytes,
                ..shot(0, 0, (1, 1), (1, 1), |_, _| [0; 4])
            };
            assert_eq!(one.pixel(0, 0), colour, "{format:?}");
        }
        // An X format's fourth byte means nothing: the pixel is opaque.
        let (stride, bytes) = picture(1, 1, Format::Xbgr8888, |_, _| [1, 2, 3, 0]);
        let opaque = Shot {
            format: Format::Xbgr8888,
            stride,
            bytes,
            ..shot(0, 0, (1, 1), (1, 1), |_, _| [0; 4])
        };
        assert_eq!(opaque.pixel(0, 0), [1, 2, 3, 255]);
        // An alpha format's colour is premultiplied, and the image's is straight.
        let (stride, bytes) = picture(1, 1, Format::Argb8888, |_, _| [64, 32, 0, 128]);
        let half = Shot {
            format: Format::Argb8888,
            stride,
            bytes,
            ..shot(0, 0, (1, 1), (1, 1), |_, _| [0; 4])
        };
        assert_eq!(half.pixel(0, 0), [128, 64, 0, 128]);
        assert_eq!(straight([10, 20, 30, 0]), [0; 4]);
    }

    #[test]
    fn outputs_compose_in_logical_coordinates_at_the_highest_scale() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        // A 2×1 logical output at scale 2, and to its right, one row lower, a 1×1 output at
        // scale 1: the image is 3×2 logical pixels at scale 2.
        let hidpi = shot(0, 0, (2, 1), (4, 2), |_, _| red);
        let lodpi = shot(2, 1, (1, 1), (1, 1), |_, _| blue);
        let image = compose(&[hidpi, lodpi]).expect("compose");
        assert_eq!((image.width, image.height), (6, 4));
        for (x, y) in [(0, 0), (3, 1)] {
            assert_eq!(image.pixel(x, y), red, "({x}, {y})");
        }
        // The scale-1 output is scaled up to 2×2 image pixels.
        for (x, y) in [(4, 2), (5, 2), (4, 3), (5, 3)] {
            assert_eq!(image.pixel(x, y), blue, "({x}, {y})");
        }
        // What no output covers is transparent.
        assert_eq!(image.pixel(0, 3), [0; 4]);
        assert_eq!(image.pixel(5, 0), [0; 4]);
    }

    #[test]
    fn a_transform_that_is_not_one_of_the_eight_is_refused() {
        let mut odd = shot(0, 0, (2, 1), (2, 1), |_, _| [255, 0, 0, 255]);
        odd.transform = 9;
        assert!(compose(&[odd]).is_err());
    }

    #[test]
    fn negative_coordinates_and_a_fractional_scale_place_each_pixel() {
        // An output at -2 of 2×2 logical pixels at scale 1.5 (3×3 buffer pixels), and one at 0
        // of 2×2 at scale 1, scaled up by 1.5 to 3×3 image pixels.
        let left = shot(-2, 0, (2, 2), (3, 3), |x, y| [x as u8, y as u8, 9, 255]);
        let right = shot(0, 0, (2, 2), (2, 2), |x, y| {
            [100 + x as u8, 100 + y as u8, 7, 255]
        });
        let image = compose(&[left, right]).expect("compose");
        assert_eq!((image.width, image.height), (6, 3));
        assert_eq!(image.pixel(0, 0), [0, 0, 9, 255]);
        assert_eq!(image.pixel(2, 2), [2, 2, 9, 255]);
        assert_eq!(image.pixel(3, 0), [100, 100, 7, 255]);
        assert_eq!(image.pixel(5, 2), [101, 101, 7, 255]);
    }

    #[test]
    fn a_turned_output_is_drawn_upright() {
        // An upright picture of 2×1 pixels, left red and right blue, and the buffer each
        // transform scans out for it, by Weston's mapping.
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let upright = |u: u32, _v: u32| if u == 0 { red } else { blue };
        for transform in 0..8u32 {
            let turned = transform & 1 == 1;
            let size = if turned { (1, 2) } else { (2, 1) };
            let mut one = shot(0, 0, (2, 1), size, |_, _| [0; 4]);
            one.transform = transform;
            let (stride, mut bytes) = picture(size.0, size.1, Format::Xrgb8888, |_, _| [0; 4]);
            for u in 0..2 {
                let (bx, by) = one.source(u, 0);
                let [r, g, b, a] = upright(u, 0);
                let at = (by * stride + bx * 4) as usize;
                bytes[at..at + 4].copy_from_slice(&[b, g, r, a]);
            }
            one.stride = stride;
            one.bytes = bytes;
            let image = compose(&[one]).expect("compose");
            assert_eq!((image.width, image.height), (2, 1), "transform {transform}");
            assert_eq!(
                [image.pixel(0, 0), image.pixel(1, 0)],
                [red, blue],
                "transform {transform}"
            );
        }
        // Spot checks against Weston's table for a 2×1 upright size: 90 reads (v, w-1-u).
        let mut ninety = shot(0, 0, (2, 1), (1, 2), |_, _| [0; 4]);
        ninety.transform = 1;
        assert_eq!(ninety.source(0, 0), (0, 1));
        assert_eq!(ninety.source(1, 0), (0, 0));
        ninety.transform = 3;
        assert_eq!(ninety.source(0, 0), (0, 0));
        assert_eq!(ninety.source(1, 0), (0, 1));
    }

    #[test]
    fn a_layout_that_cannot_be_composed_is_refused() {
        assert!(compose(&[]).is_err());
        assert!(compose(&[shot(0, 0, (0, 1), (1, 1), |_, _| [0; 4])]).is_err());
        let mut short = shot(0, 0, (1, 1), (1, 1), |_, _| [0; 4]);
        short.bytes.truncate(2);
        assert!(compose(&[short]).is_err());
        let far = [
            shot(0, 0, (1, 1), (1, 1), |_, _| [0; 4]),
            shot(100_000, 0, (1, 1), (1, 1), |_, _| [0; 4]),
        ];
        assert!(compose(&far).is_err());
    }

    #[test]
    fn a_pixel_answers_its_colour_and_its_hex() {
        assert_eq!(color([255, 0, 51, 255]), (1.0, 0.0, 0.2));
        assert_eq!(hex([255, 0, 51, 255]), "#ff0033");
        assert_eq!(hex([1, 2, 3, 0]), "#010203");
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("athanor-portal-shot-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn a_screenshot_is_a_new_private_png_answered_as_a_file_uri() {
        let dir = dir("write");
        let image = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 128],
        };
        let now = SystemTime::now();
        let first = write(&dir, &image, now).expect("write");
        let second = write(&dir, &image, now).expect("write");
        assert_ne!(first, second, "each screenshot has a new name");
        assert_eq!(fs::metadata(&first).expect("stat").mode() & 0o777, 0o600);
        let decoder = png::Decoder::new(io::BufReader::new(fs::File::open(&first).expect("open")));
        let mut reader = decoder.read_info().expect("a PNG");
        let mut decoded = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut decoded).expect("a frame");
        assert_eq!(
            (info.width, info.height, info.color_type),
            (2, 1, png::ColorType::Rgba)
        );
        assert_eq!(&decoded[..info.buffer_size()], &image.rgba[..]);
        assert!(uri(&first).starts_with("file:///"), "{}", uri(&first));
        assert_eq!(
            uri(Path::new("/run/user/1000/a b%é.png")),
            "file:///run/user/1000/a%20b%25%C3%A9.png"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_sweep_removes_only_screenshots_older_than_an_hour() {
        let dir = dir("sweep");
        let image = Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        let old = write(&dir, &image, SystemTime::now()).expect("write");
        let other = dir.join("notes.png");
        fs::write(&other, b"kept").expect("write");
        let target = dir.join("target");
        fs::write(&target, b"kept").expect("write");
        let link = dir.join("screenshot-link.png");
        symlink(&target, &link).expect("symlink");
        let hour_later = SystemTime::now() + MAX_AGE + Duration::from_secs(1);
        // At once, nothing is old enough.
        assert_eq!(sweep(&dir, SystemTime::now()), 0);
        assert!(old.exists());
        // An hour later the screenshot goes; the other name, the link and its target stay.
        assert_eq!(sweep(&dir, hour_later), 1);
        assert!(!old.exists());
        assert!(other.exists() && target.exists() && fs::symlink_metadata(&link).is_ok());
        assert_eq!(sweep(&dir.join("absent"), hour_later), 0);
        fs::remove_dir_all(&dir).ok();
    }

    /// The frontend is one connection of the test, admitted by its unique name. The capture
    /// gives one red pixel, or never answers.
    struct TestHost {
        frontend: String,
        answers: bool,
        captures: Arc<Mutex<u32>>,
    }

    impl Host for TestHost {
        async fn frontend(&self, header: &Header<'_>) -> Result<(), String> {
            match header.sender() {
                Some(sender) if sender.as_str() == self.frontend => Ok(()),
                sender => Err(format!("{sender:?} is not the frontend")),
            }
        }

        async fn capture(&self) -> Result<Vec<Shot>, String> {
            *self.captures.lock().expect("lock") += 1;
            if !self.answers {
                std::future::pending::<()>().await;
            }
            Ok(vec![shot(0, 0, (1, 1), (1, 1), |_, _| [255, 0, 0, 255])])
        }
    }

    struct Portal {
        _bus: Bus,
        _serving: Connection,
        backend: String,
        frontend: Connection,
        stranger: Connection,
        captures: Arc<Mutex<u32>>,
        dir: PathBuf,
    }

    async fn portal(name: &str, answers: bool) -> Portal {
        let bus = Bus::start(name);
        let frontend = bus.builder().build().await.expect("frontend");
        let stranger = bus.builder().build().await.expect("stranger");
        let captures = Arc::new(Mutex::new(0));
        let dir = dir(name);
        let host = TestHost {
            frontend: frontend.unique_name().expect("name").to_string(),
            answers,
            captures: captures.clone(),
        };
        let serving = bus
            .builder()
            .serve_at(DESKTOP, Screenshot::new(host, dir.clone()))
            .build()
            .await
            .expect("backend");
        Portal {
            backend: serving.unique_name().expect("name").to_string(),
            _serving: serving,
            _bus: bus,
            frontend,
            stranger,
            captures,
            dir,
        }
    }

    impl Portal {
        async fn call(
            &self,
            from: &Connection,
            member: &str,
            handle: &str,
            options: &HashMap<String, OwnedValue>,
        ) -> zbus::Result<(u32, Results)> {
            let body = (
                ObjectPath::try_from(handle).expect("path"),
                "org.example.App",
                "",
                options,
            );
            from.call_method(
                Some(self.backend.as_str()),
                DESKTOP,
                Some(SCREENSHOT),
                member,
                &body,
            )
            .await?
            .body()
            .deserialize()
        }
    }

    fn checked() -> HashMap<String, OwnedValue> {
        options(&[("permission_store_checked", OwnedValue::from(true))])
    }

    #[tokio::test]
    async fn a_checked_request_of_the_frontend_answers_the_uri_of_a_png() {
        let portal = portal("shot", true).await;
        let (response, results) = portal
            .call(
                &portal.frontend,
                "Screenshot",
                "/org/freedesktop/portal/desktop/request/1_1/a",
                &checked(),
            )
            .await
            .expect("Screenshot");
        assert_eq!(response, 0);
        let uri: String = results["uri"]
            .try_clone()
            .expect("clone")
            .try_into()
            .expect("a string");
        let path = uri.strip_prefix("file://").expect("a file URI");
        assert!(Path::new(path).starts_with(&portal.dir), "{uri}");
        assert!(fs::read(path).expect("the PNG").starts_with(b"\x89PNG"));
        fs::remove_dir_all(&portal.dir).ok();
    }

    #[tokio::test]
    async fn no_request_captures_without_the_frontend_or_without_the_check() {
        let portal = portal("refused", true).await;
        let request = "/org/freedesktop/portal/desktop/request/1_1/b";
        let refused = portal
            .call(&portal.stranger, "Screenshot", request, &checked())
            .await;
        assert!(
            matches!(&refused, Err(zbus::Error::FDO(error)) if matches!(**error, fdo::Error::AccessDenied(_))),
            "{refused:?}"
        );
        let refused = portal
            .call(&portal.stranger, "PickColor", request, &HashMap::new())
            .await;
        assert!(refused.is_err(), "{refused:?}");
        // The frontend without the permission store's check, or asking for the overlay, is
        // answered 2 until the overlay is built, and nothing is captured.
        let interactive = options(&[
            ("permission_store_checked", OwnedValue::from(true)),
            ("interactive", OwnedValue::from(true)),
        ]);
        for asked in [HashMap::new(), interactive] {
            let (response, results) = portal
                .call(&portal.frontend, "Screenshot", request, &asked)
                .await
                .expect("Screenshot");
            assert_eq!((response, results.len()), (2, 0));
        }
        let (response, _) = portal
            .call(&portal.frontend, "PickColor", request, &HashMap::new())
            .await
            .expect("PickColor");
        assert_eq!(response, 2);
        assert_eq!(*portal.captures.lock().expect("lock"), 0);
        assert_eq!(fs::read_dir(&portal.dir).expect("dir").count(), 0);
        fs::remove_dir_all(&portal.dir).ok();
    }

    #[tokio::test]
    async fn close_ends_a_capture_in_flight_with_1_and_only_the_frontend_may_close() {
        let portal = Arc::new(portal("close", false).await);
        let handle = "/org/freedesktop/portal/desktop/request/1_1/c";
        let asking = {
            let portal = portal.clone();
            tokio::spawn(async move {
                portal
                    .call(&portal.frontend, "Screenshot", handle, &checked())
                    .await
            })
        };
        let request = "org.freedesktop.impl.portal.Request";
        let close = |from: Connection| {
            let backend = portal.backend.clone();
            async move {
                from.call_method(Some(backend.as_str()), handle, Some(request), "Close", &())
                    .await
            }
        };
        // The Request object exists while the capture is in flight.
        let mut tries = 0;
        let refused = loop {
            match close(portal.stranger.clone()).await {
                Err(zbus::Error::FDO(error))
                    if matches!(*error, fdo::Error::UnknownObject(_)) && tries < 100 =>
                {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                other => break other,
            }
        };
        assert!(
            matches!(&refused, Err(zbus::Error::FDO(error)) if matches!(**error, fdo::Error::AccessDenied(_))),
            "{refused:?}"
        );
        close(portal.frontend.clone()).await.expect("Close");
        let (response, results) = asking.await.expect("task").expect("Screenshot");
        assert_eq!((response, results.len()), (1, 0));
        assert_eq!(fs::read_dir(&portal.dir).expect("dir").count(), 0);
        fs::remove_dir_all(&portal.dir).ok();
    }

    /// The interface is xdg-desktop-portal's own definition, vendored unmodified from 1.20.4:
    /// every method and the version property, with their types.
    #[tokio::test]
    async fn the_interface_matches_the_vendored_definition() {
        let portal = portal("xml", true).await;
        let xml: String = portal
            .frontend
            .call_method(
                Some(portal.backend.as_str()),
                DESKTOP,
                Some("org.freedesktop.DBus.Introspectable"),
                "Introspect",
                &(),
            )
            .await
            .expect("Introspect")
            .body()
            .deserialize()
            .expect("xml");
        let start = xml
            .find(&format!("<interface name=\"{SCREENSHOT}\">"))
            .expect("the interface");
        let end = start + xml[start..].find("</interface>").expect("its end");
        let ours = members(&xml[start..end]);
        let upstream = members(include_str!(
            "../interfaces/org.freedesktop.impl.portal.Screenshot.xml"
        ));
        assert_eq!(ours, upstream);
        assert_eq!(ours.len(), 3, "Screenshot, PickColor and version");
        fs::remove_dir_all(&portal.dir).ok();
    }
}
