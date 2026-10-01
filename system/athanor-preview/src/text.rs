//! The first lines of a text file, read in the launcher (doc_launcher.md, LA6): at most 64
//! KB, cut at a character boundary, every line cleaned of control and bidirectional
//! characters.

use std::fs::File;
use std::io::{self, Read};

pub const MAX_BYTES: usize = 64 * 1024;

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

fn clean(text: &str) -> String {
    text.lines()
        .map(|line| athanor_unit::text::line(line, athanor_unit::text::BODY_CHARS))
        .collect::<Vec<_>>()
        .join("\n")
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
    fn a_character_cut_by_the_limit_is_dropped() {
        let path = std::env::temp_dir().join(format!("athanor-preview-cut-{}", std::process::id()));
        std::fs::write(&path, [vec![b'a'; MAX_BYTES - 1], "é".as_bytes().to_vec()].concat()).expect("write");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert!(text.chars().all(|c| c == 'a' || c == '\n'));
        std::fs::remove_file(path).expect("cleanup");
    }
}
