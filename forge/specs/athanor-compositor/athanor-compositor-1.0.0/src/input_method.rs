//! zwp-text-input-v3 and zwp-input-method-v2 (OS9 item 3), served here rather than by Smithay,
//! whose input method sends a keyboard grab the keymap and the live modifiers the moment it is
//! asked for, the session locked or not. A text input is offered to every client outside a
//! security context; the input method to the client the allow-list grants it, one per seat.
//!
//! The text inputs follow the keyboard's focus, and while the session is locked they follow
//! nothing: the input method is neither activated for the lock surface nor told its text, and
//! what it commits reaches no client. A keyboard grab it asks for while locked is a dead object:
//! it is installed neither then nor at unlock, and is sent nothing, not even the keymap.

use crate::identity::Privilege;
use crate::state::{granted, unconfined, State, REPEAT_DELAY, REPEAT_RATE};
use smithay::backend::input::{InputTime, KeyState, Keycode};
use smithay::desktop::{layer_map_for_output, Space, Window, WindowSurfaceType};
use smithay::input::keyboard::{
    GrabStartData, KeyboardGrab, KeyboardInnerHandle, ModifiersState, SerializedMods,
};
use smithay::output::Output;
use smithay::reexports::rustix::fs::{memfd_create, MemfdFlags};
use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::{
    zwp_text_input_manager_v3::{self, ZwpTextInputManagerV3},
    zwp_text_input_v3::{self, ChangeCause, ContentHint, ContentPurpose, ZwpTextInputV3},
};
use smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::{
    zwp_input_method_keyboard_grab_v2::{self, ZwpInputMethodKeyboardGrabV2},
    zwp_input_method_manager_v2::{self, ZwpInputMethodManagerV2},
    zwp_input_method_v2::{self, ZwpInputMethodV2},
    zwp_input_popup_surface_v2::{self, ZwpInputPopupSurfaceV2},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_keyboard::KeymapFormat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::utils::{Logical, Point, Rectangle, Serial, SERIAL_COUNTER};
use smithay::wayland::compositor::{get_role, give_role};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::io::Write;
use std::os::fd::AsFd;

/// The role of an input method's popup surface.
const POPUP_ROLE: &str = "zwp_input_popup_surface_v2";

/// The text inputs a client may have at once.
const TEXT_INPUTS_PER_CLIENT: usize = 32;

/// The user data of both globals and of every object of the two protocols.
pub struct InputMethods;

/// An input method's popup surface, with its origin relative to the output it is drawn on.
pub type PlacedPopup = (WlSurface, Point<i32, Logical>);

/// The seat's text inputs and its input method.
#[derive(Default)]
pub struct Composing {
    method: Option<Method>,
    text_inputs: Vec<TextInput>,
    /// The surface the text inputs follow: the keyboard's focus, none while locked.
    focus: Option<WlSurface>,
    /// The text input of the focus the input method serves, once enabled.
    active: Option<ZwpTextInputV3>,
    /// The cursor of the active text input, relative to the focus.
    cursor: Rectangle<i32, Logical>,
    popups: Vec<(ZwpInputPopupSurfaceV2, WlSurface)>,
}

/// The seat's input method: the first one asked for while none lives.
struct Method {
    object: ZwpInputMethodV2,
    /// The `done` events sent, which its `commit` names.
    done: u32,
    /// Its keyboard grab while installed.
    grab: Option<ZwpInputMethodKeyboardGrabV2>,
    edits: Edits,
}

impl Method {
    fn done(&mut self) {
        self.done = self.done.wrapping_add(1);
        self.object.done();
    }

    /// Stops serving the active text input: what it sent for that one is dropped.
    fn deactivate(&mut self) {
        self.object.deactivate();
        self.edits = Edits::default();
        self.done();
    }
}

/// What the input method sends for the active text input, applied at its next `commit`.
#[derive(Default)]
struct Edits {
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
    delete: Option<(u32, u32)>,
}

struct TextInput {
    object: ZwpTextInputV3,
    /// The `commit` requests received, which its `done` events name.
    commits: u32,
    pending: Pending,
}

/// A text input's state, applied at its next `commit`.
#[derive(Default)]
struct Pending {
    enable: Option<bool>,
    surrounding: Option<(String, u32, u32)>,
    cause: Option<ChangeCause>,
    content: Option<(ContentHint, ContentPurpose)>,
    cursor: Option<Rectangle<i32, Logical>>,
}

/// Offers both globals.
pub fn globals(dh: &DisplayHandle) {
    dh.create_global::<State, ZwpTextInputManagerV3, _>(1, InputMethods);
    dh.create_global::<State, ZwpInputMethodManagerV2, _>(1, InputMethods);
}

impl State {
    /// Makes the text inputs follow `surface`, the keyboard's new focus, or nothing while the
    /// session is locked. The input method stops serving the text input it served.
    pub fn text_input_focus(&mut self, surface: Option<WlSurface>) {
        let surface = surface.filter(|_| self.lock.is_none());
        let composing = &mut self.composing;
        if composing.focus == surface {
            return;
        }
        if composing.active.take().is_some() {
            if let Some(method) = &mut composing.method {
                method.deactivate();
            }
        }
        if composing.method.is_some() {
            composing.each_focused(|text_input, focus| text_input.leave(focus));
        }
        composing.focus = surface;
        if composing.method.is_some() {
            composing.each_focused(|text_input, focus| text_input.enter(focus));
        }
    }

    /// Sends the seat's keymap and modifiers to the input method's grab, if one is installed:
    /// after the seat's keymap changes, before the key read under it.
    pub fn input_method_keymap_changed(&mut self) {
        if self
            .composing
            .method
            .as_ref()
            .is_some_and(|m| m.grab.is_some())
        {
            self.send_keyboard_to_grab();
        }
    }

    /// The keymap and the modifiers, to the installed grab.
    fn send_keyboard_to_grab(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let Some(grab) = self.composing.method.as_ref().and_then(|m| m.grab.clone()) else {
            return;
        };
        let keymap =
            keyboard.with_xkb_state(self, |context| crate::input::keymap_text(context.xkb()));
        if let Err(error) = send_keymap(&grab, &keymap) {
            tracing::warn!(%error, "the keymap could not be sent to the input method");
            return;
        }
        send_modifiers(
            &grab,
            SERIAL_COUNTER.next_serial(),
            keyboard.modifier_state().serialized,
        );
    }

    /// Installs `grab`, the input method's, unless the session is locked: then it stays a dead
    /// object, sent nothing, now and after unlock.
    fn grab_keyboard(&mut self, grab: ZwpInputMethodKeyboardGrabV2) {
        if self.lock.is_some() {
            return;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        // Set first: replacing an earlier grab of the input method unsets that one.
        keyboard.set_grab(
            self,
            ImGrab {
                object: grab.clone(),
            },
            SERIAL_COUNTER.next_serial(),
        );
        let Some(method) = &mut self.composing.method else {
            return;
        };
        method.grab = Some(grab.clone());
        grab.repeat_info(REPEAT_RATE, REPEAT_DELAY);
        self.send_keyboard_to_grab();
    }

    /// Ends `grab`, if it is the keyboard's grab: never another one that replaced it.
    fn end_grab(&mut self, grab: &ZwpInputMethodKeyboardGrabV2) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let installed = keyboard.with_grab(|_, current| {
            current
                .downcast_ref::<ImGrab>()
                .is_some_and(|current| current.object == *grab)
        });
        if installed == Some(true) {
            keyboard.unset_grab(self);
        }
    }
}

impl Composing {
    /// The input method's popups shown on `output` of `space`, below the active text input's
    /// cursor; none while the session is locked, when the text inputs follow nothing, and none
    /// whose surface its client destroyed.
    pub fn popups_on(&self, space: &Space<Window>, output: &Output) -> Vec<PlacedPopup> {
        let (Some(_), Some(focus)) = (&self.active, &self.focus) else {
            return Vec::new();
        };
        let (Some(origin), Some(area)) = (surface_origin(space, focus), space.output_geometry(output))
        else {
            return Vec::new();
        };
        let at = origin + self.cursor.loc + Point::from((0, self.cursor.size.h)) - area.loc;
        self.popups
            .iter()
            .filter(|(_, surface)| surface.is_alive())
            .map(|(_, surface)| (surface.clone(), at))
            .collect()
    }

    /// Calls `f` with each text input of the focus's client, and the focus.
    fn each_focused(&self, mut f: impl FnMut(&ZwpTextInputV3, &WlSurface)) {
        let Some(focus) = &self.focus else {
            return;
        };
        for text_input in &self.text_inputs {
            if text_input.object.id().same_client_as(&focus.id()) {
                f(&text_input.object, focus);
            }
        }
    }

    fn is_method(&self, object: &ZwpInputMethodV2) -> bool {
        self.method.as_ref().is_some_and(|m| m.object == *object)
    }

    /// The pending edits of `object`, if it is the seat's input method.
    fn edits(&mut self, object: &ZwpInputMethodV2) -> Option<&mut Edits> {
        let method = self.method.as_mut().filter(|m| m.object == *object)?;
        Some(&mut method.edits)
    }
}

/// The origin in `space` of `surface`, a window's or a layer surface's.
// ponytail: a text input on a subsurface or an xdg popup gets no popup shown; walk to the root
// surface and add the offsets when a client needs one there.
fn surface_origin(space: &Space<Window>, surface: &WlSurface) -> Option<Point<i32, Logical>> {
    let window = space
        .elements()
        .find(|window| window.toplevel().is_some_and(|t| t.wl_surface() == surface));
    if let Some(window) = window {
        return space.element_location(window);
    }
    space.outputs().find_map(|output| {
        let area = space.output_geometry(output)?;
        let map = layer_map_for_output(output);
        let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
        Some(map.layer_geometry(layer)?.loc + area.loc)
    })
}

/// Writes `keymap` to a memory file and sends it to `grab`.
fn send_keymap(grab: &ZwpInputMethodKeyboardGrabV2, keymap: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::from(memfd_create("athanor-keymap", MemfdFlags::CLOEXEC)?);
    file.write_all(keymap.as_bytes())?;
    // NUL-terminated, as a client reading it as a string expects.
    file.write_all(&[0])?;
    let size = u32::try_from(keymap.len() + 1).map_err(std::io::Error::other)?;
    grab.keymap(KeymapFormat::XkbV1, file.as_fd(), size);
    Ok(())
}

fn send_modifiers(grab: &ZwpInputMethodKeyboardGrabV2, serial: Serial, mods: SerializedMods) {
    grab.modifiers(
        serial.into(),
        mods.depressed,
        mods.latched,
        mods.locked,
        mods.layout_effective,
    );
}

/// The input method's keyboard grab: every key goes to it, none to the focused surface.
struct ImGrab {
    object: ZwpInputMethodKeyboardGrabV2,
}

impl KeyboardGrab<State> for ImGrab {
    fn input(
        &mut self,
        _: &mut State,
        _: &mut KeyboardInnerHandle<'_, State>,
        keycode: Keycode,
        key_state: KeyState,
        modifiers: Option<ModifiersState>,
        serial: Serial,
        time: InputTime,
    ) {
        // An xkb keycode is the evdev code plus 8.
        let key = keycode.raw().saturating_sub(8);
        self.object
            .key(serial.into(), time.millis(), key, key_state.into());
        if let Some(modifiers) = modifiers {
            send_modifiers(&self.object, serial, modifiers.serialized);
        }
    }

    fn set_focus(
        &mut self,
        data: &mut State,
        handle: &mut KeyboardInnerHandle<'_, State>,
        focus: Option<WlSurface>,
        serial: Serial,
    ) {
        handle.set_focus(data, focus, serial);
    }

    fn start_data(&self) -> &GrabStartData<State> {
        &GrabStartData { focus: None }
    }

    /// Ended or replaced, by the input method, the lock or another grab: the input method's
    /// grab object stays, inert.
    fn unset(&mut self, data: &mut State) {
        if let Some(method) = &mut data.composing.method {
            if method.grab.as_ref() == Some(&self.object) {
                method.grab = None;
            }
        }
    }
}

impl GlobalDispatch2<ZwpTextInputManagerV3, State> for InputMethods {
    fn bind(
        &self,
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpTextInputManagerV3>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, InputMethods);
    }

    /// Open question 13 of the OS9 plan: every client outside a security context.
    fn can_view(&self, client: &Client) -> bool {
        unconfined(client)
    }
}

impl Dispatch2<ZwpTextInputManagerV3, State> for InputMethods {
    fn request(
        &self,
        state: &mut State,
        _: &Client,
        _: &ZwpTextInputManagerV3,
        request: zwp_text_input_manager_v3::Request,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        // The seat is the only one.
        if let zwp_text_input_manager_v3::Request::GetTextInput { id, .. } = request {
            let object = data_init.init(id, InputMethods);
            let composing = &mut state.composing;
            // Each focus change walks the list: a client's share of it is bounded, and the
            // objects past it, never listed, are inert.
            let owned = composing
                .text_inputs
                .iter()
                .filter(|entry| entry.object.id().same_client_as(&object.id()))
                .count();
            if owned >= TEXT_INPUTS_PER_CLIENT {
                return;
            }
            if let (Some(_), Some(focus)) = (&composing.method, &composing.focus) {
                if object.id().same_client_as(&focus.id()) {
                    object.enter(focus);
                }
            }
            composing.text_inputs.push(TextInput {
                object,
                commits: 0,
                pending: Pending::default(),
            });
        }
    }
}

impl Dispatch2<ZwpTextInputV3, State> for InputMethods {
    fn request(
        &self,
        state: &mut State,
        _: &Client,
        resource: &ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let composing = &mut state.composing;
        let Some(entry) = composing
            .text_inputs
            .iter_mut()
            .find(|entry| entry.object == *resource)
        else {
            return;
        };
        if matches!(request, zwp_text_input_v3::Request::Commit) {
            entry.commits = entry.commits.wrapping_add(1);
        }
        // Without the input method, or from a client without the focus, a request is dropped.
        let focused = composing
            .focus
            .as_ref()
            .is_some_and(|focus| focus.id().same_client_as(&resource.id()));
        if composing.method.is_none() || !focused {
            return;
        }
        let pending = &mut entry.pending;
        match request {
            zwp_text_input_v3::Request::Enable => pending.enable = Some(true),
            zwp_text_input_v3::Request::Disable => pending.enable = Some(false),
            zwp_text_input_v3::Request::SetSurroundingText {
                text,
                cursor,
                anchor,
            } => {
                let cursor = u32::try_from(cursor).unwrap_or_default();
                let anchor = u32::try_from(anchor).unwrap_or_default();
                pending.surrounding = Some((text, cursor, anchor));
            }
            zwp_text_input_v3::Request::SetTextChangeCause { cause } => {
                pending.cause = Some(cause.into_result().unwrap_or(ChangeCause::Other));
            }
            zwp_text_input_v3::Request::SetContentType { hint, purpose } => {
                let hint = ContentHint::from_bits_truncate(u32::from(hint));
                let purpose = purpose.into_result().unwrap_or(ContentPurpose::Normal);
                pending.content = Some((hint, purpose));
            }
            zwp_text_input_v3::Request::SetCursorRectangle {
                x,
                y,
                width,
                height,
            } => {
                // A negative size is taken as none: smithay's sizes are never negative.
                let size = (width.max(0), height.max(0));
                pending.cursor = Some(Rectangle::new((x, y).into(), size.into()));
            }
            zwp_text_input_v3::Request::Commit => {
                let pending = std::mem::take(pending);
                commit(composing, resource, pending);
            }
            _ => {}
        }
    }

    fn destroyed(&self, state: &mut State, _: ClientId, resource: &ZwpTextInputV3) {
        let composing = &mut state.composing;
        composing
            .text_inputs
            .retain(|entry| entry.object != *resource);
        if composing.active.as_ref() == Some(resource) {
            composing.active = None;
            if let Some(method) = &mut composing.method {
                method.deactivate();
            }
        }
    }
}

/// Applies a text input's `commit`: the input method is activated for it, or deactivated, and
/// told its new state.
fn commit(composing: &mut Composing, resource: &ZwpTextInputV3, pending: Pending) {
    // One text input at a time: another's state waits until the active one is disabled.
    if composing
        .active
        .as_ref()
        .is_some_and(|active| active != resource)
    {
        return;
    }
    let Some(method) = &mut composing.method else {
        return;
    };
    match pending.enable {
        Some(true) => {
            composing.active = Some(resource.clone());
            method.object.activate();
        }
        Some(false) => {
            composing.active = None;
            method.deactivate();
            return;
        }
        None if composing.active.is_none() => return,
        None => {}
    }
    if let Some((text, cursor, anchor)) = pending.surrounding {
        method.object.surrounding_text(text, cursor, anchor);
    }
    if let Some(cause) = pending.cause {
        method.object.text_change_cause(cause);
    }
    if let Some((hint, purpose)) = pending.content {
        method.object.content_type(hint, purpose);
    }
    if let Some(cursor) = pending.cursor {
        composing.cursor = cursor;
        for (popup, _) in &composing.popups {
            popup.text_input_rectangle(cursor.loc.x, cursor.loc.y, cursor.size.w, cursor.size.h);
        }
    }
    method.done();
}

impl GlobalDispatch2<ZwpInputMethodManagerV2, State> for InputMethods {
    fn bind(
        &self,
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpInputMethodManagerV2>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, InputMethods);
    }

    /// The client the allow-list grants it, never one inside a security context.
    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::InputMethod)
    }
}

impl Dispatch2<ZwpInputMethodManagerV2, State> for InputMethods {
    fn request(
        &self,
        state: &mut State,
        _: &Client,
        _: &ZwpInputMethodManagerV2,
        request: zwp_input_method_manager_v2::Request,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let zwp_input_method_manager_v2::Request::GetInputMethod { input_method, .. } = request
        else {
            return;
        };
        let object = data_init.init(input_method, InputMethods);
        let composing = &mut state.composing;
        // One per seat: a second is told so, and is inert for good.
        if composing.method.is_some() {
            object.unavailable();
            return;
        }
        composing.method = Some(Method {
            object,
            done: 0,
            grab: None,
            edits: Edits::default(),
        });
        composing.each_focused(|text_input, focus| text_input.enter(focus));
    }
}

impl Dispatch2<ZwpInputMethodV2, State> for InputMethods {
    fn request(
        &self,
        state: &mut State,
        _: &Client,
        resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let current = state.composing.is_method(resource);
        match request {
            zwp_input_method_v2::Request::CommitString { text } => {
                if let Some(edits) = state.composing.edits(resource) {
                    edits.commit = Some(text);
                }
            }
            zwp_input_method_v2::Request::SetPreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                if let Some(edits) = state.composing.edits(resource) {
                    edits.preedit = Some((text, cursor_begin, cursor_end));
                }
            }
            zwp_input_method_v2::Request::DeleteSurroundingText {
                before_length,
                after_length,
            } => {
                if let Some(edits) = state.composing.edits(resource) {
                    edits.delete = Some((before_length, after_length));
                }
            }
            zwp_input_method_v2::Request::Commit { serial } => {
                let composing = &mut state.composing;
                let Some(edits) = composing.edits(resource).map(std::mem::take) else {
                    return;
                };
                // A commit made before the input method saw the latest state is dropped, with
                // its edits: they were meant for a state, or a text input, that is gone.
                let (Some(method), Some(active)) = (&composing.method, &composing.active) else {
                    return;
                };
                if serial != method.done {
                    return;
                }
                let Some(entry) = composing.text_inputs.iter().find(|e| e.object == *active) else {
                    return;
                };
                if let Some((text, begin, end)) = edits.preedit {
                    active.preedit_string(Some(text), begin, end);
                }
                if let Some(text) = edits.commit {
                    active.commit_string(Some(text));
                }
                if let Some((before, after)) = edits.delete {
                    active.delete_surrounding_text(before, after);
                }
                active.done(entry.commits);
            }
            zwp_input_method_v2::Request::GetInputPopupSurface { id, surface } => {
                let popup = data_init.init(id, InputMethods);
                if give_role(&surface, POPUP_ROLE).is_err()
                    && get_role(&surface) != Some(POPUP_ROLE)
                {
                    // The protocol asks for an error and names no code.
                    resource.post_error(0u32, "the surface already has another role");
                    return;
                }
                if current {
                    let cursor = state.composing.cursor;
                    popup.text_input_rectangle(
                        cursor.loc.x,
                        cursor.loc.y,
                        cursor.size.w,
                        cursor.size.h,
                    );
                    state.composing.popups.push((popup, surface));
                }
            }
            zwp_input_method_v2::Request::GrabKeyboard { keyboard } => {
                let grab = data_init.init(keyboard, InputMethods);
                if current {
                    state.grab_keyboard(grab);
                }
            }
            _ => {}
        }
    }

    fn destroyed(&self, state: &mut State, _: ClientId, resource: &ZwpInputMethodV2) {
        if !state.composing.is_method(resource) {
            return;
        }
        let composing = &mut state.composing;
        let grab = composing.method.take().and_then(|method| method.grab);
        composing.active = None;
        composing.popups.clear();
        composing.each_focused(|text_input, focus| text_input.leave(focus));
        if let Some(grab) = grab {
            state.end_grab(&grab);
        }
    }
}

impl Dispatch2<ZwpInputMethodKeyboardGrabV2, State> for InputMethods {
    fn request(
        &self,
        _: &mut State,
        _: &Client,
        _: &ZwpInputMethodKeyboardGrabV2,
        _: zwp_input_method_keyboard_grab_v2::Request,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        // `release` is the destructor: `destroyed` ends the grab.
    }

    fn destroyed(&self, state: &mut State, _: ClientId, resource: &ZwpInputMethodKeyboardGrabV2) {
        state.end_grab(resource);
    }
}

impl Dispatch2<ZwpInputPopupSurfaceV2, State> for InputMethods {
    fn request(
        &self,
        _: &mut State,
        _: &Client,
        _: &ZwpInputPopupSurfaceV2,
        _: zwp_input_popup_surface_v2::Request,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        // `destroy` is the only request.
    }

    fn destroyed(&self, state: &mut State, _: ClientId, resource: &ZwpInputPopupSurfaceV2) {
        state
            .composing
            .popups
            .retain(|(popup, _)| popup != resource);
    }
}
