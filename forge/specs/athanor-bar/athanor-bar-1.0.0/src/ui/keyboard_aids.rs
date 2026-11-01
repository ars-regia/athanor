//! What the bar says of the keyboard aids until `athanor-osd` exists (doc_accessibility.md AX11):
//! one service per bar, so that a notice is announced once however many surfaces carry the
//! accessibility module, which shows what the service holds.

use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_bar::keyboard_aids::{self, KEYS, SCHEMA};
use athanor_compositor_client::{Event, KeyboardAid, Modifier};
use gtk4::gio;

use super::Bar;
use crate::i18n::{tr, tr_with};

/// How long a rejected key stays on the bar: time to read it, with no measurement behind it.
const REJECTED: Duration = Duration::from_secs(3);
/// How long a gesture's offer stays: time to open the menu, with no measurement behind it.
const OFFERED: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Service {
    settings: OnceCell<gio::Settings>,
    held: Cell<(u32, u32)>,
    rejected: Cell<Option<(KeyboardAid, Instant)>>,
    offer: Cell<Option<(KeyboardAid, Instant)>>,
}

fn aid_name(aid: KeyboardAid) -> String {
    match aid {
        KeyboardAid::Sticky => tr("sticky keys"),
        KeyboardAid::Slow => tr("slow keys"),
        KeyboardAid::Bounce => tr("bounce keys"),
    }
}

fn modifier_name(modifier: Modifier) -> String {
    match modifier {
        Modifier::Shift => tr("Shift"),
        Modifier::Control => tr("Control"),
        Modifier::Alt => tr("Alt"),
        Modifier::Super => tr("Super"),
        Modifier::AltGr => tr("AltGr"),
    }
}

/// The modifiers held, a locked one said as such. Empty when none is.
fn held_text((latched, locked): (u32, u32)) -> String {
    let held: Vec<String> = Modifier::ALL
        .into_iter()
        .filter_map(|modifier| {
            let name = modifier_name(modifier);
            if modifier.is_in(locked) {
                Some(tr_with("{modifier} locked", "modifier", &name))
            } else if modifier.is_in(latched) {
                Some(tr_with("{modifier} latched", "modifier", &name))
            } else {
                None
            }
        })
        .collect();
    held.join(", ")
}

fn rejected_text(aid: KeyboardAid) -> String {
    tr_with("Key rejected: {aid}", "aid", &aid_name(aid))
}

pub fn offer_text(aid: KeyboardAid) -> String {
    tr_with("Turn on {aid}?", "aid", &aid_name(aid))
}

/// The value of `cell` while it is younger than `limit`.
fn fresh(cell: &Cell<Option<(KeyboardAid, Instant)>>, limit: Duration) -> Option<KeyboardAid> {
    cell.get()
        .filter(|(_, at)| at.elapsed() < limit)
        .map(|(aid, _)| aid)
}

impl Service {
    /// Sends the keys to the compositor, now and at each change. Without a key of `KEYS` nothing
    /// is sent and every aid stays off, as it is until settings arrive.
    pub fn start(&self, bar: &Rc<Bar>) {
        let Some(client) = bar.client() else {
            return;
        };
        if let Err(err) = client.watch_accessibility() {
            tracing::warn!(error = %err, "cannot watch the compositor's accessibility state");
            return;
        }
        let schema =
            gio::SettingsSchemaSource::default().and_then(|source| source.lookup(SCHEMA, true));
        if !schema.is_some_and(|schema| KEYS.iter().all(|key| schema.has_key(key))) {
            tracing::warn!("{SCHEMA} lacks a key of the keyboard aids; they stay off");
            return;
        }
        let settings = gio::Settings::new(SCHEMA);
        let weak = Rc::downgrade(bar);
        keyboard_aids::follow(&settings, move |aids| {
            let Some(bar) = weak.upgrade() else {
                return;
            };
            if let Some(Err(err)) = bar.client().map(|client| client.set_keyboard_aids(aids)) {
                tracing::info!(error = %err, "the keyboard aids' keys cannot reach the compositor");
            }
        });
        if self.settings.set(settings).is_err() {
            tracing::warn!("the keyboard aids' keys are followed already");
        }
    }

    /// Takes in `event`, and returns what to announce of it.
    pub fn heard(&self, event: &Event) -> Option<String> {
        match event {
            Event::Accessibility(state) => {
                let held = (state.latched, state.locked);
                if self.held.replace(held) == held {
                    return None;
                }
                let text = held_text(held);
                Some(if text.is_empty() {
                    tr("No modifier held")
                } else {
                    text
                })
            }
            Event::AidRejected(aid) => {
                self.rejected.set(Some((*aid, Instant::now())));
                Some(rejected_text(*aid))
            }
            Event::AidGesture(aid) => {
                self.offer.set(Some((*aid, Instant::now())));
                Some(offer_text(*aid))
            }
            Event::AidOff(aid) => {
                self.write(*aid, false);
                None
            }
            _ => None,
        }
    }

    pub fn offer(&self) -> Option<KeyboardAid> {
        fresh(&self.offer, OFFERED)
    }

    /// What the bar's button shows beside its icon. Empty when there is nothing to show.
    pub fn shown(&self) -> String {
        let shown: Vec<String> = [
            held_text(self.held.get()),
            fresh(&self.rejected, REJECTED).map_or_else(String::new, rejected_text),
            self.offer().map_or_else(String::new, offer_text),
        ]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect();
        shown.join(", ")
    }

    /// The offer accepted: the aid's key is written, and the change reaches the compositor.
    pub fn accept(&self) {
        if let Some((aid, _)) = self.offer.take() {
            self.write(aid, true);
        }
    }

    fn write(&self, aid: KeyboardAid, on: bool) {
        let Some(settings) = self.settings.get() else {
            tracing::warn!(?aid, on, "no key of the keyboard aids to write");
            return;
        };
        if let Err(err) = keyboard_aids::write(settings, aid, on) {
            tracing::error!(error = %err, ?aid, on, "cannot write a keyboard aid's key");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_compositor_client::Accessibility;

    fn held(latched: u32, locked: u32) -> Event {
        Event::Accessibility(Accessibility {
            latched,
            locked,
            ..Accessibility::default()
        })
    }

    #[test]
    fn each_change_of_the_modifiers_is_announced_once() {
        let service = Service::default();
        let shift = Modifier::Shift as u32;
        let control = Modifier::Control as u32;
        assert_eq!(service.heard(&held(0, 0)), None, "nothing held yet");
        assert_eq!(
            service.heard(&held(shift, control)).as_deref(),
            Some("Shift latched, Control locked")
        );
        assert_eq!(service.heard(&held(shift, control)), None, "no change");
        assert_eq!(service.shown(), "Shift latched, Control locked");
        assert_eq!(
            service.heard(&held(0, 0)).as_deref(),
            Some("No modifier held")
        );
        assert_eq!(service.shown(), "");
    }

    #[test]
    fn a_rejected_key_is_shown_for_a_moment() {
        let service = Service::default();
        let said = service.heard(&Event::AidRejected(KeyboardAid::Slow));
        assert_eq!(said.as_deref(), Some("Key rejected: slow keys"));
        assert_eq!(service.shown(), "Key rejected: slow keys");
        let past = Instant::now() - REJECTED;
        service.rejected.set(Some((KeyboardAid::Slow, past)));
        assert_eq!(service.shown(), "", "and then it is gone");
    }

    #[test]
    fn a_gesture_offers_its_aid_until_taken() {
        let service = Service::default();
        let said = service.heard(&Event::AidGesture(KeyboardAid::Sticky));
        assert_eq!(said.as_deref(), Some("Turn on sticky keys?"));
        assert_eq!(service.offer(), Some(KeyboardAid::Sticky));
        service.accept();
        assert_eq!(service.offer(), None, "accepted");
        service.heard(&Event::AidGesture(KeyboardAid::Slow));
        let past = Instant::now() - OFFERED;
        service.offer.set(Some((KeyboardAid::Slow, past)));
        assert_eq!(service.offer(), None, "or expired");
    }
}
