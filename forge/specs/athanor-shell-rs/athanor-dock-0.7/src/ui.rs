use crate::dock_config::{add_pin, load_dock_config, remove_pin, toggle_dock_mode, DockConfig};
use crate::dock_data::{reconcile_dock_items, DockItem, NiriWindowInfo, NiriWorkspaceInfo};
use crate::dock_engine::{DockEngine, DockMode};
use crate::dock_watcher::{fetch_current_niri_windows, fetch_current_workspaces, spawn_dock_watchers};
use crate::controller::DockController;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, DropControllerMotion, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, GestureClick, Image, Orientation, Popover,
};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use std::cell::RefCell;
use std::rc::Rc;

struct DockState {
    pinned: Vec<String>,
    windows: Vec<NiriWindowInfo>,
    workspaces: Vec<NiriWorkspaceInfo>,
    is_hovered: bool,
    mode: DockMode,
}

#[allow(dead_code)]
struct DockItemWidget {
    item_rc: Rc<RefCell<DockItem>>,
    button: Button,
    icon: Image,
    overlay: gtk4::Overlay,
    badge: Option<gtk4::Label>,
    indicator: GtkBox,
}


impl DockItemWidget {
    fn new(item: DockItem) -> Self {
        let btn = Button::builder().css_classes(["dock-item-btn"]).build();
        let box_inner = GtkBox::new(Orientation::Vertical, 2);
        box_inner.set_halign(Align::Center);

        let icon = Image::from_icon_name(&item.icon_name);
        icon.set_pixel_size(44);

        let overlay = gtk4::Overlay::new();
        overlay.set_child(Some(&icon));

        let badge = if item.window_ids.len() > 1 {
            let b = gtk4::Label::builder()
                .label(item.window_ids.len().to_string())
                .css_classes(["dock-instance-badge"])
                .halign(Align::End)
                .valign(Align::Start)
                .margin_top(0)
                .margin_end(0)
                .build();
            overlay.add_overlay(&b);
            Some(b)
        } else {
            None
        };
        box_inner.append(&overlay);

        let indicator = GtkBox::new(Orientation::Horizontal, 0);
        indicator.set_halign(Align::Center);
        update_indicator_style(&indicator, &item);
        box_inner.append(&indicator);
        btn.set_child(Some(&box_inner));

        let voice_text = format_voice_text(&item);
        btn.set_tooltip_text(Some(&voice_text));

        let item_rc = Rc::new(RefCell::new(item));

        let item_c1 = item_rc.clone();
        let btn_c1 = btn.clone();
        btn.connect_clicked(move |_| {
            let it = item_c1.borrow();
            if it.window_ids.len() == 1 {
                let win_id = it.window_ids[0];
                DockController::focus_window(win_id);
            } else if it.window_ids.len() > 1 {
                show_window_picker_popover(&btn_c1, &it);
            } else {
                DockController::launch_app(&it.key_id);
            }
        });

        let gesture_right = GestureClick::new();
        gesture_right.set_button(3);
        let item_c2 = item_rc.clone();
        let btn_c2 = btn.clone();
        gesture_right.connect_released(move |_, _, _, _| {
            let it = item_c2.borrow();
            let current_mode = load_dock_config().mode;
            show_dock_context_menu(&btn_c2, &it, current_mode);
        });
        btn.add_controller(gesture_right);

        let gesture_middle = GestureClick::new();
        gesture_middle.set_button(2);
        let item_c3 = item_rc.clone();
        gesture_middle.connect_released(move |_, _, _, _| {
            let it = item_c3.borrow();
            DockController::launch_app(&it.key_id);
        });
        btn.add_controller(gesture_middle);

        let scroll_ctrl = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        let item_c4 = item_rc.clone();
        scroll_ctrl.connect_scroll(move |_, _, dy| {
            let win_ids = item_c4.borrow().window_ids.clone();
            if !win_ids.is_empty() {
                let idx = if dy > 0.0 { 0 } else { win_ids.len() - 1 };
                let win_id = win_ids[idx];
                DockController::focus_window(win_id);
            }
            glib::Propagation::Stop
        });
        btn.add_controller(scroll_ctrl);

        let drop_ctrl = DropControllerMotion::new();
        let btn_c5 = btn.clone();
        drop_ctrl.connect_enter(move |_, _, _| {
            btn_c5.add_css_class("aura-active");
        });
        let btn_c6 = btn.clone();
        drop_ctrl.connect_leave(move |_| {
            btn_c6.remove_css_class("aura-active");
        });
        btn.add_controller(drop_ctrl);

        crate::preview_popup::attach_hover_preview(&btn, item_rc.clone());

        DockItemWidget {
            item_rc,
            button: btn,
            icon,
            overlay,
            badge,
            indicator,
        }
    }

    fn update(&mut self, new_item: DockItem) {
        if *self.item_rc.borrow() == new_item {
            return;
        }

        let voice_text = format_voice_text(&new_item);
        self.button.set_tooltip_text(Some(&voice_text));

        if new_item.window_ids.len() > 1 {
            let label_str = new_item.window_ids.len().to_string();
            if let Some(ref badge_label) = self.badge {
                badge_label.set_label(&label_str);
            } else {
                let b = gtk4::Label::builder()
                    .label(&label_str)
                    .css_classes(["dock-instance-badge"])
                    .halign(Align::End)
                    .valign(Align::Start)
                    .margin_top(0)
                    .margin_end(0)
                    .build();
                self.overlay.add_overlay(&b);
                self.badge = Some(b);
            }
        } else if let Some(badge_label) = self.badge.take() {
            self.overlay.remove_overlay(&badge_label);
        }

        update_indicator_style(&self.indicator, &new_item);
        *self.item_rc.borrow_mut() = new_item;
    }
}

fn update_indicator_style(indicator: &GtkBox, item: &DockItem) {
    indicator.remove_css_class("dock-indicator-focused");
    indicator.remove_css_class("dock-indicator");
    if item.is_focused {
        indicator.add_css_class("dock-indicator-focused");
        indicator.set_opacity(1.0);
        indicator.set_size_request(-1, -1);
    } else if !item.window_ids.is_empty() {
        indicator.add_css_class("dock-indicator");
        indicator.set_opacity(1.0);
        indicator.set_size_request(-1, -1);
    } else {
        indicator.set_opacity(0.0);
        indicator.set_size_request(4, 4);
    }
}

fn format_voice_text(item: &DockItem) -> String {
    let clean_id = item.key_id.replace(".desktop", "").replace("org.", "").replace("com.", "").replace("gnome.", "");
    if item.window_ids.is_empty() {
        format!("App, {}", clean_id)
    } else {
        format!("{}, {} finestre aperte", clean_id, item.window_ids.len())
    }
}

#[allow(dead_code)]
struct DockMonitorInstance {
    monitor_connector: String,
    screen_height: i32,
    window: glib::WeakRef<ApplicationWindow>,
    container: GtkBox,
    trigger_win: glib::WeakRef<ApplicationWindow>,
    state: Rc<RefCell<DockState>>,
    engine: Rc<RefCell<DockEngine>>,
    widgets: Vec<DockItemWidget>,
    separator: Option<gtk4::Separator>,
}

thread_local! {
    static DOCK_INSTANCES: RefCell<Vec<DockMonitorInstance>> = const { RefCell::new(Vec::new()) };
}

pub fn apply_dock_mode_layout(window: &ApplicationWindow, container: &GtkBox, mode: DockMode) {
    container.remove_css_class("dock-container-fashion");
    container.remove_css_class("dock-container-efficient");

    match mode {
        DockMode::Fashion => {
            window.set_anchor(Edge::Left, false);
            window.set_anchor(Edge::Right, false);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Bottom, 12);
            container.add_css_class("dock-container-fashion");
            container.set_halign(Align::Center);
        }
        DockMode::Efficient => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Bottom, 0);
            container.add_css_class("dock-container-efficient");
            container.set_halign(Align::Fill);
        }
    }
}

pub fn animate_dock_visibility(container: &GtkBox, hide: bool) {
    if hide {
        if !container.has_css_class("dock-hidden") {
            container.add_css_class("dock-hidden");
        }
    } else {
        if container.has_css_class("dock-hidden") {
            container.remove_css_class("dock-hidden");
        }
    }
}

pub fn toggle_dock_visibility() {
    DOCK_INSTANCES.with(|insts| {
        for inst in insts.borrow_mut().iter_mut() {
            let container = &inst.container;
            let is_hidden = container.has_css_class("dock-hidden");
            animate_dock_visibility(container, !is_hidden);
        }
    });
}

fn should_autohide_for_monitor(state: &DockState, monitor_connector: &str, screen_height: i32) -> bool {
    let target_ws_id = match state.workspaces.iter().find_map(|ws| {
        if ws.output.as_deref() == Some(monitor_connector) && (ws.is_active || ws.is_focused) {
            Some(ws.id)
        } else {
            None
        }
    }) {
        Some(id) => id,
        None => {
            match state.workspaces.iter().find(|ws| ws.is_focused || ws.is_active) {
                Some(ws) => ws.id,
                None => return false,
            }
        }
    };

    let overlap_threshold = (screen_height as f64) - 85.0;

    state.windows.iter().any(|w| {
        if w.workspace_id != Some(target_ws_id) {
            return false;
        }
        if let Some(layout) = &w.layout {
            let y = layout.tile_pos_in_workspace_view.map(|p| p.1).unwrap_or(0.0);
            let h = layout.window_size.map(|s| s.1).unwrap_or(0.0);
            if (y + h) >= overlap_threshold {
                return true;
            }
        }
        w.is_focused
    })
}

#[allow(deprecated)]
#[allow(dead_code)]
pub fn build_ui(app: &Application) -> ApplicationWindow {
    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => {
            return create_dock_for_monitor(
                app,
                None,
                &load_dock_config(),
                &fetch_current_niri_windows(),
                &fetch_current_workspaces(),
            );
        }
    };
    athanor_style::load_glass_theme();

    let (tx_win, rx_win) = glib::MainContext::channel::<Vec<NiriWindowInfo>>(glib::Priority::DEFAULT);
    let (tx_cfg, rx_cfg) = glib::MainContext::channel::<DockConfig>(glib::Priority::DEFAULT);
    let (tx_ws, rx_ws) = glib::MainContext::channel::<Vec<NiriWorkspaceInfo>>(glib::Priority::DEFAULT);

    spawn_dock_watchers(tx_win, tx_cfg, tx_ws);

    let initial_config = load_dock_config();
    let initial_windows = fetch_current_niri_windows();
    let initial_workspaces = fetch_current_workspaces();

    let monitors = display.monitors();
    let mut first_window: Option<ApplicationWindow> = None;

    DOCK_INSTANCES.with(|instances| {
        instances.borrow_mut().clear();
    });

    for i in 0..monitors.n_items() {
        if let Some(monitor) = monitors.item(i).and_downcast::<gtk4::gdk::Monitor>() {
            let win = create_dock_for_monitor(
                app,
                Some(&monitor),
                &initial_config,
                &initial_windows,
                &initial_workspaces,
            );
            if first_window.is_none() {
                first_window = Some(win);
            }
        }
    }

    if first_window.is_none() {
        let win = create_dock_for_monitor(
            app,
            None,
            &initial_config,
            &initial_windows,
            &initial_workspaces,
        );
        first_window = Some(win);
    }

    rx_win.attach(None, move |windows| {
        DOCK_INSTANCES.with(|insts| {
            for inst in insts.borrow_mut().iter_mut() {
                if inst.state.borrow().windows != windows {
                    inst.state.borrow_mut().windows = windows.clone();
                    refresh_monitor_instance(inst);
                }
            }
        });
        glib::ControlFlow::Continue
    });

    rx_cfg.attach(None, move |cfg| {
        DOCK_INSTANCES.with(|insts| {
            for inst in insts.borrow_mut().iter_mut() {
                let mut state = inst.state.borrow_mut();
                let pinned_changed = state.pinned != cfg.pinned;
                let mode_changed = state.mode != cfg.mode;
                if pinned_changed || mode_changed {
                    state.pinned = cfg.pinned.clone();
                    state.mode = cfg.mode;
                    drop(state);
                    refresh_monitor_instance(inst);
                }
            }
        });
        glib::ControlFlow::Continue
    });

    rx_ws.attach(None, move |workspaces| {
        DOCK_INSTANCES.with(|insts| {
            for inst in insts.borrow_mut().iter_mut() {
                if inst.state.borrow().workspaces != workspaces {
                    inst.state.borrow_mut().workspaces = workspaces.clone();
                    refresh_monitor_instance(inst);
                }
            }
        });
        glib::ControlFlow::Continue
    });

    first_window.unwrap_or_else(|| {
        create_dock_for_monitor(
            app,
            None,
            &initial_config,
            &initial_windows,
            &initial_workspaces,
        )
    })
}

fn create_dock_for_monitor(
    app: &Application,
    monitor: Option<&gtk4::gdk::Monitor>,
    initial_config: &DockConfig,
    initial_windows: &[NiriWindowInfo],
    initial_workspaces: &[NiriWorkspaceInfo],
) -> ApplicationWindow {
    let connector = monitor
        .and_then(|m| m.connector())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "DP-1".to_string());
    let screen_height = monitor.map(|m| m.geometry().height()).unwrap_or(1080);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(format!("Athanor Dock ({})", connector))
        .css_classes(["dock-window"])
        .build();

    window.init_layer_shell();
    if let Some(m) = monitor {
        window.set_monitor(m);
    }
    window.set_layer(Layer::Top);
    window.set_namespace("dock");

    let container = GtkBox::new(Orientation::Horizontal, 8);
    container.add_css_class("dock-container");
    container.set_halign(Align::Center);
    container.set_valign(Align::Center);
    container.set_size_request(64, 48);
    window.set_child(Some(&container));

    apply_dock_mode_layout(&window, &container, initial_config.mode);

    let trigger_win = ApplicationWindow::builder()
        .application(app)
        .title(format!("Athanor Dock Trigger ({})", connector))
        .css_classes(["dock-window"])
        .build();

    trigger_win.init_layer_shell();
    if let Some(m) = monitor {
        trigger_win.set_monitor(m);
    }
    trigger_win.set_namespace("dock-trigger");
    trigger_win.set_layer(Layer::Overlay);
    trigger_win.set_exclusive_zone(-1);
    trigger_win.set_anchor(Edge::Bottom, true);
    trigger_win.set_anchor(Edge::Left, true);
    trigger_win.set_anchor(Edge::Right, true);
    trigger_win.set_height_request(6);

    let trigger_box = GtkBox::new(Orientation::Horizontal, 0);
    trigger_box.set_hexpand(true);
    trigger_box.set_vexpand(true);
    trigger_box.add_css_class("dock-trigger-area");
    trigger_win.set_child(Some(&trigger_box));

    let state = Rc::new(RefCell::new(DockState {
        pinned: initial_config.pinned.clone(),
        windows: initial_windows.to_vec(),
        workspaces: initial_workspaces.to_vec(),
        is_hovered: false,
        mode: initial_config.mode,
    }));

    let engine = Rc::new(RefCell::new(DockEngine::new(initial_config.mode)));

    let motion_trigger = EventControllerMotion::new();
    let container_weak = container.downgrade();
    let window_weak = window.downgrade();
    let state_trig = state.clone();
    motion_trigger.connect_enter(move |_, _, _| {
        state_trig.borrow_mut().is_hovered = true;
        if let Some(cont) = container_weak.upgrade() {
            animate_dock_visibility(&cont, false);
        }
        if let Some(win) = window_weak.upgrade() {
            win.present();
        }
    });
    trigger_box.add_controller(motion_trigger);

    let motion_trig_win = EventControllerMotion::new();
    let container_weak_win = container.downgrade();
    let window_weak_win = window.downgrade();
    let state_trig_win = state.clone();
    motion_trig_win.connect_enter(move |_, _, _| {
        state_trig_win.borrow_mut().is_hovered = true;
        if let Some(cont) = container_weak_win.upgrade() {
            animate_dock_visibility(&cont, false);
        }
        if let Some(win) = window_weak_win.upgrade() {
            win.present();
        }
    });
    trigger_win.add_controller(motion_trig_win);

    let motion_dock_enter = EventControllerMotion::new();
    let container_weak_enter = container.downgrade();
    let state_enter = state.clone();
    motion_dock_enter.connect_enter(move |_, _, _| {
        state_enter.borrow_mut().is_hovered = true;
        if let Some(cont) = container_weak_enter.upgrade() {
            animate_dock_visibility(&cont, false);
        }
    });
    container.add_controller(motion_dock_enter);

    let motion_dock_hover = EventControllerMotion::new();
    let engine_hover = engine.clone();
    let container_weak_hover = container.downgrade();
    motion_dock_hover.connect_motion(move |_, x, _| {
        let mut eng = engine_hover.borrow_mut();
        if eng.mode.is_fashion() {
            let mut centers = Vec::new();
            if let Some(cont) = container_weak_hover.upgrade() {
                let mut child = cont.first_child();
                while let Some(c) = child {
                    if c.has_css_class("dock-item-btn") {
                        let alloc = c.allocation();
                        let cx = alloc.x() as f64 + (alloc.width() as f64) / 2.0;
                        centers.push(cx);
                    }
                    child = c.next_sibling();
                }
            }
            eng.update_cursor(Some(x), &centers);
        }
    });
    container.add_controller(motion_dock_hover);

    let motion_dock_leave = EventControllerMotion::new();
    let container_weak_leave = container.downgrade();
    let state_leave = state.clone();
    let engine_leave = engine.clone();
    let connector_clone = connector.clone();
    motion_dock_leave.connect_leave(move |_| {
        state_leave.borrow_mut().is_hovered = false;
        engine_leave.borrow_mut().update_cursor(None, &[]);
        let cont_weak = container_weak_leave.clone();
        let st = state_leave.clone();
        let conn = connector_clone.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            if let Some(cont) = cont_weak.upgrade() {
                if !st.borrow().is_hovered && should_autohide_for_monitor(&st.borrow(), &conn, screen_height) {
                    animate_dock_visibility(&cont, true);
                }
            }
            glib::ControlFlow::Break
        });
    });
    container.add_controller(motion_dock_leave);

    let motion_trig_leave = EventControllerMotion::new();
    let container_weak_trig_leave = container.downgrade();
    let state_trig_leave = state.clone();
    let connector_clone2 = connector.clone();
    motion_trig_leave.connect_leave(move |_| {
        state_trig_leave.borrow_mut().is_hovered = false;
        let cont_weak = container_weak_trig_leave.clone();
        let st = state_trig_leave.clone();
        let conn = connector_clone2.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            if let Some(cont) = cont_weak.upgrade() {
                if !st.borrow().is_hovered && should_autohide_for_monitor(&st.borrow(), &conn, screen_height) {
                    animate_dock_visibility(&cont, true);
                }
            }
            glib::ControlFlow::Break
        });
    });
    trigger_box.add_controller(motion_trig_leave);

    let engine_tick = engine.clone();
    let container_weak_tick = container.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        let mut eng = engine_tick.borrow_mut();
        let scales = eng.step_physics(0.016);

        if let Some(cont) = container_weak_tick.upgrade() {
            let mut idx = 0;
            let mut child = cont.first_child();
            while let Some(c) = child {
                if c.has_css_class("dock-item-btn") {
                    if let Some(&scale) = scales.get(idx) {
                        if let Some(btn) = c.downcast_ref::<Button>() {
                            if let Some(box_inner) = btn.child().and_then(|w| w.downcast::<GtkBox>().ok()) {
                                if let Some(overlay) = box_inner.first_child().and_then(|w| w.downcast::<gtk4::Overlay>().ok()) {
                                    if let Some(img) = overlay.child().and_then(|w| w.downcast::<Image>().ok()) {
                                        img.set_pixel_size((44.0 * scale).round() as i32);
                                    }
                                }
                            }
                        }
                    }
                    idx += 1;
                }
                child = c.next_sibling();
            }
        }

        glib::ControlFlow::Continue
    });

    trigger_win.present();

    let mut inst = DockMonitorInstance {
        monitor_connector: connector,
        screen_height,
        window: window.downgrade(),
        container: container.clone(),
        trigger_win: trigger_win.downgrade(),
        state,
        engine,
        widgets: Vec::new(),
        separator: None,
    };
    refresh_monitor_instance(&mut inst);
    window.present();

    DOCK_INSTANCES.with(|instances| {
        instances.borrow_mut().push(inst);
    });

    window
}

fn refresh_monitor_instance(inst: &mut DockMonitorInstance) {
    let state = inst.state.borrow();
    let mode = state.mode;
    inst.engine.borrow_mut().set_mode(mode);

    if let Some(win) = inst.window.upgrade() {
        apply_dock_mode_layout(&win, &inst.container, mode);
    }

    let new_items = reconcile_dock_items(&state.pinned, &state.windows);
    let is_hovered = state.is_hovered;

    let keys_match = inst.widgets.len() == new_items.len()
        && inst.widgets.iter().zip(&new_items).all(|(w, item)| w.item_rc.borrow().key_id == item.key_id);

    if keys_match {
        for (w, item) in inst.widgets.iter_mut().zip(new_items) {
            w.update(item);
        }
    } else {
        use std::collections::HashMap;
        let mut old_widgets: HashMap<String, DockItemWidget> = inst
            .widgets
            .drain(..)
            .map(|w| {
                let key_id = w.item_rc.borrow().key_id.clone();
                (key_id, w)
            })
            .collect();

        let mut updated_widgets = Vec::with_capacity(new_items.len());
        for item in new_items {
            if let Some(mut existing) = old_widgets.remove(&item.key_id) {
                existing.update(item);
                updated_widgets.push(existing);
            } else {
                updated_widgets.push(DockItemWidget::new(item));
            }
        }

        while let Some(child) = inst.container.first_child() {
            inst.container.remove(&child);
        }

        let mut added_unpinned_separator = false;
        for w in &updated_widgets {
            if !w.item_rc.borrow().is_pinned && !added_unpinned_separator {
                if inst.separator.is_none() {
                    let sep = gtk4::Separator::new(Orientation::Vertical);
                    sep.set_margin_top(8);
                    sep.set_margin_bottom(8);
                    inst.separator = Some(sep);
                }
                if let Some(ref sep) = inst.separator {
                    inst.container.append(sep);
                }
                added_unpinned_separator = true;
            }
            inst.container.append(&w.button);
        }

        inst.widgets = updated_widgets;
    }

    let should_hide = !is_hovered && should_autohide_for_monitor(&state, &inst.monitor_connector, inst.screen_height);
    animate_dock_visibility(&inst.container, should_hide);
}

fn show_window_picker_popover(anchor: &Button, item: &DockItem) {
    let popover = Popover::builder()
        .autohide(true)
        .css_classes(["dock-popover"])
        .build();
    popover.set_parent(anchor);
    popover.connect_closed(|p| {
        p.set_child(None::<&gtk4::Widget>);
        p.unparent();
    });

    let box_inner = GtkBox::new(Orientation::Vertical, 4);
    for (i, title) in item.window_titles.iter().enumerate() {
        let win_id = item.window_ids[i];
        let btn = Button::builder()
            .label(title)
            .css_classes(["dock-popover-btn"])
            .build();
        let pop_close = popover.clone();
        btn.connect_clicked(move |_| {
            DockController::focus_window(win_id);
            pop_close.popdown();
        });
        box_inner.append(&btn);
    }
    popover.set_child(Some(&box_inner));
    popover.popup();
}

fn show_dock_context_menu(anchor: &Button, item: &DockItem, mode: DockMode) {
    let popover = Popover::builder()
        .autohide(true)
        .css_classes(["dock-popover"])
        .build();
    popover.set_parent(anchor);
    popover.connect_closed(|p| {
        p.set_child(None::<&gtk4::Widget>);
        p.unparent();
    });

    let box_inner = GtkBox::new(Orientation::Vertical, 4);

    let pin_label = if item.is_pinned {
        "Rimuovi dalla Dock"
    } else {
        "Fissa nella Dock"
    };
    let btn_pin = Button::builder()
        .label(pin_label)
        .css_classes(["dock-popover-btn"])
        .build();
    let key_id = item.key_id.clone();
    let is_pinned = item.is_pinned;
    let pop_close = popover.clone();
    btn_pin.connect_clicked(move |_| {
        if is_pinned {
            let _ = remove_pin(&key_id);
        } else {
            let _ = add_pin(&key_id);
        }
        pop_close.popdown();
    });
    box_inner.append(&btn_pin);

    let btn_new = Button::builder()
        .label("Nuova Finestra")
        .css_classes(["dock-popover-btn"])
        .build();
    let key_id2 = item.key_id.clone();
    let pop_close2 = popover.clone();
    btn_new.connect_clicked(move |_| {
        DockController::launch_app(&key_id2);
        pop_close2.popdown();
    });
    box_inner.append(&btn_new);

    if !item.window_ids.is_empty() {
        let btn_close = Button::builder()
            .label("Chiudi finestre")
            .css_classes(["dock-popover-btn"])
            .build();
        let win_ids = item.window_ids.clone();
        let pop_close3 = popover.clone();
        btn_close.connect_clicked(move |_| {
            for id in &win_ids {
                DockController::close_window(*id);
            }
            pop_close3.popdown();
        });
        box_inner.append(&btn_close);
    }

    let mode_label = if mode.is_fashion() {
        "Passa a Modalità Efficiente (Taskbar)"
    } else {
        "Passa a Modalità Fashion (Pill Floating)"
    };
    let btn_mode = Button::builder()
        .label(mode_label)
        .css_classes(["dock-popover-btn"])
        .build();
    let pop_close_m = popover.clone();
    btn_mode.connect_clicked(move |_| {
        let _ = toggle_dock_mode();
        pop_close_m.popdown();
    });
    box_inner.append(&btn_mode);

    let sep1 = gtk4::Separator::new(Orientation::Horizontal);
    sep1.set_margin_top(4);
    sep1.set_margin_bottom(4);
    box_inner.append(&sep1);

    let btn_settings = Button::builder()
        .label("Impostazioni")
        .css_classes(["dock-popover-btn"])
        .build();
    let pop_close_s = popover.clone();
    btn_settings.connect_clicked(move |_| {
        DockController::launch_app("os.athanor.Settings.desktop");
        pop_close_s.popdown();
    });
    box_inner.append(&btn_settings);

    let btn_sysmon = Button::builder()
        .label("Monitor di Sistema")
        .css_classes(["dock-popover-btn"])
        .build();
    let pop_close_sm = popover.clone();
    btn_sysmon.connect_clicked(move |_| {
        DockController::launch_app("missioncenter.desktop");
        pop_close_sm.popdown();
    });
    box_inner.append(&btn_sysmon);

    popover.set_child(Some(&box_inner));
    popover.popup();
}
