//! The logic of Athanor's Settings with no toolkit type (doc_settings.md SE2): the page registry
//! and its search, the deep-link grammar, the window state that survives a restart, the
//! writable GSettings keys with their validation, the display-revert state machine, the
//! shortcut-conflict check and the About page's parsers. Its tests run without a display.

pub mod about;
pub mod i18n;
pub mod registry;
pub mod revert;
pub mod shortcuts;
pub mod target;
pub mod window_state;
pub mod writes;
