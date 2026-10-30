//! zwp_tablet_manager_v2 (doc_own_stack.md OS17 and decision 30, standard): the drawing tablets
//! libinput finds, their tools, and the events of a tool to the surface under it. A tool moves
//! the seat's pointer too, so that what is under it is focused and drawn as under a mouse; the
//! cursor a client sets for a tool is the one the seat draws (`SeatHandler::cursor_image`).
//! Tablet pads are not offered: libinput's pad events are not read yet.
//!
//! The event handlers are ported from anvil, Smithay's reference compositor.
//!
//! Copyright (c) 2017 Victor Berger and Victoria Brekenfeld (anvil, MIT)
//! Source: Smithay revision 023b78b1c5a05ca512767cacc07efca55fae8be4, `anvil/src/input_handler.rs`
//! (`on_device_added`, `on_device_removed`, `on_tablet_tool_axis`, `on_tablet_tool_proximity`,
//! `on_tablet_tool_tip`, `on_tablet_button`), under the MIT licence below.
//! Changed by the Athanor authors on 2026-10-29: the events are read in `State` methods that
//! take plain values, so that the fixture drives them without libinput; the pointer moves
//! through `State::pointer_to`; a tip down focuses the window under the tool as a button press
//! does (`State::click_focus`); the tool's cursor goes to `SeatHandler::cursor_image`.
//!
//! The MIT License
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! software and associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! to whom the Software is furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all copies or
//! substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//! INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//! FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

use crate::state::State;
use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Device, DeviceCapability, Event, InputBackend, InputEvent,
    InputTime, ProximityState, TabletToolButtonEvent, TabletToolDescriptor, TabletToolEvent,
    TabletToolProximityEvent, TabletToolTipEvent, TabletToolTipState,
};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::tablet::tool::{
    AxisFrame, ButtonEvent, DownEvent, MotionEvent, ProximityInEvent, ProximityOutEvent, UpEvent,
};
use smithay::input::tablet::{TabletDescriptor, TabletSeatHandler, TabletSeatTrait};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::tablet_manager::TabletManagerState;

/// Offers the global to every client.
pub fn global(dh: &DisplayHandle) {
    TabletManagerState::new::<State>(dh);
}

// cursor-shape-v1 serves tablet tools too.
impl TabletSeatHandler for State {
    type ToolFocus = WlSurface;

    /// A client set the cursor of a tool. One pointer is drawn, and a tool moves it, so the
    /// image is the seat's cursor as a client's `wl_pointer.set_cursor` sets it.
    fn tablet_tool_image(&mut self, _tool: &TabletToolDescriptor, image: CursorImageStatus) {
        let seat = self.seat.clone();
        smithay::input::SeatHandler::cursor_image(self, &seat, image);
    }
}

/// The axes that changed in `event`.
fn axes<I: InputBackend>(event: &impl TabletToolEvent<I>) -> AxisFrame {
    AxisFrame {
        pressure: event.pressure_has_changed().then(|| event.pressure()),
        distance: event.distance_has_changed().then(|| event.distance()),
        tilt: event.tilt_has_changed().then(|| event.tilt()),
        rotation: event.rotation_has_changed().then(|| event.rotation()),
        slider: event.slider_has_changed().then(|| event.slider_position()),
        wheel: event
            .wheel_has_changed()
            .then(|| (event.wheel_delta(), event.wheel_delta_discrete())),
    }
}

impl State {
    /// A tablet appeared: clients bound to the tablet seat are told of it.
    pub(crate) fn tablet_added(&mut self, tablet: &TabletDescriptor) {
        let dh = self.display_handle.clone();
        self.seat.tablet_seat().add_wp_tablet(&dh, tablet);
    }

    /// A tablet went. With no tablet left, no tool is left either.
    pub(crate) fn tablet_removed(&mut self, tablet: &TabletDescriptor) {
        let seat = self.seat.tablet_seat();
        seat.remove_tablet(tablet);
        if seat.count_tablets() == 0 {
            seat.clear_tools();
            self.tool_pressed.clear();
        }
    }

    /// `tool` of `tablet` came into or left the range of the tablet, at `location` in the space.
    /// `device` is the libinput device's id, the key of its last position under a pointer lock.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tool_proximity(
        &mut self,
        device: String,
        tablet: &TabletDescriptor,
        tool: &TabletToolDescriptor,
        location: Point<f64, Logical>,
        proximity: ProximityState,
        axes: AxisFrame,
        time: InputTime,
    ) {
        let seat = self.seat.tablet_seat();
        let dh = self.display_handle.clone();
        let handle = match (seat.get_tool(tool), proximity) {
            (Some(handle), _) => handle,
            (None, ProximityState::In) => seat.add_wp_tool(self, &dh, tool),
            // A tool that was never in range has none to leave.
            (None, ProximityState::Out) => return,
        };
        // A tool landing starts afresh: under a lock, its first sample moves nothing, and no
        // jump from where an earlier tool or lift left off.
        if proximity == ProximityState::In {
            self.absolute_device_gone(&device);
        }
        self.pointer_moved_to(device, location, time);
        match proximity {
            ProximityState::In => {
                // A tool of a tablet no device event announced has nothing to enter.
                let Some(tablet) = seat.get_tablet(tablet) else {
                    return;
                };
                let under = self.surface_under(location);
                // Entering while in range ends the range first and forgets the held buttons;
                // a motion onto no surface does not, and replays them on the next surface.
                if handle.current_location().is_some() {
                    self.tool_pressed.remove(tool);
                }
                handle.proximity_in(
                    self,
                    under,
                    tablet,
                    &ProximityInEvent {
                        location,
                        axis: Some(axes),
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
            }
            ProximityState::Out => {
                // Smithay forgets the held buttons on leaving; so does the record of them.
                self.tool_pressed.remove(tool);
                handle.proximity_out(
                    self,
                    &ProximityOutEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
            }
        }
        handle.frame(self, time);
    }

    /// `tool` moved to `location` in the space, its axes changed as `axes` says.
    pub(crate) fn tool_motion(
        &mut self,
        device: String,
        tool: &TabletToolDescriptor,
        location: Point<f64, Logical>,
        axes: AxisFrame,
        time: InputTime,
    ) {
        self.pointer_moved_to(device, location, time);
        let Some(handle) = self.seat.tablet_seat().get_tool(tool) else {
            return;
        };
        let under = self.surface_under(location);
        // The motion first: it moves the focus, which the axes then reach.
        handle.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        handle.axis(self, axes);
        handle.frame(self, time);
    }

    /// The tip of `tool` touched the tablet, or left it. A touch focuses the window under the
    /// tool, as a button press does.
    pub(crate) fn tool_tip(
        &mut self,
        tool: &TabletToolDescriptor,
        tip: TabletToolTipState,
        time: InputTime,
    ) {
        self.super_tap = None;
        let Some(handle) = self.seat.tablet_seat().get_tool(tool) else {
            return;
        };
        match tip {
            TabletToolTipState::Down => {
                if self.tool_refused(&handle) {
                    return;
                }
                if let Some(pointer) = self.seat.get_pointer() {
                    if !pointer.is_grabbed() {
                        self.click_focus(pointer.current_location());
                    }
                }
                let serial = SERIAL_COUNTER.next_serial();
                self.last_input_serial = Some(serial);
                handle.down(self, &DownEvent { serial, time });
            }
            TabletToolTipState::Up => {
                let serial = SERIAL_COUNTER.next_serial();
                handle.up(self, &UpEvent { serial, time });
            }
        }
        handle.frame(self, time);
    }

    /// A button of `tool`'s barrel went down or up.
    pub(crate) fn tool_button(
        &mut self,
        tool: &TabletToolDescriptor,
        button: u32,
        state: ButtonState,
        time: InputTime,
    ) {
        self.super_tap = None;
        let Some(handle) = self.seat.tablet_seat().get_tool(tool) else {
            return;
        };
        let held = self.tool_pressed.entry(tool.clone()).or_default();
        if state == ButtonState::Pressed {
            if self.tool_refused(&handle) {
                return;
            }
            let held = self.tool_pressed.entry(tool.clone()).or_default();
            if !held.contains(&button) {
                held.push(button);
            }
        } else {
            // A release of a button the focus never saw pressed (held across a lock or an
            // unlock, which `refresh_tools` forgets) is the focus's no event at all.
            let before = held.len();
            held.retain(|b| *b != button);
            if held.len() == before {
                return;
            }
        }
        let serial = SERIAL_COUNTER.next_serial();
        self.last_input_serial = Some(serial);
        handle.button(
            self,
            &ButtonEvent {
                serial,
                button,
                state,
                time,
            },
        );
        handle.frame(self, time);
    }

    /// Whether a press of `tool` reaches no client: while the session is locked, unless the tool
    /// is over a lock surface. A release of what the tool holds still goes on. Smithay keeps a
    /// tip and buttons held with no focus and replays them to the next focus, so a press made
    /// where no lock surface is would reach the lock surface.
    fn tool_refused(&self, tool: &smithay::input::tablet::tool::TabletToolHandle<State>) -> bool {
        self.lock.is_some()
            && tool
                .current_location()
                .is_none_or(|location| self.surface_under(location).is_none())
    }

    /// Gives every tool in range to what is under it now. A lock or an unlock changes that
    /// without a motion: without this, a tool hovering over a window keeps it, and a tip held
    /// down goes on drawing in it (`State::refresh_pointer` does the same for the pointer).
    /// The tool leaves the range and enters it again, on the new target: Smithay keeps the tip
    /// and the buttons held across a focus change and replays them to the new target, which
    /// would be a press the user never made on it. A tip held through the change sends no `down`
    /// to the new target, and the stroke goes on only after a new press.
    pub(crate) fn refresh_tools(&mut self) {
        self.tool_pressed.clear();
        for tool in self.seat.tablet_seat().get_tools().into_values() {
            let Some(location) = tool.current_location() else {
                continue;
            };
            let under = self.surface_under(location);
            let Some(tablet) = tool.current_tablet() else {
                continue;
            };
            let time = InputTime::now();
            tool.proximity_out(
                self,
                &ProximityOutEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            tool.frame(self, time);
            tool.proximity_in(
                self,
                under,
                tablet,
                &ProximityInEvent {
                    location,
                    axis: None,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            tool.frame(self, time);
        }
    }

    /// Where `event` is in the space: its position spans the first output, as a touchpad's
    /// absolute position does (`process_input_event`).
    fn tablet_location<I: InputBackend>(
        &self,
        event: &impl AbsolutePositionEvent<I>,
    ) -> Option<Point<f64, Logical>> {
        let area = self.output_area()?;
        Some(event.position_transformed(area.size) + area.loc.to_f64())
    }

    /// Reads a device or tablet event of the backend. Other events are not a tablet's and are
    /// left alone.
    pub(crate) fn tablet_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::DeviceAdded { device } => {
                if device.has_capability(DeviceCapability::TabletTool) {
                    self.tablet_added(&TabletDescriptor::from(&device));
                }
            }
            InputEvent::DeviceRemoved { device } => {
                self.absolute_device_gone(&device.id());
                if device.has_capability(DeviceCapability::TabletTool) {
                    self.tablet_removed(&TabletDescriptor::from(&device));
                }
            }
            InputEvent::TabletToolAxis { event } => {
                if let Some(location) = self.tablet_location(&event) {
                    self.tool_motion(
                        event.device().id(),
                        &event.tool(),
                        location,
                        axes(&event),
                        event.time(),
                    );
                }
            }
            InputEvent::TabletToolProximity { event } => {
                if let Some(location) = self.tablet_location(&event) {
                    self.tool_proximity(
                        event.device().id(),
                        &TabletDescriptor::from(&event.device()),
                        &event.tool(),
                        location,
                        event.state(),
                        axes(&event),
                        event.time(),
                    );
                }
            }
            InputEvent::TabletToolTip { event } => {
                self.tool_tip(&event.tool(), event.tip_state(), event.time());
            }
            InputEvent::TabletToolButton { event } => {
                self.tool_button(
                    &event.tool(),
                    event.button(),
                    event.button_state(),
                    event.time(),
                );
            }
            _ => {}
        }
    }
}
