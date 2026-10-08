use gtk4::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{error, info, warn};
use zbus::interface;

/// Represents the physical position of a panel or dock
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PanelPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

/// Widget layout within a shell panel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WidgetLayout {
    pub start_widgets: Vec<String>,
    pub center_widgets: Vec<String>,
    pub end_widgets: Vec<String>,
}

/// GTK Layer Shell panel layout configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PanelConfig {
    pub position: PanelPosition,
    pub height_or_width: i32,
    pub anchors: Vec<String>,
    pub exclusive_zone: bool,
    pub autohide: bool,
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    pub widget_layout: WidgetLayout,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            position: PanelPosition::Top,
            height_or_width: 28,
            anchors: vec!["top".to_string(), "left".to_string(), "right".to_string()],
            exclusive_zone: true,
            autohide: false,
            margin_top: 0,
            margin_bottom: 0,
            margin_left: 0,
            margin_right: 0,
            widget_layout: WidgetLayout::default(),
        }
    }
}

/// Dock layout configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockConfig {
    pub enabled: bool,
    pub position: PanelPosition,
    pub floating: bool,
    pub autohide: bool,
    pub icon_size: i32,
    pub anchors: Vec<String>,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: PanelPosition::Bottom,
            floating: true,
            autohide: false,
            icon_size: 56,
            anchors: vec!["bottom".to_string()],
        }
    }
}

/// Desktop widgets configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopWidgetsConfig {
    pub enabled: bool,
    pub layout_grid: String,
}

impl Default for DesktopWidgetsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            layout_grid: "classic".to_string(),
        }
    }
}

/// Complete Zorin-style layout preset representation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayoutPreset {
    pub name: String,
    pub description: String,
    pub style_variant: String,
    pub panel: PanelConfig,
    pub dock: DockConfig,
    pub desktop_widgets: DesktopWidgetsConfig,
}

impl LayoutPreset {
    /// Built-in preset: Windows Classic (Bottom Taskbar layout)
    pub fn windows_classic() -> Self {
        Self {
            name: "windows-classic".to_string(),
            description: "Traditional bottom taskbar layout with start menu and system tray"
                .to_string(),
            style_variant: "windows-classic".to_string(),
            panel: PanelConfig {
                position: PanelPosition::Bottom,
                height_or_width: 44,
                anchors: vec![
                    "bottom".to_string(),
                    "left".to_string(),
                    "right".to_string(),
                ],
                exclusive_zone: true,
                autohide: false,
                margin_top: 0,
                margin_bottom: 0,
                margin_left: 0,
                margin_right: 0,
                widget_layout: WidgetLayout {
                    start_widgets: vec!["start_menu".to_string(), "window_list".to_string()],
                    center_widgets: vec!["workspaces".to_string()],
                    end_widgets: vec![
                        "system_tray".to_string(),
                        "clock".to_string(),
                        "notifications".to_string(),
                        "powermenu".to_string(),
                    ],
                },
            },
            dock: DockConfig {
                enabled: false,
                position: PanelPosition::Bottom,
                floating: false,
                autohide: false,
                icon_size: 48,
                anchors: vec!["bottom".to_string()],
            },
            desktop_widgets: DesktopWidgetsConfig {
                enabled: true,
                layout_grid: "classic".to_string(),
            },
        }
    }

    /// Built-in preset: macOS Glass (Top Menu Bar + Bottom Dock layout)
    pub fn macos_glass() -> Self {
        Self {
            name: "macos-glass".to_string(),
            description: "Top menu bar with dynamic morphic pill and bottom centered floating dock"
                .to_string(),
            style_variant: "macos-glass".to_string(),
            panel: PanelConfig {
                position: PanelPosition::Top,
                height_or_width: 28,
                anchors: vec!["top".to_string(), "left".to_string(), "right".to_string()],
                exclusive_zone: true,
                autohide: false,
                margin_top: 0,
                margin_bottom: 0,
                margin_left: 0,
                margin_right: 0,
                widget_layout: WidgetLayout {
                    start_widgets: vec!["apple_menu".to_string(), "focused_app_title".to_string()],
                    center_widgets: vec!["workspaces".to_string()],
                    end_widgets: vec![
                        "morphic_pill".to_string(),
                        "battery".to_string(),
                        "network".to_string(),
                        "spotlight".to_string(),
                        "control_center".to_string(),
                        "desktop_widgets_btn".to_string(),
                        "live_theming_btn".to_string(),
                        "notifications".to_string(),
                        "clock".to_string(),
                    ],
                },
            },
            dock: DockConfig {
                enabled: true,
                position: PanelPosition::Bottom,
                floating: true,
                autohide: false,
                icon_size: 56,
                anchors: vec!["bottom".to_string()],
            },
            desktop_widgets: DesktopWidgetsConfig {
                enabled: true,
                layout_grid: "modern".to_string(),
            },
        }
    }

    /// Built-in preset: GNOME Modern
    pub fn gnome_modern() -> Self {
        Self {
            name: "gnome-modern".to_string(),
            description:
                "Minimalist top bar with centered workspace switcher and autohiding bottom dock"
                    .to_string(),
            style_variant: "gnome-modern".to_string(),
            panel: PanelConfig {
                position: PanelPosition::Top,
                height_or_width: 32,
                anchors: vec!["top".to_string(), "left".to_string(), "right".to_string()],
                exclusive_zone: true,
                autohide: false,
                margin_top: 0,
                margin_bottom: 0,
                margin_left: 0,
                margin_right: 0,
                widget_layout: WidgetLayout {
                    start_widgets: vec!["start_menu".to_string()],
                    center_widgets: vec!["workspaces".to_string(), "clock".to_string()],
                    end_widgets: vec!["control_center".to_string(), "notifications".to_string()],
                },
            },
            dock: DockConfig {
                enabled: true,
                position: PanelPosition::Bottom,
                floating: true,
                autohide: true,
                icon_size: 48,
                anchors: vec!["bottom".to_string()],
            },
            desktop_widgets: DesktopWidgetsConfig {
                enabled: false,
                layout_grid: "minimal".to_string(),
            },
        }
    }

    /// Built-in preset: Unity Side
    pub fn unity_side() -> Self {
        Self {
            name: "unity-side".to_string(),
            description: "Vertical left dock/panel layout with integrated top status bar"
                .to_string(),
            style_variant: "unity-side".to_string(),
            panel: PanelConfig {
                position: PanelPosition::Left,
                height_or_width: 56,
                anchors: vec!["left".to_string(), "top".to_string(), "bottom".to_string()],
                exclusive_zone: true,
                autohide: false,
                margin_top: 0,
                margin_bottom: 0,
                margin_left: 0,
                margin_right: 0,
                widget_layout: WidgetLayout {
                    start_widgets: vec!["start_menu".to_string(), "window_list".to_string()],
                    center_widgets: vec!["workspaces".to_string()],
                    end_widgets: vec!["notifications".to_string(), "clock".to_string()],
                },
            },
            dock: DockConfig {
                enabled: false,
                position: PanelPosition::Left,
                floating: false,
                autohide: false,
                icon_size: 48,
                anchors: vec!["left".to_string()],
            },
            desktop_widgets: DesktopWidgetsConfig {
                enabled: true,
                layout_grid: "classic".to_string(),
            },
        }
    }
}

/// Helper TOML parser for layout presets
pub fn parse_preset_toml(toml_str: &str) -> Result<LayoutPreset, String> {
    toml::from_str::<LayoutPreset>(toml_str)
        .map_err(|e| format!("Failed to parse layout preset TOML: {}", e))
}

/// Serialize layout preset to TOML string
pub fn serialize_preset_toml(preset: &LayoutPreset) -> Result<String, String> {
    toml::to_string_pretty(preset)
        .map_err(|e| format!("Failed to serialize layout preset TOML: {}", e))
}

thread_local! {
    static TOPBAR_WINDOW: std::cell::RefCell<Option<glib::WeakRef<gtk4::ApplicationWindow>>> = const { std::cell::RefCell::new(None) };
}

/// Register topbar window for hot-swapping GTK Layer Shell parameters live
pub fn register_topbar_window(win: &gtk4::ApplicationWindow) {
    TOPBAR_WINDOW.with(|w| {
        *w.borrow_mut() = Some(win.downgrade());
    });
}

/// Global Appearance Engine state wrapper
#[derive(Debug)]
pub struct AppearanceState {
    pub active_preset: LayoutPreset,
    pub available_presets: HashMap<String, LayoutPreset>,
}

impl Default for AppearanceState {
    fn default() -> Self {
        let mut available = HashMap::new();
        let macos = LayoutPreset::macos_glass();
        let win = LayoutPreset::windows_classic();
        let gnome = LayoutPreset::gnome_modern();
        let unity = LayoutPreset::unity_side();

        available.insert(macos.name.clone(), macos.clone());
        available.insert(win.name.clone(), win);
        available.insert(gnome.name.clone(), gnome);
        available.insert(unity.name.clone(), unity);

        Self {
            active_preset: macos,
            available_presets: available,
        }
    }
}

pub struct AppearanceEngine {
    state: Arc<RwLock<AppearanceState>>,
}

impl Default for AppearanceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AppearanceEngine {
    pub fn new() -> Self {
        let state = Arc::new(RwLock::new(AppearanceState::default()));
        let engine = Self { state };
        engine.load_custom_presets_from_disk();
        engine.load_saved_active_layout();
        engine
    }

    /// Load presets stored in system or user config directories
    pub fn load_custom_presets_from_disk(&self) {
        let mut dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(
                PathBuf::from(home)
                    .join(".config")
                    .join("athanor")
                    .join("layouts"),
            );
        }
        dirs.push(PathBuf::from("/usr/share/athanor/layouts"));

        for dir in dirs {
            if dir.exists() && dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().and_then(|s| s.to_str()) == Some("toml")
                        {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(preset) = parse_preset_toml(&content) {
                                    if let Ok(mut lock) = self.state.write() {
                                        lock.available_presets.insert(preset.name.clone(), preset);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get path for storing active layout preference; none without a home, as for the presets
    fn get_active_layout_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("athanor")
                .join("active_layout.toml"),
        )
    }

    /// Load active layout from disk
    pub fn load_saved_active_layout(&self) {
        let Some(path) = Self::get_active_layout_path() else {
            return;
        };
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(preset) = parse_preset_toml(&content) {
                    if let Ok(mut lock) = self.state.write() {
                        lock.active_preset = preset;
                    }
                }
            }
        }
    }

    /// Persist active layout to disk
    fn save_active_layout(&self, preset: &LayoutPreset) {
        let Some(path) = Self::get_active_layout_path() else {
            error!("HOME is not set: the active layout is not saved");
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!("Failed to create parent dir {:?}: {:?}", parent, e);
            }
        }
        if let Ok(content) = serialize_preset_toml(preset) {
            if let Err(e) = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, content.as_bytes()))
            {
                tracing::error!("Failed to write path {:?}: {:?}", path, e);
            }
        }
    }

    /// Zero-restart live hot-swap of GTK Layer Shell panel and widgets
    pub fn apply_preset(&self, preset: LayoutPreset) -> Result<(), String> {
        info!(preset = %preset.name, "Applying Zorin-style zero-restart layout preset live.");

        // 1. Update internal state
        {
            let mut lock = self
                .state
                .write()
                .map_err(|e| format!("Poison error: {}", e))?;
            lock.active_preset = preset.clone();
            lock.available_presets
                .insert(preset.name.clone(), preset.clone());
        }

        // 2. Persist selection
        self.save_active_layout(&preset);

        // 3. Hot-swap GTK Layer Shell parameters on main thread cleanly
        let preset_clone = preset.clone();
        glib::MainContext::default().spawn_local(async move {
            hot_swap_gtk_layer_shell(&preset_clone);
        });

        Ok(())
    }

    pub fn set_layout_by_name(&self, name: &str) -> Result<bool, String> {
        let preset = {
            let lock = self
                .state
                .read()
                .map_err(|e| format!("Poison error: {}", e))?;
            lock.available_presets.get(name).cloned()
        };

        if let Some(p) = preset {
            self.apply_preset(p)?;
            Ok(true)
        } else {
            Err(format!("Layout preset '{}' not found", name))
        }
    }

    pub fn get_active_layout_name(&self) -> String {
        self.state
            .read()
            .map(|s| s.active_preset.name.clone())
            .unwrap_or_else(|_| "macos-glass".to_string())
    }

    pub fn list_preset_names(&self) -> Vec<String> {
        self.state
            .read()
            .map(|s| s.available_presets.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_layout_toml(&self, name: &str) -> Option<String> {
        let lock = self.state.read().ok()?;
        let preset = lock.available_presets.get(name)?;
        serialize_preset_toml(preset).ok()
    }
}

/// Perform GTK Layer Shell hot-swap without restarting the process
fn hot_swap_gtk_layer_shell(preset: &LayoutPreset) {
    use gtk4_layer_shell::{Edge, LayerShell};

    TOPBAR_WINDOW.with(|cell| {
        if let Some(weak_win) = cell.borrow().as_ref() {
            if let Some(win) = weak_win.upgrade() {
                let panel = &preset.panel;

                // Update Edge Anchors live
                let top = panel.anchors.iter().any(|a| a.eq_ignore_ascii_case("top"));
                let bottom = panel
                    .anchors
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case("bottom"));
                let left = panel.anchors.iter().any(|a| a.eq_ignore_ascii_case("left"));
                let right = panel
                    .anchors
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case("right"));

                win.set_anchor(Edge::Top, top);
                win.set_anchor(Edge::Bottom, bottom);
                win.set_anchor(Edge::Left, left);
                win.set_anchor(Edge::Right, right);

                // Update Margins
                win.set_margin(Edge::Top, panel.margin_top);
                win.set_margin(Edge::Bottom, panel.margin_bottom);
                win.set_margin(Edge::Left, panel.margin_left);
                win.set_margin(Edge::Right, panel.margin_right);

                // Update Dimensions
                if panel.position == PanelPosition::Left || panel.position == PanelPosition::Right {
                    win.set_width_request(panel.height_or_width);
                    win.set_height_request(-1);
                } else {
                    win.set_height_request(panel.height_or_width);
                    win.set_width_request(-1);
                }

                // Exclusive Zone
                if panel.exclusive_zone {
                    win.auto_exclusive_zone_enable();
                } else {
                    win.set_exclusive_zone(-1);
                }

                // Hot-swap CSS classes
                let existing_classes: Vec<_> = win
                    .css_classes()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                for cls in existing_classes {
                    if cls.starts_with("layout-") {
                        win.remove_css_class(&cls);
                    }
                }
                win.add_css_class(&format!("layout-{}", preset.style_variant));

                info!(
                    position = ?panel.position,
                    height_width = panel.height_or_width,
                    "Live GTK Layer Shell parameters updated cleanly."
                );
            } else {
                warn!("Topbar ApplicationWindow weak reference expired during hot-swap");
            }
        } else {
            warn!("Topbar ApplicationWindow not registered for live hot-swap");
        }
    });

    // Notify Dock toggle if dock enabled state changed
    if !preset.dock.enabled {
        let _ = gtk4::glib::spawn_command_line_async("pkill -STOP athanor-dock");
    } else {
        let _ = gtk4::glib::spawn_command_line_async("pkill -CONT athanor-dock");
    }
}

/// DBus interface implementation for `org.athanor.Shell.Layout`
pub struct LayoutDbusServer {
    engine: Arc<AppearanceEngine>,
}

impl LayoutDbusServer {
    pub fn new(engine: Arc<AppearanceEngine>) -> Self {
        Self { engine }
    }
}

#[interface(name = "org.athanor.Shell.Layout")]
impl LayoutDbusServer {
    /// Hot-swaps the layout preset by name
    async fn set_layout(&self, preset_name: &str) -> zbus::fdo::Result<bool> {
        if preset_name.len() > 256 {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Preset name exceeds 256 byte limit".into(),
            ));
        }
        info!(preset_name = %preset_name, "DBus org.athanor.Shell.Layout.SetLayout called");
        match self.engine.set_layout_by_name(preset_name) {
            Ok(res) => Ok(res),
            Err(err) => {
                error!(error = %err, "Failed DBus SetLayout");
                Err(zbus::fdo::Error::Failed(err))
            }
        }
    }

    /// Returns the currently active layout preset name
    async fn get_active_layout(&self) -> String {
        self.engine.get_active_layout_name()
    }

    /// List all available layout preset names
    async fn list_presets(&self) -> Vec<String> {
        self.engine.list_preset_names()
    }

    /// Get TOML definition for a layout preset
    async fn get_layout_details(&self, preset_name: &str) -> String {
        if preset_name.len() > 256 {
            return String::new();
        }
        self.engine.get_layout_toml(preset_name).unwrap_or_default()
    }

    /// Apply custom TOML layout on the fly
    async fn apply_custom_toml(&self, toml_content: &str) -> zbus::fdo::Result<bool> {
        if toml_content.len() > 1_048_576 {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Custom TOML payload exceeds 1MB limit".into(),
            ));
        }
        info!("DBus org.athanor.Shell.Layout.ApplyCustomToml called");
        match parse_preset_toml(toml_content) {
            Ok(preset) => match self.engine.apply_preset(preset) {
                Ok(()) => Ok(true),
                Err(err) => Err(zbus::fdo::Error::Failed(err)),
            },
            Err(err) => Err(zbus::fdo::Error::Failed(err)),
        }
    }
}

/// Register `org.athanor.Shell.Layout` DBus interface on session bus
pub async fn register_layout_dbus(
    connection: &zbus::Connection,
    engine: Arc<AppearanceEngine>,
) -> zbus::Result<()> {
    let server = LayoutDbusServer::new(engine);
    connection
        .object_server()
        .at("/org/athanor/Shell/Layout", server)
        .await?;
    connection.request_name("org.athanor.Shell.Layout").await?;
    info!("Registered DBus service 'org.athanor.Shell.Layout' at path '/org/athanor/Shell/Layout'");
    Ok(())
}
