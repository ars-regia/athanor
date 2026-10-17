//! The Wayland driver on smithay-client-toolkit: one xdg-toplevel drawn in shared memory, the
//! keyboard and the pointer, the input method through text-input-v3, and the AccessKit adapter
//! on AT-SPI.

use crate::a11y::tree_update;
use crate::render::{render, Error};
use crate::widget::{Event, Key, Ui, WindowButton};
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::{self, ZwpTextInputManagerV3},
    zwp_text_input_v3::{self, ContentHint, ContentPurpose, ZwpTextInputV3},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    dispatch2::Dispatch2,
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
            window::{DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler},
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
    // Optional: without it, text comes from the keyboard alone.
    let text_input_manager = globals.bind(&qh, 1..=1, TextInput).ok();
    let surface = compositor.create_surface(&qh);
    // The window draws its own title bar, as GTK does, so assistive technologies find the
    // title and the window's buttons in its tree; a compositor that insists on drawing them
    // says so in the first configure.
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
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
        pointer_seat: None,
        text_input_manager,
        text_input: None,
        ime: Ime::default(),
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

/// The text-input-v3 state of the window.
#[derive(Default)]
struct Ime {
    /// The compositor sent `enter` for our surface and no `leave` since.
    entered: bool,
    /// We sent `enable` and no `disable` since.
    enabled: bool,
    pending: Pending,
}

/// The events of text-input-v3 that take effect together at its next `done`.
#[derive(Default)]
struct Pending {
    /// The preedit and its cursor, a byte range in it.
    preedit: Option<(String, Option<(usize, usize)>)>,
    commit: Option<String>,
}

/// Text from the input method without control characters: the field holds one line.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// The user data of the text-input-v3 objects.
struct TextInput;

impl Dispatch2<ZwpTextInputManagerV3, App> for TextInput {
    fn event(
        &self,
        _: &mut App,
        _: &ZwpTextInputManagerV3,
        _: zwp_text_input_manager_v3::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

impl Dispatch2<ZwpTextInputV3, App> for TextInput {
    fn event(
        &self,
        app: &mut App,
        _: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use zwp_text_input_v3::Event;
        match event {
            Event::Enter { .. } => {
                app.ime.entered = true;
                app.sync_text_input();
            }
            Event::Leave { .. } => {
                app.ime.entered = false;
                app.sync_text_input();
            }
            Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let cursor = usize::try_from(cursor_begin)
                    .ok()
                    .zip(usize::try_from(cursor_end).ok());
                app.ime.pending.preedit = Some((text.unwrap_or_default(), cursor));
            }
            Event::CommitString { text } => app.ime.pending.commit = text,
            // ponytail: the field sends no surrounding text, so an input method has none to
            // delete; send it with set_surrounding_text when an engine needs it.
            Event::DeleteSurroundingText { .. } => {}
            Event::Done { .. } => app.text_input_done(),
            _ => {}
        }
    }
}

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
    /// The seat of the pointer, which moving the window by its title bar names.
    pointer_seat: Option<wl_seat::WlSeat>,
    text_input_manager: Option<ZwpTextInputManagerV3>,
    text_input: Option<ZwpTextInputV3>,
    ime: Ime,
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
        match event {
            Some(Event::Window(WindowButton::Minimize)) => self.window.set_minimized(),
            Some(Event::Window(WindowButton::Close)) => {
                self.exit = true;
                return;
            }
            Some(event) => (self.on_event)(&mut self.ui, event),
            None => {}
        }
        if let Err(error) = self.draw() {
            self.error = Some(error);
            self.exit = true;
        }
        self.sync_text_input();
    }

    /// Enables the input method while the window has the keyboard and a field the focus, and
    /// disables it otherwise.
    fn sync_text_input(&mut self) {
        let Some(text_input) = &self.text_input else {
            return;
        };
        let field = self
            .ui
            .focus()
            .filter(|&id| self.ime.entered && self.ui.field_text(id).is_some());
        if field.is_some() == self.ime.enabled {
            return;
        }
        if let Some(id) = field {
            let r = self.ui.widget(id).rect;
            text_input.enable();
            text_input.set_content_type(ContentHint::None, ContentPurpose::Normal);
            text_input.set_cursor_rectangle(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        } else {
            text_input.disable();
        }
        text_input.commit();
        self.ime.enabled = field.is_some();
    }

    /// Applies what the input method sent since the last `done`, in the protocol's order: the
    /// committed text replaces the preedit, then the new preedit is shown.
    fn text_input_done(&mut self) {
        let Pending { preedit, commit } = std::mem::take(&mut self.ime.pending);
        let event = commit.and_then(|text| self.ui.commit(&printable(&text)));
        let (preedit, cursor) = preedit.unwrap_or_default();
        self.ui.compose(&printable(&preedit), cursor);
        self.changed(event);
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
        configure: WindowConfigure,
        _: u32,
    ) {
        // ponytail: once drawn, the title bar stays if the compositor later takes the
        // decorations over; no compositor of Athanor's switches modes on a mapped window.
        if configure.decoration_mode == DecorationMode::Client {
            self.ui.add_title_bar();
        }
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
            Capability::Keyboard if self.keyboard.is_none() => {
                if let (Some(manager), None) = (&self.text_input_manager, &self.text_input) {
                    self.text_input = Some(manager.get_text_input(&seat, qh, TextInput));
                }
                self.seat
                    .get_keyboard_with_repeat(
                        qh,
                        &seat,
                        None,
                        self.loop_handle.clone(),
                        Box::new(|app: &mut App, _, event| app.repeat(event)),
                    )
                    .map(|k| self.keyboard = Some(k))
                    .map_err(Into::into)
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer_seat = Some(seat.clone());
                self.seat
                    .get_pointer(qh, &seat)
                    .map(|p| self.pointer = Some(p))
                    .map_err(Into::into)
            }
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
                button: BTN_LEFT,
                serial,
                ..
            } = event.kind
            {
                let (x, y) = (event.position.0 as f32, event.position.1 as f32);
                // A press on the title bar outside its buttons moves the window.
                if self.ui.hit(x, y).is_none() && self.ui.in_title_bar(x, y) {
                    if let Some(seat) = &self.pointer_seat {
                        self.window.move_(seat, serial);
                    }
                    continue;
                }
                let event = self.ui.click(x, y);
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
