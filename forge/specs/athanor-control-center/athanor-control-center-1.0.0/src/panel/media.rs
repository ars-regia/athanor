//! The media section (CC4): artwork, the track, a choice of player, play and pause, skip, and
//! a seek bar. Artwork is what the model decoded from a `file:` or `data:` URI; a cover that
//! is missing or cannot be decoded shows the generic media icon, and nothing is ever fetched
//! (CC8).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use athanor_services::media::{Art, MediaCommand, MediaState, Player};
use gtk4::prelude::*;
use gtk4::{gdk, glib};

use super::a11y;
use crate::i18n::tr;

const ART_SIZE: i32 = 64;
const FALLBACK_ICON: &str = "multimedia-player-symbolic";

pub struct MediaView {
    pub root: gtk4::Box,
    pub(super) art: gtk4::Picture,
    title: gtk4::Label,
    artist: gtk4::Label,
    previous: gtk4::Button,
    play: gtk4::Button,
    next: gtk4::Button,
    seek: gtk4::Scale,
    chooser: gtk4::DropDown,
    names: gtk4::StringList,
    /// Bus names, in the order the chooser lists them.
    buses: RefCell<Vec<String>>,
    syncing: Cell<bool>,
    shown_art: RefCell<Option<Art>>,
    /// The position the player last reported, and when this view heard it.
    anchor: Cell<Option<(Instant, i64)>>,
    playing: Cell<bool>,
    length_us: Cell<Option<i64>>,
    commands: Box<dyn Fn(MediaCommand)>,
    timer: RefCell<Option<glib::SourceId>>,
}

fn button(icon: &str, label: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    a11y::name(&button, label);
    button
}

impl MediaView {
    pub fn new(commands: impl Fn(MediaCommand) + 'static) -> Rc<MediaView> {
        let art = gtk4::Picture::new();
        art.set_size_request(ART_SIZE, ART_SIZE);
        art.set_can_shrink(true);
        art.set_content_fit(gtk4::ContentFit::Cover);
        art.set_valign(gtk4::Align::Start);
        art.add_css_class("control-center-art");
        // The picture is decoration: the track's text says what plays.
        a11y::label(&art, &tr("Cover"));
        let title = gtk4::Label::new(None);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title.add_css_class("heading");
        let artist = gtk4::Label::new(None);
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        artist.add_css_class("caption");
        let names = gtk4::StringList::new(&[]);
        let chooser = gtk4::DropDown::new(Some(names.clone()), gtk4::Expression::NONE);
        a11y::name(&chooser, &tr("Media player"));
        let previous = button("media-skip-backward-symbolic", &tr("Previous track"));
        let play = button("media-playback-start-symbolic", &tr("Play"));
        let next = button("media-skip-forward-symbolic", &tr("Next track"));
        let seek = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.0, 1.0);
        seek.set_draw_value(false);
        seek.set_hexpand(true);
        a11y::name(&seek, &tr("Seek"));
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        controls.append(&previous);
        controls.append(&play);
        controls.append(&next);
        let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(&title);
        text.append(&artist);
        text.append(&controls);
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        top.append(&art);
        top.append(&text);
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        root.add_css_class("control-center-media");
        root.set_accessible_role(gtk4::AccessibleRole::Group);
        a11y::label(&root, &tr("Media"));
        root.append(&top);
        root.append(&seek);
        root.append(&chooser);
        let view = Rc::new(MediaView {
            root,
            art,
            title,
            artist,
            previous,
            play,
            next,
            seek,
            chooser,
            names,
            buses: RefCell::default(),
            syncing: Cell::new(false),
            shown_art: RefCell::default(),
            anchor: Cell::new(None),
            playing: Cell::new(false),
            length_us: Cell::new(None),
            commands: Box::new(commands),
            timer: RefCell::default(),
        });
        view.connect();
        view
    }

    fn send(&self, command: MediaCommand) {
        (self.commands)(command);
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let on = |command: fn() -> MediaCommand| {
            let weak = weak.clone();
            move |_: &gtk4::Button| {
                if let Some(view) = weak.upgrade() {
                    view.send(command());
                }
            }
        };
        self.previous.connect_clicked(on(|| MediaCommand::Previous));
        self.play.connect_clicked(on(|| MediaCommand::PlayPause));
        self.next.connect_clicked(on(|| MediaCommand::Next));
        // `change-value` is the person's: a position set from the model does not emit it.
        let weak = Rc::downgrade(self);
        self.seek.connect_change_value(move |_, _, value| {
            if let Some(view) = weak.upgrade() {
                view.seek_to(value);
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.chooser.connect_selected_notify(move |chooser| {
            let Some(view) = weak.upgrade().filter(|view| !view.syncing.get()) else {
                return;
            };
            let bus = view.buses.borrow().get(chooser.selected() as usize).cloned();
            if let Some(bus) = bus {
                view.send(MediaCommand::Choose(bus));
            }
        });
    }

    /// Seeks to `seconds` into the track, as an offset from where the player is.
    pub(super) fn seek_to(&self, seconds: f64) {
        let Some(position) = self.position_us() else {
            return;
        };
        let target = (seconds * 1_000_000.0).round() as i64;
        // Every number here is a player's: saturated, never overflowed.
        self.send(MediaCommand::Seek(target.saturating_sub(position)));
        self.anchor.set(Some((Instant::now(), target)));
    }

    fn position_us(&self) -> Option<i64> {
        let (at, position) = self.anchor.get()?;
        let moved = if self.playing.get() {
            i64::try_from(at.elapsed().as_micros()).unwrap_or(0)
        } else {
            0
        };
        // `end` is never below 0 and `clamp` would panic on an inverted range.
        let end = self.length_us.get().unwrap_or(i64::MAX).max(0);
        Some(position.saturating_add(moved).clamp(0, end))
    }

    /// Draws the position the player is at now; the timer calls it every second while the
    /// panel is shown.
    fn tick(&self) {
        if let Some(position) = self.position_us() {
            self.syncing.set(true);
            self.seek.set_value(position as f64 / 1_000_000.0);
            self.syncing.set(false);
        }
    }

    /// Follows the clock only while the panel is on screen.
    pub fn set_shown(self: &Rc<Self>, shown: bool) {
        let mut timer = self.timer.borrow_mut();
        if let Some(id) = timer.take() {
            id.remove();
        }
        if shown {
            self.tick();
            let weak = Rc::downgrade(self);
            *timer = Some(glib::timeout_add_seconds_local(1, move || {
                match weak.upgrade() {
                    Some(view) => {
                        view.tick();
                        glib::ControlFlow::Continue
                    }
                    None => glib::ControlFlow::Break,
                }
            }));
        }
    }

    /// Shows the player the controls act on; the panel hides the section when there is none (BR3).
    pub fn update(&self, state: &MediaState) {
        let Some(player) = state.current_player() else {
            return;
        };
        let track = player.track.as_ref();
        self.title
            .set_text(track.map_or(player.identity.as_str(), |track| track.title.as_str()));
        self.artist.set_text(
            track
                .and_then(|track| track.artist.as_deref())
                .unwrap_or_default(),
        );
        self.artist.set_visible(track.is_some_and(|track| track.artist.is_some()));
        self.previous.set_sensitive(player.can_previous);
        self.next.set_sensitive(player.can_next);
        let (icon, label) = if player.playing {
            ("media-playback-pause-symbolic", tr("Pause"))
        } else {
            ("media-playback-start-symbolic", tr("Play"))
        };
        self.play.set_icon_name(icon);
        a11y::name(&self.play, &label);
        self.show_art(player);
        self.show_position(player);
        self.show_players(state);
    }

    fn show_art(&self, player: &Player) {
        if *self.shown_art.borrow() == player.art && self.art.paintable().is_some() {
            return;
        }
        self.shown_art.replace(player.art.clone());
        let decoded = match &player.art {
            Some(Art::Data(bytes)) => {
                match gdk::Texture::from_bytes(&glib::Bytes::from(&bytes[..])) {
                    Ok(texture) => Some(texture.upcast::<gdk::Paintable>()),
                    Err(err) => {
                        tracing::debug!(error = %err, "the cover cannot be decoded; the player's icon is shown");
                        None
                    }
                }
            }
            None => None,
        };
        let paintable = decoded.or_else(|| self.fallback());
        self.art.set_paintable(paintable.as_ref());
    }

    /// The generic media icon at the cover's size.
    fn fallback(&self) -> Option<gdk::Paintable> {
        let display = gdk::Display::default()?;
        let theme = gtk4::IconTheme::for_display(&display);
        Some(
            theme
                .lookup_icon(
                    FALLBACK_ICON,
                    &[],
                    ART_SIZE,
                    self.art.scale_factor(),
                    gtk4::TextDirection::None,
                    gtk4::IconLookupFlags::empty(),
                )
                .upcast(),
        )
    }

    fn show_position(&self, player: &Player) {
        self.playing.set(player.playing);
        self.length_us.set(player.length_us);
        self.anchor
            .set(player.position_us.map(|position| (Instant::now(), position)));
        let seekable = player.can_seek && player.length_us.is_some_and(|length| length > 0);
        self.seek.set_visible(seekable);
        if let Some(length) = player.length_us.filter(|_| seekable) {
            self.syncing.set(true);
            self.seek.set_range(0.0, length as f64 / 1_000_000.0);
            self.syncing.set(false);
        }
        self.tick();
    }

    fn show_players(&self, state: &MediaState) {
        let buses: Vec<String> = state
            .players
            .iter()
            .map(|player| player.bus_name.clone())
            .collect();
        self.chooser.set_visible(buses.len() > 1);
        self.syncing.set(true);
        if *self.buses.borrow() != buses {
            let names: Vec<&str> = state
                .players
                .iter()
                .map(|player| player.identity.as_str())
                .collect();
            self.names.splice(0, self.names.n_items(), &names);
            self.buses.replace(buses);
        }
        if let Some(current) = state.current {
            self.chooser.set_selected(current as u32);
        }
        self.syncing.set(false);
    }
}
