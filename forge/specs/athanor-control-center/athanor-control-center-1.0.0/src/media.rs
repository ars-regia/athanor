//! The media section (CC4): artwork, the track, a choice of player, play and pause, skip, and
//! a seek bar. Artwork is what the model decoded from a `file:` or `data:` URI; until the
//! sandboxed decode draws it, and for a cover that is missing or cannot be decoded, the section
//! shows the generic media icon, and nothing is ever fetched (CC8).

use std::time::Instant;

use athanor_sal::{Event, Image, Kind, TextStyle, Ui, WidgetId};
use athanor_services::media::{MediaCommand, MediaState, Player};
use athanor_style::sal;

use crate::i18n::tr;
use crate::icons::Icons;

/// The side of the cover, in logical pixels.
pub const ART_SIZE: f32 = 64.0;
/// The icon a missing cover shows.
pub const FALLBACK_ICON: &str = "multimedia-player-symbolic";
const PREVIOUS: &str = "media-skip-backward-symbolic";
const PLAY: &str = "media-playback-start-symbolic";
const PAUSE: &str = "media-playback-pause-symbolic";
const NEXT: &str = "media-skip-forward-symbolic";

/// The section's widgets.
struct Ids {
    group: WidgetId,
    title: WidgetId,
    artist: WidgetId,
    previous: WidgetId,
    play: WidgetId,
    next: WidgetId,
    seek: Option<WidgetId>,
    chooser: WidgetId,
    /// The play and the pause icon, which the play button swaps.
    faces: (Image, Image),
}

/// The media section of one panel: the widgets, and the position the player is at.
#[derive(Default)]
pub struct Media {
    ids: Option<Ids>,
    /// Bus names, in the order the chooser lists them.
    buses: Vec<String>,
    /// The position the player last reported, and when the section heard it.
    anchor: Option<(Instant, i64)>,
    playing: bool,
    length_us: Option<i64>,
}

fn style(size: sal::Step) -> TextStyle {
    TextStyle {
        scale: size.scale,
        weight: size.weight,
        features: Vec::new(),
    }
}

impl Media {
    /// Adds the section, hidden, under `parent`, and returns its group; `None` when an icon is
    /// missing from the theme or the seek bar's range is broken.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, icons: &mut Icons) -> Option<WidgetId> {
        let group = ui.add(parent, Kind::Group(tr("Media")));
        let top = ui.inline(group, [0.0; 2]);
        let cover = icons.get(FALLBACK_ICON, ART_SIZE)?;
        ui.image(top, cover, ART_SIZE, &tr("Cover"));
        let text = ui.add(top, Kind::Column);
        ui.set_fill(text, true);
        let title = ui.add(text, Kind::Label(String::new()));
        ui.set_text_style(title, style(sal::size::HEADING));
        let artist = ui.add(text, Kind::Label(String::new()));
        ui.set_text_style(artist, style(sal::size::CAPTION));
        let controls = ui.inline(text, [0.0; 2]);
        let size = Icons::BUTTON;
        let previous = ui.icon_button(
            controls,
            &tr("Previous track"),
            icons.get(PREVIOUS, size)?,
            size,
            None,
        );
        let faces = (icons.get(PLAY, size)?, icons.get(PAUSE, size)?);
        let play = ui.icon_button(controls, &tr("Play"), faces.0.clone(), size, None);
        let next = ui.icon_button(
            controls,
            &tr("Next track"),
            icons.get(NEXT, size)?,
            size,
            None,
        );
        let seek = ui.slider(group, &tr("Seek"), 0.0, 1.0, 1.0, 0.0);
        let chooser = ui.drop_down(group, &tr("Media player"), &[], 0);
        ui.set_visible(group, false);
        self.ids = Some(Ids {
            group,
            title,
            artist,
            previous,
            play,
            next,
            seek,
            chooser,
            faces,
        });
        self.buses.clear();
        Some(group)
    }

    /// Shows the player the controls act on, or hides the section when `shown` is false (BR3).
    pub fn update(&mut self, ui: &mut Ui, state: &MediaState, shown: bool) {
        let Some(ids) = &self.ids else {
            return;
        };
        ui.set_visible(ids.group, shown);
        let Some(player) = state.current_player().filter(|_| shown) else {
            return;
        };
        let track = player.track.as_ref();
        ui.set_text(
            ids.title,
            track.map_or(player.identity.as_str(), |t| t.title.as_str()),
        );
        let artist = track.and_then(|t| t.artist.as_deref());
        ui.set_text(ids.artist, artist.unwrap_or_default());
        ui.set_visible(ids.artist, artist.is_some());
        ui.set_sensitive(ids.previous, player.can_previous);
        ui.set_sensitive(ids.next, player.can_next);
        let (face, label) = if player.playing {
            (ids.faces.1.clone(), tr("Pause"))
        } else {
            (ids.faces.0.clone(), tr("Play"))
        };
        ui.set_image(ids.play, Some(face));
        ui.set_text(ids.play, &label);
        let names: Vec<&str> = state.players.iter().map(|p| p.identity.as_str()).collect();
        // Without a current player the choice stays where it is, as GTK's does.
        let current = state.current.or_else(|| ui.chosen(ids.chooser)).unwrap_or(0);
        ui.set_drop_down(ids.chooser, &names, current);
        ui.set_visible(ids.chooser, names.len() > 1);
        self.buses = state.players.iter().map(|p| p.bus_name.clone()).collect();
        self.show_position(ui, player);
    }

    fn show_position(&mut self, ui: &mut Ui, player: &Player) {
        self.playing = player.playing;
        self.length_us = player.length_us;
        self.anchor = player
            .position_us
            .map(|position| (Instant::now(), position));
        let seekable = player.can_seek && player.length_us.is_some_and(|length| length > 0);
        let Some(seek) = self.ids.as_ref().and_then(|ids| ids.seek) else {
            return;
        };
        ui.set_visible(seek, seekable);
        if let Some(length) = player.length_us.filter(|_| seekable) {
            ui.set_range(seek, 0.0, length as f64 / 1_000_000.0, 1.0);
        }
        self.tick(ui);
    }

    /// Draws the position the player is at now; the panel calls it every second while the
    /// section is on screen.
    pub fn tick(&self, ui: &mut Ui) {
        let (Some(seek), Some(position)) =
            (self.ids.as_ref().and_then(|i| i.seek), self.position_us())
        else {
            return;
        };
        // A position set from the model is not the person's: its event is dropped.
        let _ = ui.set_value(seek, position as f64 / 1_000_000.0);
    }

    /// Whether the section is on screen and follows a playing track's clock.
    pub fn ticking(&self, ui: &Ui) -> bool {
        self.playing
            && self
                .ids
                .as_ref()
                .is_some_and(|ids| ui.is_visible(ids.group))
    }

    fn position_us(&self) -> Option<i64> {
        let (at, position) = self.anchor?;
        let moved = if self.playing {
            i64::try_from(at.elapsed().as_micros()).unwrap_or(0)
        } else {
            0
        };
        // `end` is never below 0 and `clamp` would panic on an inverted range.
        let end = self.length_us.unwrap_or(i64::MAX).max(0);
        Some(position.saturating_add(moved).clamp(0, end))
    }

    /// The command `seconds` into the track asks for: an offset from where the player is.
    pub fn seek_to(&mut self, seconds: f64) -> Option<MediaCommand> {
        let position = self.position_us()?;
        let target = (seconds * 1_000_000.0).round() as i64;
        self.anchor = Some((Instant::now(), target));
        // Every number here is a player's: saturated, never overflowed.
        Some(MediaCommand::Seek(target.saturating_sub(position)))
    }

    /// The command `event` of the section's widgets asks for; `None` for another widget's.
    pub fn event(&mut self, ui: &Ui, event: &Event) -> Option<MediaCommand> {
        let ids = self.ids.as_ref()?;
        match *event {
            Event::Clicked(id) if id == ids.previous => Some(MediaCommand::Previous),
            Event::Clicked(id) if id == ids.play => Some(MediaCommand::PlayPause),
            Event::Clicked(id) if id == ids.next => Some(MediaCommand::Next),
            Event::Changed(id) if Some(id) == ids.seek => {
                let seconds = ui.value(id)?;
                self.seek_to(seconds)
            }
            Event::Changed(id) if id == ids.chooser => {
                let bus = self.buses.get(ui.chosen(id)?)?.clone();
                Some(MediaCommand::Choose(bus))
            }
            _ => None,
        }
    }
}
