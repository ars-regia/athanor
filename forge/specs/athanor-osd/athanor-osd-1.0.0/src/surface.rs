//! The overlay's surfaces (doc_osd.md OD7 to OD11): one layer surface per output, opened once
//! and kept. Hidden, it is 1×1 on the background layer with nothing drawn, no input and out of
//! the accessibility tree; shown, it takes the overlay layer at the bottom of the output of the
//! active workspace, rises in and fades out, and names itself with its content as a status.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_osd::i18n::{self, tr};
use athanor_osd::logic::{Level, Source, View, CEILING};
use athanor_sal::driver::{Anchor, KeyboardInteractivity, Layer, OutputInfo};
use athanor_sal::{
    Application, Curve, Event, IconTheme, Image, Kind, LayerOptions, Motion, Pose, Rgba, RootRole,
    Shell, SurfaceId, TextStyle, Theme, Ui, WidgetId,
};
use athanor_style::appearance::Appearance;
use athanor_style::sal;
use athanor_sulphur_client::{Client, Event as CompositorEvent};

/// The layer shell's namespace of every overlay surface.
const NAMESPACE: &str = "athanor-osd";
/// From the bottom edge to the overlay's fill (OD8).
const MARGIN: i32 = 48;
/// How far the overlay rises as it enters (OD10). The surface keeps this much room beneath
/// the fill, since a motion moves the drawing, not the surface.
const RISE: f32 = 12.0;
/// The fill's width for a level: fixed, so that it does not jump with the number, and a
/// minimum for large text (OD10, OD11).
const WIDTH: f32 = 300.0;
/// The narrowest the bar gets, as the text grows.
const BAR_MIN: f32 = 160.0;
const ICON_SIZE: f32 = 16.0;
const ICON_THEME: &str = "Adwaita";
/// How often the value's 100 ms step is drawn.
const FRAME: Duration = Duration::from_millis(16);

/// What reaches the surfaces from outside the toolkit's loop.
pub enum Input {
    View(View),
    /// The service cannot run: the loop ends with this error.
    Fail(String),
    Appearance(Appearance),
    /// A workspace became active on this output.
    Active(String),
}

/// The widgets of a shown overlay, in place for an update of the same shape.
#[derive(Clone, Copy)]
struct Widgets {
    level: WidgetId,
    number: WidgetId,
}

/// What a surface shows; a change of it builds the widgets again, a change of the value alone
/// does not.
#[derive(Clone, Copy, PartialEq)]
struct Shape {
    source: Source,
    muted: bool,
    max: u32,
    icon: &'static str,
}

impl Shape {
    fn of(level: &Level) -> Shape {
        Shape {
            source: level.source,
            muted: level.muted,
            // 100 or 150.
            max: level.max.round() as u32,
            icon: level.icon,
        }
    }
}

#[derive(Default)]
struct Surface {
    output: Option<String>,
    scale: u16,
    shown: Option<(Shape, Widgets)>,
    leaving: bool,
}

/// The level's bar moving to a new value (OD10).
struct Step {
    surface: SurfaceId,
    from: f64,
    to: f64,
    start: Instant,
}

pub struct Osd {
    inputs: Rc<RefCell<Vec<Input>>>,
    client: Option<Client>,
    appearance: Appearance,
    icons: IconTheme,
    images: HashMap<(&'static str, u16), Option<Image>>,
    surfaces: HashMap<SurfaceId, Surface>,
    /// The output of the workspace activated last.
    active: Option<String>,
    step: Option<Step>,
    /// The timer that draws the step, while one runs: at most one exists.
    timer: Option<glib::SourceId>,
}

impl Osd {
    pub fn new(inputs: Rc<RefCell<Vec<Input>>>, client: Option<Client>) -> Osd {
        let appearance = Appearance::read();
        if let Some(client) = &client {
            let inputs = inputs.clone();
            let mut active = HashSet::new();
            client.connect_events(move |_, event| {
                if let CompositorEvent::WorkspaceAdded(workspace)
                | CompositorEvent::WorkspaceChanged(workspace) = event
                {
                    // Activated now, not changed while active.
                    if workspace.active && active.insert(workspace.id) {
                        if let Some(output) = &workspace.output {
                            inputs.borrow_mut().push(Input::Active(output.clone()));
                        }
                    } else if !workspace.active {
                        active.remove(&workspace.id);
                    }
                }
            });
        }
        {
            let inputs = inputs.clone();
            athanor_style::appearance::watch(move |appearance| {
                inputs.borrow_mut().push(Input::Appearance(appearance));
            });
        }
        Osd {
            inputs,
            client,
            appearance,
            icons: IconTheme::system(ICON_THEME),
            images: HashMap::new(),
            surfaces: HashMap::new(),
            active: None,
            step: None,
            timer: None,
        }
    }

    fn image(&mut self, icon: &'static str, scale: u16, tint: Rgba) -> Option<Image> {
        let icons = &self.icons;
        self.images
            .entry((icon, scale))
            .or_insert_with(|| match icons.image(icon, ICON_SIZE as u16, scale) {
                Ok(image) => Some(image.tinted(tint)),
                Err(error) => {
                    tracing::warn!(icon, %error, "the overlay's icon is unavailable");
                    None
                }
            })
            .clone()
    }

    /// The widgets of `level` at `scale`, the level at `value`.
    fn build(&mut self, level: &Level, value: f64, scale: u16) -> Option<(Ui, Widgets)> {
        let theme = theme(&self.appearance);
        let tint = theme.text;
        let mut ui = Ui::new("", theme, i18n::is_rtl());
        ui.root_role = RootRole::Status;
        ui.title = level.text();
        let island = ui.island(Ui::ROOT, false, [0.0, 0.0, RISE, 0.0]);
        if let Some(image) = self.image(level.icon, scale, tint) {
            ui.image(island, image, ICON_SIZE, "");
        }
        let name = match level.source {
            Source::Output => tr("Volume"),
            Source::Input => tr("Microphone"),
        };
        // `level.max` is 100 or 150, above `CEILING`'s full mark: always taken.
        let bar = ui.level(island, &name, value, CEILING, level.max)?;
        // Mute draws the bar insensitive (OD9).
        ui.set_sensitive(bar, !level.muted);
        let number = ui.add(island, Kind::Label(number(level)));
        ui.set_text_style(
            number,
            TextStyle {
                scale: sal::size::BODY.scale,
                weight: sal::size::BODY.weight,
                features: vec![*b"tnum"],
            },
        );
        fit(&mut ui, island);
        Some((ui, Widgets { level: bar, number }))
    }

    /// The surface to show on: the one on the output of the workspace activated last, else on
    /// an output with an active workspace, else the first opened.
    fn chosen(&self, shell: &Shell) -> Option<SurfaceId> {
        let active = self.active.clone().or_else(|| {
            self.client.as_ref().and_then(|client| {
                client
                    .workspaces()
                    .into_iter()
                    .find(|w| w.active && w.output.is_some())
                    .and_then(|w| w.output)
            })
        });
        let open: Vec<SurfaceId> = shell
            .surfaces()
            .filter(|id| self.surfaces.contains_key(id))
            .collect();
        open.iter()
            .copied()
            .find(|id| active.is_some() && self.surfaces[id].output == active)
            .or_else(|| open.first().copied())
    }

    fn show(&mut self, shell: &mut Shell, level: Level) {
        let Some(target) = self.chosen(shell) else {
            tracing::info!("no output to show the overlay on");
            return;
        };
        // One overlay at a time (OD5): another output's leaves at once.
        let others: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(id, s)| **id != target && s.shown.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in others {
            self.hide_now(shell, id);
        }
        let shape = Shape::of(&level);
        let Some(surface) = self.surfaces.get(&target) else {
            return;
        };
        let (scale, was) = (surface.scale, surface.shown);
        let entering = was.is_none() || surface.leaving;
        match was {
            // The same shape: the value moves, the number and the name change in place.
            Some((old, widgets)) if old == shape => {
                let Some(ui) = shell.ui_mut(target) else {
                    return;
                };
                let from = ui.value(widgets.level).unwrap_or(level.percent);
                ui.set_text(widgets.number, &number(&level));
                ui.title = level.text();
                ui.layout();
                self.move_value(shell, target, widgets.level, from, level.percent);
            }
            _ => {
                let value = was
                    .and_then(|(_, w)| shell.ui(target).and_then(|ui| ui.value(w.level)))
                    .unwrap_or(level.percent);
                let Some((ui, widgets)) = self.build(&level, value, scale) else {
                    return;
                };
                if let Some(slot) = shell.ui_mut(target) {
                    *slot = ui;
                }
                if let Some(surface) = self.surfaces.get_mut(&target) {
                    surface.shown = Some((shape, widgets));
                }
                self.move_value(shell, target, widgets.level, value, level.percent);
            }
        }
        if entering {
            self.enter(shell, target);
        }
    }

    fn enter(&mut self, shell: &mut Shell, id: SurfaceId) {
        let from = match self.surfaces.get(&id) {
            Some(surface) if surface.leaving => shell.pose(id).unwrap_or(HIDDEN),
            _ => HIDDEN,
        };
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.leaving = false;
        }
        let steps = [
            shell.set_layer(id, &options(true)),
            shell.set_accessible(id, true),
            shell.animate(
                id,
                Motion {
                    from,
                    to: Pose::REST,
                    duration: sal::motion::NORMAL,
                    curve: Curve(sal::motion::ENTER_CURVE),
                    reduced: sal::motion::REDUCED_FADE,
                },
            ),
        ];
        for result in steps {
            if let Err(error) = result {
                tracing::error!(%error, "the overlay cannot show");
            }
        }
    }

    /// The timeout ended: the overlay fades out, then hides ([`Application::settled`]).
    fn leave(&mut self, shell: &mut Shell) {
        let shown: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.shown.is_some() && !s.leaving)
            .map(|(id, _)| *id)
            .collect();
        for id in shown {
            if let Some(surface) = self.surfaces.get_mut(&id) {
                surface.leaving = true;
            }
            let motion = Motion {
                from: shell.pose(id).unwrap_or(Pose::REST),
                to: HIDDEN,
                duration: sal::motion::NORMAL.mul_f32(sal::motion::EXIT_RATIO),
                curve: Curve(sal::motion::CURVE),
                reduced: sal::motion::REDUCED_FADE,
            };
            if let Err(error) = shell.animate(id, motion) {
                tracing::error!(%error, "the overlay cannot leave; it hides at once");
                self.hide_now(shell, id);
            }
        }
    }

    fn hide_now(&mut self, shell: &mut Shell, id: SurfaceId) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.shown = None;
            surface.leaving = false;
        }
        if self.step.as_ref().is_some_and(|step| step.surface == id) {
            self.step = None;
        }
        if let Some(ui) = shell.ui_mut(id) {
            *ui = hidden(&self.appearance);
        }
        for result in [
            shell.set_layer(id, &options(false)),
            shell.set_accessible(id, false),
        ] {
            if let Err(error) = result {
                tracing::error!(%error, "the overlay cannot hide");
            }
        }
    }

    /// The bar moves from `from` to `to` in the visual language's 100 ms step, or takes `to` at
    /// once with reduced motion (OD10).
    fn move_value(
        &mut self,
        shell: &mut Shell,
        id: SurfaceId,
        level: WidgetId,
        from: f64,
        to: f64,
    ) {
        if shell.reduced_motion() || from == to {
            self.step = None;
            if let Some(ui) = shell.ui_mut(id) {
                ui.set_level(level, to);
            }
            return;
        }
        self.step = Some(Step {
            surface: id,
            from,
            to,
            start: Instant::now(),
        });
        if self.timer.is_none() {
            // ponytail: a GLib timer, not the surface's frame callbacks, which Sal does not
            // offer an application; a widget motion in Sal replaces it.
            self.timer = Some(glib::timeout_add_local(FRAME, || {
                glib::ControlFlow::Continue
            }));
        }
    }

    fn draw_step(&mut self, shell: &mut Shell) {
        let Some(step) = &self.step else {
            if let Some(timer) = self.timer.take() {
                timer.remove();
            }
            return;
        };
        let Some((_, widgets)) = self.surfaces.get(&step.surface).and_then(|s| s.shown) else {
            self.step = None;
            return;
        };
        let t = (step.start.elapsed().as_secs_f32() / sal::motion::FAST.as_secs_f32()).min(1.0);
        let value = step.from + (step.to - step.from) * f64::from(Curve(sal::motion::CURVE).at(t));
        if let Some(ui) = shell.ui_mut(step.surface) {
            ui.set_level(widgets.level, value);
        }
        if t >= 1.0 {
            self.step = None;
        }
    }

    fn appearance(&mut self, shell: &mut Shell, appearance: Appearance) {
        shell.set_reduced_motion(!appearance.animations);
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        self.images.clear();
        // The overlay on screen, if any, is drawn again in the new colours at its next key;
        // a hidden surface draws nothing.
        let shown: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.shown.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in shown {
            if let Some(ui) = shell.ui_mut(id) {
                ui.theme = theme(&appearance);
            }
        }
    }
}

impl Application for Osd {
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        Some((hidden(&self.appearance), options(false)))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, output: &OutputInfo) {
        if self.surfaces.is_empty() {
            shell.set_reduced_motion(!self.appearance.animations);
        }
        let entry = self.surfaces.entry(surface).or_default();
        entry.output = output.name.clone();
        let scale = output.scale_factor.clamp(1, 16) as u16;
        if entry.scale != scale && entry.shown.is_some() {
            // Its icon is drawn again at the new scale with the next key.
            tracing::info!(scale, "the overlay's output changed its scale");
        }
        entry.scale = scale;
        if let Err(error) = shell.set_accessible(surface, entry.shown.is_some()) {
            tracing::warn!(%error, "the overlay's surface stays in the accessibility tree");
        }
    }

    fn event(&mut self, _shell: &mut Shell, _surface: SurfaceId, _event: Event) {}

    fn turn(&mut self, shell: &mut Shell) {
        if self.client.as_ref().is_some_and(Client::is_closed) {
            tracing::warn!("the compositor client closed; the overlay shows on the first output");
            self.client = None;
        }
        let inputs = std::mem::take(&mut *self.inputs.borrow_mut());
        for input in inputs {
            match input {
                Input::View(View::Show(level)) => self.show(shell, level),
                Input::View(View::Hide) => self.leave(shell),
                Input::View(View::Announce(text)) => {
                    let shown = self.surfaces.iter().find(|(_, s)| s.shown.is_some());
                    if let Some(ui) = shown.and_then(|(id, _)| shell.ui_mut(*id)) {
                        ui.announce(&text);
                    }
                }
                Input::Fail(error) => shell.fail(error.into()),
                Input::Appearance(appearance) => self.appearance(shell, appearance),
                Input::Active(output) => self.active = Some(output),
            }
        }
        self.draw_step(shell);
    }

    fn settled(&mut self, shell: &mut Shell, surface: SurfaceId) {
        if self.surfaces.get(&surface).is_some_and(|s| s.leaving) {
            self.hide_now(shell, surface);
        }
    }

    fn closed(&mut self, _shell: &mut Shell, surface: SurfaceId) {
        self.surfaces.remove(&surface);
    }
}

/// Faded out, and lowered by the rise.
const HIDDEN: Pose = Pose {
    opacity: 0.0,
    offset: (0.0, RISE),
};

/// The surface shown or hidden (OD7, OD8): at the bottom, centred, on the overlay layer
/// above everything, with its fill [`MARGIN`] from the edge; or 1×1 on the background layer.
/// Never the keyboard, never the pointer, and no exclusive zone.
fn options(shown: bool) -> LayerOptions {
    let mut options = LayerOptions::new(if shown { Layer::Overlay } else { Layer::Background });
    options.namespace = NAMESPACE.to_owned();
    options.anchor = Anchor::BOTTOM;
    options.margin = [0, 0, if shown { MARGIN - RISE as i32 } else { 0 }, 0];
    options.exclusive_zone = 0;
    options.keyboard = KeyboardInteractivity::None;
    options.input_region = Some(Vec::new());
    options
}

/// A hidden surface's widgets: none, 1×1.
fn hidden(appearance: &Appearance) -> Ui {
    let mut ui = Ui::new("", theme(appearance), i18n::is_rtl());
    ui.root_role = RootRole::Status;
    ui.min_width = 1.0;
    ui.min_height = 1.0;
    ui
}

/// The number, padded to three figures with figure spaces so that the fill keeps its width.
fn number(level: &Level) -> String {
    format!("{:\u{2007}>3}", level.number())
}

/// The bar takes the fill to [`WIDTH`], and never shrinks below [`BAR_MIN`]: large text makes
/// the fill wider instead (OD11).
fn fit(ui: &mut Ui, island: WidgetId) {
    ui.layout();
    if let Some(fill) = ui.island_fill(island) {
        let bar = (ui.theme.field_width + WIDTH - fill.w).max(BAR_MIN);
        if bar != ui.theme.field_width {
            ui.theme.field_width = bar;
            ui.layout();
        }
    }
}

/// The overlay's colours and measures from the toolkit's tokens (OD10, VL10): the shell's
/// neutral surface, the ink, the accent's foreground for the bar and the warning colour
/// beyond 100 %, the panel radius and an 18 px padding.
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
    theme.island_padding = sal::space::L;
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(percent: f64, max: f64) -> Level {
        Level {
            source: Source::Output,
            percent,
            muted: false,
            max,
            icon: "audio-volume-high-symbolic",
        }
    }

    #[test]
    fn hidden_takes_nothing_and_shown_floats_above_everything() {
        for shown in [false, true] {
            let options = options(shown);
            assert_eq!(options.keyboard, KeyboardInteractivity::None);
            assert_eq!(
                options.input_region,
                Some(Vec::new()),
                "clicks pass through"
            );
            assert_eq!(options.exclusive_zone, 0);
            assert_eq!(options.anchor, Anchor::BOTTOM, "centred along the bottom");
        }
        assert_eq!(options(true).layer, Layer::Overlay);
        assert_eq!(options(false).layer, Layer::Background);
        assert_eq!(
            options(true).margin[2] as f32 + RISE,
            MARGIN as f32,
            "48 px to the fill"
        );
    }

    #[test]
    fn the_number_keeps_three_figures() {
        assert_eq!(number(&level(5.0, 100.0)), "\u{2007}\u{2007}5");
        assert_eq!(number(&level(45.0, 100.0)), "\u{2007}45");
        assert_eq!(number(&level(130.0, 150.0)), "130");
    }

    #[test]
    fn a_level_fills_300_px_with_room_to_rise() {
        let mut osd_ui = Ui::new("", theme(&Appearance::default()), false);
        let island = osd_ui.island(Ui::ROOT, false, [0.0, 0.0, RISE, 0.0]);
        osd_ui.level(island, "Volume", 45.0, CEILING, 100.0);
        osd_ui.add(island, Kind::Label(number(&level(45.0, 100.0))));
        fit(&mut osd_ui, island);
        let fill = osd_ui
            .island_fill(island)
            .map(|fill| (fill.w.round(), fill.bottom()));
        let (_, height) = osd_ui.size();
        assert_eq!(fill.map(|f| f.0), Some(WIDTH));
        assert!(
            fill.is_some_and(|f| height - f.1 >= RISE - 0.5),
            "{fill:?} in {height}"
        );
    }

    #[test]
    fn the_theme_is_the_panel_of_the_visual_language() {
        let appearance = Appearance::default();
        let palette = sal::palette(&appearance);
        let theme = theme(&appearance);
        assert_eq!(theme.island_radius, 30.0);
        assert_eq!(theme.island_padding, 18.0);
        assert_eq!(theme.warning, Rgba(palette.warn));
        assert_eq!(theme.island, Rgba(palette.surf));
        assert_eq!(theme.background.0[3], 0, "transparent around the fill");
    }
}
