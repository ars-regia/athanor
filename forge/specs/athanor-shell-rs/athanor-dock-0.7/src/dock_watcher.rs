use crate::dock_config::{get_dock_config_path, load_dock_config, DockConfig};
use crate::dock_data::{NiriWindowInfo, NiriWorkspaceInfo};
use athanor_niri_ipc::async_client as niri_client;
use notify::{RecursiveMode, Watcher};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

pub fn get_runtime() -> std::io::Result<&'static Runtime> {
    match RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to initialize shared Tokio runtime for athanor-dock: {}", e))
    }) {
        Ok(rt) => Ok(rt),
        Err(err_msg) => Err(std::io::Error::other(err_msg.clone())),
    }
}

pub fn fetch_current_niri_windows() -> Vec<NiriWindowInfo> {
    match get_runtime() {
        Ok(rt) => rt.block_on(async {
            niri_client::fetch_niri_data::<Vec<NiriWindowInfo>>("Windows", "Windows").await.unwrap_or_default()
        }),
        Err(e) => {
            eprintln!("[athanor-dock] Runtime initialization error: {}", e);
            Vec::new()
        }
    }
}

pub fn fetch_current_workspaces() -> Vec<NiriWorkspaceInfo> {
    match get_runtime() {
        Ok(rt) => rt.block_on(async {
            niri_client::fetch_niri_data::<Vec<NiriWorkspaceInfo>>("Workspaces", "Workspaces").await.unwrap_or_default()
        }),
        Err(e) => {
            eprintln!("[athanor-dock] Runtime initialization error: {}", e);
            Vec::new()
        }
    }
}

pub fn fetch_current_active_workspace_id() -> Option<u64> {
    let workspaces = fetch_current_workspaces();
    if let Some(focused) = workspaces.iter().find(|w| w.is_focused) {
        return Some(focused.id);
    }
    workspaces.into_iter().find(|w| w.is_active).map(|w| w.id)
}

pub fn spawn_dock_watchers(
    sender_windows: glib::Sender<Vec<NiriWindowInfo>>,
    sender_config: glib::Sender<DockConfig>,
    sender_workspaces: glib::Sender<Vec<NiriWorkspaceInfo>>,
) {
    let _ = sender_windows.send(fetch_current_niri_windows());
    let _ = sender_config.send(load_dock_config());
    let _ = sender_workspaces.send(fetch_current_workspaces());

    let win_sender = sender_windows.clone();
    let ws_sender = sender_workspaces.clone();
    niri_client::watch_niri_event_stream(move |line| {
        if line.contains("Window") || line.contains("Workspace") {
            let win_sender = win_sender.clone();
            let ws_sender = ws_sender.clone();
            if let Ok(rt) = get_runtime() {
                rt.spawn(async move {
                    let windows = niri_client::fetch_niri_data::<Vec<NiriWindowInfo>>("Windows", "Windows").await.unwrap_or_default();
                    let _ = win_sender.send(windows);
                    let workspaces = niri_client::fetch_niri_data::<Vec<NiriWorkspaceInfo>>("Workspaces", "Workspaces").await.unwrap_or_default();
                    let _ = ws_sender.send(workspaces);
                });
            } else {
                eprintln!("[athanor-dock] Runtime initialization error in watch_niri_event_stream");
            }
        }
    });

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        let path = get_dock_config_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!("Failed to create directory {:?}: {:?}", parent, e);
            }
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        while let Ok(event) = rx.recv() {
            if let Ok(ev) = event {
                if (ev.kind.is_modify() || ev.kind.is_create())
                    && ev.paths.iter().any(|p| p.file_name() == path.file_name())
                {
                    let _ = sender_config.send(load_dock_config());
                }
            }
        }
    });
}
