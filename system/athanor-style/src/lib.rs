//! Athanor's style crate. `calmo` is the identity; the three modules below are the
//! pre-Calmo glass theme, kept only because athanor-recovery still loads it. They
//! predate the lint gate and keep their warnings silenced until recovery is re-skinned
//! and they are deleted (doc_shell.md, SH4).
//!
//! Without the `gtk` feature only `appearance` and `variant` remain: what a surface drawn
//! without GTK reads.

pub mod appearance;
#[cfg(feature = "gtk")]
pub mod calmo;
pub mod variant;

#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod accent_engine;
#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod appearance_engine;
#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod glass;

#[cfg(feature = "gtk")]
pub use accent_engine::*;
#[cfg(feature = "gtk")]
pub use appearance_engine::*;
#[cfg(feature = "gtk")]
pub use glass::*;
