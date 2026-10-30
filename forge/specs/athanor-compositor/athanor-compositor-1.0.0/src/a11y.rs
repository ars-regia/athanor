//! athanor-a11y-v1 (doc_own_stack.md OS5, doc_accessibility.md AX10, ADR-0133): the magnifier
//! and the colour inversion and filters, switched by the shell, and their state. The description
//! is Athanor's own, `system/athanor-compositor-client/protocols/athanor-a11y-v1.xml`. The
//! manager is privileged (OS4): the bar's accessibility module is its one client. The keyboard
//! aids' requests are validated and change nothing yet: the aids are #865's.
//!
//! The magnifier scales each output's frame about a focal point, which follows the pointer as
//! `accessibility_zoom`'s `view_moves` says, and the colour filter is one pass over the frame
//! (`look.rs`). The stored inversion and filter, and the magnifier's behaviour, are cosmic-comp's
//! (`screen_filter.rs`).

use crate::identity::Privilege;
use crate::screen_filter::{Movement, ScreenFilter, Zoom};
use crate::state::{granted, State};
use protocol::athanor_a11y_manager_v1::{
    self, ActiveState, AthanorA11yManagerV1, Error, Filter, Modifier,
};
use smithay::desktop::{layer_map_for_output, WindowSurfaceType};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, WEnum};
use smithay::utils::{IsAlive, Logical, Physical, Point, Rectangle, Size};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::path::PathBuf;

/// The server side of the description, laid out as `wayland-protocols` lays out its own.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-compositor-client/protocols/athanor-a11y-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-a11y-v1.xml"
    );
}

/// cosmic-comp's highest level (`src/input/actions.rs:33` at `epoch-1.10.0`).
pub const MAX_LEVEL: f64 = 256.0;

/// The longest delay of slow and bounce keys, in ms (AX11, revision 3).
const MAX_DELAY: u32 = 2000;

/// The filters this version names, by the name cosmic-comp stores.
const FILTERS: [(Filter, &str); 4] = [
    (Filter::Greyscale, "Greyscale"),
    (Filter::Protanopia, "Protanopia"),
    (Filter::Deuteranopia, "Deuteranopia"),
    (Filter::Tritanopia, "Tritanopia"),
];

/// The magnifier, the colour filters, what the compositor reads of cosmic-comp's state for
/// them, and the managers they are sent to.
pub struct A11y {
    /// The magnification: 1 is the magnifier off.
    pub level: f64,
    /// The last level above 1 in this session, which the magnifier is turned on at.
    last_level: Option<f64>,
    /// cosmic-comp's `accessibility_zoom`, or its defaults.
    pub zoom: Zoom,
    /// The output the view is magnified on, the point of it (logical, from its origin) that
    /// stays in place, and the level it was placed at.
    focal: Option<(Output, Point<f64, Logical>, f64)>,
    /// The middle of the surface the keyboard last went to, and where the pointer was then:
    /// the view follows that point in place of the pointer until the pointer moves or acts.
    pub(crate) focus: Option<(Point<f64, Logical>, Point<f64, Logical>)>,
    /// A button or a pen's tip is being handled: the keyboard moving now is the pointer's doing.
    pub(crate) pointing: bool,
    /// The renderer cannot draw the colour pass: its shader did not compile.
    no_pass: bool,
    /// The inversion and the filter, as cosmic-comp stores them.
    pub screen: ScreenFilter,
    /// Where they are stored, once the state root is known.
    pub filter_path: Option<PathBuf>,
    /// Every bound manager, which the keyboard aids' events also reach (#865).
    pub(crate) managers: Vec<AthanorA11yManagerV1>,
}

impl Default for A11y {
    fn default() -> Self {
        Self {
            level: 1.0,
            last_level: None,
            zoom: Zoom::default(),
            focal: None,
            focus: None,
            pointing: false,
            no_pass: false,
            screen: ScreenFilter::default(),
            filter_path: None,
            managers: Vec::new(),
        }
    }
}

/// The user data of the manager's global and of every manager.
pub struct A11yManagers;

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorA11yManagerV1, _>(1, A11yManagers);
}

impl GlobalDispatch2<AthanorA11yManagerV1, State> for A11yManagers {
    fn bind(
        &self,
        state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorA11yManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, A11yManagers);
        send_magnifier(&manager, state.a11y.level);
        send_screen_filter(&manager, &state.a11y.shown_filter());
        // No modifier is latched or locked before the aids exist; a manager bound while the
        // session is locked is sent the event when the aids land (#865).
        if state.lock.is_none() {
            manager.modifiers(Modifier::empty(), Modifier::empty());
        }
        state.a11y.managers.push(manager);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::A11y)
    }
}

impl Dispatch2<AthanorA11yManagerV1, State> for A11yManagers {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorA11yManagerV1,
        request: athanor_a11y_manager_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_a11y_manager_v1::Request;
        let refuse = |what: &str| manager.post_error(Error::InvalidValue, what.to_string());
        match request {
            Request::SetMagnifier { active } => match active {
                WEnum::Value(active) => state.set_magnifier(active == ActiveState::Enabled),
                WEnum::Unknown(_) => refuse("no such active_state"),
            },
            Request::ZoomIn => state.zoom_step(true),
            Request::ZoomOut => state.zoom_step(false),
            Request::SetScreenFilter { inverted, filter } => match (inverted, filter) {
                (WEnum::Value(inverted), WEnum::Value(filter)) => {
                    state.set_screen_filter(inverted == ActiveState::Enabled, filter)
                }
                _ => refuse("no such active_state or filter"),
            },
            // The keyboard aids: checked as the description bounds them, applied by none until
            // #865 fills these arms.
            Request::SetStickyKeys {
                active: WEnum::Value(_),
                two_key_off: WEnum::Value(_),
            }
            | Request::SetAidGestures {
                active: WEnum::Value(_),
            } => {}
            Request::SetSlowKeys {
                active: WEnum::Value(_),
                delay,
            }
            | Request::SetBounceKeys {
                active: WEnum::Value(_),
                delay,
            } => {
                if delay > MAX_DELAY {
                    refuse("a delay above 2000 ms");
                }
            }
            Request::SetStickyKeys { .. }
            | Request::SetAidGestures { .. }
            | Request::SetSlowKeys { .. }
            | Request::SetBounceKeys { .. } => refuse("no such active_state"),
            Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, manager: &AthanorA11yManagerV1) {
        state.a11y.managers.retain(|kept| kept != manager);
    }
}

fn send_magnifier(manager: &AthanorA11yManagerV1, level: f64) {
    let active = if level > 1.0 {
        ActiveState::Enabled
    } else {
        ActiveState::Disabled
    };
    manager.magnifier(active, level);
}

fn send_screen_filter(manager: &AthanorA11yManagerV1, screen: &ScreenFilter) {
    let inverted = if screen.inverted {
        ActiveState::Enabled
    } else {
        ActiveState::Disabled
    };
    let filter = match &screen.filter {
        None => Filter::None,
        Some(name) => FILTERS
            .iter()
            .find(|(_, known)| known == name)
            .map_or(Filter::Unknown, |(filter, _)| *filter),
    };
    manager.screen_filter(inverted, filter);
}

/// The level one zoom step from `level`, up or down, by `increment` percent: kept between 1
/// and `MAX_LEVEL`, and 1 below 1.01 (cosmic-comp's `update_zoom`).
pub fn stepped(level: f64, increment: u32, up: bool) -> f64 {
    scaled(level, f64::from(increment) / 100.0, up)
}

fn scaled(level: f64, by: f64, up: bool) -> f64 {
    let factor = 1.0 + by;
    let level = if up { level * factor } else { level / factor };
    let level = level.clamp(1.0, MAX_LEVEL);
    if level < 1.01 {
        1.0
    } else {
        level
    }
}

/// Where the view of an output `size` wide and high is magnified about, so that the pointer at
/// `pointer` is shown as `moves` says; `last` is the previous focal point at the same level,
/// none when the level or the output changed. A point `p` of the output is shown at
/// `focal + (p - focal) * level`.
pub fn focal(
    moves: Movement,
    level: f64,
    size: Size<f64, Logical>,
    pointer: Point<f64, Logical>,
    last: Option<Point<f64, Logical>>,
) -> Point<f64, Logical> {
    let axis = |size: f64, pointer: f64, last: Option<f64>| match (moves, last) {
        (Movement::Continuously, _) | (Movement::OnEdge, None) => pointer,
        (Movement::Centered, _) => {
            // The offset that shows the pointer at the centre, kept so that the view covers
            // the output: the shown point of 0 is `offset`, that of `size` is at least `size`.
            let offset = (size / 2.0 - pointer * level).clamp(size * (1.0 - level), 0.0);
            offset / (1.0 - level)
        }
        (Movement::OnEdge, Some(last)) => {
            // The view shows `[start, start + size / level)`; it moves only as far as needed to
            // keep the pointer a margin of 2% of what it shows inside it.
            let seen = size / level;
            let margin = seen * 0.02;
            let start = last * (1.0 - 1.0 / level);
            let start = if pointer < start + margin {
                pointer - margin
            } else if pointer > start + seen - margin {
                pointer + margin - seen
            } else {
                start
            };
            (start.clamp(0.0, size - seen) / (1.0 - 1.0 / level)).clamp(0.0, size)
        }
    };
    Point::from((
        axis(size.w, pointer.x, last.map(|p| p.x)),
        axis(size.h, pointer.y, last.map(|p| p.y)),
    ))
}

/// What an output shows of the magnifier and the colour filters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    /// The point of the output, in physical pixels from its origin, that stays in place, and
    /// the level; none at level 1.
    pub zoom: Option<(Point<i32, Physical>, f64)>,
    /// The colour pass; none when neither the inversion nor a filter this version draws is on.
    pub filter: Option<Affine>,
}

impl A11y {
    /// What `output`, at `area` of the space, shows with the pointer at `pointer`. The view
    /// follows the pointer, or the keyboard's focus while the pointer has not moved since the
    /// focus did; only the output under the point followed is magnified, at the one level.
    pub fn look(
        &mut self,
        output: &Output,
        area: Option<Rectangle<i32, Logical>>,
        pointer: Option<Point<f64, Logical>>,
    ) -> Look {
        let pointer = match (self.focus, pointer) {
            (Some((focus, since)), Some(now)) if now == since => Some(focus),
            _ => {
                self.focus = None;
                pointer
            }
        };
        let filter = Affine::of(&self.shown_filter());
        if filter.is_none() {
            crate::look::release(output);
        }
        Look {
            zoom: self.zoom_on(output, area, pointer),
            filter,
        }
    }

    /// The inversion and the filter as they are drawn, which is what a manager is told: with no
    /// pass, none, whatever is stored.
    fn shown_filter(&self) -> ScreenFilter {
        if self.no_pass {
            ScreenFilter::default()
        } else {
            self.screen.clone()
        }
    }

    /// A button or a pen's tip started or finished acting at the pointer: the view goes back
    /// to the pointer, and meanwhile what the keyboard does is not followed.
    pub(crate) fn set_pointing(&mut self, acting: bool) {
        self.pointing = acting;
        self.focus = None;
    }

    fn zoom_on(
        &mut self,
        output: &Output,
        area: Option<Rectangle<i32, Logical>>,
        pointer: Option<Point<f64, Logical>>,
    ) -> Option<(Point<i32, Physical>, f64)> {
        let (area, pointer) = (area?, pointer?);
        if self.level <= 1.0 {
            self.focal = None;
            return None;
        }
        if !area.to_f64().contains(pointer) {
            return None;
        }
        let last = self
            .focal
            .as_ref()
            .filter(|(on, _, level)| on == output && *level == self.level)
            .map(|(_, at, _)| *at);
        let at = focal(
            self.zoom.moves,
            self.level,
            area.size.to_f64(),
            pointer - area.loc.to_f64(),
            last,
        );
        self.focal = Some((output.clone(), at, self.level));
        let scale = output.current_scale().fractional_scale();
        Some((at.to_physical(scale).to_i32_round(), self.level))
    }
}

impl State {
    /// The keyboard went to `surface`, by no doing of the pointer: with the magnifier on, the
    /// view follows the middle of its window or layer surface as `view_moves` says it follows
    /// the pointer, until the pointer moves (`doc_accessibility.md` section 5, item 8). The
    /// middle, not the text caret, which the compositor is not told of (AX10).
    pub(crate) fn zoom_to_focus(&mut self, surface: &WlSurface) {
        // Called once the dispatch that moved the keyboard is done: by then the surface may be
        // gone, or the keyboard elsewhere.
        let focused = self.seat.get_keyboard().and_then(|k| k.current_focus());
        if self.a11y.level <= 1.0 || !surface.alive() || focused.as_ref() != Some(surface) {
            return;
        }
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(area) = self.surface_area(surface) else {
            return;
        };
        let middle = (area.loc + area.size.downscale(2)).to_f64();
        self.a11y.focus = Some((middle, pointer.current_location()));
        self.request_redraw();
    }

    /// Where `surface`, a window's or a layer surface's, is in the space.
    fn surface_area(&self, surface: &WlSurface) -> Option<Rectangle<i32, Logical>> {
        if let Some(window) = self.window_for(surface) {
            return self.space.element_geometry(&window);
        }
        self.space.outputs().find_map(|output| {
            let origin = self.space.output_geometry(output)?.loc;
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
            let area = map.layer_geometry(layer)?;
            Some(Rectangle::new(area.loc + origin, area.size))
        })
    }

    /// The point of the space a device that points at the screen names at `location`. A view
    /// that follows the pointer at every motion moves with that point, so the point is
    /// `location` itself: continuously it is then shown there, and any other answer would
    /// drift while the device stays still. A view that moves at its edge stays put, so the point
    /// is the one shown at `location` now: the view of the next frame on the pointer's output.
    pub(crate) fn unzoomed(&self, location: Point<f64, Logical>) -> Point<f64, Logical> {
        let level = self.a11y.level;
        if level <= 1.0 || self.a11y.zoom.moves != Movement::OnEdge {
            return location;
        }
        let Some(pointer) = self.seat.get_pointer().map(|p| p.current_location()) else {
            return location;
        };
        let on = self.space.outputs().find_map(|output| {
            let area = self.space.output_geometry(output)?.to_f64();
            area.contains(pointer).then_some((output, area))
        });
        let Some((output, area)) = on.filter(|(_, area)| area.contains(location)) else {
            return location;
        };
        let last = self
            .a11y
            .focal
            .as_ref()
            .filter(|(on, _, at)| on == output && *at == level)
            .map(|(_, at, _)| *at);
        let origin = area.loc;
        let at = focal(Movement::OnEdge, level, area.size, pointer - origin, last);
        origin + at + (location - origin - at).downscale(level)
    }

    /// Turns the magnifier on, at its last level in this session or one step, or off.
    fn set_magnifier(&mut self, on: bool) {
        let level = if on {
            self.a11y
                .last_level
                .unwrap_or_else(|| stepped(1.0, self.a11y.zoom.increment, true))
        } else {
            1.0
        };
        self.set_zoom(level);
    }

    /// One zoom step in or out: Super+= and Super+-, and the manager's zoom_in and zoom_out.
    pub(crate) fn zoom_step(&mut self, up: bool) {
        self.set_zoom(stepped(self.a11y.level, self.a11y.zoom.increment, up));
    }

    /// Super with the scroll wheel: `change` is a fraction, positive to zoom in; zooming out
    /// with the magnifier off does nothing (cosmic-comp's `update_zoom`).
    pub(crate) fn scroll_zoom(&mut self, change: f64) {
        if self.a11y.level <= 1.0 && change <= 0.0 {
            return;
        }
        self.set_zoom(scaled(self.a11y.level, change.abs(), change > 0.0));
    }

    fn set_zoom(&mut self, level: f64) {
        if level == self.a11y.level {
            return;
        }
        tracing::debug!(level, "magnifier");
        self.a11y.level = level;
        if level > 1.0 {
            self.a11y.last_level = Some(level);
        }
        for manager in &self.a11y.managers {
            send_magnifier(manager, level);
        }
        self.request_redraw();
    }

    /// Sets the inversion and the filter, `Filter::Unknown` keeping the current one, stores
    /// them as cosmic-comp does, and sends them to every manager.
    fn set_screen_filter(&mut self, inverted: bool, filter: Filter) {
        self.a11y.screen.inverted = inverted;
        if filter != Filter::Unknown {
            self.a11y.screen.filter = FILTERS
                .iter()
                .find(|(known, _)| *known == filter)
                .map(|(_, name)| (*name).to_string());
        }
        self.store_screen_filter();
        self.send_shown_filter();
        self.request_redraw();
    }

    /// The renderer cannot draw the colour pass: every manager is told it is off, and it stays
    /// stored, for the session that can.
    pub(crate) fn no_filter_pass(&mut self) {
        self.a11y.no_pass = true;
        self.send_shown_filter();
    }

    fn send_shown_filter(&self) {
        let shown = self.a11y.shown_filter();
        for manager in &self.a11y.managers {
            send_screen_filter(manager, &shown);
        }
    }
}

/// The colour pass on premultiplied RGB, in the encoded values as the frame holds them:
/// `rgb' = clamp(matrix * rgb + offset * alpha, 0, alpha)`, alpha unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Row-major.
    pub matrix: [[f32; 3]; 3],
    pub offset: f32,
}

/// Greyscale: the Rec. 709 luma of each colour.
const GREYSCALE: [[f32; 3]; 3] = [[0.2126, 0.7152, 0.0722]; 3];

/// How each dichromacy sees a colour: Machado, Oliveira and Fernandes (2009), severity 1.
const PROTANOPIA: [[f32; 3]; 3] = [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
];
const DEUTERANOPIA: [[f32; 3]; 3] = [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.011820, 0.042940, 0.968881],
];
const TRITANOPIA: [[f32; 3]; 3] = [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.303900],
];

/// Where the colours lost to a dichromacy are moved to (Fidaner, Lin and Ozguven's daltonize).
const SHIFT: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [0.7, 1.0, 0.0], [0.7, 0.0, 1.0]];

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn product(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (row, out) in out.iter_mut().enumerate() {
        for (column, out) in out.iter_mut().enumerate() {
            *out = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

/// Daltonized for a dichromacy that sees as `seen`: the colour plus `SHIFT` of what it loses,
/// `I + SHIFT (I - seen)`.
fn daltonized(seen: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut lost = IDENTITY;
    for (row, lost) in lost.iter_mut().enumerate() {
        for (column, lost) in lost.iter_mut().enumerate() {
            *lost -= seen[row][column];
        }
    }
    let mut out = product(SHIFT, lost);
    for (row, out) in out.iter_mut().enumerate() {
        out[row] += 1.0;
    }
    out
}

impl Affine {
    /// The pass of `screen`: its filter, then the inversion. A filter this version does not
    /// name is not drawn, and is kept in the file as it is.
    pub fn of(screen: &ScreenFilter) -> Option<Self> {
        let matrix = match screen.filter.as_deref() {
            Some("Greyscale") => Some(GREYSCALE),
            Some("Protanopia") => Some(daltonized(PROTANOPIA)),
            Some("Deuteranopia") => Some(daltonized(DEUTERANOPIA)),
            Some("Tritanopia") => Some(daltonized(TRITANOPIA)),
            _ => None,
        };
        if !screen.inverted {
            return matrix.map(|matrix| Self {
                matrix,
                offset: 0.0,
            });
        }
        // Premultiplied, 1 - c is alpha - c.
        let mut matrix = matrix.unwrap_or(IDENTITY);
        for value in matrix.iter_mut().flatten() {
            *value = -*value;
        }
        Some(Self {
            matrix,
            offset: 1.0,
        })
    }

    /// The pass on one pixel of 8-bit premultiplied RGBA.
    pub fn apply(&self, pixel: [u8; 4]) -> [u8; 4] {
        let [r, g, b, a] = pixel.map(|value| f32::from(value) / 255.0);
        let mut out = [0u8; 4];
        for (row, out) in out.iter_mut().take(3).enumerate() {
            let m = self.matrix[row];
            let value = (m[0] * r + m[1] * g + m[2] * b + self.offset * a).clamp(0.0, a);
            *out = (value * 255.0).round() as u8;
        }
        out[3] = pixel[3];
        out
    }

    /// The matrix in the column-major order GLSL takes it.
    pub fn columns(&self) -> [f32; 9] {
        let m = self.matrix;
        [
            m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_multiplies_by_the_increment_and_stays_in_bounds() {
        assert_eq!(stepped(1.0, 50, true), 1.5);
        assert_eq!(stepped(1.5, 50, true), 2.25);
        assert_eq!(stepped(1.5, 50, false), 1.0);
        assert_eq!(stepped(1.0, 50, false), 1.0, "off stays off");
        assert_eq!(stepped(200.0, 50, true), MAX_LEVEL);
        assert_eq!(stepped(1.009, 0, true), 1.0, "below 1.01 is off");
        assert_eq!(stepped(1.0, 1, true), 1.01);
    }

    #[test]
    fn the_view_follows_the_pointer_as_view_moves_says() {
        let size = Size::from((1000.0, 800.0));
        let at = Point::from((300.0, 200.0));
        // Continuously: the point under the pointer stays.
        assert_eq!(focal(Movement::Continuously, 2.0, size, at, None), at);
        // Centered: the pointer is shown at the centre, while the view can move.
        let centre = focal(
            Movement::Centered,
            2.0,
            size,
            Point::from((500.0, 400.0)),
            None,
        );
        assert_eq!(centre, Point::from((500.0, 400.0)));
        let corner = focal(
            Movement::Centered,
            2.0,
            size,
            Point::from((10.0, 10.0)),
            None,
        );
        assert_eq!(corner, Point::from((0.0, 0.0)), "kept on the output");
        // OnEdge: placed at the pointer, then still while the pointer is inside the view.
        let placed = focal(Movement::OnEdge, 2.0, size, at, None);
        assert_eq!(placed, at);
        let inside = focal(
            Movement::OnEdge,
            2.0,
            size,
            Point::from((320.0, 210.0)),
            Some(placed),
        );
        assert_eq!(inside, placed);
        // The view shows [150, 650) wide; past its edge it moves by as much.
        let past = focal(
            Movement::OnEdge,
            2.0,
            size,
            Point::from((700.0, 200.0)),
            Some(placed),
        );
        let start = past.x * 0.5;
        assert!((start + 500.0 - 10.0 - 700.0).abs() < 1e-9, "{past:?}");
    }

    #[test]
    fn the_filters_are_the_named_matrices() {
        let screen = |inverted, name: Option<&str>| ScreenFilter {
            inverted,
            filter: name.map(String::from),
        };
        assert_eq!(Affine::of(&screen(false, None)), None);
        assert_eq!(Affine::of(&screen(false, Some("Achromatopsia"))), None);
        let inverted = Affine::of(&screen(true, None)).expect("inversion");
        assert_eq!(inverted.apply([255, 0, 64, 255]), [0, 255, 191, 255]);
        assert_eq!(
            inverted.apply([0, 0, 0, 0]),
            [0, 0, 0, 0],
            "transparent stays"
        );
        let grey = Affine::of(&screen(false, Some("Greyscale"))).expect("greyscale");
        assert_eq!(grey.apply([255, 0, 0, 255]), [54, 54, 54, 255]);
        // A grey is seen alike by every dichromacy, so daltonizing leaves it.
        for name in ["Protanopia", "Deuteranopia", "Tritanopia"] {
            let pass = Affine::of(&screen(false, Some(name))).expect(name);
            let grey = pass.apply([128, 128, 128, 255]);
            for (channel, value) in grey.iter().take(3).enumerate() {
                assert!(
                    value.abs_diff(128) <= 1,
                    "{name} channel {channel}: {grey:?}"
                );
            }
        }
    }
}
