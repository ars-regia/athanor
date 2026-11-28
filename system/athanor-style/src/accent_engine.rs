use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::interface;
use zbus::object_server::SignalEmitter;

/// The user's Athanor configuration directory: `$XDG_CONFIG_HOME/athanor`, else
/// `~/.config/athanor`, else `/var/lib/athanor`.
pub fn get_config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("athanor")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("athanor")
    } else {
        PathBuf::from("/var/lib/athanor")
    }
}

/// RGB Color representation with floating point and hex conversions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorRgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_rgba_string(&self, alpha: f64) -> String {
        format!("rgba({}, {}, {}, {:.2})", self.r, self.g, self.b, alpha)
    }

    pub fn parse_hex(hex: &str) -> Self {
        let clean = hex.trim().trim_start_matches('#');
        if clean.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&clean[0..2], 16),
                u8::from_str_radix(&clean[2..4], 16),
                u8::from_str_radix(&clean[4..6], 16),
            ) {
                return Self::new(r, g, b);
            }
        } else if clean.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&clean[0..1].repeat(2), 16),
                u8::from_str_radix(&clean[1..2].repeat(2), 16),
                u8::from_str_radix(&clean[2..3].repeat(2), 16),
            ) {
                return Self::new(r, g, b);
            }
        }
        // Default Athanor OS Accent (#89b4fa)
        Self::new(137, 180, 250)
    }

    /// WCAG relative luminance calculation for contrast determination
    pub fn luminance(&self) -> f64 {
        let rf = self.r as f64 / 255.0;
        let gf = self.g as f64 / 255.0;
        let bf = self.b as f64 / 255.0;
        0.2126 * rf + 0.7152 * gf + 0.0722 * bf
    }

    /// High contrast text foreground selection based on luminance
    pub fn contrasting_fg(&self) -> Self {
        if self.luminance() > 0.45 {
            Self::new(17, 17, 27) // Dark contrast text (#11111b)
        } else {
            Self::new(255, 255, 255) // Light contrast text (#ffffff)
        }
    }

    pub fn to_hsl(&self) -> ColorHsl {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let l = (max + min) / 2.0;

        let s = if delta == 0.0 {
            0.0
        } else if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let mut h = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta) % 6.0
        } else if max == g {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };

        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }

        ColorHsl { h, s, l }
    }
}

/// HSL Color representation for shade variations and lightness shifts
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorHsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

impl ColorHsl {
    pub fn adjust_lightness(&self, factor: f64) -> Self {
        let new_l = (self.l * factor).clamp(0.0, 1.0);
        Self {
            h: self.h,
            s: self.s,
            l: new_l,
        }
    }

    pub fn to_rgb(&self) -> ColorRgb {
        let c = (1.0 - (2.0 * self.l - 1.0).abs()) * self.s;
        let x = c * (1.0 - ((self.h / 60.0) % 2.0 - 1.0).abs());
        let m = self.l - c / 2.0;

        let (r_prime, g_prime, b_prime) = if self.h < 60.0 {
            (c, x, 0.0)
        } else if self.h < 120.0 {
            (x, c, 0.0)
        } else if self.h < 180.0 {
            (0.0, c, x)
        } else if self.h < 240.0 {
            (0.0, x, c)
        } else if self.h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        let r = ((r_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = ((g_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = ((b_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8;

        ColorRgb::new(r, g, b)
    }
}

/// Full derived accent color palette (Feren OS / XeroLinux reverse-engineered engine)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AccentPalette {
    pub base: ColorRgb,
    pub hover: ColorRgb,
    pub active: ColorRgb,
    pub fg: ColorRgb,
    pub hex: String,
    pub subtle_alpha: String,
    pub glass_alpha: String,
    pub focus_alpha: String,
}

impl AccentPalette {
    pub fn from_hex(hex: &str) -> Self {
        let base = ColorRgb::parse_hex(hex);
        let hsl = base.to_hsl();

        let hover = hsl.adjust_lightness(1.15).to_rgb();
        let active = hsl.adjust_lightness(0.85).to_rgb();
        let fg = base.contrasting_fg();

        Self {
            base,
            hover,
            active,
            fg,
            hex: base.to_hex(),
            subtle_alpha: base.to_rgba_string(0.15),
            glass_alpha: base.to_rgba_string(0.25),
            focus_alpha: base.to_rgba_string(0.50),
        }
    }

    /// Generates dynamic GTK4, Libadwaita, Compositor, Shell, and Dock CSS
    pub fn generate_gtk_css(&self) -> String {
        let template = include_str!("accent_template.css");
        template
            .replace("{hex}", &self.hex)
            .replace("{fg_hex}", &self.fg.to_hex())
            .replace("{hover_hex}", &self.hover.to_hex())
            .replace("{active_hex}", &self.active.to_hex())
            .replace("{subtle_alpha}", &self.subtle_alpha)
            .replace("{glass_alpha}", &self.glass_alpha)
            .replace("{focus_alpha}", &self.focus_alpha)
    }
}

pub fn get_accent_css_path() -> PathBuf {
    get_config_dir().join("accent.css")
}

pub fn get_theme_css_path() -> PathBuf {
    get_config_dir().join("theme.css")
}

thread_local! {
    static ACCENT_CSS_PROVIDER: std::cell::RefCell<Option<gtk4::CssProvider>> = const { std::cell::RefCell::new(None) };
}

/// Applies accent color hex: generates GTK CSS, persists accent.css and theme.css, and injects into GTK4 display
pub fn apply_accent_color(hex_color: &str) -> Result<String, String> {
    let palette = AccentPalette::from_hex(hex_color);
    let css_content = palette.generate_gtk_css();

    let config_dir = get_config_dir();
    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        return Err(format!("Failed to create config dir: {}", e));
    }

    let accent_path = get_accent_css_path();
    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&accent_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, css_content.as_bytes()))
    {
        return Err(format!("Failed to write accent.css: {}", e));
    }

    // Update theme.css so processes monitoring theme.css reload instantly
    let theme_path = get_theme_css_path();
    let updated_theme_css = if theme_path.exists() {
        if let Ok(existing) = std::fs::read_to_string(&theme_path) {
            if let Some(pos) = existing.find("/* Dynamic Global Accent Color Engine") {
                format!("{}\n{}", &existing[..pos], css_content)
            } else {
                format!("{}\n\n{}", existing, css_content)
            }
        } else {
            css_content.clone()
        }
    } else {
        css_content.clone()
    };

    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&theme_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, updated_theme_css.as_bytes()))
    {
        tracing::error!("Failed to write theme_path {:?}: {:?}", theme_path, e);
    }

    // Inject into current process GTK display live
    inject_gtk_css(&css_content);

    Ok(css_content)
}

pub async fn apply_accent_color_async(hex_color: &str) -> Result<String, String> {
    let palette = AccentPalette::from_hex(hex_color);
    let css_content = palette.generate_gtk_css();

    let config_dir = get_config_dir();
    if let Err(e) = tokio::fs::create_dir_all(&config_dir).await {
        return Err(format!("Failed to create config dir: {}", e));
    }

    let accent_path = get_accent_css_path();
    let write_res = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&accent_path)
            .await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, css_content.as_bytes()).await
    }.await;
    if let Err(e) = write_res {
        return Err(format!("Failed to write accent.css: {}", e));
    }

    let theme_path = get_theme_css_path();
    let updated_theme_css = if tokio::fs::metadata(&theme_path).await.is_ok() {
        if let Ok(existing) = tokio::fs::read_to_string(&theme_path).await {
            if let Some(pos) = existing.find("/* Dynamic Global Accent Color Engine") {
                format!("{}\n{}", &existing[..pos], css_content)
            } else {
                format!("{}\n\n{}", existing, css_content)
            }
        } else {
            css_content.clone()
        }
    } else {
        css_content.clone()
    };

    if let Err(e) = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&theme_path)
            .await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, updated_theme_css.as_bytes()).await
    }.await {
        tracing::error!("Failed to write theme_path {:?}: {:?}", theme_path, e);
    }

    Ok(css_content)
}

/// Inject generated CSS string into the default GTK4 display
pub fn inject_gtk_css(css: &str) {
    ACCENT_CSS_PROVIDER.with(|provider_cell| {
        let mut cell = provider_cell.borrow_mut();
        let provider = cell.get_or_insert_with(gtk4::CssProvider::new);
        provider.load_from_data(css);

        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER + 100,
            );
        }
    });
}

/// DBus Service implementation for Global Accent Color Engine
#[derive(Clone, Debug)]
pub struct AccentEngineService {
    current_hex: Arc<Mutex<String>>,
}

impl Default for AccentEngineService {
    fn default() -> Self {
        Self::new()
    }
}

impl AccentEngineService {
    pub fn new() -> Self {
        Self {
            current_hex: Arc::new(Mutex::new("#89b4fa".to_string())),
        }
    }
}

#[interface(name = "org.athanor.AccentEngine")]
impl AccentEngineService {
    pub async fn set_accent_color(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        hex: String,
    ) -> zbus::fdo::Result<String> {
        let css = apply_accent_color(&hex).map_err(zbus::fdo::Error::Failed)?;
        if let Ok(mut lock) = self.current_hex.lock() {
            *lock = hex.clone();
        }
        let _ = Self::accent_changed(&emitter, &hex, &css).await;
        Ok(css)
    }

    pub fn get_accent_color(&self) -> String {
        if let Ok(lock) = self.current_hex.lock() {
            lock.clone()
        } else {
            "#89b4fa".to_string()
        }
    }

    #[zbus(property)]
    pub fn accent_color(&self) -> String {
        self.get_accent_color()
    }

    #[zbus(property)]
    pub async fn set_accent_color_prop(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        hex: String,
    ) -> zbus::fdo::Result<()> {
        self.set_accent_color(emitter, hex).await?;
        Ok(())
    }

    pub fn generate_gtk_css(&self, hex: String) -> String {
        let palette = AccentPalette::from_hex(&hex);
        palette.generate_gtk_css()
    }

    #[zbus(signal)]
    pub async fn accent_changed(
        emitter: &SignalEmitter<'_>,
        hex: &str,
        css: &str,
    ) -> zbus::Result<()>;
}

pub async fn start_accent_engine_service(conn: &zbus::Connection) -> zbus::Result<()> {
    let service = AccentEngineService::new();
    conn.object_server()
        .at("/org/athanor/AccentEngine", service.clone())
        .await?;
    conn.object_server()
        .at("/os/athanor/AccentEngine", service)
        .await?;
    Ok(())
}
