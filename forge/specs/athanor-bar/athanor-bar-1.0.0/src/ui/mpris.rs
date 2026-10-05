//! The media controls' player (doc_bar.md, BR3): the first MPRIS player on the session bus,
//! by name, followed by the model in athanor-services while it owns its name. A player that
//! appears or leaves makes the choice again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_services::media::{self, MediaCommand, MediaState, Track};
use tokio::sync::mpsc;

use super::bridge;

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub track: Track,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

pub struct Media {
    /// The model's last state, which `bridge::follow` keeps current. Without a runtime there
    /// is no model and nothing to wait for, so it starts settled.
    state: RefCell<MediaState>,
    refused: Cell<u32>,
    commands: Option<mpsc::UnboundedSender<MediaCommand>>,
}

impl Media {
    /// `notify` runs when the state changed; `failed` when a player refused a command or did
    /// not answer.
    pub fn new(notify: impl Fn() + 'static, failed: impl Fn() + 'static) -> Rc<Media> {
        let model = bridge::runtime().map(|(handle, buses)| media::spawn(&handle, buses));
        let (states, commands) = match model {
            Some((states, commands)) => (Some(states), Some(commands)),
            None => {
                tracing::warn!("no runtime for the models; the media controls are hidden");
                (None, None)
            }
        };
        let media = Rc::new(Media {
            state: RefCell::new(MediaState {
                settled: states.is_none(),
                ..MediaState::default()
            }),
            refused: Cell::new(0),
            commands,
        });
        if let Some(states) = states {
            let weak = Rc::downgrade(&media);
            bridge::follow(states, move |state| {
                let Some(media) = weak.upgrade() else { return };
                let refused = media.refused.replace(state.refused) != state.refused;
                media.state.replace(state.clone());
                notify();
                if refused {
                    failed();
                }
            });
        }
        media
    }

    /// Whether the media controls show what they will keep showing: the players were
    /// listed, and the followed one, if any, has answered with its properties.
    pub fn settled(&self) -> bool {
        self.state.borrow().settled
    }

    pub fn now(&self) -> Option<NowPlaying> {
        let state = self.state.borrow();
        let player = state.current_player()?;
        Some(NowPlaying {
            track: player.track.clone()?,
            playing: player.playing,
            can_next: player.can_next,
            can_previous: player.can_previous,
        })
    }

    /// Hands `command` to the model, which carries it out on the followed player; the `failed`
    /// of [`Media::new`] runs if the player refuses it or does not answer.
    pub fn command(&self, command: MediaCommand) {
        if let Some(commands) = &self.commands {
            if commands.send(command).is_err() {
                tracing::debug!("the media model ended; the command is dropped");
            }
        }
    }
}
