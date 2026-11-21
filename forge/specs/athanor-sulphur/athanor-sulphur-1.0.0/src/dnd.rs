//! Drag and drop between clients, and the drag icon (doc_own_stack.md decision 40): Smithay's
//! grab, started only from the client's own press or touch point and never while locked, whose
//! target reads the data only at the drop. The icon is drawn by `marks::stack`.

use crate::state::State;
use smithay::backend::input::TouchSlot;
use smithay::desktop::utils::bbox_from_surface_tree;
use smithay::input::dnd::SourceMetadata;
use smithay::input::dnd::{DnDGrab, DndAction, DndGrabHandler, DndTarget, GrabType, Source};
use smithay::input::pointer::Focus;
use smithay::input::Seat;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{IsAlive, Logical, Point, Serial};
use smithay::wayland::compositor::{with_states, SurfaceAttributes};
use smithay::wayland::selection::data_device::WaylandDndGrabHandler;
use std::any::Any;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The icon of a drag in progress: a surface of the dragging client, at the touch point the
/// drag follows, or at the pointer when `slot` is `None`. A pointer drag and a touch drag may be
/// held together, each with its icon.
pub struct DragIcon {
    surface: WlSurface,
    slot: Option<TouchSlot>,
    /// Where the client puts the icon from the pointer or the touch point: the sum of the
    /// offsets of `wl_surface.attach` and `wl_surface.offset` it committed since the drag began.
    offset: Point<i32, Logical>,
    /// Set by the drag's source when that drag ends (`AtDrop`), so that its end takes this icon
    /// away and no other.
    ended: Arc<AtomicBool>,
}

/// A drag's source that passes the target's reads on only once the drop is performed: a read
/// while the drag hovers gets its descriptor closed, and the source is not asked. The drop is
/// the user's choice of the target, as a paste is, so the rule of the clipboard holds.
struct AtDrop<S> {
    source: S,
    dropped: AtomicBool,
    /// Shared with the drag's icon: set at the drop or the cancel, which end the drag.
    ended: Arc<AtomicBool>,
}

impl<S: Source> IsAlive for AtDrop<S> {
    fn alive(&self) -> bool {
        self.source.alive()
    }
}

impl<S: Source> Source for AtDrop<S> {
    fn is_client_local(&self, target: &dyn Any) -> bool {
        self.source.is_client_local(target)
    }

    fn metadata(&self) -> Option<SourceMetadata> {
        self.source.metadata()
    }

    fn accepted(&self, mime_type: Option<String>) {
        self.source.accepted(mime_type);
    }

    fn choose_action(&self, action: DndAction) {
        self.source.choose_action(action);
    }

    fn send(&self, mime_type: &str, fd: OwnedFd) {
        if self.dropped.load(Ordering::SeqCst) {
            self.source.send(mime_type, fd);
        }
    }

    fn drop_performed(&self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.ended.store(true, Ordering::SeqCst);
        self.source.drop_performed();
    }

    fn cancel(&self) {
        self.ended.store(true, Ordering::SeqCst);
        self.source.cancel();
    }

    fn finished(&self) {
        self.source.finished();
    }
}

/// The client that asks for a drag: its source's, or, for a drag inside one client, which
/// names no source, its origin surface's.
fn requester(source: &dyn Any) -> Option<ClientId> {
    let client = match source.downcast_ref::<WlDataSource>() {
        Some(source) => source.client(),
        None => source
            .downcast_ref::<WlSurface>()
            .and_then(|surface| surface.client()),
    };
    client.map(|client| client.id())
}

impl WaylandDndGrabHandler for State {
    /// Smithay has checked that `serial` is the press or the touch point the seat's grab holds;
    /// the drag also needs it sent to the client that asks, as a move does (`window_grab`), still
    /// held, and no lock. A drag refused is cancelled.
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        icon: Option<WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        let own = requester(&source).is_some_and(|client| match type_ {
            GrabType::Pointer => self.grab_serials.clicked(serial, &client),
            GrabType::Touch => self.grab_serials.touched(serial, &client),
        });
        if self.lock.is_some() || !own || !self.held(serial, type_) {
            source.cancel();
            return;
        }
        let ended = Arc::new(AtomicBool::new(false));
        let source = AtDrop {
            source,
            dropped: AtomicBool::new(false),
            ended: ended.clone(),
        };
        let display = self.display_handle.clone();
        // The icon is taken after the grab is set: setting it ends a drag the seat held before,
        // whose end takes that drag's icon away (`DndGrabHandler`).
        let slot = match type_ {
            GrabType::Pointer => {
                let Some((pointer, start)) = seat
                    .get_pointer()
                    .and_then(|pointer| Some((pointer.clone(), pointer.grab_start_data()?)))
                else {
                    source.cancel();
                    return;
                };
                let grab = DnDGrab::new_pointer(&display, start, source, seat);
                pointer.set_grab(self, grab, serial, Focus::Keep);
                None
            }
            GrabType::Touch => {
                let Some((touch, start)) = seat
                    .get_touch()
                    .and_then(|touch| Some((touch.clone(), touch.grab_start_data()?)))
                else {
                    source.cancel();
                    return;
                };
                let slot = start.slot;
                let grab = DnDGrab::new_touch(&display, start, source, seat);
                touch.set_grab(self, grab, serial);
                Some(slot)
            }
        };
        if let Some(surface) = icon {
            self.drag_icons.push(DragIcon {
                surface,
                slot,
                offset: Point::default(),
                ended,
            });
        }
    }
}

/// A drag ends at its drop, or cancelled, the lock's included (`lock.rs` ends the seat's grabs):
/// its icon goes with it, and the icon of a drag still held stays.
impl DndGrabHandler for State {
    fn dropped(
        &mut self,
        _target: Option<DndTarget<'_, Self>>,
        _validated: bool,
        _seat: Seat<Self>,
        _location: Point<f64, Logical>,
    ) {
        self.drag_icons
            .retain(|icon| !icon.ended.load(Ordering::SeqCst));
        self.request_redraw();
    }

    fn cancelled(&mut self, _seat: Seat<Self>, _location: Point<f64, Logical>) {
        self.drag_icons
            .retain(|icon| !icon.ended.load(Ordering::SeqCst));
        self.request_redraw();
    }
}

impl State {
    /// Whether the press or the touch point `serial` names is still held: its button not
    /// released, or its finger not lifted. A popup's grab keeps the serial of the press that
    /// opened it past the release, and a drag started with that serial is then refused.
    fn held(&self, serial: Serial, type_: GrabType) -> bool {
        match type_ {
            GrabType::Pointer => self
                .grab_serials
                .held_button
                .is_some_and(|(pressed, _)| pressed == serial),
            GrabType::Touch => self
                .seat
                .get_touch()
                .and_then(|touch| touch.grab_start_data())
                .is_some_and(|start| self.touch_at(start.slot).is_some()),
        }
    }

    /// The drag icons, each with the touch point its drag follows or the pointer. Each is sent
    /// its frames, drawn or not, by the output under that point, as the cursor is.
    pub(crate) fn drag_icons(&self) -> Vec<(WlSurface, Point<f64, Logical>)> {
        self.drag_icons
            .iter()
            .filter_map(|icon| Some((icon.surface.clone(), self.followed(icon)?)))
            .collect()
    }

    /// The drag icons that are drawn, and where each surface goes: at the touch point its drag
    /// follows, or at the pointer, moved by the icon's offset; less an icon whose offsets moved
    /// it off that point, as a cursor whose hotspot lies off it is not drawn (`cursor.rs`).
    pub(crate) fn drawn_drag_icons(&self) -> Vec<(WlSurface, Point<i32, Logical>)> {
        self.drag_icons
            .iter()
            .filter(|icon| {
                let hotspot = Point::from((
                    0i32.saturating_sub(icon.offset.x),
                    0i32.saturating_sub(icon.offset.y),
                ));
                crate::cursor::drawable(bbox_from_surface_tree(&icon.surface, (0, 0)), hotspot)
            })
            .filter_map(|icon| {
                let at = self.followed(icon)?.to_i32_round();
                Some((icon.surface.clone(), moved(at, icon.offset)))
            })
            .collect()
    }

    /// The touch point or the pointer that `icon` follows, if its surface is alive and the
    /// point is there.
    fn followed(&self, icon: &DragIcon) -> Option<Point<f64, Logical>> {
        if !icon.surface.alive() {
            return None;
        }
        match icon.slot {
            Some(slot) => self.touch_at(slot),
            None => Some(self.seat.get_pointer()?.current_location()),
        }
    }

    /// A commit of a drag icon's surface moves the icon by the offset it carries, of
    /// `wl_surface.attach` or of `wl_surface.offset`, from where it was: the icon starts with
    /// its top left at the pointer or the touch point, as `wl_data_device.start_drag` has it.
    pub(crate) fn drag_icon_committed(&mut self, surface: &WlSurface) {
        for icon in self
            .drag_icons
            .iter_mut()
            .filter(|icon| &icon.surface == surface)
        {
            let delta = with_states(surface, |states| {
                states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .buffer_delta
                    .take()
            });
            icon.offset = moved(icon.offset, delta.unwrap_or_default());
        }
    }
}

/// `at` moved by `by`: a client's offsets add up without bound, so the sum saturates.
fn moved(at: Point<i32, Logical>, by: Point<i32, Logical>) -> Point<i32, Logical> {
    Point::from((at.x.saturating_add(by.x), at.y.saturating_add(by.y)))
}
