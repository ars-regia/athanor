//! The control center's surfaces (CC9): one layer surface per output, opened once and kept.
//! Hidden, it is 1×1 on the background layer with nothing drawn, no input, no keyboard and out
//! of the accessibility tree. Shown, it covers its output on the overlay layer and takes the
//! keyboard, and draws the panel in an island at the corner next to the bar's button; a press
//! outside the island, Escape, or the loss of the keyboard closes it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use athanor_control_center::{anchor, focused_output, Page, Side, GAP};
use athanor_layout::loader::Source;
use athanor_sal::driver::{Anchor, KeyboardInteractivity, Layer, OutputInfo};
use athanor_sal::{
    Align, Application, Event, Kind, LayerOptions, Rgba, Shell, SurfaceId, Theme, Ui, WidgetId,
};
use athanor_style::appearance::{self, Appearance};
use athanor_style::sal;
use athanor_sulphur_client::{theme as cosmic, Client};

use crate::bus::Bus;
use crate::i18n::{is_rtl, tr};
use crate::icons::Icons;
use crate::panel::{Action, Panel, WIDTH};

/// The layer shell's namespace of every control center surface.
const NAMESPACE: &str = "athanor-control-center";
/// What starts Settings from the footer (CC4).
const SETTINGS: &str = "com.system76.CosmicSettings.desktop";
/// How often the media section's position moves while a track plays.
const TICK: Duration = Duration::from_secs(1);

/// What reaches the surfaces from outside the toolkit's loop.
pub enum Input {
    /// `Show` on the bus: the panel, or the page named.
    Show(Option<Page>),
    Toggle,
    Appearance(Appearance),
    /// A second passed: the position of the track that plays moves.
    Tick,
}

/// One output's surface.
#[derive(Default)]
struct Output {
    connector: Option<String>,
    scale: u16,
}

/// The panel on screen.
struct Shown {
    surface: SurfaceId,
    /// The surface has had the keyboard since the panel was shown, so losing it now is a loss
    /// of focus and not the keyboard not having arrived yet.
    focused: bool,
}

pub struct Center {
    inputs: Rc<RefCell<Vec<Input>>>,
    client: Option<Rc<Client>>,
    layout: Source,
    bus: Rc<Bus>,
    appearance: Appearance,
    icons: Icons,
    panel: Panel,
    outputs: HashMap<SurfaceId, Output>,
    shown: Option<Shown>,
}

impl Center {
    pub fn new(
        inputs: Rc<RefCell<Vec<Input>>>,
        client: Option<Client>,
        layout: Source,
        bus: Rc<Bus>,
        panel: Panel,
    ) -> Center {
        let appearance = Appearance::read();
        if let Some(client) = &client {
            // The focused window's output, where the panel opens.
            if let Err(error) = client.watch_windows() {
                tracing::warn!(%error, "the windows are not followed");
            }
        }
        {
            let inputs = inputs.clone();
            appearance::watch(move |appearance| {
                inputs.borrow_mut().push(Input::Appearance(appearance));
            });
        }
        {
            let inputs = inputs.clone();
            glib::timeout_add_local(TICK, move || {
                inputs.borrow_mut().push(Input::Tick);
                glib::ControlFlow::Continue
            });
        }
        let tint = Rgba(sal::palette(&appearance).ink);
        Center {
            inputs,
            client: client.map(Rc::new),
            layout,
            bus,
            appearance,
            icons: Icons::new(tint),
            panel,
            outputs: HashMap::new(),
            shown: None,
        }
    }

    /// The surface to show on: the one on the output the activated window entered first, else
    /// the first one opened.
    fn chosen(&self, shell: &Shell) -> Option<SurfaceId> {
        let open: Vec<SurfaceId> = shell
            .surfaces()
            .filter(|id| self.outputs.contains_key(id))
            .collect();
        let connectors: Vec<Option<String>> = open
            .iter()
            .map(|id| self.outputs[id].connector.clone())
            .collect();
        let windows = self
            .client
            .as_deref()
            .map(Client::windows)
            .unwrap_or_default();
        let activated = windows
            .iter()
            .find(|window| window.state.activated)
            .map(|window| window.outputs.as_slice());
        open.get(focused_output(activated, &connectors)).copied()
    }

    /// Shows the panel, or the page `page` names; a shown panel moves to that page.
    fn show(&mut self, shell: &mut Shell, page: Option<Page>) {
        if let Some(shown) = &self.shown {
            let surface = shown.surface;
            if page.is_some_and(|page| self.panel.view() != crate::panel::View::Page(page)) {
                self.leave_view(shell, surface);
                self.panel.show(page);
                self.rebuild(shell, surface, false);
            }
            return;
        }
        let Some(surface) = self.chosen(shell) else {
            tracing::error!("no output to show the control center on");
            return;
        };
        self.panel.show(page);
        self.shown = Some(Shown {
            surface,
            focused: false,
        });
        self.rebuild(shell, surface, page.is_none());
        for result in [
            shell.set_layer(surface, &options(true, false)),
            shell.set_accessible(surface, true),
        ] {
            if let Err(error) = result {
                tracing::error!(%error, "the control center cannot show");
            }
        }
        if let Some(ui) = shell.ui_mut(surface) {
            ui.announce(&tr("Control center"));
        }
        self.bus.set_open(true);
    }

    fn toggle(&mut self, shell: &mut Shell) {
        if self.shown.is_some() {
            self.hide(shell);
        } else {
            self.show(shell, None);
        }
    }

    pub fn hide(&mut self, shell: &mut Shell) {
        let Some(shown) = self.shown.take() else {
            return;
        };
        self.leave_view(shell, shown.surface);
        if let Some(ui) = shell.ui_mut(shown.surface) {
            *ui = hidden(&self.appearance);
        }
        for result in [
            shell.set_layer(shown.surface, &options(false, false)),
            shell.set_accessible(shown.surface, false),
        ] {
            if let Err(error) = result {
                tracing::error!(%error, "the control center cannot hide");
            }
        }
        self.bus.set_open(false);
    }

    /// The view on `surface` leaves the screen.
    fn leave_view(&mut self, shell: &mut Shell, surface: SurfaceId) {
        if let Some(ui) = shell.ui_mut(surface) {
            self.panel.closed(ui);
        }
    }

    /// Builds the view in a new `Ui` on `surface`, the keyboard on its first control when
    /// `focus` says so, or on the page's Back button.
    fn rebuild(&mut self, shell: &mut Shell, surface: SurfaceId, focus: bool) {
        let scale = self.outputs.get(&surface).map_or(1, |output| output.scale);
        let theme = theme(&self.appearance);
        self.icons.set(scale, theme.text);
        let rtl = is_rtl();
        let mut ui = Ui::new(&tr("Control center"), theme, rtl);
        let island = ui.island(Ui::ROOT, true, [GAP as f32; 4]);
        let place = anchor(self.layout.layout().panel(), rtl);
        let side = |side| match side {
            Side::Start => Align::Start,
            Side::End => Align::End,
        };
        ui.set_align(island, [side(place.horizontal), side(place.vertical)]);
        ui.set_min_width(island, WIDTH);
        let content = ui.add(island, Kind::Group(tr("Control center")));
        // Keep the keyboard where it was across a rebuild of the same view, as GTK's widgets
        // kept it; else the first control.
        let kept = shell.ui(surface).and_then(focus_path);
        self.panel.build(&mut ui, content, &mut self.icons);
        if let Some(slot) = shell.ui_mut(surface) {
            *slot = ui;
            let target = if focus { self.panel.first() } else { None };
            let target = target.or_else(|| kept.and_then(|path| find_path(slot, &path)));
            let target = target.or_else(|| self.panel.first());
            if let Some(target) = target {
                slot.set_focus(Some(target));
            }
        }
    }

    fn appearance(&mut self, shell: &mut Shell, appearance: Appearance) {
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        let ui = self.shown.as_ref().and_then(|s| shell.ui_mut(s.surface));
        self.panel.set_dark(ui, appearance.dark);
        // The icons are tinted with the text: the view is drawn again in the new colours.
        if let Some(surface) = self.shown.as_ref().map(|s| s.surface) {
            self.rebuild(shell, surface, false);
        }
    }

    fn act(&mut self, shell: &mut Shell, surface: SurfaceId, action: Action) {
        match action {
            Action::Done => {}
            Action::Rebuild => self.rebuild(shell, surface, false),
            Action::Close => self.hide(shell),
            Action::OpenSettings => {
                self.open_settings();
                self.hide(shell);
            }
            Action::SetDark(dark) => {
                if let Err(error) =
                    appearance::apply(|choice| choice.dark = dark, cosmic::write_cosmic)
                {
                    tracing::warn!(%error, "the theme mode was not changed");
                    let now = self.appearance.dark;
                    self.panel.set_dark(shell.ui_mut(surface), now);
                }
            }
        }
    }

    fn open_settings(&self) {
        let Some(client) = self.client.clone() else {
            tracing::warn!("no compositor client; Settings is not opened");
            return;
        };
        let Some(app) = gio_unix::DesktopAppInfo::new(SETTINGS) else {
            tracing::warn!("{SETTINGS} is not installed");
            return;
        };
        glib::spawn_future_local(async move {
            if let Err(error) = client.launch(&app).await {
                tracing::warn!(%error, "Settings did not start");
            }
        });
    }
}

impl Application for Center {
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        Some((hidden(&self.appearance), options(false, false)))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, output: &OutputInfo) {
        let entry = self.outputs.entry(surface).or_default();
        entry.connector = output.name.clone();
        let scale = output.scale_factor.clamp(1, 16) as u16;
        let rescaled = entry.scale != scale;
        entry.scale = scale;
        let shown = self.shown.as_ref().is_some_and(|s| s.surface == surface);
        if let Err(error) = shell.set_accessible(surface, shown) {
            tracing::warn!(%error, "the surface stays in the accessibility tree");
        }
        if shown && rescaled {
            self.rebuild(shell, surface, false);
        }
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if self.shown.as_ref().map(|s| s.surface) != Some(surface) {
            return;
        }
        let action = match event {
            Event::PressedOutside | Event::Escape => Action::Close,
            event => match shell.ui_mut(surface) {
                Some(ui) => self.panel.event(ui, &event),
                None => Action::Done,
            },
        };
        self.act(shell, surface, action);
    }

    fn keyboard(&mut self, shell: &mut Shell, surface: SurfaceId, focused: bool) {
        let Some(shown) = self.shown.as_mut().filter(|s| s.surface == surface) else {
            return;
        };
        if focused {
            if !shown.focused {
                shown.focused = true;
                // Exclusive keyboard mode never reports a loss of focus, so once the surface
                // has the keyboard it asks for it on demand instead (CC9).
                if let Err(error) = shell.set_layer(surface, &options(true, true)) {
                    tracing::warn!(%error, "the panel keeps the keyboard exclusively");
                }
            }
        } else if shown.focused && shell.popup_parent() != Some(surface) {
            // A menu of the panel takes the keyboard on its own surface: that is no loss.
            self.hide(shell);
        }
    }

    fn turn(&mut self, shell: &mut Shell) {
        if self.client.as_deref().is_some_and(Client::is_closed) {
            tracing::warn!("the compositor client closed; the panel opens on the first output");
            self.client = None;
        }
        let inputs = std::mem::take(&mut *self.inputs.borrow_mut());
        let mut tick = false;
        for input in inputs {
            match input {
                Input::Show(page) => self.show(shell, page),
                Input::Toggle => self.toggle(shell),
                Input::Appearance(appearance) => self.appearance(shell, appearance),
                Input::Tick => tick = true,
            }
        }
        let Some(surface) = self.shown.as_ref().map(|s| s.surface) else {
            return;
        };
        if let Some(page) = self.panel.wanted() {
            // NetworkManager asks for a password: the page shows its prompt (CC5).
            self.show(shell, Some(page));
        }
        let Some(ui) = shell.ui_mut(surface) else {
            return;
        };
        let action = self.panel.turn(ui, &mut self.icons);
        if tick {
            self.panel.tick(ui);
        }
        self.act(shell, surface, action);
    }

    fn closed(&mut self, _shell: &mut Shell, surface: SurfaceId) {
        self.outputs.remove(&surface);
        if self.shown.as_ref().is_some_and(|s| s.surface == surface) {
            // Its output left with the panel on it.
            self.shown = None;
            self.bus.set_open(false);
        }
    }
}

/// Where the focus is, as the place of each widget among its parent's children from the root:
/// the same place in a view built again from the same state is the same control.
fn focus_path(ui: &Ui) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    let mut at = ui.focus()?;
    while let Some(parent) = ui.widget(at).parent {
        path.push(ui.widget(parent).children.iter().position(|&c| c == at)?);
        at = parent;
    }
    path.reverse();
    Some(path)
}

/// The widget at `path` of [`focus_path`], if the view still has one there.
fn find_path(ui: &Ui, path: &[usize]) -> Option<WidgetId> {
    path.iter()
        .try_fold(Ui::ROOT, |at, &i| ui.widget(at).children.get(i).copied())
}

/// The surface shown or hidden (CC9): over the whole output on the overlay layer, with the
/// keyboard, exclusively until it arrives; or 1×1 on the background layer with no input.
fn options(shown: bool, focused: bool) -> LayerOptions {
    let mut options = LayerOptions::new(if shown {
        Layer::Overlay
    } else {
        Layer::Background
    });
    options.namespace = NAMESPACE.to_owned();
    options.exclusive_zone = 0;
    if shown {
        options.anchor = Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
        options.keyboard = if focused {
            KeyboardInteractivity::OnDemand
        } else {
            KeyboardInteractivity::Exclusive
        };
        options.input_region = None;
    } else {
        options.anchor = Anchor::empty();
        options.keyboard = KeyboardInteractivity::None;
        options.input_region = Some(Vec::new());
    }
    options
}

/// A hidden surface's widgets: none, 1×1.
fn hidden(appearance: &Appearance) -> Ui {
    let mut ui = Ui::new("", theme(appearance), is_rtl());
    ui.min_width = 1.0;
    ui.min_height = 1.0;
    ui
}

/// The panel's colours and measures from the toolkit's tokens (VL10): the shell's neutral
/// surface and ink, the accent's foreground on an active tile's disc and on the focus ring,
/// the panel radius and a 12 px padding (CC4).
fn theme(appearance: &Appearance) -> Theme {
    let palette = sal::palette(appearance);
    let mut theme = Theme::default();
    theme.background = Rgba([0, 0, 0, 0]);
    theme.surface = Rgba(palette.surf);
    theme.text = Rgba(palette.ink);
    theme.accent = Rgba(palette.acc_fg);
    theme.on_accent = Rgba(palette.acc_ink);
    theme.border = Rgba(palette.line);
    theme.warning = Rgba(palette.warn);
    theme.focus = Rgba(palette.acc_fg);
    theme.margin = 0.0;
    theme.gap = sal::space::M;
    theme.island = Rgba(palette.surf);
    theme.island_radius = sal::radius::PANEL;
    theme.island_padding = sal::space::M;
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_takes_nothing_and_shown_covers_the_output() {
        let hidden = options(false, false);
        assert_eq!(hidden.layer, Layer::Background);
        assert_eq!(hidden.keyboard, KeyboardInteractivity::None);
        assert_eq!(
            hidden.input_region,
            Some(Vec::new()),
            "presses pass through"
        );
        assert_eq!(hidden.anchor, Anchor::empty());
        let shown = options(true, false);
        assert_eq!(shown.layer, Layer::Overlay);
        assert_eq!(shown.keyboard, KeyboardInteractivity::Exclusive);
        assert_eq!(
            shown.input_region, None,
            "a press outside the panel closes it"
        );
        assert_eq!(shown.anchor, Anchor::all());
        assert_eq!(shown.exclusive_zone, 0, "the bar stays usable");
        assert_eq!(
            options(true, true).keyboard,
            KeyboardInteractivity::OnDemand,
            "a loss of focus is reported once the keyboard came"
        );
        for options in [hidden, shown] {
            assert_eq!(options.namespace, NAMESPACE);
        }
    }

    #[test]
    fn the_focus_stays_on_the_same_control_across_a_rebuild() {
        let build = || {
            let mut ui = Ui::new("Control center", Theme::default(), false);
            let group = ui.add(Ui::ROOT, Kind::Group("Panel".into()));
            ui.add(group, Kind::Button("Wi-Fi".into()));
            let row = ui.add(group, Kind::Row);
            ui.add(row, Kind::Button("Bluetooth".into()));
            let dark = ui.add(row, Kind::Button("Dark mode".into()));
            (ui, dark)
        };
        let (mut before, dark) = build();
        assert_eq!(focus_path(&before), None, "no focus, no path");
        before.set_focus(Some(dark));
        let path = focus_path(&before).expect("a focus");
        let (after, again) = build();
        assert_eq!(find_path(&after, &path), Some(again));
        assert_eq!(find_path(&after, &[0, 9]), None, "a place the view no longer has");
    }

    #[test]
    fn the_theme_is_the_panel_of_the_visual_language() {
        let appearance = Appearance::default();
        let theme = theme(&appearance);
        assert_eq!(theme.island_radius, 30.0);
        assert_eq!(theme.island_padding, 12.0);
        assert_eq!(theme.background.0[3], 0, "transparent around the panel");
        assert_eq!(theme.focus, theme.accent, "purple on the focus ring");
    }
}
