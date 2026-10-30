//! The magnifier and the colour filters (doc_accessibility.md AX10): cosmic-comp's
//! `cosmic_a11y_manager_v1` up to version 3, bound at connect, else athanor-compositor's
//! `athanor_a11y_manager_v1`, which it offers to the bar alone, bound at the first
//! `Client::watch_accessibility`, so a client that never watches never binds it. Every name of
//! the latter in this crate is in this file; the rest of the crate sees one interface over both.

use wayland_client::globals::GlobalList;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::connection::{bind, Client, Error, State};
use crate::model::ScreenFilter;
use crate::protocols::a11y::client::cosmic_a11y_manager_v1::{
    self, CosmicA11yManagerV1, Filter as CosmicFilter,
};
use crate::protocols::athanor_a11y::client::athanor_a11y_manager_v1::{
    self, AthanorA11yManagerV1, Filter as AthanorFilter,
};

/// The version of `cosmic_a11y_manager_v1` that replaced the screen filter's messages.
const SCREEN_FILTER2: u32 = 3;

pub(crate) enum Manager {
    Athanor(AthanorA11yManagerV1),
    Cosmic(CosmicA11yManagerV1),
}

/// cosmic-comp's manager, at connect.
pub(crate) fn bind_cosmic(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<Manager> {
    bind(globals, qh, 2..=SCREEN_FILTER2).map(Manager::Cosmic)
}

impl Client {
    /// Binds athanor-compositor's manager, so that `accessibility()` and its setters reach it.
    /// Nothing to do under cosmic-comp, whose manager is bound at connect, nor where a manager
    /// is already bound.
    pub fn watch_accessibility(&self) -> Result<(), Error> {
        self.live()?;
        {
            let mut state = self.inner.state.borrow_mut();
            if state.globals.a11y.is_some() {
                return Ok(());
            }
            state.globals.a11y =
                bind(&self.inner.registry, &self.inner.qh, 1..=1).map(Manager::Athanor);
        }
        self.flush()
    }
}

impl Manager {
    pub(crate) fn set_magnifier(&self, enabled: bool) {
        match self {
            Manager::Athanor(manager) => manager.set_magnifier(athanor_state(enabled)),
            Manager::Cosmic(manager) => manager.set_magnifier(cosmic_state(enabled)),
        }
    }

    /// `filter` is never `Unknown`: the caller refuses it.
    pub(crate) fn set_screen_filter(&self, inverted: bool, filter: ScreenFilter) {
        match self {
            Manager::Athanor(manager) => {
                let filter = match filter {
                    ScreenFilter::None => AthanorFilter::None,
                    ScreenFilter::Greyscale => AthanorFilter::Greyscale,
                    ScreenFilter::Protanopia => AthanorFilter::Protanopia,
                    ScreenFilter::Deuteranopia => AthanorFilter::Deuteranopia,
                    ScreenFilter::Tritanopia => AthanorFilter::Tritanopia,
                    ScreenFilter::Unknown => AthanorFilter::Unknown,
                };
                manager.set_screen_filter(athanor_state(inverted), filter);
            }
            Manager::Cosmic(manager) => {
                let selected = match filter {
                    ScreenFilter::None => None,
                    ScreenFilter::Greyscale => Some(CosmicFilter::Greyscale),
                    ScreenFilter::Protanopia => Some(CosmicFilter::DaltonizeProtanopia),
                    ScreenFilter::Deuteranopia => Some(CosmicFilter::DaltonizeDeuteranopia),
                    ScreenFilter::Tritanopia => Some(CosmicFilter::DaltonizeTritanopia),
                    ScreenFilter::Unknown => Some(CosmicFilter::Unknown),
                };
                if manager.version() >= SCREEN_FILTER2 {
                    // Version 3 refuses `disabled`: no filter is the selected one turned off,
                    // and `unknown` keeps the selection.
                    manager.set_screen_filter2(
                        cosmic_state(inverted),
                        selected.unwrap_or(CosmicFilter::Unknown),
                        cosmic_state(selected.is_some()),
                    );
                } else {
                    manager.set_screen_filter(
                        cosmic_state(inverted),
                        selected.unwrap_or(CosmicFilter::Disabled),
                    );
                }
            }
        }
    }
}

fn athanor_state(enabled: bool) -> athanor_a11y_manager_v1::ActiveState {
    if enabled {
        athanor_a11y_manager_v1::ActiveState::Enabled
    } else {
        athanor_a11y_manager_v1::ActiveState::Disabled
    }
}

fn cosmic_state(enabled: bool) -> cosmic_a11y_manager_v1::ActiveState {
    if enabled {
        cosmic_a11y_manager_v1::ActiveState::Enabled
    } else {
        cosmic_a11y_manager_v1::ActiveState::Disabled
    }
}

fn cosmic_filter(filter: WEnum<CosmicFilter>) -> ScreenFilter {
    match filter {
        WEnum::Value(CosmicFilter::Disabled) => ScreenFilter::None,
        WEnum::Value(CosmicFilter::Greyscale) => ScreenFilter::Greyscale,
        WEnum::Value(CosmicFilter::DaltonizeProtanopia) => ScreenFilter::Protanopia,
        WEnum::Value(CosmicFilter::DaltonizeDeuteranopia) => ScreenFilter::Deuteranopia,
        WEnum::Value(CosmicFilter::DaltonizeTritanopia) => ScreenFilter::Tritanopia,
        WEnum::Value(CosmicFilter::Unknown) | WEnum::Unknown(_) => ScreenFilter::Unknown,
    }
}

impl Dispatch<CosmicA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &CosmicA11yManagerV1,
        event: cosmic_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let enabled = |value: WEnum<cosmic_a11y_manager_v1::ActiveState>| {
            value == WEnum::Value(cosmic_a11y_manager_v1::ActiveState::Enabled)
        };
        let mut next = state.accessibility;
        match event {
            cosmic_a11y_manager_v1::Event::Magnifier { active } => next.magnifier = enabled(active),
            cosmic_a11y_manager_v1::Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = cosmic_filter(filter);
            }
            cosmic_a11y_manager_v1::Event::ScreenFilter2 {
                inverted,
                filter,
                filter_state,
            } => {
                next.inverted = enabled(inverted);
                next.filter = if enabled(filter_state) {
                    cosmic_filter(filter)
                } else {
                    ScreenFilter::None
                };
            }
        }
        state.set_accessibility(next);
    }
}

impl Dispatch<AthanorA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &AthanorA11yManagerV1,
        event: athanor_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_a11y_manager_v1::{ActiveState, Event};
        let enabled = |value: WEnum<ActiveState>| value == WEnum::Value(ActiveState::Enabled);
        let mut next = state.accessibility;
        match event {
            Event::Magnifier { active, .. } => next.magnifier = enabled(active),
            Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = match filter {
                    WEnum::Value(AthanorFilter::None) => ScreenFilter::None,
                    WEnum::Value(AthanorFilter::Greyscale) => ScreenFilter::Greyscale,
                    WEnum::Value(AthanorFilter::Protanopia) => ScreenFilter::Protanopia,
                    WEnum::Value(AthanorFilter::Deuteranopia) => ScreenFilter::Deuteranopia,
                    WEnum::Value(AthanorFilter::Tritanopia) => ScreenFilter::Tritanopia,
                    WEnum::Value(AthanorFilter::Unknown) | WEnum::Unknown(_) => {
                        ScreenFilter::Unknown
                    }
                };
            }
            // The keyboard aids' state is #865's.
            _ => return,
        }
        state.set_accessibility(next);
    }
}
