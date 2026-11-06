//! The identity of a monitor, read from the base block of its EDID
//! (docs/architecture/doc_osd.md, OD4, "How the strings are equal").
//!
//! Sulphur applies [`parse`] to the EDID blob of a DRM connector and `athanor-osd` to the bytes
//! it reads at I2C address 0x50, so both name a monitor with the same strings. The crate does
//! no I/O and never panics: the input is the monitor's own bytes, which are untrusted.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

/// Length of the EDID base block. Extension blocks that follow it are not read.
pub const BASE_BLOCK_LEN: usize = 128;

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
/// The four 18-byte descriptor slots of the base block.
const DESCRIPTORS: std::ops::Range<usize> = 54..126;
const DESCRIPTOR_LEN: usize = 18;
const TAG_SERIAL: u8 = 0xFF;
const TAG_NAME: u8 = 0xFC;

/// Why an EDID was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Fewer than [`BASE_BLOCK_LEN`] bytes.
    #[error("EDID is {0} bytes, the base block needs 128")]
    Truncated(usize),
    /// The first eight bytes are not the fixed EDID header.
    #[error("EDID header is not 00 FF FF FF FF FF FF 00")]
    BadHeader,
    /// The 128 bytes of the base block do not sum to 0 modulo 256.
    #[error("EDID base block checksum does not sum to zero")]
    BadChecksum,
    /// The manufacturer ID is not three letters.
    #[error("EDID manufacturer ID is not three letters A to Z")]
    BadManufacturer,
}

/// What names one monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The three-letter PNP ID, such as `DEL`; not a name mapped from it.
    pub make: String,
    /// The monitor-name descriptor (0xFC), else the product code as four lowercase hex digits.
    pub model: String,
    /// The serial descriptor (0xFF), else the 32-bit serial when not zero, else empty.
    pub serial: String,
}

/// Reads the identity from an EDID, which may be longer than its base block.
pub fn parse(edid: &[u8]) -> Result<Identity, Error> {
    let block: &[u8; BASE_BLOCK_LEN] = edid
        .get(..BASE_BLOCK_LEN)
        .and_then(|b| b.try_into().ok())
        .ok_or(Error::Truncated(edid.len()))?;
    if block[..8] != HEADER {
        return Err(Error::BadHeader);
    }
    if block.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) != 0 {
        return Err(Error::BadChecksum);
    }

    let id = u16::from_be_bytes([block[8], block[9]]);
    let make = [id >> 10, id >> 5, id]
        .into_iter()
        .map(|v| match v & 0x1F {
            n @ 1..=26 => Ok(char::from(b'A' + (n as u8) - 1)),
            _ => Err(Error::BadManufacturer),
        })
        .collect::<Result<String, _>>()?;

    let product = u16::from_le_bytes([block[10], block[11]]);
    let number = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);

    let model = descriptor(block, TAG_NAME).unwrap_or_else(|| format!("{product:04x}"));
    let serial = descriptor(block, TAG_SERIAL).unwrap_or_else(|| {
        let mut s = String::new();
        if number != 0 {
            // Writing to a String does not fail.
            let _ = write!(s, "{number}");
        }
        s
    });
    Ok(Identity {
        make,
        model,
        serial,
    })
}

/// The text of the first display descriptor with `tag`, or `None` when there is none, when it
/// is empty, or when it holds a byte outside printable ASCII: the value is then not reported
/// rather than reported altered.
fn descriptor(block: &[u8; BASE_BLOCK_LEN], tag: u8) -> Option<String> {
    let (slots, _) = block[DESCRIPTORS].as_chunks::<DESCRIPTOR_LEN>();
    let text = slots
        .iter()
        .find(|d| d[..3] == [0, 0, 0] && d[3] == tag && d[4] == 0)?
        .get(5..)?;
    let end = text.iter().position(|b| *b == 0x0A).unwrap_or(text.len());
    let text = text.get(..end)?.trim_ascii_end();
    if text.is_empty() || !text.iter().all(|b| (0x20..=0x7E).contains(b)) {
        return None;
    }
    Some(text.iter().map(|b| char::from(*b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base block with the given PNP ID, product code and serial, and its descriptor
    /// slots filled from `descs`, the checksum made correct.
    fn edid(pnp: &str, product: u16, serial: u32, descs: &[(u8, &[u8])]) -> Vec<u8> {
        let mut b = vec![0u8; BASE_BLOCK_LEN];
        b[..8].copy_from_slice(&HEADER);
        let l: Vec<u16> = pnp.bytes().map(|c| u16::from(c - b'A' + 1)).collect();
        b[8..10].copy_from_slice(&((l[0] << 10) | (l[1] << 5) | l[2]).to_be_bytes());
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&serial.to_le_bytes());
        b[18] = 1;
        b[19] = 4;
        for (i, (tag, text)) in descs.iter().enumerate() {
            let at = 54 + i * DESCRIPTOR_LEN;
            b[at + 3] = *tag;
            b[at + 5..at + 5 + text.len()].copy_from_slice(text);
        }
        seal(&mut b);
        b
    }

    fn seal(b: &mut [u8]) {
        b[127] = 0;
        let sum = b.iter().fold(0u8, |s, x| s.wrapping_add(*x));
        b[127] = 0u8.wrapping_sub(sum);
    }

    fn dell() -> Vec<u8> {
        edid(
            "DEL",
            0xA0B3,
            0x4C32_3331,
            &[
                (TAG_SERIAL, b"7MT0168S\n    "),
                (TAG_NAME, b"DELL U2412M\n "),
            ],
        )
    }

    #[test]
    fn reads_the_descriptors() {
        let id = parse(&dell()).unwrap();
        assert_eq!(id.make, "DEL");
        assert_eq!(id.model, "DELL U2412M");
        assert_eq!(id.serial, "7MT0168S");
    }

    #[test]
    fn space_padding_without_a_newline_is_trimmed() {
        let e = edid("SAM", 1, 0, &[(TAG_NAME, b"LS27A800     ")]);
        assert_eq!(parse(&e).unwrap().model, "LS27A800");
    }

    #[test]
    fn missing_descriptors_fall_back_to_the_numbers() {
        let id = parse(&edid("AOC", 0x0A1F, 16843009, &[])).unwrap();
        assert_eq!(id.make, "AOC");
        assert_eq!(id.model, "0a1f");
        assert_eq!(id.serial, "16843009");
    }

    #[test]
    fn a_zero_serial_is_no_serial() {
        let id = parse(&edid("AOC", 2, 0, &[])).unwrap();
        assert_eq!(id.serial, "");
    }

    #[test]
    fn a_descriptor_beats_the_number() {
        let e = edid("LGD", 5, 77, &[(TAG_SERIAL, b"ABC123\n")]);
        assert_eq!(parse(&e).unwrap().serial, "ABC123");
    }

    #[test]
    fn non_ascii_or_control_bytes_drop_the_descriptor() {
        let e = edid(
            "DEL",
            0x12,
            9,
            &[(TAG_NAME, b"Caf\xE9 mon\n"), (TAG_SERIAL, b"A\x01B\n")],
        );
        let id = parse(&e).unwrap();
        assert_eq!(id.model, "0012");
        assert_eq!(id.serial, "9");
    }

    #[test]
    fn bytes_after_the_newline_are_ignored() {
        let e = edid("DEL", 1, 0, &[(TAG_NAME, b"OK\n\xFF\xFF")]);
        assert_eq!(parse(&e).unwrap().model, "OK");
    }

    #[test]
    fn an_empty_descriptor_falls_back() {
        let e = edid("DEL", 0x20, 0, &[(TAG_NAME, b"\n")]);
        assert_eq!(parse(&e).unwrap().model, "0020");
    }

    #[test]
    fn a_full_width_name_has_no_terminator() {
        let e = edid("DEL", 1, 0, &[(TAG_NAME, b"ABCDEFGHIJKLM")]);
        assert_eq!(parse(&e).unwrap().model, "ABCDEFGHIJKLM");
    }

    #[test]
    fn a_timing_descriptor_is_not_a_text_descriptor() {
        // Pixel clock nonzero: a detailed timing, whatever its fourth byte is.
        let mut e = dell();
        e[54] = 0x01;
        e[54 + 3] = TAG_NAME;
        e[54 + 5..54 + 9].copy_from_slice(b"EVIL");
        seal(&mut e);
        assert_eq!(parse(&e).unwrap().serial, 0x4C32_3331u32.to_string());
    }

    #[test]
    fn extension_blocks_are_ignored() {
        let mut e = dell();
        e.extend_from_slice(&[0xAB; 128]);
        assert_eq!(parse(&e).unwrap().make, "DEL");
    }

    #[test]
    fn truncation_is_refused_at_every_length() {
        let e = dell();
        for n in 0..BASE_BLOCK_LEN {
            assert_eq!(parse(&e[..n]), Err(Error::Truncated(n)));
        }
    }

    #[test]
    fn a_bad_header_is_refused() {
        let mut e = dell();
        e[0] = 1;
        seal(&mut e);
        assert_eq!(parse(&e), Err(Error::BadHeader));
    }

    #[test]
    fn a_bad_checksum_is_refused() {
        let mut e = dell();
        e[127] = e[127].wrapping_add(1);
        assert_eq!(parse(&e), Err(Error::BadChecksum));
        let mut e = dell();
        e[20] ^= 0x10;
        assert_eq!(parse(&e), Err(Error::BadChecksum));
    }

    #[test]
    fn a_manufacturer_that_is_not_three_letters_is_refused() {
        let mut e = dell();
        e[8] = 0;
        e[9] = 0;
        seal(&mut e);
        assert_eq!(parse(&e), Err(Error::BadManufacturer));
        e[8] = 0x7F;
        e[9] = 0xFF;
        seal(&mut e);
        assert_eq!(parse(&e), Err(Error::BadManufacturer));
    }

    #[test]
    fn hostile_blocks_do_not_panic() {
        for fill in [0x00u8, 0xFF, 0x0A, 0x20, 0x80] {
            let mut e = vec![fill; 300];
            e[..8].copy_from_slice(&HEADER);
            for slot in 0..4 {
                let at = 54 + slot * DESCRIPTOR_LEN;
                e[at..at + 5].copy_from_slice(&[0, 0, 0, TAG_NAME, 0]);
            }
            seal(&mut e[..BASE_BLOCK_LEN]);
            let _ = parse(&e);
        }
    }
}
