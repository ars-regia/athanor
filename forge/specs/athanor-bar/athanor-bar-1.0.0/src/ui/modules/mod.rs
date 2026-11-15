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
use athanor_sal::{Event, Theme, Ui, WidgetId};
use athanor_sulphur_client::{Client, Event as Compositor};

/// The size of a module's icon in the bar.
pub const ICON: u16 = 16;

/// What a module's source asks of the bar, from a watch or a reply that arrives later.
pub enum Wakeup {
    /// The source changed: the bar asks every module whether its buttons are stale and sets
    /// the open popover's widgets again.
    Changed,
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
    pub rtl: bool,
    /// The bar is at the top of its output: the popover's arrow points up.
    pub top: bool,
    pub scale: u16,
    /// The surface's output, by connector name.
    pub connector: Option<&'a str>,
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

    /// The popover closed, by Escape, a press outside it, its button or a reply.
    fn closed(&mut self) {}

    /// An event of the compositor; a sentence to announce.
    fn heard(&mut self, _event: &Compositor) -> Option<String> {
        None
    }
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
