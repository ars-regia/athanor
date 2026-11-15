//! The bar's audio (doc_bar.md, BR3), as the GTK bar's `ui/audio.rs` was: a button with the icon
//! of the device in use, and a popover with athanor-controls' audio page, the media controls
//! included. The module is hidden while the sound server reports no device, and without the
//! runtime of the models.

use athanor_bar::i18n::tr;
use athanor_controls::bridge;
use athanor_controls::sal::audio::{MediaIcons, Page};
use athanor_controls::sal::Turn;
use athanor_sal::{Event, Ui, WidgetId};

use super::{controls, Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct Audio {
    page: Page,
    /// The icon the buttons show, as last drawn; `None` while hidden.
    drawn: Option<&'static str>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let Some(services) = bridge::services() else {
        tracing::warn!("no runtime for the models; the audio module is hidden");
        return None;
    };
    // The page first: its follower of each model is woken before the bar's.
    let page = Page::new(&services);
    bridge::follow(services.audio.0.clone(), controls::waker(start.wake));
    bridge::follow(services.media.0.clone(), controls::waker(start.wake));
    Some(Box::new(Audio { page, drawn: None }))
}

/// The icon of the device in use; `None` while there is none.
fn icon(page: &Page) -> Option<&'static str> {
    page.face().map(|face| face.icon)
}

impl Module for Audio {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        self.drawn = icon(&self.page);
        let icon = self.drawn?;
        Some(
            cx.icons
                .button(ui, group, icon, ICON, cx.scale, &tr("Sound"), None),
        )
    }

    fn stale(&self, _cx: &Cx) -> bool {
        icon(&self.page) != self.drawn
    }

    /// The captures open the popover once the media controls are final, as the GTK bar did: a
    /// popover keeps the size of its first frame.
    fn ready(&self) -> bool {
        self.page.face().is_some_and(|face| face.settled)
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let (mut ui, frame) = look::popover(&tr("Sound"), cx.theme.clone(), cx.rtl, cx.top);
        let media = media_icons(cx);
        self.page
            .build(&mut ui, frame, media, &controls::look(cx.icons, cx.scale));
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        self.page.event(ui, &event);
        Reply::Stay
    }

    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        if icon(&self.page).is_none() {
            // The last device left: the popover closes with its button.
            return Reply::Close;
        }
        let size = ui.size();
        match self.page.turn(ui) {
            Turn::Unchanged => Reply::Stay,
            // A device came or went, or a section or the media controls showed or hid: the
            // popover keeps its size once open, so it is built again at the new one.
            Turn::Rebuild => Reply::Reopen,
            Turn::Updated if ui.layout() != size => Reply::Reopen,
            Turn::Updated => Reply::Stay,
        }
    }
}

/// The icons of the media controls at a module's size; `None` when the theme lacks one, and
/// the buttons then show their names.
fn media_icons(cx: &mut Cx) -> Option<MediaIcons> {
    let mut image = |name| cx.icons.image(name, ICON, cx.scale);
    Some(MediaIcons {
        size: f32::from(ICON),
        previous: image(MediaIcons::PREVIOUS)?,
        play: image(MediaIcons::PLAY)?,
        pause: image(MediaIcons::PAUSE)?,
        next: image(MediaIcons::NEXT)?,
    })
}
