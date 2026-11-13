//! The launcher's preview pane (doc_launcher.md, LA6), without its widgets: what the pane
//! shows, as a [`Card`] handed to the launcher at each change, which draws it with its toolkit.
//! Every string from outside is cut through athanor-unit::text and is plain text, never markup;
//! no untrusted file is decoded in this process.

pub mod origin;
pub mod render;
pub mod text;

use std::cell::{Cell, RefCell};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::rc::Rc;

use athanor_search::calc::Calc;
use athanor_unit::text::{line, lines, BODY_CHARS, NAME_CHARS, SUMMARY_CHARS};
use gio::prelude::*;

/// The longest side of a decoded picture, in logical pixels.
const PICTURE_SIDE: u32 = 512;
/// `flatpak list` that takes longer is given up.
const FLATPAK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(1);

pub enum Subject {
    File { uri: String },
    App { info: gio_unix::DesktopAppInfo },
    Window { title: String, app: String, picture: Option<Picture> },
    Calc { calc: Calc, rates_date: Option<String> },
    Note { title: String, body: String },
}

/// A picture: `height` rows of `width` pixels of red, green, blue and alpha, the colour
/// premultiplied by the alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Shared, so that a toolkit that keeps the bytes, as GDK's textures do, takes them
    /// without a copy.
    pub rgba: glib::Bytes,
}

/// What the pane shows: from the top, the picture or else the icon, the title, the body and
/// the facts. An empty text is not shown.
#[derive(Clone, Debug, Default)]
pub struct Card {
    pub picture: Option<Rc<Picture>>,
    pub icon: Option<gio::Icon>,
    pub title: String,
    pub body: String,
    pub facts: String,
}

pub struct Preview {
    card: RefCell<Card>,
    /// Called with the card at each change.
    changed: Box<dyn Fn(&Card)>,
    /// The scale of the surface that shows the pane, which sizes a decoded picture.
    scale: Cell<f64>,
    /// Every `show` takes a new value; an answer for an older one is dropped.
    generation: Cell<u64>,
    /// One decode at a time: the unit of a decode that was passed over is stopped before the
    /// next one starts, so holding an arrow key never piles up decoders.
    inflight: Cell<bool>,
    /// The latest request that waits for the decode in flight.
    queued: RefCell<Option<Job>>,
    /// Fires when `show` or `clear` leaves the decode in flight behind.
    cancellable: RefCell<gio::Cancellable>,
}

/// An image or a PDF waiting for the decoder.
struct Job {
    file: std::fs::File,
    kind: render::Kind,
    side: u32,
    path: PathBuf,
    generation: u64,
}

/// The icon the preview may show for something described by another process: a themed one.
/// A file icon or a bytes icon would make the launcher decode a file (LA9).
pub fn themed(icon: Option<gio::Icon>) -> gio::Icon {
    match icon {
        Some(icon) if icon.is::<gio::ThemedIcon>() => icon,
        _ => gio::ThemedIcon::new("application-x-executable").upcast(),
    }
}

impl Preview {
    /// An empty pane, which calls `changed` with what it shows at each change.
    pub fn new(changed: impl Fn(&Card) + 'static) -> Rc<Self> {
        Rc::new(Self {
            card: RefCell::default(),
            changed: Box::new(changed),
            scale: Cell::new(1.0),
            generation: Cell::new(0),
            inflight: Cell::new(false),
            queued: RefCell::new(None),
            cancellable: RefCell::new(gio::Cancellable::new()),
        })
    }

    /// What the pane shows now.
    pub fn card(&self) -> Card {
        self.card.borrow().clone()
    }

    /// The longest side, in pixels, of a picture drawn at the pane's scale: what a picture
    /// handed to [`Preview::show`] needs at most.
    pub fn picture_side(&self) -> u32 {
        // `set_scale` keeps the scale finite and positive; `render` clamps the side.
        (f64::from(PICTURE_SIDE) * self.scale.get()).ceil().max(1.0) as u32
    }

    /// The scale of the surface that shows the pane, for the pictures decoded after this call.
    pub fn set_scale(&self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.scale.set(scale);
        }
    }

    pub fn clear(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.queued.borrow_mut().take();
        self.cancellable.replace(gio::Cancellable::new()).cancel();
        self.card.replace(Card::default());
        self.notify();
    }

    /// Calls `changed` with a copy of the card, so that it may show or clear the pane again.
    fn notify(&self) {
        let card = self.card.borrow().clone();
        (self.changed)(&card);
    }

    /// Changes the card and tells the launcher.
    fn update(&self, change: impl FnOnce(&mut Card)) {
        change(&mut self.card.borrow_mut());
        self.notify();
    }

    fn set_picture(&self, picture: Picture) {
        self.update(|card| {
            card.picture = Some(Rc::new(picture));
            card.icon = None;
        });
    }

    pub fn show(self: &Rc<Self>, subject: Subject) {
        self.clear();
        let generation = self.generation.get();
        match subject {
            Subject::Note { title, body } => self.update(|card| {
                card.title = line(&title, SUMMARY_CHARS);
                card.body = lines(&body, BODY_CHARS);
            }),
            Subject::Calc { calc, rates_date } => self.update(|card| {
                card.title = line(&calc.result, SUMMARY_CHARS);
                card.body = line(&calc.expression, SUMMARY_CHARS);
                if let Some(date) = rates_date {
                    card.facts = tr_with("Exchange rates of {date}", "date", &line(&date, SUMMARY_CHARS));
                }
            }),
            Subject::Window { title, app, picture } => self.update(|card| {
                card.picture = picture.map(Rc::new);
                card.title = line(&title, SUMMARY_CHARS);
                card.body = line(&app, NAME_CHARS);
            }),
            Subject::App { info } => self.show_app(&info, generation),
            Subject::File { uri } => {
                let this = Rc::clone(self);
                glib::spawn_future_local(async move { this.show_file(uri, generation).await });
            }
        }
    }

    fn show_app(self: &Rc<Self>, info: &gio_unix::DesktopAppInfo, generation: u64) {
        let provenance = info.filename().map_or(origin::Provenance::User, |path| {
            origin::provenance(&path, info.string("X-Flatpak").as_deref(), &glib::user_data_dir())
        });
        let facts = match &provenance {
            origin::Provenance::Flatpak(_) => String::new(),
            origin::Provenance::Image => tr("System image"),
            origin::Provenance::User => tr("Installed by you"),
        };
        self.update(|card| {
            card.icon = Some(themed(info.icon()));
            card.title = line(&info.name(), NAME_CHARS);
            card.body = lines(&info.description().unwrap_or_default(), BODY_CHARS);
            card.facts = facts;
        });
        let origin::Provenance::Flatpak(app_id) = provenance else { return };
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            if this.generation.get() != generation {
                return;
            }
            let origin = match gio::Subprocess::newv(
                &["flatpak", "list", "--app", "--columns=application,origin,version"].map(std::ffi::OsStr::new),
                gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
            ) {
                Ok(process) => match glib::future_with_timeout(FLATPAK_DEADLINE, process.communicate_utf8_future(None)).await {
                    Ok(Ok((Some(stdout), _))) => origin::flatpak_origin(&stdout, &app_id),
                    Ok(Ok((None, _))) => None,
                    Ok(Err(err)) => {
                        tracing::warn!("flatpak list failed: {err}");
                        None
                    }
                    Err(_) => {
                        tracing::warn!("flatpak list did not answer in time");
                        process.force_exit();
                        None
                    }
                },
                Err(err) => {
                    tracing::warn!("flatpak list did not start: {err}");
                    None
                }
            };
            if this.generation.get() != generation {
                return;
            }
            let text = match origin {
                Some(origin::Origin { remote, version: Some(version) }) => format!("Flatpak · {} · {}", line(&remote, NAME_CHARS), line(&version, NAME_CHARS)),
                Some(origin::Origin { remote, version: None }) => format!("Flatpak · {}", line(&remote, NAME_CHARS)),
                None => "Flatpak".to_owned(),
            };
            this.update(|card| card.facts = text);
        });
    }

    async fn show_file(self: Rc<Self>, uri: String, generation: u64) {
        let file = gio::File::for_uri(&uri);
        let attributes = "standard::content-type,standard::display-name,standard::size,standard::icon,time::modified";
        let info = match file.query_info_future(attributes, gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT).await {
            Ok(info) => info,
            Err(err) => {
                tracing::warn!(uri, "the file cannot be described: {err}");
                return;
            }
        };
        if self.generation.get() != generation {
            return;
        }
        // The card: shown at once, and kept when the content cannot be shown.
        let folder = file.parent().and_then(|parent| parent.path()).map(|path| path.display().to_string()).unwrap_or_default();
        let modified = info
            .modification_date_time()
            .and_then(|time| time.to_local().ok())
            .and_then(|time| time.format("%x %X").ok())
            .unwrap_or_default();
        self.update(|card| {
            card.icon = Some(themed(info.icon()));
            card.title = line(&info.display_name(), NAME_CHARS);
            card.facts = [line(&folder, SUMMARY_CHARS), glib::format_size(info.size().max(0) as u64).to_string(), modified.to_string()].join(" · ");
        });

        let Some(path) = file.path() else { return };
        let kind = info.content_type().unwrap_or_default();
        // Images and PDFs first: SVG is also text, and goes to the decoder.
        let decode = if kind.starts_with("image/") {
            Some(render::Kind::Image)
        } else if gio::content_type_is_a(&kind, "application/pdf") {
            Some(render::Kind::Pdf)
        } else if gio::content_type_is_a(&kind, "text/plain") {
            None
        } else {
            return;
        };
        // Opened off the main loop: a stale network or FUSE mount must not freeze the
        // launcher. O_NONBLOCK: a FIFO named like a document must not hang the worker.
        let opening = path.clone();
        let opened = gio::spawn_blocking(move || {
            let opened = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&opening)?;
            if !opened.metadata()?.is_file() {
                return Ok(None);
            }
            let head = if decode.is_none() { Some(text::head(&opened)?) } else { None };
            Ok::<_, std::io::Error>(Some((opened, head)))
        })
        .await;
        let (opened, head) = match opened {
            Ok(Ok(Some(opened))) => opened,
            Ok(Ok(None)) => return,
            Ok(Err(err)) => {
                tracing::warn!(path = %path.display(), "the file cannot be read: {err}");
                return;
            }
            Err(_) => {
                tracing::warn!(path = %path.display(), "reading the file failed");
                return;
            }
        };
        if self.generation.get() != generation {
            return;
        }
        let Some(decode) = decode else {
            self.update(|card| card.body = head.unwrap_or_default());
            return;
        };
        let side = self.picture_side();
        *self.queued.borrow_mut() = Some(Job { file: opened, kind: decode, side, path, generation });
        self.pump();
    }

    /// Starts the queued decode when none is in flight; called again when one ends.
    fn pump(self: &Rc<Self>) {
        if self.inflight.get() {
            return;
        }
        let Some(job) = self.queued.borrow_mut().take() else { return };
        if job.generation != self.generation.get() {
            return;
        }
        self.inflight.set(true);
        let cancellable = self.cancellable.borrow().clone();
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let answer = render::render(job.file, job.kind, job.side, &cancellable).await;
            this.inflight.set(false);
            match answer {
                Ok(picture) if this.generation.get() == job.generation => this.set_picture(picture),
                Ok(_) | Err(render::RenderError::Cancelled) => {}
                Err(err) => tracing::warn!(path = %job.path.display(), "no preview: {err}"),
            }
            this.pump();
        });
    }
}

static CATALOG: std::sync::OnceLock<&'static athanor_i18n::Catalog> = std::sync::OnceLock::new();

/// The launcher's catalog; the preview speaks English until it is set.
pub fn set_catalog(catalog: &'static athanor_i18n::Catalog) {
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the preview's catalog was already set");
    }
}

fn tr(msgid: &str) -> String {
    CATALOG.get().map_or_else(|| msgid.to_owned(), |catalog| catalog.tr(msgid).to_string())
}

fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_change_reaches_the_launcher_and_a_new_subject_starts_from_an_empty_card() {
        let seen = Rc::new(RefCell::new(Vec::<Card>::new()));
        let preview = Preview::new({
            let seen = Rc::clone(&seen);
            move |card| seen.borrow_mut().push(card.clone())
        });
        let picture = Picture { width: 1, height: 1, rgba: glib::Bytes::from_owned(vec![1u8, 2, 3, 255]) };
        preview.show(Subject::Window { title: "Files".into(), app: "Nautilus".into(), picture: Some(picture.clone()) });
        let card = preview.card();
        assert_eq!((card.title.as_str(), card.body.as_str(), card.facts.as_str()), ("Files", "Nautilus", ""));
        assert_eq!(card.picture.as_deref(), Some(&picture));
        preview.show(Subject::Note { title: "Note".into(), body: "One\ntwo".into() });
        let card = preview.card();
        assert!(card.picture.is_none() && card.icon.is_none(), "nothing of the window is left");
        assert_eq!((card.title.as_str(), card.body.as_str()), ("Note", "One\ntwo"));
        preview.clear();
        let seen = seen.borrow();
        // show: clear, then the subject, twice; then clear.
        assert_eq!(seen.len(), 5);
        assert!(seen.last().is_some_and(|card| card.title.is_empty() && card.picture.is_none()));
    }

    #[test]
    fn the_launcher_may_change_the_pane_from_its_callback() {
        let preview: Rc<RefCell<std::rc::Weak<Preview>>> = Rc::default();
        let calls = Rc::new(Cell::new(0));
        let pane = Preview::new({
            let (preview, calls) = (Rc::clone(&preview), Rc::clone(&calls));
            move |card| {
                calls.set(calls.get() + 1);
                // The first card with a title clears the pane again, inside the callback.
                if !card.title.is_empty() {
                    if let Some(preview) = preview.borrow().upgrade() {
                        preview.clear();
                    }
                }
            }
        });
        preview.replace(Rc::downgrade(&pane));
        pane.show(Subject::Note { title: "Note".into(), body: String::new() });
        // show: clear, then the note, whose callback cleared it.
        assert_eq!(calls.get(), 3);
        assert!(pane.card().title.is_empty());
    }

    #[test]
    fn a_window_picture_needs_the_side_at_the_panes_scale() {
        let preview = Preview::new(|_| {});
        assert_eq!(preview.picture_side(), PICTURE_SIDE);
        preview.set_scale(1.5);
        assert_eq!(preview.picture_side(), 768);
    }

    #[test]
    fn a_scale_that_is_not_a_positive_number_is_ignored() {
        let preview = Preview::new(|_| {});
        preview.set_scale(1.5);
        for wrong in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            preview.set_scale(wrong);
        }
        assert_eq!(preview.scale.get(), 1.5);
    }

    #[test]
    fn only_a_themed_icon_is_shown() {
        let themed_icon: gio::Icon = gio::ThemedIcon::new("text-x-generic").upcast();
        assert!(themed(Some(themed_icon.clone())).equal(Some(&themed_icon)));
        let file_icon: gio::Icon = gio::FileIcon::new(&gio::File::for_path("/nonexistent/x.png")).upcast();
        assert!(themed(Some(file_icon)).is::<gio::ThemedIcon>());
        assert!(themed(None).is::<gio::ThemedIcon>());
    }
}
