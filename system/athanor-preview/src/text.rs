//! The first lines of a text file, read in the launcher (doc_launcher.md, LA6): at most 64
//! KB, cut at a character boundary, every line cleaned of control and bidirectional
//! characters.

use std::fs::File;
use std::io::{self, Read};

pub const MAX_BYTES: usize = 64 * 1024;
/// A 64 KB file of newlines would make a label tens of thousands of lines tall.
pub const MAX_LINES: usize = 400;

pub fn head(file: &File) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(MAX_BYTES);
    file.take(MAX_BYTES as u64).read_to_end(&mut bytes)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        // A character cut by the limit is dropped; anything else invalid shows as U+FFFD.
        Err(err) if err.error_len().is_none() => std::str::from_utf8(&bytes[..err.valid_up_to()]).unwrap_or_default(),
        Err(_) => return Ok(lossy(&bytes)),
    };
    Ok(clean(text))
}

fn lossy(bytes: &[u8]) -> String {
    clean(&String::from_utf8_lossy(bytes))
}

/// At most MAX_LINES lines, then the shared filter over the whole body.
fn clean(text: &str) -> String {
    let body = text.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    athanor_unit::text::lines(&body, MAX_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_file_is_read_to_its_first_64_kilobytes_and_cleaned() {
        let path = std::env::temp_dir().join(format!("athanor-preview-head-{}", std::process::id()));
        std::fs::write(&path, "line\u{202E}one\nline two\n").expect("write");
        // Sparse: 2 GB on paper, a few blocks on disk.
        let file = std::fs::OpenOptions::new().write(true).open(&path).expect("open");
        file.set_len(2 << 30).expect("grow");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert!(text.starts_with("lineone\nline two"), "bidirectional controls removed: {text:?}");
        assert!(text.len() <= MAX_BYTES);
        std::fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn a_file_of_newlines_is_cut_to_the_line_cap() {
        let text = clean(&"x\n".repeat(MAX_LINES * 3));
        assert_eq!(text.lines().count(), MAX_LINES);
    }

    #[test]
    fn a_character_cut_by_the_limit_is_dropped() {
        let path = std::env::temp_dir().join(format!("athanor-preview-cut-{}", std::process::id()));
        std::fs::write(&path, [vec![b'a'; MAX_BYTES - 1], "é".as_bytes().to_vec()].concat()).expect("write");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert_eq!(text.len(), MAX_BYTES - 1, "the é is dropped, the 'a's stay");
        assert!(text.chars().all(|c| c == 'a'));
        std::fs::remove_file(path).expect("cleanup");
    }
}
