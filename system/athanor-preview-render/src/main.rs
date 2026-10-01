//! athanor-preview-render: one untrusted image or PDF on standard input, one bitmap in the
//! ATPV1 format on standard output (doc_launcher.md, LA6, LA9). The launcher starts it in a
//! transient unit with no network, no home, no runtime directory and no bus; inside it,
//! glycin runs its loaders in bubblewrap. Usage: `athanor-preview-render image|pdf <side>`.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Duration;

const MAGIC: &[u8; 5] = b"ATPV1";
/// Larger files are not previewed.
const MAX_INPUT: u64 = 64 << 20;
/// The longest side of a bitmap, whatever the caller asks.
const MAX_SIDE: u32 = 1024;
/// 8192 × 8192 × 4 bytes is the largest decode; the unit's MemoryMax bounds the rest.
const MAX_DECODE: u32 = 8192;

struct Picture {
    width: u32,
    height: u32,
    /// Premultiplied RGBA, rows packed.
    rgba: Vec<u8>,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(kind), Some(side)) = (args.next(), args.next().and_then(|side| side.parse::<u32>().ok())) else {
        eprintln!("usage: athanor-preview-render image|pdf <side>");
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
        "image" => image(input),
        "pdf" => pdf(input, side),
        other => Err(format!("unknown kind {other}")),
    };
    let written = picture.map(|picture| fit(picture, side)).and_then(|picture| {
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

fn image(input: Vec<u8>) -> Result<Picture, String> {
    glib::MainContext::new().block_on(async move {
        let mut loader = glycin::Loader::new_vec(input);
        loader
            .sandbox_selector(glycin::SandboxSelector::Bwrap)
            .accepted_memory_formats(glycin::MemoryFormatSelection::R8g8b8a8Premultiplied)
            .limits(glycin::Limits::default().timeout(Duration::from_secs(3)).max_dimensions((MAX_DECODE, MAX_DECODE)));
        let mut image = loader.load().await.map_err(|err| err.to_string())?;
        let frame = image.next_frame().await.map_err(|err| err.to_string())?;
        if frame.memory_format() != glycin::MemoryFormat::R8g8b8a8Premultiplied {
            return Err("the loader returned another memory format".to_owned());
        }
        packed(frame.width(), frame.height(), frame.stride() as usize, frame.buf_slice(), |px| px)
    })
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
    packed(w as u32, h as u32, stride, &data, |[b, g, r, a]| [r, g, b, a])
}

/// Copies `height` rows of `width` pixels out of a buffer with `stride` bytes per row.
fn packed(width: u32, height: u32, stride: usize, buf: &[u8], pixel: impl Fn([u8; 4]) -> [u8; 4]) -> Result<Picture, String> {
    let row = width as usize * 4;
    if width == 0 || height == 0 || stride < row || buf.len() < stride * (height as usize - 1) + row {
        return Err("the decoded buffer is shorter than its size".to_owned());
    }
    let mut rgba = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        for px in buf[y * stride..y * stride + row].as_chunks::<4>().0 {
            rgba.extend_from_slice(&pixel(*px));
        }
    }
    Ok(Picture { width, height, rgba })
}

/// Scales a picture down, averaging each block of source pixels, until its longest side
/// is at most `side`. Averaging premultiplied pixels is exact.
fn fit(picture: Picture, side: u32) -> Picture {
    let longest = picture.width.max(picture.height);
    if longest <= side {
        return picture;
    }
    let scaled = |n: u32| ((u64::from(n) * u64::from(side) / u64::from(longest)) as u32).max(1);
    let (w, h) = (scaled(picture.width), scaled(picture.height));
    let (sw, sh) = (picture.width as usize, picture.height as usize);
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let (y0, y1) = (y * sh / h as usize, ((y + 1) * sh / h as usize).max(y * sh / h as usize + 1));
        for x in 0..w as usize {
            let (x0, x1) = (x * sw / w as usize, ((x + 1) * sw / w as usize).max(x * sw / w as usize + 1));
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy * sw + sx) * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(picture.rgba[at + c]);
                    }
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            rgba.extend(sum.map(|total| (total / count) as u8));
        }
    }
    Picture { width: w, height: h, rgba }
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

    #[test]
    fn the_frame_carries_its_size() {
        let mut out = Vec::new();
        write(&mut out, &Picture { width: 1, height: 2, rgba: vec![1; 8] }).expect("write");
        assert_eq!(&out[..5], b"ATPV1");
        assert_eq!(&out[5..13], [1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(out.len(), 13 + 8);
    }
}
