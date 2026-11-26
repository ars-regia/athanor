//! `is_rtl` reads the environment, so it has a test binary of its own: no other test runs in
//! this process while the variables change.

use athanor_i18n::{is_rtl, Catalog, OVERRIDE_VARIABLE};

/// The direction is the one the program's own catalog gives: by the override file when one is
/// named, else by the catalog of the session's language, and left to right when that language
/// has none; a catalog that cannot be read is an error, which names its file.
#[test]
fn the_direction_is_the_one_of_the_catalog_the_program_loads() {
    let ar = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ar.mo");
    let bar = |domain: &str| Catalog::load(domain).expect("a catalog").is_rtl();
    std::env::set_var(OVERRIDE_VARIABLE, ar);
    assert_eq!(is_rtl("athanor-bar"), Ok(true), "the override's Arabic");
    assert!(bar("athanor-bar"));

    let unreadable = |path: &str| {
        std::env::set_var(OVERRIDE_VARIABLE, path);
        assert!(
            Catalog::load("athanor-bar").is_err(),
            "{path} is no catalog"
        );
        is_rtl("athanor-bar").map_err(|(named, _)| named)
    };
    let missing = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/missing.mo");
    assert_eq!(
        unreadable(missing),
        Err(missing.into()),
        "a missing override is its error"
    );
    let source = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ar.po");
    assert_eq!(
        unreadable(source),
        Err(source.into()),
        "an override that is not a .mo is its error"
    );

    std::env::remove_var(OVERRIDE_VARIABLE);
    std::env::remove_var("LC_ALL");
    std::env::remove_var("LC_MESSAGES");
    std::env::set_var("LANG", "he_IL.UTF-8");
    let none = "athanor-i18n-test-no-catalog";
    assert_eq!(is_rtl(none), Ok(false), "Hebrew with no Hebrew catalog is English");
    assert!(!bar(none));
}
