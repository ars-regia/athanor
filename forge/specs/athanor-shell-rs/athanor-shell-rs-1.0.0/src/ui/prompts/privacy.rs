use gtk4::prelude::*;
use gtk4::{gdk, glib, Align, Application, ApplicationWindow, Box as GtkBox, Button, Image, Label, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

/// The exit status of the Allow button, and of nothing else. The portal reads it and grants
/// only on this status: a closed window, a crash, or a second instance that forwarded its
/// request all end with 0 or a signal, and are denials. It must equal `GRANTED_EXIT_CODE`
/// in `xdg-desktop-portal-athanor` (`src/prompt.rs`); the two programs are in different
/// workspaces and cannot share the constant.
const EXIT_GRANTED: i32 = 100;
/// The exit status of a refusal: the Deny button, Escape, or the window being closed.
const EXIT_DENIED: i32 = 1;

pub fn build_ui(app: &Application, request_info: &str) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Richiesta Permessi")
        .css_classes(["privacy-window"])
        .default_width(420)
        .build();

    window.init_layer_shell();
    window.set_namespace("privacy");
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
    window.auto_exclusive_zone_enable();
    
    // Top right corner like macOS notification
    window.set_margin(Edge::Top, 16);
    window.set_margin(Edge::Right, 16);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);

    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .css_classes(["privacy-card"])
        .build();

    let parts: Vec<&str> = request_info.splitn(2, ':').collect();
    let resource = parts.first().copied().unwrap_or("Risorsa");
    let app_id = parts.get(1).copied().unwrap_or("Applicazione Sconosciuta");

    let icon_name = match resource {
        "Camera" => "camera-web-symbolic",
        "Microphone" => "audio-input-microphone-symbolic",
        "ScreenCast" => "video-display-symbolic",
        "Location" => "mark-location-symbolic",
        _ => "dialog-question-symbolic",
    };

    let icon = Image::builder()
        .icon_name(icon_name)
        .pixel_size(48)
        .css_classes(["privacy-icon"])
        .halign(Align::Center)
        .build();
    vbox.append(&icon);

    let title = Label::builder()
        .label(format!("\"{}\" desidera accedere a {}", app_id, resource))
        .css_classes(["privacy-title"])
        .halign(Align::Center)
        .wrap(true)
        .build();
    vbox.append(&title);

    let desc = Label::builder()
        .label("Concedendo l'accesso, l'applicazione potrà utilizzare questa risorsa fino alla sua chiusura.")
        .css_classes(["privacy-desc"])
        .halign(Align::Center)
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .build();
    vbox.append(&desc);

    let hbox = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(16)
        .halign(Align::Center)
        .margin_top(16)
        .build();

    let btn_cancel = Button::builder()
        .label("Nega")
        .css_classes(["privacy-btn"])
        .build();
        
    btn_cancel.connect_clicked(move |_btn| {
        std::process::exit(EXIT_DENIED);
    });

    let btn_approve = Button::builder()
        .label("Consenti")
        .css_classes(["suggested-action", "privacy-btn"])
        .build();

    btn_approve.connect_clicked(move |_btn| {
        std::process::exit(EXIT_GRANTED);
    });

    hbox.append(&btn_cancel);
    hbox.append(&btn_approve);
    vbox.append(&hbox);

    window.set_child(Some(&vbox));

    // Closing the window, by any means, is a refusal. Without this the application quits
    // with status 0 when its last window closes.
    window.connect_close_request(|_| std::process::exit(EXIT_DENIED));

    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed(|_, key, _, _| {
        if key == gdk::Key::Escape {
            std::process::exit(EXIT_DENIED);
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    window.present();
}
