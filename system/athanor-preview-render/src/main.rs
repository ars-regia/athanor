//! athanor-preview-render: one untrusted image or PDF on standard input, one bitmap in the
//! ATPV1 format on standard output (doc_launcher.md, LA6, LA9). The launcher starts it in a
//! transient unit with no network, no home, no runtime directory and no bus; inside it,
//! glycin runs its loaders in bubblewrap. Usage: `athanor-preview-render image|pdf <side>`.
//!
//! `athanor-preview-render wallpaper` is the decoder of `athanor-wallpaper`
//! (doc_session_daemons.md SD17, maintainer decision, 2026-11-07), started by its socket unit
//! for each connection, the connection on standard input and output. It admits only the
//! wallpaper daemon as its peer, receives the image file as a descriptor with the sizes of a
//! request (`athanor_wallpaper::protocol`), and answers one frame of straight RGBA per size,
//! scaled with zoom.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use athanor_wallpaper::protocol::{self, REQUEST_LEN};

mod scm;

const MAGIC: &[u8; 5] = b"ATPV1";
/// Larger files are not previewed.
const MAX_INPUT: u64 = 64 << 20;
/// The longest side of a bitmap, whatever the caller asks.
const MAX_SIDE: u32 = 1024;
/// The preview's largest decode: 6144 × 6144 × 4 bytes = 144 MiB. Peak memory of an image in the
/// unit (MemoryMax 512 MiB): the input, at most 64 MiB; the loader's frame, at most 144 MiB;
/// that frame's copy inside the loader before it is handed over, at most 144 MiB; the
/// output, at most 4 MiB (the picture is averaged straight from the frame). Total at most
/// 356 MiB, plus the runtime. 8192² would have been 256 MiB twice, with the old full copy
/// a third time.
const MAX_DECODE: u32 = 6144;
/// How long the preview's loader may run.
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(3);
/// The wallpaper's longest side, for an 8K output (maintainer decision, 2026-11-07). The bound is
/// on the side, not the area: the decoder's unit (`MemoryMax=1G`) was measured at 651M for
/// 12000 × 8000 (96 MP), and past about 150 MP a decode may be killed for memory, which the
/// daemon reads as a fault and answers with the hearth. The launcher's 512M has room for neither.
const WALLPAPER_MAX_DECODE: u32 = 16384;
/// How long the wallpaper's loader may run: a 100 MP picture takes about 3 s on the reference
/// laptop (maintainer decision, 2026-11-07).
const WALLPAPER_TIMEOUT: Duration = Duration::from_secs(15);

/// What a caller allows a loader: the longest side it decodes and how long it may run.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    side: u32,
    timeout: Duration,
}

/// The launcher's previews and the wallpaper's decoder run in units of different sizes, so
/// each has its own bounds.
const PREVIEW: Bounds = Bounds { side: MAX_DECODE, timeout: PREVIEW_TIMEOUT };
const WALLPAPER: Bounds = Bounds { side: WALLPAPER_MAX_DECODE, timeout: WALLPAPER_TIMEOUT };

struct Picture {
    width: u32,
    height: u32,
    /// Premultiplied RGBA, rows packed.
    rgba: Vec<u8>,
}

fn main() -> ExitCode {
    if std::env::args().skip(1).eq(["wallpaper"]) {
        return match wallpaper() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        };
    }
    let mut args = std::env::args().skip(1);
    let (Some(kind), Some(side)) = (args.next(), args.next().and_then(|side| side.parse::<u32>().ok())) else {
        eprintln!("usage: athanor-preview-render image|pdf <side> | wallpaper");
        return ExitCode::from(2);
    };
    let side = side.clamp(1, MAX_SIDE);
    let mut input = Vec::new();
    if let Err(err) = io::stdin().take(MAX_INPUT + 1).read_to_end(&mut input) {
        eprintln!("the file cannot be read: {err}");
        return ExitCode::FAILURE;
    }
    if input.len() as u64 > MAX_INPUT {
        eprintln!("the file is larger than {MAX_INPUT} bytes");
        return ExitCode::FAILURE;
    }
    let picture = match kind.as_str() {
        "image" => image(input, side),
        "pdf" => pdf(input, side),
        other => Err(format!("unknown kind {other}")),
    };
    let written = picture.and_then(|picture| {
        write(&mut io::stdout().lock(), &picture).map_err(|err| err.to_string())
    });
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn image(input: Vec<u8>, side: u32) -> Result<Picture, String> {
    let frame = decode(input, glycin::MemoryFormat::R8g8b8a8Premultiplied, PREVIEW).map_err(|err| err.to_string())?;
    packed(frame.width(), frame.height(), frame.stride() as usize, frame.buf_slice(), side, |px| px)
}

/// Why a picture has no frame.
#[derive(Debug)]
enum Undecoded {
    /// The picture's own: it is not a regular file, is too large, or is in a format no loader
    /// reads. Asking again gives the same.
    Picture(String),
    /// Anything else: a loader that did not start, was killed or ran past its time. Asking
    /// again may succeed. glycin 4 names no other kind of error to the caller, so a picture
    /// a loader finds corrupt is here too, and is asked for again.
    Fault(String),
}

impl std::fmt::Display for Undecoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Picture(why) | Self::Fault(why) => f.write_str(why),
        }
    }
}

impl From<glycin::Error> for Undecoded {
    fn from(err: glycin::Error) -> Self {
        if err.unsupported_format().is_some() {
            Self::Picture(err.to_string())
        } else {
            Self::Fault(err.to_string())
        }
    }
}

/// The first frame of an image, decoded by glycin's loaders in bubblewrap, in `format`.
fn decode(input: Vec<u8>, format: glycin::MemoryFormat, bounds: Bounds) -> Result<glycin::Frame, Undecoded> {
    let selection = match format {
        glycin::MemoryFormat::R8g8b8a8 => glycin::MemoryFormatSelection::R8g8b8a8,
        _ => glycin::MemoryFormatSelection::R8g8b8a8Premultiplied,
    };
    glib::MainContext::new().block_on(async move {
        let mut loader = glycin::Loader::new_vec(input);
        loader
            .sandbox_selector(glycin::SandboxSelector::Bwrap)
            .accepted_memory_formats(selection)
            .limits(glycin::Limits::default().timeout(bounds.timeout).max_dimensions((bounds.side, bounds.side)));
        let mut image = loader.load().await?;
        // glycin's own check of the limit tells it from no other error: this one does.
        let details = image.details();
        if details.width() > bounds.side || details.height() > bounds.side {
            return Err(Undecoded::Picture(format!("a side is over {} pixels", bounds.side)));
        }
        let frame = image.next_frame().await?;
        if frame.memory_format() != format {
            return Err(Undecoded::Fault("the loader returned another memory format".to_owned()));
        }
        Ok(frame)
    })
}

/// One connection of the wallpaper's decoder: the peer admitted, the request and its file
/// received, one frame written per size. Any failure ends the connection with no frame or
/// with fewer frames than asked, which the daemon reads as a failed decode.
fn wallpaper() -> Result<(), String> {
    let connection = io::stdin().as_fd().try_clone_to_owned().map_err(|err| format!("the connection cannot be duplicated: {err}"))?;
    athanor_unit::peer::admit_user_unit(connection.as_fd(), protocol::DAEMON_SLICE, protocol::DAEMON_UNIT, Path::new(protocol::DAEMON_EXE))
        .map_err(|refused| format!("peer refused: {refused}"))?;
    let (sizes, file) = request(connection)?;
    let mut out = BufWriter::with_capacity(1 << 20, io::stdout().lock());
    let frame = match picture(File::from(file)) {
        Ok(frame) => frame,
        Err(Undecoded::Picture(err)) => {
            // The picture's fault, not the decoder's: told so, the daemon does not ask again.
            out.write_all(&protocol::refusal()).and_then(|()| out.flush()).map_err(|err| err.to_string())?;
            return Err(err);
        }
        // No frame: the daemon sees a fault and asks again.
        Err(Undecoded::Fault(err)) => return Err(err),
    };
    let image = (frame.width(), frame.height());
    for size in sizes {
        out.write_all(&protocol::frame_header(size)).map_err(|err| err.to_string())?;
        athanor_wallpaper::write_zoomed(frame.buf_slice(), frame.stride() as usize, image, size, &mut out).map_err(|err| err.to_string())?;
    }
    out.flush().map_err(|err| err.to_string())
}

/// The picture in `file`, which must be a regular file, decoded.
fn picture(file: File) -> Result<glycin::Frame, Undecoded> {
    let unread = |err| Undecoded::Fault(format!("the file cannot be read: {err}"));
    let metadata = file.metadata().map_err(unread)?;
    if !metadata.file_type().is_file() {
        return Err(Undecoded::Picture("the descriptor is not a regular file".to_owned()));
    }
    let mut input = Vec::new();
    file.take(MAX_INPUT + 1).read_to_end(&mut input).map_err(unread)?;
    if input.len() as u64 > MAX_INPUT {
        return Err(Undecoded::Picture(format!("the file is larger than {MAX_INPUT} bytes")));
    }
    decode(input, glycin::MemoryFormat::R8g8b8a8, WALLPAPER)
}

/// The request on `connection` and the one file descriptor that came with it.
fn request(connection: OwnedFd) -> Result<(Vec<(u32, u32)>, OwnedFd), String> {
    let mut bytes = [0u8; REQUEST_LEN];
    let (mut read, mut files) = scm::receive(connection.as_fd(), &mut bytes).map_err(|err| format!("the request cannot be read: {err}"))?;
    // The descriptor rides on the first byte; a stream may split the rest of the request.
    let mut rest = File::from(connection);
    while read > 0 && read < REQUEST_LEN {
        match rest.read(&mut bytes[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(err) => return Err(format!("the request cannot be read: {err}")),
        }
    }
    let sizes = (read == REQUEST_LEN).then(|| protocol::parse_request(&bytes)).flatten().ok_or("the request is malformed")?;
    match (files.pop(), files.is_empty()) {
        (Some(file), true) => Ok((sizes, file)),
        _ => Err("the request carries no file, or more than one".to_owned()),
    }
}

fn pdf(input: Vec<u8>, side: u32) -> Result<Picture, String> {
    let document = poppler::Document::from_data(&input, None)
        .map_err(|err| err.to_string())?;
    let page = document.page(0).ok_or("the document has no page")?;
    let (width, height) = page.size();
    if !(width >= 1.0 && height >= 1.0) {
        return Err("the page has no size".to_owned());
    }
    let scale = f64::from(side) / width.max(height);
    // Both sides are at most `side`, which is at most MAX_SIDE.
    let (w, h) = (((width * scale).round() as i32).max(1), ((height * scale).round() as i32).max(1));
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|err| err.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|err| err.to_string())?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().map_err(|err| err.to_string())?;
        cr.scale(scale, scale);
        page.render(&cr);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().map_err(|err| err.to_string())?;
    // CAIRO_FORMAT_ARGB32 is a native-endian word: on little-endian, B, G, R, A.
    packed(w as u32, h as u32, stride, &data, side, |[b, g, r, a]| [r, g, b, a])
}

/// Reads `height` rows of `width` pixels out of a buffer with `stride` bytes per row,
/// scaled down to at most `side` on the longest side, in one pass: the decoded buffer is
/// never copied whole, so peak memory stays at the loader's own frame.
fn packed(width: u32, height: u32, stride: usize, buf: &[u8], side: u32, pixel: impl Fn([u8; 4]) -> [u8; 4]) -> Result<Picture, String> {
    let row = width as usize * 4;
    if width == 0 || height == 0 || stride < row || buf.len() < stride * (height as usize - 1) + row {
        return Err("the decoded buffer is shorter than its size".to_owned());
    }
    Ok(average(width, height, stride, buf, side, pixel))
}

/// Averages each block of source pixels until the longest side is at most `side`; a
/// picture that fits is copied pixel by pixel. Averaging premultiplied pixels is exact, and
/// a channel permutation commutes with it, so `pixel` is applied to the average. The
/// caller has checked that `buf` holds `height` rows of `stride` bytes.
fn average(width: u32, height: u32, stride: usize, buf: &[u8], side: u32, pixel: impl Fn([u8; 4]) -> [u8; 4]) -> Picture {
    let longest = width.max(height);
    let scaled = |n: u32| {
        if longest <= side {
            n
        } else {
            ((u64::from(n) * u64::from(side) / u64::from(longest)) as u32).max(1)
        }
    };
    let (w, h) = (scaled(width), scaled(height));
    let (sw, sh) = (width as usize, height as usize);
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let (y0, y1) = (y * sh / h as usize, ((y + 1) * sh / h as usize).max(y * sh / h as usize + 1));
        for x in 0..w as usize {
            let (x0, x1) = (x * sw / w as usize, ((x + 1) * sw / w as usize).max(x * sw / w as usize + 1));
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = sy * stride + sx * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(buf[at + c]);
                    }
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            rgba.extend_from_slice(&pixel(sum.map(|total| (total / count) as u8)));
        }
    }
    Picture { width: w, height: h, rgba }
}

/// A packed picture scaled down to `side`.
#[cfg(test)]
fn fit(picture: Picture, side: u32) -> Picture {
    average(picture.width, picture.height, picture.width as usize * 4, &picture.rgba, side, |px| px)
}

fn write(out: &mut impl Write, picture: &Picture) -> io::Result<()> {
    out.write_all(MAGIC)?;
    out.write_all(&picture.width.to_le_bytes())?;
    out.write_all(&picture.height.to_le_bytes())?;
    out.write_all(&picture.rgba)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_and_the_wallpaper_each_get_their_own_bounds() {
        assert_eq!(PREVIEW, Bounds { side: 6144, timeout: Duration::from_secs(3) });
        assert_eq!(WALLPAPER, Bounds { side: 16384, timeout: Duration::from_secs(15) });
    }

    // poppler rebuilds the missing cross-reference table, as it does for a damaged file.
    const PAGE: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";

    #[test]
    fn a_page_is_rendered_white_at_the_asked_size() {
        let picture = pdf(PAGE.to_vec(), 64).expect("rendered");
        assert_eq!((picture.width, picture.height), (64, 32));
        assert!(picture.rgba.chunks(4).all(|px| px == [255, 255, 255, 255]));
    }

    #[test]
    fn a_damaged_pdf_is_an_error() {
        assert!(pdf(b"%PDF-1.4\nnot a document".to_vec(), 64).is_err());
    }

    #[test]
    fn a_large_picture_is_averaged_down() {
        let big = Picture { width: 4, height: 2, rgba: [[0, 0, 0, 255], [255, 255, 255, 255]].repeat(4).concat() };
        let small = fit(big, 2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.rgba, [127, 127, 127, 255, 127, 127, 127, 255]);
        let tiny = Picture { width: 3, height: 1, rgba: vec![9; 12] };
        assert_eq!(fit(tiny, 1024).rgba, vec![9; 12], "a small picture is left alone");
    }

    /// Sends `bytes` with `files` attached on one end of a pair and reads the request on the
    /// other, as the decoder does.
    fn sent(bytes: &[u8], files: &[&File]) -> Result<(Vec<(u32, u32)>, OwnedFd), String> {
        use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
        use std::os::fd::AsRawFd;
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        let raw: Vec<_> = files.iter().map(|file| file.as_raw_fd()).collect();
        let rights = [ControlMessage::ScmRights(&raw)];
        let control = if raw.is_empty() { &[][..] } else { &rights[..] };
        sendmsg::<()>(ours.as_raw_fd(), &[io::IoSlice::new(bytes)], control, MsgFlags::empty(), None).expect("send");
        drop(ours);
        request(OwnedFd::from(theirs))
    }

    #[test]
    fn a_request_brings_its_sizes_and_its_one_file() {
        let file = File::open("/proc/self/exe").expect("a file");
        let bytes = protocol::request(&[(4, 3), (2, 1)]).expect("valid");
        let (sizes, received) = sent(&bytes, &[&file]).expect("received");
        assert_eq!(sizes, vec![(4, 3), (2, 1)]);
        let (a, b) = (file.metadata().expect("stat"), File::from(received).metadata().expect("stat"));
        use std::os::unix::fs::MetadataExt;
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()), "the same file");
    }

    #[test]
    fn a_request_without_one_file_or_with_bad_bytes_is_refused() {
        let file = File::open("/proc/self/exe").expect("a file");
        let bytes = protocol::request(&[(4, 3)]).expect("valid");
        assert!(sent(&bytes, &[]).is_err(), "no file");
        assert!(sent(&bytes, &[&file, &file]).is_err(), "two files");
        assert!(sent(&bytes[..REQUEST_LEN - 1], &[&file]).is_err(), "short");
        let mut bad = bytes;
        bad[0] = b'X';
        assert!(sent(&bad, &[&file]).is_err(), "magic");
    }

    #[test]
    fn the_frame_carries_its_size() {
        let mut out = Vec::new();
        write(&mut out, &Picture { width: 1, height: 2, rgba: vec![1; 8] }).expect("write");
        assert_eq!(&out[..5], b"ATPV1");
        assert_eq!(&out[5..13], [1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(out.len(), 13 + 8);
    }

    #[test]
    fn only_a_regular_file_is_read() {
        let device = File::open("/dev/null").expect("a device");
        let refused = picture(device).err();
        assert!(matches!(&refused, Some(Undecoded::Picture(why)) if why.contains("not a regular file")), "{refused:?}");
    }

    #[test]
    fn a_loader_s_failure_is_a_fault_not_a_refusal() {
        // A timeout, a spawn failure or a killed loader: none is an unsupported format.
        let undecoded = Undecoded::from(glycin::Error::other("the loader was killed"));
        assert!(matches!(undecoded, Undecoded::Fault(_)), "{undecoded:?}");
    }
}
