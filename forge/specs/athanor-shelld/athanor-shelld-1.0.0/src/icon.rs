//! Where a picture comes from when it is named rather than sent (doc_bar.md BR4): an icon
//! name of the theme or a local file, never a remote URL.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    Name(String),
    File(String),
}

const NAME_BYTES: usize = 128;
const PATH_BYTES: usize = 4096;

/// `app_icon`, or the `image-path` hint. `None` for anything else, empty included.
#[must_use]
pub fn parse(value: &str) -> Option<Icon> {
    if let Some(rest) = value.strip_prefix("file://") {
        return local_file(&percent_decode(rest)?);
    }
    if value.starts_with('/') {
        return local_file(value);
    }
    let is_name = !value.is_empty()
        && value.len() <= NAME_BYTES
        && !value.starts_with('.')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'));
    is_name.then(|| Icon::Name(value.to_owned()))
}

/// An absolute path with no `..` and no control character. `file://host/…` arrives here
/// without its leading slash and is refused.
fn local_file(path: &str) -> Option<Icon> {
    let ok = path.starts_with('/')
        && path.len() <= PATH_BYTES
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "..");
    ok.then(|| Icon::File(path.to_owned()))
}

/// The `%XX` escapes of a file URI. `None` when an escape is malformed or the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_local_files_are_accepted() {
        assert_eq!(
            parse("dialog-information"),
            Some(Icon::Name("dialog-information".into()))
        );
        assert_eq!(
            parse("org.gnome.Nautilus"),
            Some(Icon::Name("org.gnome.Nautilus".into()))
        );
        assert_eq!(
            parse("/usr/share/icons/a.png"),
            Some(Icon::File("/usr/share/icons/a.png".into()))
        );
        assert_eq!(
            parse("file:///home/u/My%20Pic.png"),
            Some(Icon::File("/home/u/My Pic.png".into()))
        );
    }

    #[test]
    fn remote_relative_and_malformed_values_are_refused() {
        for value in [
            "",
            "https://evil.example/x.png",
            "file://evil.example/x.png",
            "../x",
            "a/b",
            "/a/../etc/shadow",
            "file:///a%2",
            "file:///a%ZZ",
            "file:///a%0A",
            ".hidden",
        ] {
            assert_eq!(parse(value), None, "{value:?}");
        }
    }
}
