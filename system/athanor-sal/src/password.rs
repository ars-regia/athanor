//! The text of a password field (doc_accessibility.md, AX4), kept apart from the field's editor.
//!
//! A password field's editor holds one [`MASK`] for each character typed, never the characters:
//! what the field draws, what AT-SPI reads (its name, value, text and text-changed events), what
//! a selection offers and what the edit menu copies all come from the editor, so none of them
//! can hold the secret. The secret lives here alone, follows each edit of the editor, never
//! leaves through `Debug`, and is overwritten with zeros when it changes, is cleared or is
//! dropped.

use std::fmt;

use zeroize::Zeroizing;

/// The character a password field shows, and AT-SPI reads, for each character of the secret:
/// GTK's.
pub(crate) const MASK: char = '\u{25CF}';

/// `count` masks.
pub(crate) fn mask(count: usize) -> String {
    std::iter::repeat_n(MASK, count).collect()
}

/// The characters of `text` that `keep` takes, at most `max` of them, in a buffer of `text`'s
/// size that is overwritten when dropped. It never grows, so no buffer it outgrew is freed
/// holding the start of a password field's text, as one collected from the characters would be.
pub(crate) fn kept(text: &str, max: usize, keep: impl Fn(char) -> bool) -> Zeroizing<String> {
    let mut kept = Zeroizing::new(String::with_capacity(text.len()));
    for c in text.chars().filter(|&c| keep(c)).take(max) {
        kept.push(c);
    }
    kept
}

/// The secret a password field holds. Every change builds the new text in a buffer of exactly
/// its size and drops the old one, which [`Zeroizing`] overwrites, and every text typed, pasted,
/// committed or set reaches it through [`kept`]: no reallocation leaves a copy of the secret
/// behind in freed memory.
#[derive(Default)]
pub(crate) struct Secret(Zeroizing<String>);

impl Secret {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn chars(&self) -> usize {
        self.0.chars().count()
    }

    /// Replaces the whole secret with `text`.
    pub(crate) fn set(&mut self, text: &str) {
        let mut new = Zeroizing::new(String::with_capacity(text.len()));
        new.push_str(text);
        self.0 = new;
    }

    /// Replaces `removed` characters from character `start` with `inserted`. `false`, and the
    /// secret emptied, when the range falls outside the secret: the masks and the secret no
    /// longer match, and an empty field is the only state both can agree on.
    pub(crate) fn splice(&mut self, start: usize, removed: usize, inserted: &str) -> bool {
        let byte = |chars: usize| -> Option<usize> {
            let mut indices = self.0.char_indices().map(|(i, _)| i).chain([self.0.len()]);
            indices.nth(chars)
        };
        let (Some(from), Some(to)) = (byte(start), start.checked_add(removed).and_then(byte))
        else {
            self.set("");
            return false;
        };
        let len = self.0.len() - (to - from) + inserted.len();
        let mut new = Zeroizing::new(String::with_capacity(len));
        new.push_str(&self.0[..from]);
        new.push_str(inserted);
        new.push_str(&self.0[to..]);
        self.0 = new;
        true
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A collect from a filter grows its buffer by doubling, so the 8 and 16 byte buffers it
    /// frees hold the start of the text; the kept characters are built in one buffer instead.
    #[test]
    fn the_kept_characters_are_built_in_one_buffer_of_the_texts_size() {
        let text = "correct horse battery staple\u{7}";
        let all = kept(text, usize::MAX, |c| !c.is_control());
        assert_eq!(all.as_str(), "correct horse battery staple");
        assert_eq!(all.capacity(), text.len());
        let first = kept(text, 7, |_| true);
        assert_eq!((first.as_str(), first.capacity()), ("correct", text.len()));
    }

    #[test]
    fn a_splice_replaces_characters_not_bytes() {
        let mut secret = Secret::default();
        secret.set("pä§w");
        assert!(secret.splice(1, 2, "XY"));
        assert_eq!(secret.as_str(), "pXYw");
        assert!(secret.splice(4, 0, "é"));
        assert_eq!(secret.as_str(), "pXYwé");
        assert!(secret.splice(0, 5, ""));
        assert_eq!(secret.as_str(), "");
    }

    #[test]
    fn a_splice_outside_the_secret_empties_it() {
        let mut secret = Secret::default();
        secret.set("abc");
        assert!(!secret.splice(2, 2, "x"));
        assert_eq!(secret.as_str(), "");
        secret.set("abc");
        assert!(!secret.splice(usize::MAX, 1, ""));
        assert_eq!(secret.as_str(), "");
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let mut secret = Secret::default();
        secret.set("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        assert_eq!(format!("{secret:#?}"), "Secret(..)");
    }

    #[test]
    fn the_mask_is_one_character_per_character() {
        assert_eq!(mask(3), "\u{25CF}\u{25CF}\u{25CF}");
        assert_eq!(mask(0), "");
    }
}
