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
//! Changed for Athanor on 2026-11-04 (#860): a confinement holds the pointer to its surface's
//! own input region, its subsurfaces left out, keeps the pointer's focus on that surface, gives
//! it the focus when it activates, and stops a motion that would leave it at the edge rather
//! than dropping the motion.

use crate::state::State;
use smithay::backend::input::InputTime;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::utils::under_from_surface_tree;
use smithay::desktop::WindowSurfaceType;
use smithay::input::pointer::{MotionEvent, PointerHandle, RelativeMotionEvent};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::utils::{Logical, Point, Rectangle, Size, SERIAL_COUNTER};
use smithay::wayland::compositor::{
    get_parent, with_states, RectangleKind, RegionAttributes, SubsurfaceCachedState,
    SurfaceAttributes,
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

/// Whether `at` is in the own input region of `root`, which is at `origin`: its subsurfaces
/// left out, as pointer-constraints-unstable-v1 takes the region of the surface a constraint is
/// made for. It locks the surface's state, so never inside `with_pointer_constraint`.
fn on_surface(root: &WlSurface, origin: Point<f64, Logical>, at: Point<f64, Logical>) -> bool {
    under_from_surface_tree(root, at - origin, (0, 0), WindowSurfaceType::TOPLEVEL).is_some()
}

/// Whether `at` is in `region`, in the coordinates of the surface at `origin`, if there is one.
fn in_region(
    region: Option<&RegionAttributes>,
    origin: Point<f64, Logical>,
    at: Point<f64, Logical>,
) -> bool {
    region.is_none_or(|region| region.contains((at - origin).to_i32_round()))
}

/// At most this many rectangles make up a confinement's area, and past it their bounds stand
/// for them: the client sets the regions, and a motion looks at every rectangle.
const AREA_RECTS: usize = 64;

/// The bounds of `rects`, if there are any.
fn bounds(
    rects: impl IntoIterator<Item = Rectangle<i32, Logical>>,
) -> Option<Rectangle<i32, Logical>> {
    rects.into_iter().reduce(Rectangle::merge)
}

/// The rectangles of `region`, which may overlap: each subtraction cut out of the additions
/// before it, as `RegionAttributes::contains` reads them. Past `AREA_RECTS` of them, the
/// bounds of its additions, holes and all.
fn rects_of(region: &RegionAttributes) -> Vec<Rectangle<i32, Logical>> {
    let mut rects = Vec::new();
    for (kind, rect) in &region.rects {
        match kind {
            RectangleKind::Add => rects.push(*rect),
            RectangleKind::Subtract => rects = Rectangle::subtract_rects_many(rects, [*rect]),
        }
        if rects.len() > AREA_RECTS {
            let adds = region.rects.iter().filter(|(kind, _)| matches!(kind, RectangleKind::Add));
            return bounds(adds.map(|(_, rect)| *rect)).into_iter().collect();
        }
    }
    rects
}

/// A confinement's area in its surface's coordinates, as at most `AREA_RECTS` rectangles: the
/// surface of `size` cut to each of `regions`, or the bounds of the cuts past that many.
fn area_of<'a>(
    size: Size<i32, Logical>,
    regions: impl IntoIterator<Item = &'a RegionAttributes>,
) -> Vec<Rectangle<i32, Logical>> {
    regions
        .into_iter()
        .fold(vec![Rectangle::from_size(size)], |areas, region| {
            let rects = rects_of(region);
            let cut: Vec<_> = areas
                .iter()
                .flat_map(|area| rects.iter().filter_map(move |rect| rect.intersection(*area)))
                .collect();
            if cut.len() > AREA_RECTS {
                bounds(cut).into_iter().collect()
            } else {
                cut
            }
        })
}

/// Whether a constraint of `root`, which is at `origin`, may hold the pointer at `at`: on the
/// surface's own input region and in `region`.
fn confines(
    root: &WlSurface,
    origin: Point<f64, Logical>,
    region: Option<&RegionAttributes>,
    at: Point<f64, Logical>,
) -> bool {
    on_surface(root, origin, at) && in_region(region, origin, at)
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

    /// The surface the pointer at `at` goes to: under an active confinement, the surface it
    /// was made for wherever the confinement may hold the pointer, over that surface's
    /// subsurfaces too; else the surface under `at`. Smithay's `PointerTarget::leave` for
    /// `WlSurface` deactivates the constraint of the surface the pointer leaves, so a focus
    /// that went to a subsurface would end the confinement, a oneshot one for good (#860).
    pub(crate) fn pointer_target(
        &self,
        at: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let held = self
            .seat
            .get_pointer()
            .and_then(|pointer| self.hold(&pointer, at));
        match held {
            Some(Hold::Confined(root, origin, region))
                if confines(&root, origin, region.as_ref(), at) =>
            {
                Some((root, origin))
            }
            _ => self.surface_under(at),
        }
    }

    /// Activates the constraint of the surface the pointer is on, or of the surface that one
    /// is a subsurface of, when it may apply, inside that surface's own input region and its
    /// region if it has one, and deactivates it when it may not. A confinement that activates
    /// over a subsurface gives the pointer's focus to its surface at once (`State::repoint`),
    /// as "confined" promises, so that a click before the next motion reaches that surface.
    /// A lock, too, activates only over its surface's own input region, not over a subsurface
    /// outside it, as the protocol scopes either constraint to the surface it is made for.
    pub(crate) fn refresh_constraint(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(focus) = pointer.current_focus() else {
            return;
        };
        let at = pointer.current_location();
        // The focus is the surface under the pointer, or under a confinement its surface
        // (`pointer_target`): either way a surface of the tree under the pointer.
        let root = root_of(focus, Point::default()).0;
        let under = self
            .surface_under(at)
            .map(|(surface, origin)| root_of(surface, origin))
            .filter(|(under, _)| *under == root);
        let on = under
            .as_ref()
            .is_some_and(|(_, origin)| on_surface(&root, *origin, at));
        let allowed = self.has_keyboard(&root) && !self.is_suspended(&root);
        let activated = with_pointer_constraint(&root, &pointer, |constraint| {
            let Some(constraint) = constraint else {
                return false;
            };
            if !allowed {
                constraint.deactivate();
                return false;
            }
            let Some((_, origin)) = under else {
                return false;
            };
            if constraint.is_active() || !on || !in_region(constraint.region(), origin, at) {
                return false;
            }
            constraint.activate();
            true
        });
        if activated {
            self.repoint();
        }
    }

    /// A relative motion by `delta`: the pointer stays where it is while locked, and on its
    /// surface, in its region and on the outputs while confined, at the edge it would cross;
    /// the surface it is on is sent the motion as relative-pointer-unstable-v1 reports it,
    /// whether the pointer moved or not.
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
                crate::input::confine(&outputs, from, from + delta)
                    .and_then(|to| self.confined_to(&outputs, &root, origin, region.as_ref(), to))
            }
            None => crate::input::confine(&outputs, from, from + delta),
        };
        if let Some(location) = to {
            let under = self.pointer_target(location);
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

    /// Where a pointer confined to `root`, which is at `origin`, and to `region` stops on its
    /// way to `to`: at `to` when the confinement may hold it there, on `outputs` and not under
    /// another window; else at the nearest such point of the confinement's area, the surface's
    /// input region and `region`, the edge it ran into. None when there is no such point.
    fn confined_to(
        &self,
        outputs: &[Rectangle<i32, Logical>],
        root: &WlSurface,
        origin: Point<f64, Logical>,
        region: Option<&RegionAttributes>,
        to: Point<f64, Logical>,
    ) -> Option<Point<f64, Logical>> {
        let holds = |at: &Point<f64, Logical>| {
            outputs.iter().any(|area| area.to_f64().contains(*at))
                && self
                    .surface_under(*at)
                    .is_some_and(|(under, on)| root_of(under, on).0 == *root)
                && confines(root, origin, region, *at)
        };
        if holds(&to) {
            return Some(to);
        }
        // The area in the surface's coordinates, as rectangles: the surface's size, as smithay
        // hit-tests it, cut to its input region and to `region`. A root's view has no offset,
        // which is a subsurface's location.
        let size = with_renderer_surface_state(root, |state| state.view())
            .flatten()?
            .dst;
        let input = with_states(root, |states| {
            states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .input_region
                .clone()
        });
        let areas = area_of(size, [input.as_ref(), region].into_iter().flatten());
        // ponytail: the nearest point of the area, so a motion past the middle of a hole
        // narrower than its step lands across the hole; clip the motion's segment to the area,
        // as Weston does, if that matters.
        let distance = |at: &Point<f64, Logical>| {
            let off = *at - to;
            off.x * off.x + off.y * off.y
        };
        let mut stops: Vec<_> = areas
            .into_iter()
            .map(|area| origin + crate::input::clamp_into(area, to - origin))
            .collect();
        stops.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        stops.into_iter().find(holds)
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

#[cfg(test)]
mod tests {
    use super::*;
    use RectangleKind::{Add, Subtract};

    fn region(rects: &[(RectangleKind, [i32; 4])]) -> RegionAttributes {
        RegionAttributes {
            rects: rects
                .iter()
                .map(|&(kind, [x, y, w, h])| (kind, Rectangle::new((x, y).into(), (w, h).into())))
                .collect(),
        }
    }

    /// `rects_of` covers what `RegionAttributes::contains` does, in its order: a subtraction
    /// cuts the additions before it, an addition after it fills the hole again, and
    /// overlapping additions cover their union.
    #[test]
    fn rects_of_reads_a_region_in_order() {
        let region = region(&[
            (Add, [0, 0, 20, 10]),
            (Add, [10, 5, 20, 10]),
            (Subtract, [5, 0, 10, 15]),
            (Add, [8, 2, 4, 4]),
        ]);
        let rects = rects_of(&region);
        for x in -2..32 {
            for y in -2..17 {
                let at = Point::from((x, y));
                let covered = rects.iter().any(|rect| rect.contains(at));
                assert_eq!(covered, region.contains(at), "at {at:?}");
            }
        }
    }

    /// A region of more rectangles than `AREA_RECTS` stands as the bounds of its additions,
    /// so a client's region cannot make a motion costly.
    #[test]
    fn rects_of_bounds_a_region_of_many_rectangles() {
        let adds: Vec<_> = (0..1000).map(|x| (Add, [x, 0, 2, 1])).collect();
        let bounds = Rectangle::new((0, 0).into(), (1001, 1).into());
        assert_eq!(rects_of(&region(&adds)), vec![bounds]);
    }

    /// A confinement's area is at most `AREA_RECTS` rectangles, however many the cuts of its
    /// regions make: stripes across stripes stand as their bounds.
    #[test]
    fn area_of_is_bounded() {
        let columns: Vec<_> = (0..60).map(|x| (Add, [2 * x, 0, 1, 200])).collect();
        let rows: Vec<_> = (0..60).map(|y| (Add, [0, 2 * y, 200, 1])).collect();
        let (columns, rows) = (region(&columns), region(&rows));
        let area = area_of((200, 200).into(), [&columns, &rows]);
        assert_eq!(area, vec![Rectangle::new((0, 0).into(), (119, 119).into())]);
    }
}
