//! The modules of the bar with a button and a popover (doc_bar.md, BR3), one file each. `new`
//! is the one place a module is registered; the bar draws each one through [`Module`]: its
//! button on every surface, its popover from that button, and the events of the popover. The
//! launcher, application-library and workspaces buttons, the running applications and the
//! control center's button have no popover and stay in `bar.rs`.

mod accessibility;
mod aids;
mod audio;
mod battery;
mod bluetooth;
mod clock;
mod controls;
mod input;
mod network;
mod notifications;
mod power;
mod shield;
mod tiling;
mod tray;

use std::rc::Rc;

use athanor_apps::sal::Icons;
use athanor_bar::order;
use athanor_sal::driver::{Shell, SurfaceId};
use athanor_sal::{Event, Rgba, Theme, Ui, WidgetId};
use athanor_sulphur_client::{Client, Event as Compositor};

/// The size of a module's icon in the bar.
pub const ICON: u16 = 16;

/// What a module's source asks of the bar, from a watch or a reply that arrives later.
pub enum Wakeup {
    /// The source changed: the bar asks every module whether its buttons are stale and sets
    /// the open popover's widgets again.
    Changed,
    /// The module asks for an xdg-activation token for an action of the input just handled,
    /// which lets the application it calls raise its window: the bar asks the compositor with
    /// the last input's surface and serial, and hands the token to `activation_token`.
    Activation(order::Module),
}

/// The bar's handle for a module's sources, which may outlive it.
pub type Wake = Rc<dyn Fn(Wakeup)>;

/// What a module gets once, when the bar starts.
pub struct Start<'a> {
    /// `None` without the privileged globals (SH1): the modules that need it stay hidden.
    pub client: Option<&'a Rc<Client>>,
    pub wake: &'a Wake,
}

/// What a module draws with, for one surface of the bar.
pub struct Cx<'a> {
    /// The icons, tinted with the bar's ink.
    pub icons: &'a mut Icons,
    /// The popovers' look.
    pub theme: &'a Theme,
    /// The popovers' secondary ink, calmo's ink2, as `bar-popover-note`.
    pub secondary: Rgba,
    pub rtl: bool,
    /// The bar is at the top of its output: the popover's arrow points up.
    pub top: bool,
    pub scale: u16,
    /// The surface's output, by connector name.
    pub connector: Option<&'a str>,
}

/// What a module's own surfaces see of the bar, once a turn.
pub struct Around<'a> {
    /// The popovers' look.
    pub theme: &'a Theme,
    pub rtl: bool,
    /// The bar is at the top of its outputs.
    pub top: bool,
    /// The output of the open popover, by name: a module's surface there shows nothing, so
    /// that the overlap never depends on the compositor's drawing order (BR6, "Stacking").
    pub covered: Option<&'a str>,
    /// A popover of the bar is open.
    pub popover: bool,
}

/// What the bar does after an event of a module's popover, or a refresh of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Stay,
    Close,
    /// Build the popover again from its button: a second page, or a row that went.
    Reopen,
}

/// A module with a button and a popover.
pub trait Module {
    /// Adds the module's button to `group`, or `None` while the module is hidden. The bar
    /// marks it as opening a popover.
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId>;

    /// Whether this surface's button shows something other than its source now: polled at
    /// every turn of the loop, so it reads only state already at hand.
    fn stale(&self, _cx: &Cx) -> bool {
        false
    }

    /// Once a second, at the bar's tick, before [`Module::stale`]: where a module reads a
    /// source that no watch follows, as the clock's wall time and zone.
    fn tick(&mut self) {}

    /// Sets `button` to what the source says now; false when the surface must be drawn
    /// again, as when the button comes, goes or changes shape.
    fn update(&mut self, _cx: &mut Cx, _ui: &mut Ui, _button: WidgetId) -> bool {
        false
    }

    /// Whether the popover the captures ask for (`ATHANOR_BAR_OPEN`) may open now, its content
    /// final; the bar asks again at every turn until it may.
    fn ready(&self) -> bool {
        true
    }

    /// The popover, built from the source now; `None`, logged, when it cannot be drawn.
    fn open(&mut self, cx: &mut Cx) -> Option<Ui>;

    /// An event of the open popover.
    fn event(&mut self, cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply;

    /// A source changed while the popover is open: its widgets follow in place.
    fn refresh(&mut self, _cx: &mut Cx, _ui: &mut Ui) -> Reply {
        Reply::Stay
    }

    /// Whether the module's source asks for its popover, as the network's password prompt, the
    /// tray's menu once its item answered, or a refusal the shield's sheet must show: polled at
    /// every turn of the loop once the bar is ready, and true once for each ask. A source that
    /// asks wakes the bar with [`Wakeup::Changed`], unless the page polls its own model. The bar
    /// opens a closed popover from [`Module::anchor`] on the surface of the module's last
    /// input; an open one follows through [`Module::refresh`].
    fn wants_open(&mut self) -> bool {
        false
    }

    /// Whether the open popover's source changed without a [`Wakeup`], as a page that follows
    /// its model itself: polled at every turn of the loop while the popover is open, and
    /// [`Module::refresh`] follows.
    fn changed(&self) -> bool {
        false
    }

    /// The popover closed, by Escape, a press outside it, its button or a reply.
    fn closed(&mut self) {}

    /// An event of the compositor; a sentence to announce.
    fn heard(&mut self, _event: &Compositor) -> Option<String> {
        None
    }

    /// The widget the popover opens from: `button`, or for a module whose button is a row of
    /// widgets of its own (the tray's items), one of them, or `None` while the row is empty.
    /// The bar marks `button` as opening a popover only when it is its own anchor.
    fn anchor(&self, _ui: &Ui, button: WidgetId) -> Option<WidgetId> {
        Some(button)
    }

    /// An input on `widget`, one of the widgets inside `button` that [`Module::anchor`] can
    /// name: [`Event::Clicked`], [`Event::ContextMenu`], [`Event::MiddleClick`] or
    /// [`Event::Wheel`]. A module that opens its popover from it asks through
    /// [`Module::wants_open`].
    fn input(&mut self, _ui: &Ui, _button: WidgetId, _widget: WidgetId, _event: &Event) {}

    /// The module's own layer surfaces, beside the bar's, drawn at the end of every turn: it
    /// opens, draws and hides them through `shell`. The driver closes one whose output left,
    /// and `shell.ui` then answers `None`.
    fn surfaces(&mut self, _shell: &mut Shell, _around: &Around) {}

    /// An event of a widget of `surface`, which is not the bar's nor its popover: one of the
    /// module's own surfaces, or another module's.
    fn surface_event(&mut self, _surface: SurfaceId, _event: &Event) {}

    /// The pointer entered (`inside`) or left `surface`, which may be one of the module's own.
    fn pointer(&mut self, _surface: SurfaceId, _inside: bool) {}

    /// The primary button was pressed at `at`, in logical pixels, on `ui`: the module's open
    /// popover for `None`, else `Some` surface that is not the bar's, which may be one of the
    /// module's own. Where a widget that takes no focus answers the pointer, as GTK's click
    /// gesture on a box.
    fn pressed(&mut self, _ui: &Ui, _surface: Option<SurfaceId>, _at: (f32, f32)) {}

    /// The token of a `Wakeup::Activation`, in the order they were asked for; `None` when the
    /// compositor offers no xdg-activation.
    fn activation_token(&mut self, _token: Option<String>) {}
}

/// The module that draws `module`, or `None` for one without a popover or not ported yet.
pub fn new(module: order::Module, start: &Start) -> Option<Box<dyn Module>> {
    use order::Module as M;
    match module {
        M::Clock => clock::new(start),
        M::InputSource => input::new(start),
        M::Accessibility => accessibility::new(start),
        M::Tray => tray::new(start),
        M::Tiling => tiling::new(start),
        M::Audio => audio::new(start),
        M::Bluetooth => bluetooth::new(start),
        M::Network => network::new(start),
        M::Battery => battery::new(start),
        M::Notifications => notifications::new(start),
        M::Power => power::new(start),
        M::Shield => shield::new(start),
        M::Launcher | M::AppLibrary | M::Workspaces | M::RunningApps | M::ControlCenter => None,
    }
}
