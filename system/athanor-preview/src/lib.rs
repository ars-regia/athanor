//! The launcher's preview pane (doc_launcher.md, LA6). Every string from outside is set as
//! plain text through athanor-unit::text; no untrusted file is decoded in this process.

pub mod origin;
pub mod render;
pub mod text;

use std::cell::{Cell, RefCell};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::rc::Rc;

use athanor_search::calc::Calc;
use athanor_unit::text::{line, lines, BODY_CHARS, NAME_CHARS, SUMMARY_CHARS};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

/// The longest side of a decoded picture, in device pixels.
const PICTURE_SIDE: u32 = 512;
/// `flatpak list` that takes longer is given up.
const FLATPAK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(1);

pub enum Subject {
    File { uri: String },
    App { info: gio_unix::DesktopAppInfo },
    Window { title: String, app: String, picture: Option<gdk::Texture> },
    Calc { calc: Calc, rates_date: Option<String> },
    Note { title: String, body: String },
}

pub struct Preview {
    root: gtk4::Box,
    picture: gtk4::Picture,
    icon: gtk4::Image,
    title: gtk4::Label,
    body: gtk4::Label,
    facts: gtk4::Label,
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
fn themed(icon: Option<gio::Icon>) -> gio::Icon {
    match icon {
        Some(icon) if icon.is::<gio::ThemedIcon>() => icon,
        _ => gio::ThemedIcon::new("application-x-executable").upcast(),
    }
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::builder().xalign(0.0).wrap(true).selectable(false).use_markup(false).build();
    label.add_css_class(class);
    label
}

impl Preview {
    pub fn new() -> Rc<Self> {
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        root.add_css_class("preview");
        root.set_accessible_role(gtk4::AccessibleRole::Region);
        let picture = gtk4::Picture::builder().can_shrink(true).content_fit(gtk4::ContentFit::Contain).height_request(220).build();
        let icon = gtk4::Image::builder().pixel_size(96).build();
        let (title, body, facts) = (label("preview-title"), label("preview-body"), label("preview-facts"));
        body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        for widget in [picture.upcast_ref::<gtk4::Widget>(), icon.upcast_ref(), title.upcast_ref(), body.upcast_ref(), facts.upcast_ref()] {
            root.append(widget);
        }
        let preview = Rc::new(Self { root, picture, icon, title, body, facts, generation: Cell::new(0), inflight: Cell::new(false), queued: RefCell::new(None), cancellable: RefCell::new(gio::Cancellable::new()) });
        preview.clear();
        preview
    }

    pub fn widget(&self) -> &gtk4::Widget {
        self.root.upcast_ref()
    }

    pub fn clear(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.queued.borrow_mut().take();
        self.cancellable.replace(gio::Cancellable::new()).cancel();
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.picture.set_visible(false);
        self.icon.set_visible(false);
        for label in [&self.title, &self.body, &self.facts] {
            label.set_text("");
            label.set_visible(false);
        }
    }

    fn set(label: &gtk4::Label, text: &str) {
        label.set_text(text);
        label.set_visible(!text.is_empty());
    }

    fn set_icon(&self, icon: Option<&gio::Icon>) {
        if let Some(icon) = icon {
            self.icon.set_from_gicon(icon);
        }
        self.icon.set_visible(icon.is_some());
    }

    fn set_picture(&self, texture: &gdk::Texture) {
        self.picture.set_paintable(Some(texture));
        self.picture.set_visible(true);
        self.icon.set_visible(false);
    }

    pub fn show(self: &Rc<Self>, subject: Subject) {
        self.clear();
        let generation = self.generation.get();
        match subject {
            Subject::Note { title, body } => {
                Self::set(&self.title, &line(&title, SUMMARY_CHARS));
                Self::set(&self.body, &lines(&body, BODY_CHARS));
            }
            Subject::Calc { calc, rates_date } => {
                Self::set(&self.title, &line(&calc.result, SUMMARY_CHARS));
                Self::set(&self.body, &line(&calc.expression, SUMMARY_CHARS));
                if let Some(date) = rates_date {
                    Self::set(&self.facts, &tr_with("Exchange rates of {date}", "date", &line(&date, SUMMARY_CHARS)));
                }
            }
            Subject::Window { title, app, picture } => {
                if let Some(texture) = picture {
                    self.set_picture(&texture);
                }
                Self::set(&self.title, &line(&title, SUMMARY_CHARS));
                Self::set(&self.body, &line(&app, NAME_CHARS));
            }
            Subject::App { info } => self.show_app(&info, generation),
            Subject::File { uri } => {
                let this = Rc::clone(self);
                glib::spawn_future_local(async move { this.show_file(uri, generation).await });
            }
        }
    }

    fn show_app(self: &Rc<Self>, info: &gio_unix::DesktopAppInfo, generation: u64) {
        self.set_icon(Some(&themed(info.icon())));
        Self::set(&self.title, &line(&info.name(), NAME_CHARS));
        Self::set(&self.body, &lines(&info.description().unwrap_or_default(), BODY_CHARS));
        let Some(app_id) = info.string("X-Flatpak") else {
            Self::set(&self.facts, &tr("System image"));
            return;
        };
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
            Self::set(&this.facts, &text);
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
        self.set_icon(Some(&themed(info.icon())));
        Self::set(&self.title, &line(&info.display_name(), NAME_CHARS));
        let folder = file.parent().and_then(|parent| parent.path()).map(|path| path.display().to_string()).unwrap_or_default();
        let modified = info
            .modification_date_time()
            .and_then(|time| time.to_local().ok())
            .and_then(|time| time.format("%x %X").ok())
            .unwrap_or_default();
        Self::set(&self.facts, &[line(&folder, SUMMARY_CHARS), glib::format_size(info.size().max(0) as u64).to_string(), modified.to_string()].join(" · "));

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
            Self::set(&self.body, &head.unwrap_or_default());
            return;
        };
        let side = (PICTURE_SIDE as i32 * self.root.scale_factor()).max(1) as u32;
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
                Ok(texture) if this.generation.get() == job.generation => this.set_picture(texture.upcast_ref()),
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
    fn only_a_themed_icon_is_shown() {
        let themed_icon: gio::Icon = gio::ThemedIcon::new("text-x-generic").upcast();
        assert!(themed(Some(themed_icon.clone())).equal(Some(&themed_icon)));
        let file_icon: gio::Icon = gio::FileIcon::new(&gio::File::for_path("/tmp/x.png")).upcast();
        assert!(themed(Some(file_icon)).is::<gio::ThemedIcon>());
        assert!(themed(None).is::<gio::ThemedIcon>());
    }
}
