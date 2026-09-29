#![allow(clippy::all, warnings)]
//! Athanor Dock - Executable Main Entrypoint (Fase 13)
//!
//! Visual Dock and taskbar application for Athanor OS.
//! Anchors to desktop shell edge via `gtk4-layer-shell`, injects Glassmorphism
//! styling via `athanor_style::glass::load_glass_theme()`, and listens for ECS / zero-copy IPC events.

use anyhow::Result;
use gtk4::prelude::*;
use gtk4::Application;
use gtk4_layer_shell::Edge;

mod controller;
mod dock;
mod dock_config;
mod dock_data;
mod dock_engine;
mod dock_watcher;
mod preview_popup;
mod ui;

fn main() -> Result<()> {
    // 1. Inietta il design "Glassmorphism"
    athanor_style::glass::load_glass_theme();

    let app = Application::builder()
        .application_id("org.athanor.dock")
        .build();

    app.connect_activate(|app| {
        // 2. Ancoraggio taskbar via gtk4-layer-shell ed ECS integration
        match dock::DockTaskbar::new(app, Edge::Bottom) {
            Ok(dock_taskbar) => {
                dock_taskbar.window.present();
            }
            Err(e) => {
                eprintln!("Failed to initialize DockTaskbar: {}, falling back to full UI builder", e);
                let win = ui::build_ui(app);
                win.present();
            }
        }
    });

    app.run();
    Ok(())
}
