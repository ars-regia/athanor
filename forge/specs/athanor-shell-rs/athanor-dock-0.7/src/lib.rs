#![allow(deprecated)]
pub mod dock_config;
pub mod dock_data;
pub mod dock_engine;
pub mod dock_watcher;
pub mod controller;
pub mod ui;
pub mod preview_popup;
pub mod dock;

pub use dock_config::*;
pub use dock_data::*;
pub use dock_engine::*;
pub use dock_watcher::*;
pub use ui::{build_ui, animate_dock_visibility, toggle_dock_visibility};
pub use preview_popup::*;
pub use dock::*;

