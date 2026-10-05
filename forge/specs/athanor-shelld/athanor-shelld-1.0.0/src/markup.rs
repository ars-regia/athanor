//! The body markup a sender may use, reduced to spans (doc_notification_center.md, NC10):
//! bold, italic, underline and links with a safe scheme. Nothing of the sender's text is
//! kept as markup: the readers render the spans, and only the spans, back to Pango.

use athanor_services::notifications::wire::is_safe_href;
use athanor_unit::text;

const BOLD: u32 = 1;
const ITALIC: u32 = 2;
const UNDERLINE: u32 = 4;

type Span = (String, u32, String);

enum Tag {
    Open(char, String),
    Close(char),
    Vanish,
}

/// The plain text and the styled spans of `body`.
#[must_use]
pub fn parse(body: &str) -> (String, Vec<Span>) {
    let mut spans: Vec<Span> = Vec::new();
    let mut open: Vec<(char, String)> = Vec::new();
    let mut buf = String::new();
    let mut rest = body;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some((tag, len)) = tag(rest) {
                flush(&mut spans, &mut buf, &open);
                match tag {
                    Tag::Open(name, href) => open.push((name, href)),
                    Tag::Close(name) => {
                        if let Some(at) = open.iter().rposition(|(n, _)| *n == name) {
                            open.truncate(at);
                        }
                    }
                    Tag::Vanish => {}
                }
                rest = &rest[len..];
                continue;
            }
        }
        if c == '&' {
            if let Some((decoded, len)) = entity(rest) {
                buf.push(decoded);
                rest = &rest[len..];
                continue;
            }
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut spans, &mut buf, &open);
    let plain = spans.iter().map(|(t, _, _)| t.as_str()).collect();
    (plain, spans)
}

fn flush(spans: &mut Vec<Span>, buf: &mut String, open: &[(char, String)]) {
    // Decoded text passes the same filter as the raw text: an entity cannot bring back a
    // control or bidirectional character.
    let clean = text::lines(buf, usize::MAX);
    buf.clear();
    if clean.is_empty() {
        return;
    }
    let style = open.iter().fold(0, |bits, (name, _)| {
        bits | match name {
            'b' => BOLD,
            'i' => ITALIC,
            'u' => UNDERLINE,
            _ => 0,
        }
    });
    let href = open
        .iter()
        .rev()
        .find(|(name, _)| *name == 'a')
        .map(|(_, href)| href.clone())
        .unwrap_or_default();
    spans.push((clean, style, href));
}

/// The tag at the start of `s` (which starts with `<`) and its length, or `None` when it is
/// to stay as text.
fn tag(s: &str) -> Option<(Tag, usize)> {
    let end = s.find('>')?;
    let inner = &s[1..end];
    let len = end + 1;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let inner = inner.trim();
    let (name, attrs) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
    let name = name.to_ascii_lowercase();
    let attrs = attrs.trim();
    match (name.as_str(), closing) {
        ("b" | "i" | "u", false) if attrs.is_empty() => {
            Some((Tag::Open(name.chars().next()?, String::new()), len))
        }
        ("b" | "i" | "u" | "a", true) if attrs.is_empty() => {
            Some((Tag::Close(name.chars().next()?), len))
        }
        ("a", false) => Some((Tag::Open('a', href(attrs)), len)),
        ("img", false) => Some((Tag::Vanish, len)),
        _ => None,
    }
}

/// The `href` of an `<a>` tag's attributes when it has a safe scheme, else empty.
fn href(attrs: &str) -> String {
    let lower = attrs.to_ascii_lowercase();
    let Some(at) = lower.find("href") else {
        return String::new();
    };
    let value = attrs[at + 4..].trim_start();
    let Some(value) = value.strip_prefix('=') else {
        return String::new();
    };
    let value = value.trim_start();
    let Some(quote) = value.chars().next().filter(|q| matches!(q, '"' | '\'')) else {
        return String::new();
    };
    let value = &value[1..];
    let Some(close) = value.find(quote) else {
        return String::new();
    };
    let decoded = decode(&value[..close]);
    let decoded = text::line(&decoded, usize::MAX);
    if is_safe_href(&decoded) {
        decoded
    } else {
        String::new()
    }
}

fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        match (c == '&').then(|| entity(rest)).flatten() {
            Some((decoded, len)) => {
                out.push(decoded);
                rest = &rest[len..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The character of the entity at the start of `s` (which starts with `&`) and its length.
fn entity(s: &str) -> Option<(char, usize)> {
    let end = s.find(';').filter(|&end| end <= 10)?;
    let name = &s[1..end];
    let decoded = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((decoded, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allowed_tags_survive() {
        let (plain, spans) = parse("<b>Bold</b> <a href=\"https://x.org/a\">link</a> <a href=\"file:///etc/passwd\">file</a><img src=\"x\"/> <script>x</script> &amp; &nbsp;");
        assert_eq!(plain, "Bold link file <script>x</script> & &nbsp;");
        assert_eq!(spans[0], ("Bold".into(), 1, String::new()));
        assert!(spans.contains(&("link".into(), 0, "https://x.org/a".into())));
        assert!(
            spans.contains(&("file".into(), 0, String::new())),
            "a file: link loses its target"
        );
    }

    #[test]
    fn schemes_are_checked_whatever_their_case() {
        assert_eq!(
            parse("<a href=\"JavaScript:alert(1)\">x</a>").1,
            [("x".into(), 0, String::new())]
        );
        assert_eq!(
            parse("<a href=\"MAILTO:a@b\">m</a>").1,
            [("m".into(), 0, "MAILTO:a@b".into())]
        );
    }

    #[test]
    fn an_entity_cannot_bring_back_a_stripped_character() {
        assert_eq!(parse("a&#x202E;b&#7;c&#65;").0, "abcA");
    }

    #[test]
    fn nested_and_unclosed_tags_are_closed() {
        let (_, spans) = parse("<b>a<i>b</b>c");
        assert_eq!(
            spans,
            [
                ("a".into(), 1, String::new()),
                ("b".into(), 3, String::new()),
                ("c".into(), 0, String::new())
            ]
        );
    }
}
