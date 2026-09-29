//! Athanor's style crate. `calmo` is the identity; the three modules below are the
//! pre-Calmo glass theme, kept only because athanor-recovery still loads it. They
//! predate the lint gate and keep their warnings silenced until recovery is re-skinned
//! and they are deleted (doc_shell.md, SH4).

pub mod calmo;

#[allow(clippy::all, warnings)]
pub mod accent_engine;
#[allow(clippy::all, warnings)]
pub mod appearance_engine;
#[allow(clippy::all, warnings)]
pub mod glass;

pub use accent_engine::*;
pub use appearance_engine::*;
pub use glass::*;
