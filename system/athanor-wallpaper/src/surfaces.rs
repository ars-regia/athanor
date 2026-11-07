//! The Sal application (SD6): one background surface per output, showing the picture its
//! connector's keys name, decoded at the output's buffer size. A surface shows the plain colour
//! of [`PLACEHOLDER`] until its pixels arrive, and keeps the picture it shows until the next
//! one is decoded. A picture that cannot be opened or decoded shows the hearth; a hearth that
//! cannot keeps the colour.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_sal::driver::{Anchor, Application, Layer, LayerOptions, OutputInfo, Shell};
use athanor_sal::{Event, Image, SurfaceId, Theme, Ui};
use athanor_wallpaper::plan::{buffer_size, requests};
use athanor_wallpaper::{hearth_path, open_regular, ColorScheme, Settings, Source};

use crate::decoder::{self, Frames};

/// The namespace of the surfaces (SD6).
const NAMESPACE: &str = "wallpaper";
/// The colour shown until the pixels arrive, and when no picture can be shown: the hearth's
/// darkest tone.
pub const PLACEHOLDER: [u8; 4] = [0x27, 0x29, 0x2a, 0xff];

/// A picture at a size.
type Key = (PathBuf, (u32, u32));
/// A buffer size.
type Size = (u32, u32);
/// A decode that ended: the picture, the sizes asked, and the frames or why there are none.
type Decoded = (PathBuf, Vec<Size>, Result<Frames, String>);

/// What reaches the application from GLib's sources between turns.
#[derive(Default)]
pub struct Inbox {
    /// The keys changed: read them again.
    pub settings_changed: bool,
    /// A decode ended: the picture, the sizes asked, and the frames or why there are none.
    decoded: Vec<Decoded>,
}

/// One output's surface: its connector, its buffer size and the picture it shows.
struct Output {
    connector: String,
    size: Option<(u32, u32)>,
    showing: Option<Key>,
}

/// What the application reads the keys with: called at start and at each change.
pub type Read = Box<dyn Fn() -> (Settings, ColorScheme)>;

pub struct Wallpaper {
    read: Read,
    socket: PathBuf,
    settings: Settings,
    scheme: ColorScheme,
    inbox: Rc<RefCell<Inbox>>,
    outputs: HashMap<SurfaceId, Output>,
    cache: HashMap<Key, Image>,
    in_flight: HashSet<Key>,
    /// The pictures that failed since the keys last changed: they show the hearth.
    failed: HashSet<PathBuf>,
    /// The invalid value last logged for each connector.
    invalid: HashMap<String, String>,
    placeholder: Option<Image>,
    ready: bool,
}

impl Wallpaper {
    pub fn new(read: Read, socket: PathBuf, inbox: Rc<RefCell<Inbox>>) -> Self {
        let (settings, scheme) = read();
        let placeholder = Image::from_rgba8(1, 1, PLACEHOLDER.to_vec())
            .map_err(|error| tracing::error!(%error, "no placeholder colour"))
            .ok();
        Self {
            read,
            socket,
            settings,
            scheme,
            inbox,
            outputs: HashMap::new(),
            cache: HashMap::new(),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
            invalid: HashMap::new(),
            placeholder,
            ready: false,
        }
    }

    /// The picture `connector` shows at `size`: its source, or the hearth when that failed.
    fn key(&mut self, connector: &str, size: (u32, u32)) -> Key {
        match self.settings.invalid_value(connector) {
            Some(value) if self.invalid.get(connector).map(String::as_str) != Some(value) => {
                tracing::warn!(
                    connector,
                    value,
                    "not \"hearth\" nor an absolute path; the hearth is shown"
                );
                self.invalid.insert(connector.to_owned(), value.to_owned());
            }
            Some(_) => {}
            None => {
                self.invalid.remove(connector);
            }
        }
        let path = match self.settings.source_for(connector) {
            Source::Path(path) if !self.failed.contains(&path) => path,
            _ => hearth_path(self.scheme),
        };
        (path, size)
    }

    /// The surface's widgets: the picture, or the placeholder.
    fn ui(&self, image: Option<&Image>) -> Ui {
        let mut theme = Theme::default();
        theme.margin = 0.0;
        let mut ui = Ui::new(NAMESPACE, theme, false);
        if let Some(image) = image.or(self.placeholder.as_ref()) {
            ui.picture(Ui::ROOT, image.clone(), "");
        }
        ui
    }

    /// Applies what the inbox holds, starts the decodes still needed and shows what is decoded.
    fn refresh(&mut self, shell: &mut Shell) {
        let inbox = std::mem::take(&mut *self.inbox.borrow_mut());
        if inbox.settings_changed {
            (self.settings, self.scheme) = (self.read)();
            self.failed.clear();
        }
        for (path, sizes, result) in inbox.decoded {
            for size in &sizes {
                self.in_flight.remove(&(path.clone(), *size));
            }
            match result.and_then(images) {
                Ok(images) => {
                    for (size, image) in images {
                        self.cache.insert((path.clone(), size), image);
                    }
                }
                Err(error) => self.fail(&path, &error),
            }
        }
        // Each failure to open turns its outputs to the hearth, which is then opened in turn;
        // a failed hearth asks for nothing more, so this ends within two rounds.
        loop {
            let wants = self.wants();
            self.cache
                .retain(|key, _| wants.iter().any(|(_, k)| k == key));
            let missing = wants.iter().map(|(_, key)| key.clone()).filter(|key| {
                !self.cache.contains_key(key)
                    && !self.in_flight.contains(key)
                    && !self.failed.contains(&key.0)
            });
            let mut opened = true;
            for (path, sizes) in requests(missing) {
                match open_regular(&path) {
                    Ok(file) => self.start(path, file, sizes),
                    Err(error) => {
                        self.fail(&path, &error.to_string());
                        opened = false;
                    }
                }
            }
            if opened {
                self.show(shell, wants);
                return;
            }
        }
    }

    /// Each surface with a size and the picture it wants.
    fn wants(&mut self) -> Vec<(SurfaceId, Key)> {
        let outputs: Vec<_> = self
            .outputs
            .iter()
            .filter_map(|(id, output)| Some((*id, output.connector.clone(), output.size?)))
            .collect();
        outputs
            .into_iter()
            .map(|(id, connector, size)| (id, self.key(&connector, size)))
            .collect()
    }

    fn show(&mut self, shell: &mut Shell, wants: Vec<(SurfaceId, Key)>) {
        for (id, key) in wants {
            let Some(image) = self.cache.get(&key) else {
                continue;
            };
            let ui = self.ui(Some(image));
            let Some(output) = self.outputs.get_mut(&id) else {
                continue;
            };
            if output.showing.as_ref() == Some(&key) {
                continue;
            }
            if let Some(widgets) = shell.ui_mut(id) {
                *widgets = ui;
                output.showing = Some(key);
            }
        }
    }

    fn fail(&mut self, path: &Path, error: &str) {
        if path == hearth_path(self.scheme) {
            tracing::error!(path = %path.display(), error, "the hearth cannot be shown");
        } else {
            tracing::warn!(path = %path.display(), error, "the picture cannot be shown; the hearth is shown");
        }
        self.failed.insert(path.to_owned());
    }

    /// Decodes `file` at `sizes` on GLib's main context; the outcome lands in the inbox.
    fn start(&mut self, path: PathBuf, file: std::fs::File, sizes: Vec<(u32, u32)>) {
        for size in &sizes {
            self.in_flight.insert((path.clone(), *size));
        }
        let (socket, inbox) = (self.socket.clone(), Rc::clone(&self.inbox));
        glib::MainContext::default().spawn_local(async move {
            let result = decoder::decode(&socket, file, &sizes).await;
            inbox.borrow_mut().decoded.push((path, sizes, result));
        });
    }
}

/// The frames as Sal's images.
fn images(frames: Frames) -> Result<Vec<(Size, Image)>, String> {
    frames
        .into_iter()
        .map(|(size, pixels)| {
            Image::from_rgba8(size.0, size.1, pixels)
                .map(|image| (size, image))
                .map_err(|error| error.to_string())
        })
        .collect()
}

/// The buffer size of `info`'s output (SD6: its mode at its scale).
fn size(info: &OutputInfo) -> Option<(u32, u32)> {
    let mode = info
        .modes
        .iter()
        .find(|mode| mode.current)
        .map(|mode| mode.dimensions);
    buffer_size(mode, info.logical_size, info.scale_factor)
}

impl Application for Wallpaper {
    fn output(&mut self, info: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        let cached = info
            .name
            .clone()
            .zip(size(info))
            .and_then(|(connector, size)| {
                let key = self.key(&connector, size);
                self.cache.get(&key).cloned()
            });
        let mut options = LayerOptions::default();
        options.layer = Layer::Background;
        options.namespace = NAMESPACE.into();
        options.anchor = Anchor::all();
        options.exclusive_zone = -1;
        options.input_region = Some(Vec::new());
        Some((self.ui(cached.as_ref()), options))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, info: &OutputInfo) {
        let output = self.outputs.entry(surface).or_insert(Output {
            connector: String::new(),
            size: None,
            showing: None,
        });
        output.connector = info.name.clone().unwrap_or_default();
        output.size = size(info);
        self.refresh(shell);
    }

    fn event(&mut self, _: &mut Shell, _: SurfaceId, _: Event) {}

    fn turn(&mut self, shell: &mut Shell) {
        self.refresh(shell);
    }

    fn drawn(&mut self, _: &mut Shell, _: SurfaceId) {
        if !self.ready {
            self.ready = true;
            if let Err(error) = athanor_unit::notify::notify_ready() {
                tracing::warn!(%error, "cannot tell systemd the wallpaper is ready");
            }
        }
    }

    fn closed(&mut self, shell: &mut Shell, surface: SurfaceId) {
        self.outputs.remove(&surface);
        self.refresh(shell);
    }
}
