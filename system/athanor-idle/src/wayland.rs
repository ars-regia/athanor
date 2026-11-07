//! The Wayland side of `athanor-idle` (SD4), on `wayland-client` with no toolkit: the two
//! `ext_idle_notification_v1` timers, the overlay that fades each output to black with a
//! `wp_single_pixel_buffer_v1` buffer, and `zwlr_output_power_v1`.
//!
//! [`Display`] never blocks: [`Display::read`] takes what the socket holds and
//! [`Display::dispatch`] hands back the notices the events made, so the caller can drive it
//! from an async loop on [`Display::fd`], and a test from Sulphur's fixture, in one thread.

use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use wayland_client::backend::WaylandError;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_region, wl_registry, wl_seat, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1;
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1, zwlr_output_power_v1,
};

/// The layer surface's namespace.
const NAMESPACE: &str = "athanor-idle";

/// Which of the two notifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Blank,
    Suspend,
}

/// What an `ext_idle_notification_v1` reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    Idled(Which),
    Resumed(Which),
}

/// A connection's error, or a global the daemon cannot run without.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Wayland(WaylandError),
    Missing(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Wayland(e) => write!(f, "{e}"),
            Error::Missing(global) => write!(f, "the compositor offers no {global}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wayland_client::DispatchError> for Error {
    fn from(error: wayland_client::DispatchError) -> Self {
        match error {
            wayland_client::DispatchError::Backend(e) => e.into(),
            other => Error::Io(std::io::Error::other(other)),
        }
    }
}

impl From<WaylandError> for Error {
    fn from(error: WaylandError) -> Self {
        match error {
            WaylandError::Io(e) => Error::Io(e),
            other => Error::Wayland(other),
        }
    }
}

/// The overlay of one output.
struct Overlay {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    viewport: wp_viewport::WpViewport,
    /// The size of the last configure, once acknowledged.
    size: Option<(i32, i32)>,
    /// A frame callback is pending.
    frame: bool,
    /// The buffer last attached. Each draw destroys the one before, released or not, so a
    /// compositor that never sends `release` cannot make them grow: a single-pixel buffer has
    /// no storage the compositor could still be reading.
    buffer: Option<wl_buffer::WlBuffer>,
}

impl Overlay {
    fn destroy(self) {
        self.viewport.destroy();
        self.layer.destroy();
        self.surface.destroy();
        if let Some(buffer) = self.buffer {
            buffer.destroy();
        }
    }
}

struct Output {
    name: u32,
    output: wl_output::WlOutput,
    power: Option<zwlr_output_power_v1::ZwlrOutputPowerV1>,
    overlay: Option<Overlay>,
}

/// How the overlay is drawn: rising from `start` over `fade`, or opaque.
#[derive(Clone, Copy)]
struct Shown {
    start: Instant,
    fade: Duration,
}

impl Shown {
    /// Linear in time, from transparent to opaque.
    fn alpha(self, now: Instant) -> f64 {
        if self.fade.is_zero() {
            return 1.0;
        }
        (now.duration_since(self.start).as_secs_f64() / self.fade.as_secs_f64()).min(1.0)
    }
}

/// The dispatch state.
#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    seat: Option<wl_seat::WlSeat>,
    notifier: Option<ext_idle_notifier_v1::ExtIdleNotifierV1>,
    layers: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pixels: Option<wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    power: Option<zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1>,
    outputs: Vec<Output>,
    /// The timeout each notification should have, and the one it has.
    want: [Option<Duration>; 2],
    notifications: [Option<ext_idle_notification_v1::ExtIdleNotificationV1>; 2],
    shown: Option<Shown>,
    off: bool,
    notices: Vec<Notice>,
}

fn index(which: Which) -> usize {
    match which {
        Which::Blank => 0,
        Which::Suspend => 1,
    }
}

impl State {
    /// Creates the notifications that are wanted and do not exist, once the seat and the
    /// notifier are bound.
    fn notify(&mut self, qh: &QueueHandle<State>) {
        let (Some(notifier), Some(seat)) = (&self.notifier, &self.seat) else {
            return;
        };
        for which in [Which::Blank, Which::Suspend] {
            let i = index(which);
            if let (Some(after), None) = (self.want[i], &self.notifications[i]) {
                let ms = u32::try_from(after.as_millis()).unwrap_or(u32::MAX);
                self.notifications[i] = Some(notifier.get_idle_notification(ms, seat, qh, which));
            }
        }
    }

    /// An overlay surface on `output`, configured by the compositor before it is drawn.
    fn overlay(&self, output: &wl_output::WlOutput, qh: &QueueHandle<State>) -> Option<Overlay> {
        let (Some(compositor), Some(layers), Some(viewporter)) =
            (&self.compositor, &self.layers, &self.viewporter)
        else {
            return None;
        };
        self.pixels.as_ref()?;
        let surface = compositor.create_surface(qh, ());
        let region = compositor.create_region(qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let viewport = viewporter.get_viewport(&surface, qh, ());
        let layer = layers.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Overlay,
            NAMESPACE.into(),
            qh,
            (),
        );
        use zwlr_layer_surface_v1::Anchor;
        layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        surface.commit();
        Some(Overlay {
            surface,
            layer,
            viewport,
            size: None,
            frame: false,
            buffer: None,
        })
    }

    /// Draws `overlay` at the alpha of now, and asks for the next frame while it fades.
    fn draw(&self, overlay: &mut Overlay, qh: &QueueHandle<State>) {
        let (Some(shown), Some(pixels), Some((width, height))) =
            (self.shown, &self.pixels, overlay.size)
        else {
            return;
        };
        let alpha = shown.alpha(Instant::now());
        // Premultiplied black: only alpha is not 0.
        let a = (alpha * f64::from(u32::MAX)).round() as u32;
        let buffer = pixels.create_u32_rgba_buffer(0, 0, 0, a, qh, ());
        overlay.surface.attach(Some(&buffer), 0, 0);
        overlay.viewport.set_destination(width, height);
        overlay.surface.damage_buffer(0, 0, 1, 1);
        if alpha < 1.0 && !overlay.frame {
            overlay.surface.frame(qh, overlay.surface.clone());
            overlay.frame = true;
        }
        overlay.surface.commit();
        if let Some(old) = overlay.buffer.replace(buffer) {
            old.destroy();
        }
    }

    fn set_power(&self, output: &Output) {
        if let Some(power) = &output.power {
            power.set_mode(if self.off {
                zwlr_output_power_v1::Mode::Off
            } else {
                zwlr_output_power_v1::Mode::On
            });
        }
    }

    fn add_output(&mut self, name: u32, output: wl_output::WlOutput, qh: &QueueHandle<State>) {
        let power = self
            .power
            .as_ref()
            .map(|manager| manager.get_output_power(&output, qh, ()));
        let overlay = self.shown.and_then(|_| self.overlay(&output, qh));
        let output = Output {
            name,
            output,
            power,
            overlay,
        };
        if self.off {
            self.set_power(&output);
        }
        self.outputs.push(output);
    }
}

/// The connection to the compositor and what the daemon holds on it.
pub struct Display {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl Display {
    /// Connects through `WAYLAND_DISPLAY` and binds the globals, waiting for the compositor.
    pub fn connect() -> Result<Self, Error> {
        let connection =
            Connection::connect_to_env().map_err(|e| Error::Io(std::io::Error::other(e)))?;
        let mut display = Self::new(connection);
        // The globals, then the objects bound from them.
        display.queue.roundtrip(&mut display.state)?;
        display.queue.roundtrip(&mut display.state)?;
        display.check()?;
        Ok(display)
    }

    /// On a connection made already; the globals arrive with the next events, so a caller
    /// that cannot block pumps [`Display::read`] and [`Display::dispatch`] until
    /// [`Display::check`] passes.
    pub fn new(connection: Connection) -> Self {
        let queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        Self {
            connection,
            queue,
            state: State::default(),
        }
    }

    /// Fails on the first global the daemon cannot run without: the seat and the idle
    /// notifier. The overlay and the output power are logged once when missing, and the rest
    /// of the sequence runs without them.
    pub fn check(&self) -> Result<(), Error> {
        let s = &self.state;
        if s.compositor.is_none() {
            return Err(Error::Missing("wl_compositor"));
        }
        if s.seat.is_none() {
            return Err(Error::Missing("wl_seat"));
        }
        if s.notifier.is_none() {
            return Err(Error::Missing("ext_idle_notifier_v1"));
        }
        for (missing, global) in [
            (s.layers.is_none(), "zwlr_layer_shell_v1"),
            (s.pixels.is_none(), "wp_single_pixel_buffer_manager_v1"),
            (s.viewporter.is_none(), "wp_viewporter"),
        ] {
            if missing {
                tracing::warn!(
                    "the compositor offers no {global}; the screens blank without a fade"
                );
            }
        }
        if s.power.is_none() {
            tracing::warn!(
                "the compositor offers no zwlr_output_power_manager_v1; the outputs stay on"
            );
        }
        Ok(())
    }

    /// The socket, to wait on for events.
    pub fn fd(&self) -> std::io::Result<OwnedFd> {
        self.connection.backend().poll_fd().try_clone_to_owned()
    }

    /// Sends what is queued. A full socket keeps the rest for the next flush.
    pub fn flush(&self) -> Result<(), Error> {
        match self.connection.flush() {
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            other => Ok(other?),
        }
    }

    /// Flushes, then reads what the socket holds without waiting. `WouldBlock` when it holds
    /// nothing, so an async caller can clear the socket's readiness.
    pub fn read(&mut self) -> std::io::Result<()> {
        self.flush().map_err(|e| match e {
            Error::Io(e) => e,
            other => std::io::Error::other(other.to_string()),
        })?;
        match self.queue.prepare_read() {
            Some(guard) => match guard.read() {
                Ok(_) => Ok(()),
                Err(WaylandError::Io(e)) => Err(e),
                Err(other) => Err(std::io::Error::other(other)),
            },
            // Events are queued already: dispatching them comes first.
            None => Ok(()),
        }
    }

    /// Dispatches the events read so far and returns the notices they made, in order.
    pub fn dispatch(&mut self) -> Result<Vec<Notice>, Error> {
        self.queue.dispatch_pending(&mut self.state)?;
        self.flush()?;
        Ok(std::mem::take(&mut self.state.notices))
    }

    /// The timeout of a notification: `None` creates none. A changed timeout destroys the
    /// notification and creates it again, so the caller MUST first report a blank
    /// notification that idled without a `resumed` (`Event::Withdrawn`).
    pub fn set_timeout(&mut self, which: Which, after: Option<Duration>) {
        let i = index(which);
        if self.state.want[i] == after {
            return;
        }
        self.state.want[i] = after;
        if let Some(notification) = self.state.notifications[i].take() {
            notification.destroy();
        }
        self.state.notify(&self.queue.handle());
    }

    /// The timeout a notification has.
    pub fn timeout(&self, which: Which) -> Option<Duration> {
        self.state.want[index(which)]
    }

    /// An overlay on every output, rising to opaque black over `fade`, opaque at once when
    /// `fade` is zero.
    pub fn show_overlay(&mut self, fade: Duration) {
        self.remove_overlay();
        self.state.shown = Some(Shown {
            start: Instant::now(),
            fade,
        });
        let qh = self.queue.handle();
        for i in 0..self.state.outputs.len() {
            let overlay = self.state.overlay(&self.state.outputs[i].output, &qh);
            self.state.outputs[i].overlay = overlay;
        }
    }

    pub fn remove_overlay(&mut self) {
        self.state.shown = None;
        for output in &mut self.state.outputs {
            if let Some(overlay) = output.overlay.take() {
                overlay.destroy();
            }
        }
    }

    /// Every output off, or on; an output that appears takes the same mode.
    pub fn set_outputs_off(&mut self, off: bool) {
        self.state.off = off;
        for output in &self.state.outputs {
            self.state.set_power(output);
        }
    }

    /// Whether the overlay is shown.
    pub fn overlay_shown(&self) -> bool {
        self.state.shown.is_some()
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, 1, qh, ()));
                    state.notify(qh);
                }
                // Version 1: its notifications honour idle inhibitors (SD4).
                "ext_idle_notifier_v1" if state.notifier.is_none() => {
                    state.notifier = Some(registry.bind(name, 1, qh, ()));
                    state.notify(qh);
                }
                "zwlr_layer_shell_v1" if state.layers.is_none() => {
                    state.layers = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wp_single_pixel_buffer_manager_v1" if state.pixels.is_none() => {
                    state.pixels = Some(registry.bind(name, 1, qh, ()));
                }
                "wp_viewporter" if state.viewporter.is_none() => {
                    state.viewporter = Some(registry.bind(name, 1, qh, ()));
                }
                "zwlr_output_power_manager_v1" if state.power.is_none() => {
                    let manager: zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1 =
                        registry.bind(name, 1, qh, ());
                    for output in &mut state.outputs {
                        output.power = Some(manager.get_output_power(&output.output, qh, ()));
                    }
                    state.power = Some(manager);
                    if state.off {
                        for output in &state.outputs {
                            state.set_power(output);
                        }
                    }
                }
                "wl_output" => {
                    let output = registry.bind(name, version.min(4), qh, ());
                    state.add_output(name, output, qh);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(at) = state.outputs.iter().position(|o| o.name == name) {
                    let output = state.outputs.remove(at);
                    if let Some(overlay) = output.overlay {
                        overlay.destroy();
                    }
                    if let Some(power) = output.power {
                        power.destroy();
                    }
                    if output.output.version() >= 3 {
                        output.output.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, Which> for State {
    fn event(
        state: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        which: &Which,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => state.notices.push(Notice::Idled(*which)),
            ext_idle_notification_v1::Event::Resumed => state.notices.push(Notice::Resumed(*which)),
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let Some(at) = state.outputs.iter().position(|o| {
            o.overlay
                .as_ref()
                .is_some_and(|overlay| &overlay.layer == layer)
        }) else {
            return;
        };
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
                    return;
                };
                if width == 0 || height == 0 {
                    return;
                }
                if let Some(mut overlay) = state.outputs[at].overlay.take() {
                    overlay.size = Some((width, height));
                    state.draw(&mut overlay, qh);
                    state.outputs[at].overlay = Some(overlay);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(overlay) = state.outputs[at].overlay.take() {
                    overlay.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, wl_surface::WlSurface> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !matches!(event, wl_callback::Event::Done { .. }) {
            return;
        }
        let Some(at) = state.outputs.iter().position(|o| {
            o.overlay
                .as_ref()
                .is_some_and(|overlay| &overlay.surface == surface)
        }) else {
            return;
        };
        if let Some(mut overlay) = state.outputs[at].overlay.take() {
            overlay.frame = false;
            state.draw(&mut overlay, qh);
            state.outputs[at].overlay = Some(overlay);
        }
    }
}

impl Dispatch<zwlr_output_power_v1::ZwlrOutputPowerV1, ()> for State {
    fn event(
        state: &mut Self,
        power: &zwlr_output_power_v1::ZwlrOutputPowerV1,
        event: zwlr_output_power_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The output is gone or another client holds its power: this object is inert.
        if let zwlr_output_power_v1::Event::Failed = event {
            power.destroy();
            for output in &mut state.outputs {
                if output.power.as_ref() == Some(power) {
                    output.power = None;
                }
            }
        }
    }
}

wayland_client::delegate_noop!(State: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore wl_output::WlOutput);
wayland_client::delegate_noop!(State: ignore wl_surface::WlSurface);
// `release`: the next draw or the overlay's removal destroys the buffer (`Overlay::buffer`).
wayland_client::delegate_noop!(State: ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(State: wl_region::WlRegion);
wayland_client::delegate_noop!(State: ext_idle_notifier_v1::ExtIdleNotifierV1);
wayland_client::delegate_noop!(State: zwlr_layer_shell_v1::ZwlrLayerShellV1);
wayland_client::delegate_noop!(State: wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1);
wayland_client::delegate_noop!(State: wp_viewporter::WpViewporter);
wayland_client::delegate_noop!(State: wp_viewport::WpViewport);
wayland_client::delegate_noop!(State: zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FADE;

    #[test]
    fn the_fade_is_linear_and_ends_opaque() {
        let start = Instant::now();
        let shown = Shown { start, fade: FADE };
        assert_eq!(shown.alpha(start), 0.0);
        assert!((shown.alpha(start + FADE / 2) - 0.5).abs() < 1e-9);
        assert_eq!(shown.alpha(start + FADE), 1.0);
        assert_eq!(shown.alpha(start + FADE * 2), 1.0);
        let opaque = Shown {
            start,
            fade: Duration::ZERO,
        };
        assert_eq!(opaque.alpha(start), 1.0);
    }
}
