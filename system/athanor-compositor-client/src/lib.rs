//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod connection;
mod cosmic_config;
pub mod favorites;
mod keymap;
mod launch;
pub mod model;
pub mod outputs;
mod protocols;
pub mod theme;
mod unit;

pub use connection::{Client, Error};
pub use launch::{LaunchError, Opener};
pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
