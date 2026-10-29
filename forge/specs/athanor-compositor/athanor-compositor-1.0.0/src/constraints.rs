//! What a client may hold of the input, under doc_own_stack.md decision 30 (all three
//! `standard`): its pointer locked or confined (pointer-constraints-unstable-v1), the pointer's
//! raw motion (relative-pointer-unstable-v1) and the compositor's key bindings
//! (keyboard-shortcuts-inhibit-unstable-v1).
//!
//! - A lock or a confinement is active only while its surface has the keyboard.
//! - While a surface with an active inhibitor has the keyboard, the bound keys go to it, but
//!   those `Action::never_inhibited` names. No dialog asks first.
//! - Ctrl+Alt+Tab suspends the inhibitors and the constraints of the client of the focused
//!   surface, on all its surfaces, whether or not the cycle has a surface to go to; until the
//!   user clicks one of its surfaces or moves the keyboard to one by a key. A focus by
//!   xdg-activation or a new surface lifts nothing, and what the client asks for meanwhile stays
//!   inactive. On the lift its persistent constraints, and those asked for meanwhile, apply
//!   again; a oneshot constraint active when it was suspended has ended (Smithay destroys it).
//!
//! Derived in part from niri (https://github.com/niri-wm/niri), revision
//! fcbc971cf49a987b1e8d5ff1d64da6b1f61a1787: `pointer_moved_by` from `on_pointer_motion`
//! (src/input/mod.rs), `refresh_constraint` from `maybe_activate_pointer_constraint`
//! (src/niri.rs), and the inhibitor list of `KeyboardShortcutsInhibitHandler`
//! (src/handlers/mod.rs).
//! Copyright (C) the niri authors (Ivan Molodetskikh and contributors).
//! niri is free software: you can redistribute it and/or modify it under the terms of the GNU
//! General Public License as published by the Free Software Foundation, either version 3 of the
//! License, or (at your option) any later version (GPL-3.0-or-later). It is distributed in the
//! hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
//! more details.
//! Changed for Athanor on 2026-10-29: a constraint applies only while its surface has the
//! keyboard and its client is not suspended, and it is deactivated otherwise; an inhibitor is
//! activated unless its client is suspended; a constraint is looked up on the surface the one
//! under the pointer is a subsurface of; motion off the outputs, confined or not, is kept on
//! them by `input::confine`; an absolute motion under a constraint is taken as a relative one;
//! the hot corner, the grabs' warps and the position hint are left out.

use crate::state::State;
use smithay::backend::input::InputTime;
use smithay::input::pointer::{MotionEvent, PointerHandle, RelativeMotionEvent};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::compositor::{
    get_parent, with_states, RegionAttributes, SubsurfaceCachedState,
};
use smithay::wayland::keyboard_shortcuts_inhibit::{
    KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
};
use smithay::wayland::pointer_constraints::{
    with_pointer_constraint, PointerConstraint, PointerConstraintsHandler, PointerConstraintsState,
};
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use std::collections::HashMap;

/// The inhibitors and the suspended clients.
pub struct Held {
    shortcuts: KeyboardShortcutsInhibitState,
    inhibitors: Vec<KeyboardShortcutsInhibitor>,
    /// The clients Ctrl+Alt+Tab suspended; one that has left is dropped at the next suspension.
    pub suspended: Vec<ClientId>,
    /// Where each absolute device, by its `Device::id`, last put the pointer, or would have,
    /// had no lock kept it: a lock sends the move from there as relative motion. A device's
    /// entry goes when the device does.
    pub(crate) last_absolute: HashMap<String, Point<f64, Logical>>,
}

impl Held {
    /// Creates the three globals.
    pub fn new(dh: &DisplayHandle) -> Self {
        PointerConstraintsState::new::<State>(dh);
        RelativePointerManagerState::new::<State>(dh);
        Self {
            shortcuts: KeyboardShortcutsInhibitState::new::<State>(dh),
            inhibitors: Vec::new(),
            suspended: Vec::new(),
            last_absolute: HashMap::new(),
        }
    }
}

/// How the active constraint of the surface under the pointer holds it.
enum Hold {
    Locked,
    /// To the surface, at its origin, and to the region when there is one.
    Confined(WlSurface, Point<f64, Logical>, Option<RegionAttributes>),
}

/// The surface `surface` is a subsurface of, through every level, and where it is, from
/// `origin`, where `surface` is; `surface` itself when it is no subsurface.
fn root_of(surface: WlSurface, origin: Point<f64, Logical>) -> (WlSurface, Point<f64, Logical>) {
    let (mut surface, mut origin) = (surface, origin);
    while let Some(parent) = get_parent(&surface) {
        let offset = with_states(&surface, |states| {
            states
                .cached_state
                .get::<SubsurfaceCachedState>()
                .current()
                .location
        });
        origin -= offset.to_f64();
        surface = parent;
    }
    (surface, origin)
}

impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, _surface: &WlSurface, _pointer: &PointerHandle<Self>) {
        self.refresh_constraint();
    }
}

impl KeyboardShortcutsInhibitHandler for State {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.input_hold.shortcuts
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        if !self.is_suspended(inhibitor.wl_surface()) {
            inhibitor.activate();
        }
        self.input_hold.inhibitors.push(inhibitor);
    }

    fn inhibitor_destroyed(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        self.input_hold.inhibitors.retain(|kept| *kept != inhibitor);
    }
}

impl State {
    fn keyboard_focus(&self) -> Option<WlSurface> {
        self.seat.get_keyboard()?.current_focus()
    }

    fn is_suspended(&self, surface: &WlSurface) -> bool {
        surface
            .client()
            .is_some_and(|client| self.input_hold.suspended.contains(&client.id()))
    }

    /// Whether `surface`, or the surface it is a subsurface of, has the keyboard.
    fn has_keyboard(&self, surface: &WlSurface) -> bool {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        self.keyboard_focus() == Some(root)
    }

    /// Whether the bound keys go to the focused surface: it holds an active inhibitor. Not
    /// while the session is locked, when only the lock's keys act.
    pub(crate) fn shortcuts_inhibited(&self) -> bool {
        let Some(focus) = self.keyboard_focus() else {
            return false;
        };
        self.lock.is_none()
            && self
                .input_hold
                .inhibitors
                .iter()
                .any(|inhibitor| *inhibitor.wl_surface() == focus && inhibitor.is_active())
    }

    /// The active constraint the pointer at `at` is held by, looked up on the root of the
    /// surface under it: a lock on a window's surface holds over its subsurfaces too. None
    /// when the pointer is outside the constraint's region.
    fn hold(&self, pointer: &PointerHandle<Self>, at: Point<f64, Logical>) -> Option<Hold> {
        let (surface, origin) = self.surface_under(at)?;
        let (root, origin) = root_of(surface, origin);
        with_pointer_constraint(&root, pointer, |constraint| {
            let constraint = constraint.filter(|constraint| constraint.is_active())?;
            let region = constraint.region().cloned();
            if region
                .as_ref()
                .is_some_and(|region| !region.contains((at - origin).to_i32_round()))
            {
                return None;
            }
            Some(match &*constraint {
                PointerConstraint::Locked(_) => Hold::Locked,
                PointerConstraint::Confined(_) => Hold::Confined(root.clone(), origin, region),
            })
        })
    }

    /// Activates the constraint of the surface the pointer is on, or of the surface that one
    /// is a subsurface of, when it may apply, inside its region if it has one, and deactivates
    /// it when it may not.
    ///
    /// Known limit: when a confined pointer moves from its surface onto one of its subsurfaces,
    /// Smithay's `PointerTarget::leave` for `WlSurface` deactivates the surface's constraint
    /// before this runs: a persistent confinement is sent `unconfined` then `confined` again
    /// here, and a oneshot one has ended. The pointer focus is the surface under the pointer, as
    /// wl_pointer requires, so nothing here can keep the leave from happening; the fix belongs
    /// in Smithay, which would have to keep a constraint across a move between surfaces of one
    /// tree (`fixture::constraints::a_move_onto_a_subsurface_reconfines`).
    pub(crate) fn refresh_constraint(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(focus) = pointer.current_focus() else {
            return;
        };
        let at = pointer.current_location();
        let under = self
            .surface_under(at)
            .filter(|(surface, _)| *surface == focus)
            .map(|(surface, origin)| root_of(surface, origin));
        let root = match &under {
            Some((root, _)) => root.clone(),
            None => root_of(focus, Point::default()).0,
        };
        let allowed = self.has_keyboard(&root) && !self.is_suspended(&root);
        with_pointer_constraint(&root, &pointer, |constraint| {
            let Some(constraint) = constraint else {
                return;
            };
            if !allowed {
                constraint.deactivate();
                return;
            }
            let Some((_, origin)) = under else {
                return;
            };
            if constraint.is_active() {
                return;
            }
            if let Some(region) = constraint.region() {
                if !region.contains((at - origin).to_i32_round()) {
                    return;
                }
            }
            constraint.activate();
        });
    }

    /// A relative motion by `delta`: the pointer stays where it is while locked, and on its
    /// surface, in its region and on the outputs while confined; the surface it is on is sent
    /// the motion as relative-pointer-unstable-v1 reports it, whether the pointer moved or not.
    pub(crate) fn pointer_moved_by(
        &mut self,
        delta: Point<f64, Logical>,
        delta_unaccel: Point<f64, Logical>,
        time: InputTime,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let from = pointer.current_location();
        let outputs = self.output_areas();
        let to = match self.hold(&pointer, from) {
            Some(Hold::Locked) => None,
            Some(Hold::Confined(root, origin, region)) => {
                crate::input::confine(&outputs, from, from + delta).filter(|to| {
                    let on_root = self
                        .surface_under(*to)
                        .is_some_and(|(under, at)| root_of(under, at).0 == root);
                    on_root
                        && region
                            .as_ref()
                            .is_none_or(|region| region.contains((*to - origin).to_i32_round()))
                })
            }
            None => crate::input::confine(&outputs, from, from + delta),
        };
        if let Some(location) = to {
            let under = self.surface_under(location);
            pointer.motion(
                self,
                under,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
        }
        // Smithay sends it to the pointer's focus.
        let focus = pointer
            .current_focus()
            .map(|surface| (surface, Point::default()));
        pointer.relative_motion(
            self,
            focus,
            &RelativeMotionEvent {
                delta,
                delta_unaccel,
                time,
            },
        );
        pointer.frame(self);
        self.refresh_constraint();
    }

    /// An absolute motion of `device` to `to`, from a nested backend, an absolute pointer or a
    /// virtual pointer: a locked pointer stays, and its surface is sent the move from where the
    /// same device last was as relative motion, or nothing at its first sample; a confined
    /// pointer goes to `to` only on its surface and in its region; otherwise the pointer goes
    /// to `to`.
    pub(crate) fn pointer_moved_to(
        &mut self,
        device: String,
        to: Point<f64, Logical>,
        time: InputTime,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        let last = self.input_hold.last_absolute.insert(device, to);
        let delta = match self.hold(&pointer, at) {
            None => return self.pointer_to(to, time),
            Some(Hold::Locked) => match last {
                Some(last) => to - last,
                None => return,
            },
            Some(Hold::Confined(..)) => to - at,
        };
        self.pointer_moved_by(delta, delta, time);
    }

    /// An input device went: its last absolute position with it.
    pub(crate) fn absolute_device_gone(&mut self, device: &str) {
        self.input_hold.last_absolute.remove(device);
    }

    /// Ctrl+Alt+Tab: suspends the inhibitors and the constraints of the focused surface's client.
    pub(crate) fn suspend_focused(&mut self) {
        let Some(client) = self.keyboard_focus().and_then(|surface| surface.client()) else {
            return;
        };
        let backend = self.display_handle.backend_handle();
        self.input_hold
            .suspended
            .retain(|id| backend.get_client_data(id.clone()).is_ok());
        let id = client.id();
        if !self.input_hold.suspended.contains(&id) {
            self.input_hold.suspended.push(id.clone());
        }
        for inhibitor in &self.input_hold.inhibitors {
            let owned = inhibitor.wl_surface().client().map(|c| c.id()) == Some(id.clone());
            if owned && inhibitor.is_active() {
                inhibitor.inactivate();
            }
        }
        self.refresh_constraint();
    }

    /// The user clicked a surface of `client`, or moved the keyboard to one by a key: its
    /// suspension, if any, ends.
    pub(crate) fn lift(&mut self, client: &ClientId) {
        let before = self.input_hold.suspended.len();
        self.input_hold.suspended.retain(|id| id != client);
        if self.input_hold.suspended.len() == before {
            return;
        }
        for inhibitor in &self.input_hold.inhibitors {
            let owned = inhibitor.wl_surface().client().map(|c| c.id()) == Some(client.clone());
            if owned && !inhibitor.is_active() {
                inhibitor.activate();
            }
        }
        self.refresh_constraint();
    }

    /// A bound key's action: a key that moves the keyboard to a surface of a suspended client
    /// lifts its suspension.
    pub(crate) fn fire_by_key(&mut self, action: crate::bindings::Action) {
        let before = self.keyboard_focus();
        self.fire(action);
        let after = self.keyboard_focus();
        if after != before {
            if let Some(client) = after.and_then(|surface| surface.client()) {
                self.lift(&client.id());
            }
        }
    }
}
