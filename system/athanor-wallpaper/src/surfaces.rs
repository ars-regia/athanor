//! The Sal application (SD6): one background surface per output, showing the picture its
//! connector's keys name, decoded at the output's buffer size. A surface shows the plain colour
//! of [`PLACEHOLDER`] until its pixels arrive, and keeps the picture it shows until the next
//! one is decoded. A picture that cannot be opened, or that the decoder refuses, shows the
//! hearth until the keys change; a hearth that cannot keeps the colour. A fault of the decoder
//! is retried (see [`Wallpaper::fault`]).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use athanor_sal::driver::{Anchor, Application, Layer, LayerOptions, OutputInfo, Shell};
use athanor_sal::{Event, Image, SurfaceId, Theme, Ui};
use athanor_wallpaper::plan::{buffer_size, requests};
use athanor_wallpaper::protocol::MAX_CONNECTIONS;
use athanor_wallpaper::{hearth_path, open_regular, ColorScheme, Settings, Source};

use crate::decoder::{self, Failure, Frames};

/// The namespace of the surfaces (SD6).
const NAMESPACE: &str = "wallpaper";
/// The colour shown until the pixels arrive, and when no picture can be shown: the hearth's
/// darkest tone.
pub const PLACEHOLDER: [u8; 4] = [0x27, 0x29, 0x2a, 0xff];
/// How long a picture whose decode met a fault of the decoder waits before it is asked again.
// ponytail: a fixed back-off; double it up to a cap if a broken decoder restarting every
// two seconds for the hearth matters.
const BACKOFF: Duration = Duration::from_secs(2);

/// A picture at a size.
type Key = (PathBuf, (u32, u32));
/// A buffer size.
type Size = (u32, u32);
/// A decode that ended: the picture, the sizes asked, its count of faults when it started, and
/// the frames or why there are none.
type Decoded = (PathBuf, Vec<Size>, u32, Result<Frames, Failure>);

/// What reaches the application from GLib's sources between turns.
#[derive(Default)]
pub struct Inbox {
    /// The keys changed: read them again.
    pub settings_changed: bool,
    /// A decode ended: the picture, the sizes asked, and the frames or why there are none.
    decoded: Vec<Decoded>,
    /// The pictures whose back-off ended.
    retry: Vec<PathBuf>,
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
    /// The connections open to the decoder, at most [`MAX_CONNECTIONS`].
    connections: usize,
    /// The pictures that failed to open or that the decoder refused, since the keys last
    /// changed: they show the hearth.
    failed: HashSet<PathBuf>,
    /// The pictures waiting out their back-off after a fault of the decoder.
    waiting: HashSet<PathBuf>,
    /// The pictures, the hearth never, that met a second fault: they show the hearth until an
    /// output or the keys change.
    stalled: HashSet<PathBuf>,
    /// The faults of each picture since it last decoded.
    faults: HashMap<PathBuf, u32>,
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
            connections: 0,
            failed: HashSet::new(),
            waiting: HashSet::new(),
            stalled: HashSet::new(),
            faults: HashMap::new(),
            invalid: HashMap::new(),
            placeholder,
            ready: false,
        }
    }

    /// The picture `connector` shows at `size`: its source, or the hearth when that failed.
    fn picture_for(&mut self, connector: &str, size: (u32, u32)) -> Key {
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
            Source::Path(path) if !self.held(&path) => path,
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

    /// Whether `path` is not asked for now: failed, waiting or stalled.
    fn held(&self, path: &Path) -> bool {
        self.failed.contains(path) || self.waiting.contains(path) || self.stalled.contains(path)
    }

    /// Applies what the inbox holds, starts the decodes still needed and shows what is decoded.
    fn refresh(&mut self, shell: &mut Shell) {
        let wants = self.plan();
        self.show(shell, wants);
    }

    /// Each surface with a size and the picture it wants, its decode started (see
    /// [`Wallpaper::decode_for`]).
    fn plan(&mut self) -> Vec<(SurfaceId, Key)> {
        let targets = self
            .outputs
            .iter()
            .filter_map(|(id, output)| Some((*id, output.connector.clone(), output.size?)))
            .collect();
        self.decode_for(targets)
    }

    /// Applies what the inbox holds and starts the decodes `targets`, (surface, connector,
    /// size), still need, at most [`MAX_CONNECTIONS`] at once; the rest start as those end.
    /// Generic over the surface's id so that it is tested without a display.
    fn decode_for<I: Copy>(&mut self, targets: Vec<(I, String, Size)>) -> Vec<(I, Key)> {
        let inbox = std::mem::take(&mut *self.inbox.borrow_mut());
        if inbox.settings_changed {
            (self.settings, self.scheme) = (self.read)();
            self.failed.clear();
            self.stalled.clear();
            self.faults.clear();
        }
        for path in inbox.retry {
            self.waiting.remove(&path);
        }
        for (path, sizes, seen, result) in inbox.decoded {
            self.connections = self.connections.saturating_sub(1);
            for size in &sizes {
                self.in_flight.remove(&(path.clone(), *size));
            }
            match result {
                Ok(frames) => match images(frames) {
                    Ok(images) => {
                        self.faults.remove(&path);
                        // The decoder works again: each stalled picture gets one more attempt.
                        self.stalled.clear();
                        for (size, image) in images {
                            self.cache.insert((path.clone(), size), image);
                        }
                    }
                    Err(error) => self.fail(&path, &error),
                },
                Err(Failure::Refused) => self.fail(&path, &Failure::Refused.to_string()),
                Err(Failure::Fault(error)) => self.fault(path, seen, &error),
            }
        }
        // Each failure to open turns its outputs to the hearth, which is then opened in turn;
        // a failed hearth asks for nothing more, so this ends within two rounds.
        loop {
            let wants: Vec<(I, Key)> = targets
                .iter()
                .map(|(id, connector, size)| (*id, self.picture_for(connector, *size)))
                .collect();
            self.cache
                .retain(|key, _| wants.iter().any(|(_, k)| k == key));
            let missing = wants.iter().map(|(_, key)| key.clone()).filter(|key| {
                !self.cache.contains_key(key) && !self.in_flight.contains(key) && !self.held(&key.0)
            });
            let mut opened = true;
            for (path, sizes) in requests(missing) {
                if self.connections >= MAX_CONNECTIONS {
                    break;
                }
                match open_regular(&path) {
                    Ok(file) => self.start(path, file, sizes),
                    Err(error) => {
                        self.fail(&path, &error.to_string());
                        opened = false;
                    }
                }
            }
            if opened {
                return wants;
            }
        }
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

    /// A fault of the decoder on `path`: it is asked for again after [`BACKOFF`], the hearth
    /// every time and any other picture once, after which it waits for a change of an output
    /// or of the keys. Meanwhile the picture's outputs show the hearth, and the hearth's keep
    /// what they show. `seen` is the picture's count of faults when the decode started: a
    /// decode started before the last fault met that same outage, and does not count again.
    fn fault(&mut self, path: PathBuf, seen: u32, error: &str) {
        let faults = self.faults.entry(path.clone()).or_default();
        if *faults != seen {
            return;
        }
        *faults += 1;
        let faults = *faults;
        if faults > 1 && path != hearth_path(self.scheme) {
            tracing::warn!(path = %path.display(), error, "the decoder failed again; the hearth is shown until an output or the keys change");
            self.stalled.insert(path);
            return;
        }
        tracing::warn!(path = %path.display(), error, faults, "the decoder failed; the picture is asked for again in {BACKOFF:?}");
        self.waiting.insert(path.clone());
        let inbox = Rc::clone(&self.inbox);
        // On the context the decodes run on, like them.
        glib::MainContext::ref_thread_default().spawn_local(async move {
            glib::timeout_future(BACKOFF).await;
            inbox.borrow_mut().retry.push(path);
        });
    }

    /// Decodes `file` at `sizes` on GLib's main context; the outcome lands in the inbox.
    fn start(&mut self, path: PathBuf, file: std::fs::File, sizes: Vec<(u32, u32)>) {
        for size in &sizes {
            self.in_flight.insert((path.clone(), *size));
        }
        self.connections += 1;
        let seen = self.faults.get(&path).copied().unwrap_or(0);
        let (socket, inbox) = (self.socket.clone(), Rc::clone(&self.inbox));
        glib::MainContext::ref_thread_default().spawn_local(async move {
            let result = decoder::decode(&socket, file, &sizes).await;
            inbox.borrow_mut().decoded.push((path, sizes, seen, result));
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
                let wanted = self.picture_for(&connector, size);
                self.cache.get(&wanted).cloned()
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
        self.stalled.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Long enough for a fault and a back-off.
    const WITHIN: Duration = Duration::from_secs(10);

    /// A wallpaper showing a picture of its own on each of `count` outputs, with its decoder at
    /// `socket`; and those outputs as targets.
    fn wallpaper(
        name: &str,
        count: usize,
        socket: &Path,
    ) -> (Wallpaper, Vec<(usize, String, Size)>) {
        let dir =
            std::env::temp_dir().join(format!("athanor-wallpaper-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("directory");
        let mut per_output = BTreeMap::new();
        let mut targets = Vec::new();
        for i in 0..count {
            let (connector, picture) = (format!("DP-{i}"), dir.join(format!("{i}.png")));
            std::fs::write(&picture, b"not decoded here").expect("picture");
            per_output.insert(connector.clone(), picture.display().to_string());
            targets.push((i, connector, (4, 3)));
        }
        let settings = Settings {
            per_output,
            same_on_all: false,
            ..Settings::default()
        };
        let read: Read = Box::new(move || (settings.clone(), ColorScheme::Light));
        let inbox = Rc::new(RefCell::new(Inbox::default()));
        (Wallpaper::new(read, socket.to_owned(), inbox), targets)
    }

    /// Runs `context` until the inbox holds what `done` waits for, or [`WITHIN`] passes.
    fn until(context: &glib::MainContext, wallpaper: &Wallpaper, done: impl Fn(&Inbox) -> bool) {
        let start = std::time::Instant::now();
        while !done(&wallpaper.inbox.borrow()) && start.elapsed() < WITHIN {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn decodes_open_at_most_the_socket_s_connections() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (mut wallpaper, targets) =
                    wallpaper("cap", MAX_CONNECTIONS + 2, Path::new("/nonexistent/decode"));
                let wants = wallpaper.decode_for(targets);
                assert_eq!(wants.len(), MAX_CONNECTIONS + 2);
                assert_eq!(wallpaper.connections, MAX_CONNECTIONS);
                assert_eq!(wallpaper.in_flight.len(), MAX_CONNECTIONS);
            })
            .expect("a thread-default context");
    }

    #[test]
    fn a_fault_is_asked_again_and_a_second_one_waits_for_a_change() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (mut wallpaper, targets) =
                    wallpaper("fault", 1, Path::new("/nonexistent/decode"));
                let picture = wallpaper.decode_for(targets.clone())[0].1 .0.clone();
                let hearth = hearth_path(ColorScheme::Light);
                until(&context, &wallpaper, |inbox| !inbox.decoded.is_empty());
                // The decoder cannot be reached: the picture waits, it has not failed, and its
                // output asks for the hearth meanwhile.
                let wants = wallpaper.decode_for(targets.clone());
                assert!(wallpaper.waiting.contains(&picture));
                assert!(!wallpaper.failed.contains(&picture));
                assert_eq!(wants[0].1 .0, hearth);
                until(&context, &wallpaper, |inbox| inbox.retry.contains(&picture));
                let wants = wallpaper.decode_for(targets.clone());
                assert_eq!(wants[0].1 .0, picture, "asked again after the back-off");
                until(&context, &wallpaper, |inbox| {
                    inbox.decoded.iter().any(|(path, _, _, _)| *path == picture)
                });
                wallpaper.decode_for(targets.clone());
                assert!(wallpaper.stalled.contains(&picture));
                assert!(!wallpaper.failed.contains(&picture));
                // The hearth, faulting as often, is never given up.
                for _ in 0..3 {
                    let seen = wallpaper.faults.get(&hearth).copied().unwrap_or(0);
                    wallpaper.fault(hearth.clone(), seen, "test");
                }
                assert!(!wallpaper.stalled.contains(&hearth));
                assert!(wallpaper.waiting.contains(&hearth));
            })
            .expect("a thread-default context");
    }

    #[test]
    fn decodes_that_meet_one_outage_count_one_fault() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (mut wallpaper, _) = wallpaper("outage", 1, Path::new("/nonexistent/decode"));
                let picture = PathBuf::from("/a.png");
                // Two decodes of the picture, at two sizes, both started before any fault.
                wallpaper.fault(picture.clone(), 0, "first size");
                wallpaper.fault(picture.clone(), 0, "second size");
                assert!(wallpaper.waiting.contains(&picture));
                assert!(!wallpaper.stalled.contains(&picture));
                assert_eq!(wallpaper.faults.get(&picture), Some(&1));
            })
            .expect("a thread-default context");
    }

    #[test]
    fn a_decode_that_succeeds_gives_the_stalled_pictures_another_attempt() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (mut wallpaper, targets) =
                    wallpaper("healthy", 2, Path::new("/nonexistent/decode"));
                let stalled = PathBuf::from(&wallpaper.settings.per_output["DP-0"]);
                let healthy = PathBuf::from(&wallpaper.settings.per_output["DP-1"]);
                wallpaper.stalled.insert(stalled.clone());
                wallpaper.faults.insert(stalled.clone(), 2);
                let pixels = vec![0; athanor_wallpaper::protocol::frame_len((4, 3))];
                wallpaper.inbox.borrow_mut().decoded.push((
                    healthy.clone(),
                    vec![(4, 3)],
                    0,
                    Ok(vec![((4, 3), pixels)]),
                ));
                wallpaper.decode_for(targets);
                assert!(wallpaper.stalled.is_empty());
                assert!(wallpaper.in_flight.contains(&(stalled, (4, 3))));
                assert!(wallpaper.cache.contains_key(&(healthy, (4, 3))));
            })
            .expect("a thread-default context");
    }
}
