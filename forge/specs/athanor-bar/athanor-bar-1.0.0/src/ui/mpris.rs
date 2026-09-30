//! The media controls' player (doc_bar.md, BR3): the first MPRIS player on the session bus,
//! by name, mirrored while it owns its name. A player that appears or leaves makes the
//! choice again.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_bar::audio::{self, Track};
use athanor_bar::props;
use gtk4::{gio, glib};

use super::bus::{self, Mirror, Source};

const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub track: Track,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

pub struct Media {
    session: RefCell<Option<gio::DBusConnection>>,
    player: RefCell<Option<(String, Rc<Mirror>)>>,
    notify: Rc<dyn Fn()>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

impl Media {
    pub fn new(notify: impl Fn() + 'static) -> Rc<Media> {
        let media = Rc::new(Media {
            session: RefCell::new(None),
            player: RefCell::new(None),
            notify: Rc::new(notify),
            subscription: RefCell::new(None),
        });
        let weak = Rc::downgrade(&media);
        glib::spawn_future_local(async move {
            let session = match gio::bus_get_future(gio::BusType::Session).await {
                Ok(session) => session,
                Err(err) => {
                    tracing::warn!(error = %err, "no session bus; the media controls are hidden");
                    return;
                }
            };
            let Some(media) = weak.upgrade() else { return };
            let watcher = Rc::downgrade(&media);
            let subscription = session.subscribe_to_signal(
                Some(DBUS),
                Some(DBUS),
                Some("NameOwnerChanged"),
                Some(DBUS_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let player = signal
                        .parameters
                        .try_child_value(0)
                        .and_then(|name| name.str().map(audio::is_player))
                        .unwrap_or(false);
                    if let (true, Some(media)) = (player, watcher.upgrade()) {
                        media.choose();
                    }
                },
            );
            media.subscription.replace(Some(subscription));
            media.session.replace(Some(session));
            media.choose();
        });
        media
    }

    /// Follows the first player by name; the choice is stable while players come and go.
    fn choose(self: &Rc<Self>) {
        let Some(session) = self.session.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let reply = bus::call(
                &session,
                DBUS,
                DBUS_PATH,
                DBUS,
                "ListNames",
                None,
                bus::TIMEOUT_MS,
            )
            .await;
            let Some(media) = weak.upgrade() else { return };
            let names = reply
                .ok()
                .filter(|reply| props::has_type(reply, "(as)"))
                .and_then(|reply| reply.try_child_value(0))
                .and_then(|names| names.get::<Vec<String>>());
            let Some(names) = names else {
                tracing::warn!("ListNames failed; the media controls keep their player");
                return;
            };
            let first = names
                .into_iter()
                .filter(|name| audio::is_player(name))
                .min();
            let current = media.player.borrow().as_ref().map(|(name, _)| name.clone());
            if first == current {
                return;
            }
            let player = first.map(|name| {
                let notify = media.notify.clone();
                let mirror = Mirror::new(
                    &session,
                    &name,
                    Source::Fixed(vec![(audio::MPRIS_PATH, audio::MPRIS_PLAYER)]),
                    move || notify(),
                );
                (name, mirror)
            });
            media.player.replace(player);
            (media.notify)();
        });
    }

    pub fn now(&self) -> Option<NowPlaying> {
        let player = self.player.borrow();
        let (_, mirror) = player.as_ref()?;
        let objects = mirror.objects();
        let props = props::lookup(&objects, audio::MPRIS_PATH, audio::MPRIS_PLAYER)?;
        Some(NowPlaying {
            track: audio::track(props)?,
            playing: audio::playing(props),
            can_next: props::value::<bool>(props, "CanGoNext").unwrap_or(false),
            can_previous: props::value::<bool>(props, "CanGoPrevious").unwrap_or(false),
        })
    }

    /// `PlayPause`, `Next` or `Previous` to the followed player.
    pub fn command(&self, method: &'static str) {
        let session = self.session.borrow().clone();
        let player = self.player.borrow();
        let (Some(session), Some((name, _))) = (session, player.as_ref()) else {
            return;
        };
        bus::spawn(
            method,
            bus::call(
                &session,
                name,
                audio::MPRIS_PATH,
                audio::MPRIS_PLAYER,
                method,
                None,
                bus::TIMEOUT_MS,
            ),
        );
    }
}
