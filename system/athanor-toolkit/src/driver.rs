//! The Wayland driver on smithay-client-toolkit: one xdg-toplevel drawn in shared memory, the
//! keyboard and the pointer, and the AccessKit adapter on AT-SPI.

use crate::a11y::tree_update;
use crate::render::{render, Error};
use crate::widget::{Event, Key, Ui};
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::{Arc, Mutex};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// What the application does with each event; it may change the widgets.
pub type Handler = Box<dyn FnMut(&mut Ui, Event)>;

/// Opens `ui` in a window and runs until the user closes it.
pub fn run(ui: Ui, app_id: &str, on_event: Handler) -> Result<(), Error> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title(ui.title.as_str());
    window.set_app_id(app_id);
    window.commit();

    // The adapter calls its handlers on its own thread: the first tree comes from the last
    // update drawn, and actions travel to the event loop.
    let initial = Arc::new(Mutex::new(tree_update(&ui, 1.0)));
    let (actions, receiver) = channel::channel();
    event_loop
        .handle()
        .insert_source(receiver, |event, _, app: &mut App| {
            if let channel::Event::Msg(request) = event {
                let event = app.ui.do_action(&request);
                app.changed(event);
            }
        })
        .map_err(|e| e.error.to_string())?;
    let a11y = accesskit_unix::Adapter::new(
        Activation(Arc::clone(&initial)),
        Actions(actions),
        Deactivation,
    );

    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm)?,
        shm,
        window,
        keyboard: None,
        pointer: None,
        loop_handle: event_loop.handle(),
        modifiers: Modifiers::default(),
        scale: 1,
        configured: false,
        exit: false,
        ui,
        on_event,
        a11y,
        initial,
        error: None,
    };
    while !app.exit {
        event_loop.dispatch(None, &mut app)?;
        if let Some(error) = app.error.take() {
            return Err(error);
        }
    }
    Ok(())
}

struct Activation(Arc<Mutex<TreeUpdate>>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok().map(|tree| tree.clone())
    }
}

struct Actions(Sender<ActionRequest>);

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // The receiver lives as long as the event loop; once it is gone the window is closing
        // and there is nothing left to act on.
        let _ = self.0.send(request);
    }
}

struct Deactivation;

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

struct App {
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    /// For the keyboard's repeat timer. The keyboard's data keeps a clone, and the loop owns the
    /// connection that owns that data: the cycle lives until the process ends, as `run` is
    /// called once per process.
    loop_handle: LoopHandle<'static, App>,
    modifiers: Modifiers,
    scale: i32,
    configured: bool,
    exit: bool,
    ui: Ui,
    on_event: Handler,
    a11y: accesskit_unix::Adapter,
    initial: Arc<Mutex<TreeUpdate>>,
    /// An error raised inside a callback, returned by [`run`].
    error: Option<Error>,
}

impl App {
    /// Hands an event to the application, then redraws and updates the accessibility tree.
    fn changed(&mut self, event: Option<Event>) {
        if let Some(event) = event {
            (self.on_event)(&mut self.ui, event);
        }
        if let Err(error) = self.draw() {
            self.error = Some(error);
            self.exit = true;
        }
    }

    fn draw(&mut self) -> Result<(), Error> {
        if !self.configured {
            return Ok(());
        }
        let pixmap = render(&mut self.ui, self.scale as f32)?;
        let (w, h) = (i32::from(pixmap.width()), i32::from(pixmap.height()));
        let (buffer, canvas) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)?;
        // vello_cpu writes premultiplied RGBA bytes; ARGB8888 is BGRA in memory.
        for (dst, src) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixmap.data_as_u8_slice().as_chunks::<4>().0)
        {
            *dst = [src[2], src[1], src[0], src[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(self.scale);
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(surface)?;
        self.window.commit();

        let update = tree_update(&self.ui, f64::from(self.scale));
        if let Ok(mut initial) = self.initial.lock() {
            initial.clone_from(&update);
        }
        self.a11y.update_if_active(|| update);
        Ok(())
    }

    /// A key held down: as pressed again, except Enter, and Space outside a field, which
    /// activate once.
    fn repeat(&mut self, event: KeyEvent) {
        let in_field = self
            .ui
            .focus()
            .is_some_and(|id| self.ui.field_text(id).is_some());
        let activates = match event.keysym {
            Keysym::Return | Keysym::KP_Enter => true,
            Keysym::space => !in_field,
            _ => false,
        };
        if !activates {
            self.key(event);
        }
    }

    fn key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Tab if self.modifiers.shift => Key::BackTab,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            Keysym::Left => Key::Left,
            Keysym::Right => Key::Right,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::BackSpace => Key::Backspace,
            Keysym::Delete => Key::Delete,
            _ => match event.utf8 {
                Some(text) if !text.chars().any(char::is_control) => Key::Text(text),
                _ => return,
            },
        };
        let event = self.ui.key(key);
        self.changed(event);
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        // ponytail: integer buffer scale; fractional scale (wp_fractional_scale_v1 with
        // wp_viewporter) comes with the scaling milestone.
        self.scale = factor.max(1);
        self.changed(None);
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
        // ponytail: the window keeps the widgets' natural size whatever the compositor
        // suggests; stretching to the configured size comes with the layout milestone.
        self.configured = true;
        self.changed(None);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let result: Result<(), Error> = match capability {
            Capability::Keyboard if self.keyboard.is_none() => self
                .seat
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.loop_handle.clone(),
                    Box::new(|app: &mut App, _, event| app.repeat(event)),
                )
                .map(|k| self.keyboard = Some(k))
                .map_err(Into::into),
            Capability::Pointer if self.pointer.is_none() => self
                .seat
                .get_pointer(qh, &seat)
                .map(|p| self.pointer = Some(p))
                .map_err(Into::into),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.error = Some(error);
            self.exit = true;
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
            }
            Capability::Pointer => {
                if let Some(pointer) = self.pointer.take() {
                    pointer.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.a11y.update_window_focus_state(true);
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.a11y.update_window_focus_state(false);
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat(event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if let PointerEventKind::Press {
                button: BTN_LEFT, ..
            } = event.kind
            {
                let (x, y) = event.position;
                let event = self.ui.click(x as f32, y as f32);
                self.changed(event);
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
