//! What `athanor-wallpaper` and its decoder say to each other (SD17, maintainer decision,
//! 2026-11-07). The daemon connects to the decoder's socket and sends, in one message, the
//! opened image file as `SCM_RIGHTS` and a request: [`REQUEST_MAGIC`], the number of sizes,
//! then up to [`MAX_SIZES`] buffer sizes as little-endian `u32` pairs, the unused ones zero.
//! The decoder answers with one frame per size, in the order asked: [`FRAME_MAGIC`], the
//! width and the height as little-endian `u32`, then that many straight RGBA8 pixels, row by
//! row from the top left, scaled with zoom to that size. It closes the connection after the
//! last frame, or at once when it fails.

/// The socket the decoder's unit listens on, beneath `$XDG_RUNTIME_DIR`.
pub const SOCKET: &str = "athanor-wallpaper/decode";
/// The only program the decoder admits as its peer.
pub const DAEMON_EXE: &str = "/usr/bin/athanor-wallpaper";
/// The only unit, in the user manager's `session.slice`, the decoder admits its peer from.
pub const DAEMON_UNIT: &str = "athanor-wallpaper.service";
/// The slice of [`DAEMON_UNIT`].
pub const DAEMON_SLICE: &str = "session.slice";

/// The first bytes of a request.
pub const REQUEST_MAGIC: &[u8; 5] = b"ATWR1";
/// The first bytes of a frame: `athanor-preview-render`'s picture header.
pub const FRAME_MAGIC: &[u8; 5] = b"ATPV1";
/// The most sizes one request asks for: more outputs show the image in more requests.
pub const MAX_SIZES: usize = 8;
/// The longest buffer side asked: an 8K output.
pub const MAX_SIDE: u32 = 8192;
/// A request's length, always the same.
pub const REQUEST_LEN: usize = REQUEST_MAGIC.len() + 1 + MAX_SIZES * 8;
/// A frame header's length.
pub const FRAME_HEADER_LEN: usize = FRAME_MAGIC.len() + 8;
/// The most connections the daemon holds open to the decoder at once: the socket unit's
/// `MaxConnections=` and `MaxConnectionsPerSource=` (row W4 of SD22) are this value.
pub const MAX_CONNECTIONS: usize = 4;

/// A size every side of which is 1 to [`MAX_SIDE`].
fn valid((width, height): (u32, u32)) -> bool {
    (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height)
}

/// The request for `sizes`; `None` for none, more than [`MAX_SIZES`], or a side out of range.
pub fn request(sizes: &[(u32, u32)]) -> Option<[u8; REQUEST_LEN]> {
    if sizes.is_empty() || sizes.len() > MAX_SIZES || !sizes.iter().copied().all(valid) {
        return None;
    }
    let mut out = [0; REQUEST_LEN];
    out[..5].copy_from_slice(REQUEST_MAGIC);
    out[5] = sizes.len() as u8;
    for (i, (w, h)) in sizes.iter().enumerate() {
        let at = 6 + i * 8;
        out[at..at + 4].copy_from_slice(&w.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&h.to_le_bytes());
    }
    Some(out)
}

/// The sizes a request asks for; `None` for anything [`request`] does not write, unused
/// pairs that are not zero included.
pub fn parse_request(bytes: &[u8; REQUEST_LEN]) -> Option<Vec<(u32, u32)>> {
    let count = usize::from(bytes[5]);
    if &bytes[..5] != REQUEST_MAGIC || count == 0 || count > MAX_SIZES {
        return None;
    }
    let pair = |i: usize| {
        let at = 6 + i * 8;
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        (word(at), word(at + 4))
    };
    let sizes: Vec<_> = (0..count).map(pair).collect();
    let unused_zero = (count..MAX_SIZES).all(|i| pair(i) == (0, 0));
    (sizes.iter().copied().all(valid) && unused_zero).then_some(sizes)
}

/// The header of a frame of `size`.
pub fn frame_header((width, height): (u32, u32)) -> [u8; FRAME_HEADER_LEN] {
    let mut out = [0; FRAME_HEADER_LEN];
    out[..5].copy_from_slice(FRAME_MAGIC);
    out[5..9].copy_from_slice(&width.to_le_bytes());
    out[9..13].copy_from_slice(&height.to_le_bytes());
    out
}

/// The size a frame header announces, when it is `expected`: the daemon reads only the sizes
/// it asked for, so a decoder cannot make it allocate more.
pub fn check_frame(header: &[u8; FRAME_HEADER_LEN], expected: (u32, u32)) -> bool {
    header == &frame_header(expected)
}

/// The header the decoder answers in place of the first frame when the picture cannot be
/// decoded: a size of zero, which no request asks for. Any other end of the answer is the
/// decoder's fault, not the picture's.
pub fn refusal() -> [u8; FRAME_HEADER_LEN] {
    frame_header((0, 0))
}

/// The bytes of a frame's pixels.
pub fn frame_len((width, height): (u32, u32)) -> usize {
    width as usize * height as usize * 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reads_back_as_written() {
        let sizes = [(3840, 2160), (1, 1), (MAX_SIDE, 1)];
        let bytes = request(&sizes).expect("valid");
        assert_eq!(parse_request(&bytes), Some(sizes.to_vec()));
        let full = [(1920, 1080); MAX_SIZES];
        assert_eq!(
            parse_request(&request(&full).expect("valid")),
            Some(full.to_vec())
        );
    }

    #[test]
    fn an_invalid_request_is_neither_written_nor_read() {
        for sizes in [
            &[][..],
            &[(0, 1)],
            &[(1, MAX_SIDE + 1)],
            &[(1, 1); MAX_SIZES + 1],
        ] {
            assert_eq!(request(sizes), None, "{sizes:?}");
        }
        let good = request(&[(2, 2)]).expect("valid");
        let mut magic = good;
        magic[0] = b'X';
        let mut none = good;
        none[5] = 0;
        let mut many = good;
        many[5] = MAX_SIZES as u8 + 1;
        let mut tail = good;
        tail[REQUEST_LEN - 1] = 1;
        let mut zero = good;
        zero[6..10].fill(0);
        for bytes in [magic, none, many, tail, zero] {
            assert_eq!(parse_request(&bytes), None);
        }
    }

    #[test]
    fn only_the_frame_asked_for_is_accepted() {
        let header = frame_header((640, 480));
        assert!(check_frame(&header, (640, 480)));
        assert!(!check_frame(&header, (480, 640)));
        let mut magic = header;
        magic[4] = b'2';
        assert!(!check_frame(&magic, (640, 480)));
        assert_eq!(frame_len((640, 480)), 640 * 480 * 4);
    }
}
