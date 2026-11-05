//! The font GTK draws Latin text with, asked of fontconfig as Pango asks it (#590).
//!
//! Pango gives a Latin run the locale's language when that language is written in Latin, and
//! otherwise `en`, its sample language for the script. It draws the run with the first font of
//! fontconfig's sort of sans-serif for that language that has the letters. Fontique sorts
//! sans-serif only for the locale's languages, which under Arabic, Hebrew or Japanese start
//! with a font without Latin letters, so this module asks fontconfig itself, always for `en`.
//! Under the image's locales written in Latin (de_DE, it_IT, fr_FR, pl_PL, tr_TR, vi_VN and
//! others) the sort for the locale's language and the sort for `en` give the same first Latin
//! font, Noto Sans, so asking for the locale's language would change no font and would need
//! Pango's table of the scripts each language is written in. It is the crate's only fontconfig
//! FFI: each call is one `unsafe` block, and every pattern, font set and configuration it takes
//! is released on every path.

#![allow(unsafe_code)]

use fontconfig_sys::constants::{FC_CHARSET, FC_FAMILY, FC_LANG};
use fontconfig_sys::{
    FcCharSetHasChar, FcConfig, FcConfigDestroy, FcConfigReference, FcConfigSubstitute,
    FcDefaultSubstitute, FcFontSetDestroy, FcFontSort, FcMatchPattern, FcPattern,
    FcPatternAddString, FcPatternCreate, FcPatternDestroy, FcPatternGetCharSet, FcPatternGetString,
    FcResultMatch, FcResultNoMatch,
};
use std::ffi::CStr;
use std::ptr;
use std::slice;

/// The letters a font must have to draw a Latin run.
fn latin_letters() -> impl Iterator<Item = char> {
    ('A'..='Z').chain('a'..='z')
}

/// The first family of `sorted`, fonts in fontconfig's order with whether each has the Latin
/// letters, that draws Latin text.
pub(super) fn first_latin<'a>(
    sorted: impl IntoIterator<Item = (&'a str, bool)>,
) -> Option<&'a str> {
    sorted
        .into_iter()
        .find_map(|(family, latin)| latin.then_some(family))
}

/// Fontconfig's sort of sans-serif for the language `en`, best first: each font's family and
/// whether it has the Latin letters. `None` when fontconfig has no configuration, cannot build
/// the pattern or sorts nothing.
pub(super) fn sans_serif_for_english() -> Option<Vec<(String, bool)>> {
    // SAFETY: with a null argument fontconfig returns its current configuration, loading it if
    // there is none, with a reference the caller owns; null when it cannot load one.
    let config = unsafe { FcConfigReference(ptr::null_mut()) };
    if config.is_null() {
        return None;
    }
    // SAFETY: takes nothing and returns an empty pattern the caller owns, or null.
    let pattern = unsafe { FcPatternCreate() };
    let fonts = if pattern.is_null() {
        None
    } else {
        let fonts = sort(config, pattern);
        // SAFETY: the pattern created above, released once; `sort` keeps nothing of it.
        unsafe { FcPatternDestroy(pattern) };
        fonts
    };
    // SAFETY: the reference taken above, released once.
    unsafe { FcConfigDestroy(config) };
    fonts
}

/// The fonts `config` sorts for `pattern`, a live empty pattern, once it asks for sans-serif in
/// `en` and has the configuration's and the defaults' substitutions, as Pango's patterns have.
fn sort(config: *mut FcConfig, pattern: *mut FcPattern) -> Option<Vec<(String, bool)>> {
    let sans = c"sans-serif".as_ptr().cast();
    // SAFETY: `pattern` is live, and fontconfig copies the NUL-terminated name and value.
    let family = unsafe { FcPatternAddString(pattern, FC_FAMILY.as_ptr(), sans) };
    let en = c"en".as_ptr().cast();
    // SAFETY: as above.
    let lang = unsafe { FcPatternAddString(pattern, FC_LANG.as_ptr(), en) };
    if family == 0 || lang == 0 {
        return None;
    }
    // SAFETY: `config` and `pattern` are live; the substitution edits the pattern in place.
    if unsafe { FcConfigSubstitute(config, pattern, FcMatchPattern) } == 0 {
        return None;
    }
    // SAFETY: `pattern` is live; the defaults are added in place.
    unsafe { FcDefaultSubstitute(pattern) };
    let mut result = FcResultNoMatch;
    // SAFETY: `config` and `pattern` are live, a null coverage pointer asks for no coverage set,
    // and `result` outlives the call. The font set returned is the caller's, or null.
    let set = unsafe { FcFontSort(config, pattern, 0, ptr::null_mut(), &mut result) };
    if set.is_null() {
        return None;
    }
    // SAFETY: a non-null font set fontconfig returned, live until it is destroyed below.
    let fonts = unsafe { &*set };
    let sorted = match usize::try_from(fonts.nfont) {
        Ok(count) if count > 0 && !fonts.fonts.is_null() => {
            // SAFETY: the set holds `count` font patterns from `fonts`, live while the set is.
            let fonts = unsafe { slice::from_raw_parts(fonts.fonts, count) };
            fonts.iter().filter_map(|&font| describe(font)).collect()
        }
        _ => Vec::new(),
    };
    // SAFETY: the font set returned above, released once; `sorted` owns copies of its strings.
    unsafe { FcFontSetDestroy(set) };
    Some(sorted)
}

/// The family of `font`, a live font pattern, and whether it has the Latin letters; `None` for a
/// font without a family name.
fn describe(font: *mut FcPattern) -> Option<(String, bool)> {
    let mut family = ptr::null_mut();
    // SAFETY: `font` is live; on a match `family` points to a string the pattern owns.
    if unsafe { FcPatternGetString(font, FC_FAMILY.as_ptr(), 0, &mut family) } != FcResultMatch {
        return None;
    }
    // SAFETY: a NUL-terminated string that lives as long as `font`, copied before it goes.
    let family = unsafe { CStr::from_ptr(family.cast()) }
        .to_string_lossy()
        .into_owned();
    let mut charset = ptr::null_mut();
    // SAFETY: `font` is live; on a match `charset` points to a set the pattern owns.
    let found = unsafe { FcPatternGetCharSet(font, FC_CHARSET.as_ptr(), 0, &mut charset) };
    let latin = found == FcResultMatch
        && latin_letters().all(|c| {
            // SAFETY: the character set of `font`, live as long as it is.
            unsafe { FcCharSetHasChar(charset, u32::from(c)) != 0 }
        });
    Some((family, latin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_takes_the_first_font_with_the_letters() {
        // Fontconfig's order under ar_EG and he_IL: the locale's fonts first, without Latin.
        let arabic = [
            ("Noto Sans Arabic", false),
            ("Noto Sans", true),
            ("Adwaita Sans", true),
        ];
        assert_eq!(first_latin(arabic), Some("Noto Sans"));
        let hebrew = [("Droid Sans Hebrew", false), ("Noto Sans", true)];
        assert_eq!(first_latin(hebrew), Some("Noto Sans"));
        // Under en_US the first font already has them.
        assert_eq!(
            first_latin([("Noto Sans", true), ("Noto Sans Arabic", false)]),
            Some("Noto Sans")
        );
        assert_eq!(first_latin([("Noto Sans Arabic", false)]), None);
        assert_eq!(first_latin([]), None);
    }
}
