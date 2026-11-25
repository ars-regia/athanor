//! `is_rtl` reads the environment, so it has a test binary of its own: no other test runs in
//! this process while the variables change.

use athanor_i18n::{is_rtl, Catalog, OVERRIDE_VARIABLE};

/// The direction is the one the program's own catalog gives: by the override file when one is
/// named, else by the catalog of the session's language, and left to right when that language
/// has none.
#[test]
fn the_direction_is_the_one_of_the_catalog_the_program_loads() {
    let ar = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ar.mo");
    let bar = |domain: &str| Catalog::load(domain).expect("a catalog").is_rtl();
    std::env::set_var(OVERRIDE_VARIABLE, ar);
    assert!(is_rtl("athanor-bar"), "the override's Arabic");
    assert_eq!(is_rtl("athanor-bar"), bar("athanor-bar"));

    std::env::remove_var(OVERRIDE_VARIABLE);
    std::env::remove_var("LC_ALL");
    std::env::remove_var("LC_MESSAGES");
    std::env::set_var("LANG", "he_IL.UTF-8");
    let none = "athanor-i18n-test-no-catalog";
    assert!(!is_rtl(none), "Hebrew with no Hebrew catalog is English");
    assert_eq!(is_rtl(none), bar(none));
}
