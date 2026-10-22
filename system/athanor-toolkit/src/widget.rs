//! The widget tree: a flat arena of widgets, laid out in logical pixels and mirrored in
//! right-to-left.

use crate::focus;
use crate::text::{keeps, printable, Brush, TextContext, TextField};
use crate::theme::Theme;
use parley::Layout;

/// A widget's index in its [`Ui`]; the root column is 0.
pub type WidgetId = usize;

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// The distance from the left edge.
    pub x: f32,
    /// The distance from the top edge.
    pub y: f32,
    /// The width.
    pub w: f32,
    /// The height.
    pub h: f32,
}

impl Rect {
    /// The part of the rectangle inside `other`, empty where they do not meet.
    pub(crate) fn intersect(self, other: Rect) -> Rect {
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = self.bottom().min(other.bottom());
        let (w, h) = ((right - x).max(0.0), (bottom - y).max(0.0));
        Rect { x, y, w, h }
    }

    /// Whether the point is inside the rectangle; the right and bottom edges are outside.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// The horizontal middle.
    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    /// The vertical position of the bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// What a widget is and what it shows. New kinds are added over time: match with a `_` arm.
#[non_exhaustive]
pub enum Kind {
    /// Children stacked top to bottom, aligned to the start edge.
    Column,
    /// Children side by side in reading order, left to right or right to left, all as wide as
    /// the widest, as a homogeneous GTK box. In a column or a group it fills the width.
    Row,
    /// A column with an accessible name, for a heading and the controls it names.
    Group(String),
    /// A line of text that takes no input.
    Label(String),
    /// A button with a label; activating it raises [`Event::Clicked`].
    Button(String),
    /// A toggle button. The toggles that share a parent form a radio group: activating one
    /// releases the others.
    Toggle {
        /// The text of the toggle.
        label: String,
        /// Whether this toggle is the active one of its group.
        active: bool,
    },
    /// A check button: a box and a label, on or off on its own. Unlike the toggles, the checks
    /// that share a parent are independent; activating one raises [`Event::Toggled`].
    Check {
        /// The text beside the box.
        label: String,
        /// Whether the box is checked.
        active: bool,
    },
    /// A switch: a label at the start edge and a track with a knob at the end edge, on or off
    /// on its own. It fills the width of its column or group, or its cell in a row, where every
    /// child gets an equal share of the row's width; activating it raises [`Event::Toggled`].
    Switch {
        /// The text of the switch, which names it for assistive technologies.
        label: String,
        /// Whether the switch is on.
        active: bool,
    },
    /// A slider: a knob on a track that picks a number between a minimum and a maximum, in
    /// steps. It fills the width of its column or group, or its cell in a row, and takes the
    /// pointer, the arrows, Home, End, Page Up and Page Down; it raises [`Event::Changed`]. Add
    /// it with [`Ui::slider`], which keeps its range valid.
    Slider(Slider),
    /// A single-line text field.
    Field(Box<TextField>),
    /// A text field of several lines, as GTK's `TextView` in a `ScrolledWindow`: as wide as a
    /// field, its text wrapped at that width, a number of lines tall, and scrolled to keep the
    /// caret in view and under the wheel. Enter breaks the line, Up and Down move between lines,
    /// and Tab leaves it. Add it with [`Ui::text_area`].
    TextArea(Box<TextField>),
    /// A viewport `height` logical pixels high onto its child, which it fills the width with and
    /// scrolls under the wheel and the touchpad, and to keep the keyboard focus in view, as GTK's
    /// `ScrolledWindow`. What lies outside the viewport is not drawn and takes no pointer input;
    /// a thin indicator at the end edge shows the position while the child is taller. Add it
    /// with [`Ui::scroll`].
    Scroll {
        /// The height of the viewport, in logical pixels.
        height: f32,
        /// How far the child is scrolled, in logical pixels from its top: from 0 to its height
        /// less the viewport's.
        offset: f32,
    },
    /// A list of rows of text, stacked top to bottom, of which the user chooses one, as GTK's
    /// `ListBox`. It is one stop of the Tab order, on its selected row (the first while none is):
    /// the arrows, Page Up, Page Down, Home and End choose another; Up on the first row and Down
    /// on the last leave the list as Shift+Tab and Tab do. A row chosen by the keyboard, a click
    /// or Enter raises [`Event::Selected`]. It fills the width of its column, group or scroll.
    /// Add it with [`Ui::list`] and its rows with [`Ui::list_row`].
    List {
        /// The name assistive technologies read for the list.
        name: String,
        /// The row chosen, if any; see [`Ui::selected`].
        selected: Option<WidgetId>,
    },
    /// A row of a [`Kind::List`], with its text at the start edge. The pointer chooses it; the
    /// keyboard reaches it only through its list.
    ListRow(String),
    /// The title bar the window draws when the compositor leaves the decorations to it, as GTK
    /// does: the title centred and the window's buttons at the end edge. See [`Ui::add_title_bar`].
    TitleBar,
    /// One of the title bar's buttons.
    WindowButton(WindowButton),
    /// A menu: its items and separators stacked top to bottom, all as wide as the widest. The
    /// up and down arrows cycle through its items. Shown in a popup, see
    /// [`crate::driver::Shell::open_popup`].
    Menu,
    /// An item of a [`Kind::Menu`], with its label; activating it raises [`Event::Clicked`].
    MenuItem(String),
    /// A line between groups of a menu's items. It takes no input.
    Separator,
    /// The text of a tooltip, which the driver shows in a popup of its own.
    Tooltip(String),
    /// An image drawn in a square of `size` logical pixels, scaled from its own pixels to fit it
    /// with its aspect ratio kept, centred. It takes no input.
    Image {
        /// The pixels: a PNG, or an SVG rasterised at the size it is drawn at.
        image: crate::icon::Image,
        /// The side of the square, in logical pixels.
        size: f32,
        /// What the image shows, which names it for assistive technologies; empty for an image
        /// that only decorates.
        description: String,
    },
}

/// The state of a [`Kind::Slider`]: a name, a range and a value that is always on one of its
/// steps. The fields are closed to applications so that the invariants hold: the range is
/// finite and `min < max`, `step > 0`, and the value lies on a step counted from `min`, or at
/// `max`, which is always a value even when the step does not divide the range. Make one with
/// [`Ui::slider`] and change it with [`Ui::set_value`].
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    pub(crate) name: String,
    pub(crate) value: f64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) step: f64,
}

impl Slider {
    /// How many steps `value` is from `min`, the last one being `max`.
    fn last(&self) -> i64 {
        // A hair under the exact quotient, so that a range the step divides does not count
        // `max` twice when the division rounds up.
        (((self.max - self.min) / self.step) - 1e-9).ceil() as i64
    }

    /// The value `index` steps from `min`, never past `max`.
    fn at(&self, index: i64) -> f64 {
        (self.min + index as f64 * self.step).min(self.max)
    }

    /// The index of the step `value` is on, or the nearest one.
    fn index(&self, value: f64) -> i64 {
        if value >= self.max {
            return self.last();
        }
        (((value - self.min) / self.step).round() as i64).clamp(0, self.last())
    }

    /// The step nearest to `value`, inside the range; `None` for NaN.
    fn snap(&self, value: f64) -> Option<f64> {
        if value.is_nan() {
            return None;
        }
        let value = value.clamp(self.min, self.max);
        let below = self.at(self.index(value));
        Some(if (self.max - value).abs() < (value - below).abs() {
            self.max
        } else {
            below
        })
    }
}

/// A button of the title bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowButton {
    /// Minimizes the window.
    Minimize,
    /// Closes the window.
    Close,
}

impl WindowButton {
    /// The name assistive technologies read: GTK's for the same buttons.
    pub fn name(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::Close => "Close",
        }
    }
}

/// One node of the tree: what it is, where it sits and where it was laid out.
pub struct Widget {
    /// What the widget is and shows.
    pub kind: Kind,
    /// The widget it is a child of; `None` for the root.
    pub parent: Option<WidgetId>,
    /// The children, in reading order.
    pub children: Vec<WidgetId>,
    /// Where the last layout put the widget, in logical pixels.
    pub rect: Rect,
    pub(crate) visible: bool,
    pub(crate) text: Option<Layout<Brush>>,
    /// `Some` for a widget that opens a popup: whether it is open.
    pub(crate) expanded: Option<bool>,
    pub(crate) tooltip: Option<String>,
    pub(crate) description: Option<String>,
}

impl Widget {
    fn new(kind: Kind, parent: Option<WidgetId>) -> Self {
        Self {
            kind,
            parent,
            children: Vec::new(),
            rect: Rect::default(),
            visible: true,
            text: None,
            expanded: None,
            tooltip: None,
            description: None,
        }
    }

    /// Whether the keyboard can focus the widget.
    pub fn is_focusable(&self) -> bool {
        matches!(
            self.kind,
            Kind::Button(_)
                | Kind::Toggle { .. }
                | Kind::Check { .. }
                | Kind::Switch { .. }
                | Kind::Slider(_)
                | Kind::Field(_)
                | Kind::TextArea(_)
                | Kind::WindowButton(_)
                | Kind::MenuItem(_)
                | Kind::ListRow(_)
        )
    }
}

/// What the user did, for the application to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A button was activated by the pointer, the keyboard or an assistive technology.
    Clicked(WidgetId),
    /// A toggle became the active one of its group.
    Selected(WidgetId),
    /// A check button or a switch was turned on or off; ask [`Ui::is_active`] which.
    Toggled(WidgetId),
    /// A slider's value changed; ask [`Ui::value`] what it is.
    Changed(WidgetId),
    /// A field's committed text changed.
    Edited(WidgetId),
    /// A button of the title bar was activated; the driver acts on it.
    Window(WindowButton),
    /// The secondary button was pressed on a widget, or Shift+F10 or the Menu key with the
    /// widget focused: the application may open its context menu.
    ContextMenu(WidgetId),
    /// The pointer, pressed on the widget, moved past [`Ui::DRAG_THRESHOLD`]. A drag never
    /// activates the widget.
    DragStarted(WidgetId),
    /// The button that started a drag was released, over the widget `to` if any.
    DragEnded {
        /// The widget the drag started from.
        from: WidgetId,
        /// The widget under the pointer at the release, if any.
        to: Option<WidgetId>,
    },
}

/// A pointer button, as the driver maps it from the evdev codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerButton {
    /// The primary button, the left one of a right-handed mouse: it activates and drags.
    Primary,
    /// The secondary button: it asks for the context menu.
    Secondary,
}

/// A press of the primary button, until its release.
#[derive(Clone, Copy, Debug)]
struct Press {
    widget: WidgetId,
    x: f32,
    y: f32,
    dragging: bool,
}

/// A key, already translated from the keyboard layout by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    /// Moves the focus to the next widget in reading order.
    Tab,
    /// Moves the focus to the previous widget in reading order.
    BackTab,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// Activates the focused widget.
    Enter,
    /// Deletes before the caret.
    Backspace,
    /// Deletes after the caret.
    Delete,
    /// Text to type, which may be several characters (a composed or pasted character).
    Text(String),
    /// Home: the least value of a slider.
    Home,
    /// End: the greatest value of a slider.
    End,
    /// Page Up: ten steps more on a slider.
    PageUp,
    /// Page Down: ten steps fewer on a slider.
    PageDown,
    /// Escape: closes the open popup, which the driver handles.
    Escape,
    /// Shift+F10 or the Menu key: asks for the focused widget's context menu.
    Menu,
}

/// One window's widgets, theme, reading direction and keyboard focus.
pub struct Ui {
    pub(crate) widgets: Vec<Widget>,
    /// The title of the window, which the title bar shows and assistive technologies read.
    pub title: String,
    /// The colours and sizes the widgets draw with.
    pub theme: Theme,
    /// Whether the interface is mirrored, for a right-to-left language.
    pub rtl: bool,
    /// The window's width in logical pixels when its widgets need less, as GTK's default width.
    pub min_width: f32,
    pub(crate) focus: Option<WidgetId>,
    pub(crate) text: TextContext,
    title_bar: Option<WidgetId>,
    size: (f32, f32),
    hover: Option<WidgetId>,
    press: Option<Press>,
}

impl Ui {
    /// An empty window titled `title`, with only the root column, drawn with `theme` and mirrored when `rtl`.
    pub fn new(title: &str, theme: Theme, rtl: bool) -> Self {
        Self {
            widgets: vec![Widget::new(Kind::Column, None)],
            title: title.to_owned(),
            theme,
            rtl,
            min_width: 0.0,
            focus: None,
            text: TextContext::default(),
            title_bar: None,
            size: (0.0, 0.0),
            hover: None,
            press: None,
        }
    }

    /// The root column every widget descends from.
    pub const ROOT: WidgetId = 0;
    /// The most steps a slider may have between its minimum and its maximum.
    pub const SLIDER_MAX_STEPS: f64 = 2_147_483_648.0;
    /// The longest text a field takes from outside the keyboard, in characters.
    pub const FIELD_MAX_CHARS: usize = 1024;
    /// How far the pointer moves on either axis, in logical pixels, from a press before a drag
    /// starts: GTK's default `gtk-dnd-drag-threshold`.
    pub const DRAG_THRESHOLD: f32 = 8.0;

    /// Adds `kind` as the last child of `parent`.
    pub fn add(&mut self, parent: WidgetId, kind: Kind) -> WidgetId {
        let id = self.widgets.len();
        self.widgets.push(Widget::new(kind, Some(parent)));
        self.widgets[parent].children.push(id);
        id
    }

    /// Adds a text field named `name` for assistive technologies.
    pub fn field(&mut self, parent: WidgetId, name: &str, text: &str) -> WidgetId {
        let field = TextField::new(name, text, self.theme.font_size, self.rtl);
        self.add(parent, Kind::Field(Box::new(field)))
    }

    /// Adds a text area named `name` for assistive technologies, holding `text`: `lines` lines
    /// tall and as wide as the theme's field, its text wrapped at that width. See
    /// [`Kind::TextArea`].
    pub fn text_area(&mut self, parent: WidgetId, name: &str, text: &str, lines: usize) -> WidgetId {
        let t = &self.theme;
        let width = t.field_width - 2.0 * t.padding;
        let mut area = TextField::area(name, text, (t.font_size, self.rtl), width, lines);
        // The caret follows the text, as gtk_text_buffer_set_text leaves a text view's.
        area.move_to_end(&mut self.text);
        self.add(parent, Kind::TextArea(Box::new(area)))
    }

    /// Puts the title bar above the widgets, once: the root's first child, so it comes first in
    /// the tree, as GTK's does. Its buttons come first in the Tab order too, where GTK's Tab
    /// never reaches them, so the keyboard can minimize and close the window.
    pub fn add_title_bar(&mut self) {
        if self.title_bar.is_some() {
            return;
        }
        let bar = self.add(Self::ROOT, Kind::TitleBar);
        let root = &mut self.widgets[Self::ROOT].children;
        root.pop();
        root.insert(0, bar);
        self.add(bar, Kind::Label(self.title.clone()));
        self.add(bar, Kind::WindowButton(WindowButton::Minimize));
        self.add(bar, Kind::WindowButton(WindowButton::Close));
        self.title_bar = Some(bar);
    }

    /// Whether the point, in logical pixels, is on the title bar: where a press moves the window.
    pub fn in_title_bar(&self, x: f32, y: f32) -> bool {
        self.title_bar
            .is_some_and(|bar| self.widgets[bar].rect.contains(x, y))
    }

    /// The widget `id`. Panics if `id` is not from this tree.
    pub fn widget(&self, id: WidgetId) -> &Widget {
        &self.widgets[id]
    }

    /// The number of widgets in the tree, the root included.
    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    /// Whether the tree has no widgets; always `false`, as the root exists from the start.
    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    /// Whether `id` is a toggle that is the active one of its group, or a check button that is
    /// checked, or a switch that is on.
    pub fn is_active(&self, id: WidgetId) -> bool {
        matches!(
            self.widgets[id].kind,
            Kind::Toggle { active: true, .. }
                | Kind::Check { active: true, .. }
                | Kind::Switch { active: true, .. }
        )
    }

    /// Adds a slider named `name` for assistive technologies, from `min` to `max` in steps of
    /// `step` counted from `min` (`max` is always a value, even when the step does not divide
    /// the range), set to the step nearest to `value`. `None`, and nothing added, unless every
    /// number is finite, `min < max`, `step > 0`, `max - min` is finite and the range holds
    /// at most [`Ui::SLIDER_MAX_STEPS`] steps.
    pub fn slider(
        &mut self,
        parent: WidgetId,
        name: &str,
        min: f64,
        max: f64,
        step: f64,
        value: f64,
    ) -> Option<WidgetId> {
        let finite = [min, max, step, value].iter().all(|n| n.is_finite());
        if !(finite && min < max && step > 0.0) {
            return None;
        }
        // The span must be finite too (-1e308 to 1e308 is not), and the steps few enough to be
        // counted, so that the index arithmetic and the drawing never leave the numbers.
        let span = max - min;
        if !(span.is_finite() && span / step <= Self::SLIDER_MAX_STEPS) {
            return None;
        }
        let mut slider = Slider {
            name: name.to_owned(),
            value: min,
            min,
            max,
            step,
        };
        slider.value = slider.snap(value)?;
        Some(self.add(parent, Kind::Slider(slider)))
    }

    /// The value of the slider `id`, or `None` if it is not a slider.
    pub fn value(&self, id: WidgetId) -> Option<f64> {
        match &self.widgets[id].kind {
            Kind::Slider(slider) => Some(slider.value),
            _ => None,
        }
    }

    /// Sets the slider `id` to the step nearest to `value`, inside its range, as an assistive
    /// technology's SetValue does. `None`, and nothing changes, for a widget that is not a
    /// slider, for NaN and for a value that lands where the slider already is.
    pub fn set_value(&mut self, id: WidgetId, value: f64) -> Option<Event> {
        let Kind::Slider(slider) = &mut self.widgets[id].kind else {
            return None;
        };
        let value = slider.snap(value)?;
        let moved = value != slider.value;
        slider.value = value;
        moved.then_some(Event::Changed(id))
    }

    /// Moves the slider `id` by `steps` steps, stopping at either end.
    pub(crate) fn step_value(&mut self, id: WidgetId, steps: i64) -> Option<Event> {
        let Kind::Slider(slider) = &self.widgets[id].kind else {
            return None;
        };
        let index = (slider.index(slider.value).saturating_add(steps)).clamp(0, slider.last());
        let value = slider.at(index);
        self.set_value(id, value)
    }

    /// Sets the slider `id` from the pointer at `x`, in logical pixels.
    fn slide(&mut self, id: WidgetId, x: f32) -> Option<Event> {
        let Kind::Slider(slider) = &self.widgets[id].kind else {
            return None;
        };
        let fraction = slider_fraction(self.widgets[id].rect, self.rtl, self.theme.font_size, x);
        let value = slider.min + f64::from(fraction) * (slider.max - slider.min);
        self.set_value(id, value)
    }

    /// Adds a scroll whose viewport is `height` logical pixels high; add its child to the id it
    /// returns. A height that is negative or not finite counts as 0.
    pub fn scroll(&mut self, parent: WidgetId, height: f32) -> WidgetId {
        let height = if height.is_finite() { height.max(0.0) } else { 0.0 };
        self.add(parent, Kind::Scroll { height, offset: 0.0 })
    }

    /// Adds a list named `name` for assistive technologies, with no row selected.
    pub fn list(&mut self, parent: WidgetId, name: &str) -> WidgetId {
        let name = name.to_owned();
        self.add(parent, Kind::List { name, selected: None })
    }

    /// Adds a row showing `text` at the end of the list `list`.
    pub fn list_row(&mut self, list: WidgetId, text: &str) -> WidgetId {
        self.add(list, Kind::ListRow(text.to_owned()))
    }

    /// The selected row of the list `list`, or `None` if no row is or `list` is not a list.
    pub fn selected(&self, list: WidgetId) -> Option<WidgetId> {
        match self.widgets[list].kind {
            Kind::List { selected, .. } => selected,
            _ => None,
        }
    }

    /// The list `id` is a row of, if it is one.
    fn list_of(&self, id: WidgetId) -> Option<WidgetId> {
        let parent = self.widgets[id].parent?;
        let is_row = matches!(self.widgets[id].kind, Kind::ListRow(_));
        let in_list = matches!(self.widgets[parent].kind, Kind::List { .. });
        (is_row && in_list).then_some(parent)
    }

    /// The height of the visible children of the scroll `id`, which its viewport shows part of.
    pub(crate) fn content_height(&self, id: WidgetId) -> f32 {
        let children = self.visible_children(id);
        children.iter().map(|&c| self.widgets[c].rect.h).sum()
    }

    /// Scrolls the scroll `id` by `dy` logical pixels, down when positive, within its content,
    /// and places its content again; returns whether it moved.
    fn scroll_by(&mut self, id: WidgetId, dy: f32) -> bool {
        if let Kind::TextArea(area) = &mut self.widgets[id].kind {
            return area.scroll_by(&mut self.text, dy);
        }
        let most = self.content_height(id);
        let Kind::Scroll { height, offset } = &mut self.widgets[id].kind else {
            return false;
        };
        let to = (*offset + dy).min(most - *height).max(0.0);
        if to == *offset {
            return false;
        }
        *offset = to;
        let r = self.widgets[id].rect;
        self.place(id, r.x, r.y);
        true
    }

    /// Scrolls every scroll that holds `id`, the innermost first, until `id` is inside its
    /// viewport, or its top is when it is taller than the viewport.
    fn reveal(&mut self, id: WidgetId) {
        let mut at = self.widgets[id].parent;
        while let Some(scroll) = at {
            if let Kind::Scroll { .. } = self.widgets[scroll].kind {
                let (view, r) = (self.widgets[scroll].rect, self.widgets[id].rect);
                let dy = if r.y < view.y {
                    r.y - view.y
                } else {
                    (r.bottom() - view.bottom()).min(r.y - view.y).max(0.0)
                };
                self.scroll_by(scroll, dy);
            }
            at = self.widgets[scroll].parent;
        }
    }

    /// The part of the window where widget `id` can be seen: the intersection of the viewports
    /// of the scrolls that hold it, or `None` when no scroll does.
    pub(crate) fn clip(&self, id: WidgetId) -> Option<Rect> {
        let mut clip: Option<Rect> = None;
        let mut at = self.widgets[id].parent;
        while let Some(w) = at {
            if let Kind::Scroll { .. } = self.widgets[w].kind {
                let v = self.widgets[w].rect;
                clip = Some(clip.map_or(v, |c| c.intersect(v)));
            }
            at = self.widgets[w].parent;
        }
        clip
    }

    /// The pointer's wheel or touchpad scrolled by `dy` logical pixels, down when positive, at
    /// the point, in logical pixels: the innermost scroll under it follows, within its content.
    /// Returns whether anything moved, and so needs drawing again.
    pub fn pointer_axis(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !dy.is_finite() {
            return false;
        }
        let under = (0..self.widgets.len()).filter(|&id| {
            matches!(self.widgets[id].kind, Kind::Scroll { .. } | Kind::TextArea(_))
                && self.widgets[id].rect.contains(x, y)
                && self.clip(id).is_none_or(|c| c.contains(x, y))
                && self.is_visible(id)
        });
        // The innermost: the one with the most ancestors.
        let depth = |id: WidgetId| {
            std::iter::successors(Some(id), |&w| self.widgets[w].parent).count()
        };
        let Some(scroll) = under.max_by_key(|&id| depth(id)) else {
            return false;
        };
        let moved = self.scroll_by(scroll, dy);
        if moved {
            self.hover = self.hit(x, y);
        }
        moved
    }

    /// The text of the field `id`, or `None` if it is not a field.
    pub fn field_text(&self, id: WidgetId) -> Option<String> {
        match &self.widgets[id].kind {
            Kind::Field(field) | Kind::TextArea(field) => Some(field.text()),
            _ => None,
        }
    }

    /// Says whether widget `id` opens a popup, as GTK's `HasPopup` property: assistive
    /// technologies then read it as collapsed or expanded. [`crate::driver::Shell::open_popup`]
    /// expands it while its menu is open.
    pub fn set_has_popup(&mut self, id: WidgetId, has: bool) {
        let expanded = &mut self.widgets[id].expanded;
        *expanded = has.then_some(expanded.unwrap_or(false));
    }

    /// Whether widget `id` opens a popup.
    pub fn has_popup(&self, id: WidgetId) -> bool {
        self.widgets[id].expanded.is_some()
    }

    /// Marks the popup of widget `id` open or closed; a widget without one stays as it is.
    pub fn set_expanded(&mut self, id: WidgetId, open: bool) {
        if let Some(expanded) = &mut self.widgets[id].expanded {
            *expanded = open;
        }
    }

    /// Whether the popup of widget `id` is open.
    pub fn is_expanded(&self, id: WidgetId) -> bool {
        self.widgets[id].expanded == Some(true)
    }

    /// Sets the tooltip of widget `id`, or removes it with `None`. The driver shows it after
    /// GTK's delay while the pointer rests on the widget or the keyboard focuses it, and it is
    /// the widget's accessible description unless [`Ui::set_description`] gives another, as GTK's.
    pub fn set_tooltip(&mut self, id: WidgetId, text: Option<&str>) {
        self.widgets[id].tooltip = text.map(str::to_owned);
    }

    /// The tooltip of widget `id`, if any.
    pub fn tooltip(&self, id: WidgetId) -> Option<&str> {
        self.widgets[id].tooltip.as_deref()
    }

    /// Sets the accessible description of widget `id`, which assistive technologies read after
    /// its name; `None` falls back to its tooltip.
    pub fn set_description(&mut self, id: WidgetId, text: Option<&str>) {
        self.widgets[id].description = text.map(str::to_owned);
    }

    /// The accessible description of widget `id`: the one set, else its tooltip.
    pub fn description(&self, id: WidgetId) -> Option<&str> {
        let widget = &self.widgets[id];
        widget.description.as_deref().or(widget.tooltip.as_deref())
    }

    /// The popup opened from widget `id` closed: the focus returns to the widget, which reads as
    /// collapsed, as when GTK's popover closes.
    pub(crate) fn popup_closed(&mut self, id: WidgetId) {
        self.set_expanded(id, false);
        self.set_focus(Some(id));
    }

    /// The widget that has the keyboard focus, if any.
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// Moves the focus to `id`, or clears it with `None`; a widget that takes no focus leaves
    /// it where it is. A widget newly focused inside a scroll is scrolled into view.
    pub fn set_focus(&mut self, id: Option<WidgetId>) {
        match id {
            Some(id) if !self.widgets[id].is_focusable() || !self.is_visible(id) => {}
            Some(id) if self.focus != Some(id) => {
                self.move_focus(Some(id));
                self.reveal(id);
            }
            _ => self.move_focus(id),
        }
    }

    /// Moves the focus to `to`. A field that loses it ends its input method's composition: the
    /// input method sends a preedit only for the focused field, so another would stay shown.
    fn move_focus(&mut self, to: Option<WidgetId>) {
        if to == self.focus {
            return;
        }
        self.end_composition();
        self.focus = to;
    }

    /// Ends the input method's composition in the focused field, without its text, as when the
    /// field loses the focus or its window the text input. Whether there was one.
    pub(crate) fn end_composition(&mut self) -> bool {
        let Some(Kind::Field(field) | Kind::TextArea(field)) =
            self.focus.map(|id| &mut self.widgets[id].kind)
        else {
            return false;
        };
        if !field.is_composing() {
            return false;
        }
        field.compose(&mut self.text, "", None);
        // A preedit that scrolled a text area leaves the view past the shorter text.
        field.scroll_by(&mut self.text, 0.0);
        true
    }

    /// Shows or hides widget `id` and everything under it. A hidden widget takes no room, is not
    /// drawn, takes no focus or input and has no node for assistive technologies; it keeps its
    /// state. Hiding the widget that has the focus moves the focus to the next visible widget that
    /// takes it, or the previous one. The root and the title bar cannot be hidden: asking for it
    /// changes nothing. Call [`Ui::layout`] afterwards.
    pub fn set_visible(&mut self, id: WidgetId, visible: bool) {
        if id == Self::ROOT || Some(id) == self.title_bar || self.widgets[id].visible == visible {
            return;
        }
        let before = focus::chain(self);
        self.widgets[id].visible = visible;
        let Some(at) = self.focus.and_then(|f| before.iter().position(|&w| w == f)) else {
            return;
        };
        if !self.is_visible(before[at]) {
            let after = focus::chain(self);
            let next = before[at + 1..].iter().find(|w| after.contains(w));
            let previous = before[..at].iter().rev().find(|w| after.contains(w));
            let to = next.or(previous).copied();
            self.move_focus(to);
        }
    }

    /// Whether `id` and all its ancestors are visible: what the user can see and reach.
    pub fn is_visible(&self, id: WidgetId) -> bool {
        let mut at = Some(id);
        while let Some(w) = at {
            if !self.widgets[w].visible {
                return false;
            }
            at = self.widgets[w].parent;
        }
        true
    }

    /// The window's size in logical pixels, after [`Ui::layout`].
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Shapes every text and places every widget; returns the window's size in logical pixels.
    pub fn layout(&mut self) -> (f32, f32) {
        let (w, h) = self.measure(Self::ROOT);
        self.place(Self::ROOT, 0.0, 0.0);
        self.size = (w, h);
        self.size
    }

    pub(crate) fn visible_children(&self, id: WidgetId) -> Vec<WidgetId> {
        let widgets = &self.widgets;
        let shown = widgets[id].children.iter();
        shown.copied().filter(|&c| widgets[c].visible).collect()
    }

    fn measure(&mut self, id: WidgetId) -> (f32, f32) {
        let t = &self.theme;
        let (pad, gap, size, field_width) = (t.padding, t.gap, t.font_size, t.field_width);
        let (margin, section_gap, min_width) = (t.margin, t.section_gap, self.min_width);
        let rtl = self.rtl;
        let bar_first = id == Self::ROOT && self.title_bar.is_some();
        let is_switch = matches!(self.widgets[id].kind, Kind::Switch { .. });
        let children = self.visible_children(id);
        let sizes: Vec<(f32, f32)> = children.iter().map(|&c| self.measure(c)).collect();
        let spacing = gap * children.len().saturating_sub(1) as f32;
        let between = match self.widgets[id].kind {
            Kind::Column => section_gap,
            _ => gap,
        };
        let widget = &mut self.widgets[id];
        let (w, h) = match &mut widget.kind {
            Kind::Menu | Kind::List { .. } => {
                let w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
                (w, sizes.iter().map(|s| s.1).sum::<f32>())
            }
            Kind::Scroll { height, offset } => {
                let w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
                let content = sizes.iter().map(|s| s.1).sum::<f32>();
                // A height set through `Ui::add` is checked here; NaN falls to 0 in `max`.
                *height = if height.is_finite() { height.max(0.0) } else { 0.0 };
                *offset = offset.min(content - *height).max(0.0);
                (w, *height)
            }
            // A line in the middle of a gap.
            Kind::Separator => (0.0, gap + 1.0),
            Kind::Image { size, .. } => (*size, *size),
            Kind::Column | Kind::Group(_) => {
                // The title bar spans the window above the root's margin.
                let (bar, content) = match sizes.split_first() {
                    Some((bar, rest)) if bar_first => (*bar, rest),
                    _ => ((0.0, 0.0), &sizes[..]),
                };
                let spacing = between * content.len().saturating_sub(1) as f32;
                let w = content.iter().map(|s| s.0).fold(0.0, f32::max);
                let h = content.iter().map(|s| s.1).sum::<f32>() + spacing;
                if id == Self::ROOT {
                    let edge = 2.0 * margin;
                    ((w + edge).max(bar.0).max(min_width), h + edge + bar.1)
                } else {
                    (w, h)
                }
            }
            Kind::TitleBar => {
                // Room for the buttons on both sides, so the centred title never meets them.
                let title = sizes.first().map_or(0.0, |s| s.0);
                let buttons = sizes.iter().skip(1).map(|s| s.0 + gap).sum::<f32>();
                let h = sizes.iter().map(|s| s.1).fold(0.0, f32::max);
                (title + 2.0 * buttons + 2.0 * pad, h + pad)
            }
            Kind::WindowButton(_) => (size + pad, size + pad),
            Kind::Row => {
                let widest = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
                let w = widest * sizes.len() as f32 + spacing;
                (w, sizes.iter().map(|s| s.1).fold(0.0, f32::max))
            }
            Kind::Label(text) | Kind::Tooltip(text) => {
                let layout = self.text.layout(text, size, rtl);
                let wh = (layout.width(), layout.height());
                widget.text = Some(layout);
                wh
            }
            Kind::Button(text)
            | Kind::Toggle { label: text, .. }
            | Kind::MenuItem(text)
            | Kind::ListRow(text) => {
                let layout = self.text.layout(text, size, rtl);
                let wh = (layout.width() + 2.0 * pad, layout.height() + pad);
                widget.text = Some(layout);
                wh
            }
            Kind::Check { label, .. } | Kind::Switch { label, .. } => {
                let (mark_w, mark_h) = mark_size(is_switch, size);
                let layout = self.text.layout(label, size, rtl);
                let wh = (
                    mark_w + gap + layout.width(),
                    mark_h.max(layout.height()) + pad,
                );
                widget.text = Some(layout);
                wh
            }
            Kind::Slider(_) => (field_width, slider_knob(size) + pad),
            Kind::Field(field) => {
                let line = field.layout(&mut self.text).height().max(size * 1.25);
                (field_width, line + pad)
            }
            Kind::TextArea(area) => (field_width, area.view_height(&mut self.text) + pad),
        };
        widget.rect.w = w;
        widget.rect.h = h;
        (w, h)
    }

    fn place(&mut self, id: WidgetId, x: f32, y: f32) {
        self.widgets[id].rect.x = x;
        self.widgets[id].rect.y = y;
        let edge = if id == Self::ROOT {
            self.theme.margin
        } else {
            0.0
        };
        let gap = match self.widgets[id].kind {
            Kind::Column => self.theme.section_gap,
            _ => self.theme.gap,
        };
        let outer = self.widgets[id].rect;
        let children = self.visible_children(id);
        match self.widgets[id].kind {
            Kind::Row => {
                let n = children.len().max(1) as f32;
                let w = ((outer.w - gap * (n - 1.0)) / n).max(0.0);
                let mut cx = if self.rtl { x + outer.w } else { x };
                for c in children {
                    self.widgets[c].rect.w = w;
                    if self.rtl {
                        cx -= w;
                        self.place(c, cx, y);
                        cx -= gap;
                    } else {
                        self.place(c, cx, y);
                        cx += w + gap;
                    }
                }
            }
            Kind::Menu | Kind::List { .. } | Kind::Scroll { .. } => {
                let mut cy = match self.widgets[id].kind {
                    Kind::Scroll { offset, .. } => y - offset,
                    _ => y,
                };
                for c in children {
                    self.widgets[c].rect.w = outer.w;
                    self.place(c, x, cy);
                    cy += self.widgets[c].rect.h;
                }
            }
            Kind::TitleBar => {
                let pad = self.theme.padding;
                let Some((&title, buttons)) = children.split_first() else {
                    return;
                };
                let r = self.widgets[title].rect;
                self.place(title, x + (outer.w - r.w) / 2.0, y + (outer.h - r.h) / 2.0);
                // From the end edge inwards, so Close sits at the edge.
                let mut edge_x = if self.rtl { x + pad } else { x + outer.w - pad };
                for &b in buttons.iter().rev() {
                    let r = self.widgets[b].rect;
                    let by = y + (outer.h - r.h) / 2.0;
                    if self.rtl {
                        self.place(b, edge_x, by);
                        edge_x += r.w + gap;
                    } else {
                        edge_x -= r.w;
                        self.place(b, edge_x, by);
                        edge_x -= gap;
                    }
                }
            }
            Kind::Column | Kind::Group(_) => {
                let mut cy = y + edge;
                let mut children = &children[..];
                if let (Some(bar), true) = (self.title_bar, id == Self::ROOT) {
                    self.widgets[bar].rect.w = outer.w;
                    self.place(bar, x, y);
                    cy += self.widgets[bar].rect.h;
                    children = &children[1..];
                }
                for &c in children {
                    if matches!(
                        self.widgets[c].kind,
                        Kind::Row
                            | Kind::Group(_)
                            | Kind::Menu
                            | Kind::Switch { .. }
                            | Kind::Slider(_)
                            | Kind::Scroll { .. }
                            | Kind::List { .. }
                    ) {
                        self.widgets[c].rect.w = outer.w - 2.0 * edge;
                    }
                    let r = self.widgets[c].rect;
                    let cx = if self.rtl {
                        x + outer.w - edge - r.w
                    } else {
                        x + edge
                    };
                    self.place(c, cx, cy);
                    cy += r.h + gap;
                }
            }
            _ => {}
        }
    }

    /// Activates `id` as a click would: a button reports it, a toggle becomes the active one of
    /// its group, a check button or a switch turns over, a list's row becomes its selected one.
    /// Each takes the focus.
    pub fn activate(&mut self, id: WidgetId) -> Option<Event> {
        if !self.is_visible(id) {
            return None;
        }
        self.set_focus(Some(id));
        match self.widgets[id].kind {
            Kind::Button(_) | Kind::MenuItem(_) => Some(Event::Clicked(id)),
            Kind::WindowButton(button) => Some(Event::Window(button)),
            Kind::Check { ref mut active, .. } | Kind::Switch { ref mut active, .. } => {
                *active = !*active;
                Some(Event::Toggled(id))
            }
            Kind::Toggle { .. } => {
                let siblings = self.widgets[id]
                    .parent
                    .map(|p| self.widgets[p].children.clone())
                    .unwrap_or_default();
                for s in siblings {
                    if let Kind::Toggle { active, .. } = &mut self.widgets[s].kind {
                        *active = s == id;
                    }
                }
                Some(Event::Selected(id))
            }
            Kind::ListRow(_) => {
                let list = self.list_of(id);
                if let Some(Kind::List { selected, .. }) = list.map(|l| &mut self.widgets[l].kind) {
                    *selected = Some(id);
                }
                Some(Event::Selected(id))
            }
            _ => None,
        }
    }

    /// The focusable widget under the point, in logical pixels; a widget scrolled out of its
    /// scroll's viewport is not under any.
    pub fn hit(&self, x: f32, y: f32) -> Option<WidgetId> {
        (0..self.widgets.len()).find(|&id| {
            self.widgets[id].is_focusable()
                && self.widgets[id].rect.contains(x, y)
                && self.clip(id).is_none_or(|c| c.contains(x, y))
                && self.is_visible(id)
        })
    }

    /// Presses and releases the primary button at the point, in logical pixels, and returns the
    /// event of the widget there, if any.
    pub fn click(&mut self, x: f32, y: f32) -> Option<Event> {
        self.pointer_press(x, y, PointerButton::Primary);
        self.pointer_release(x, y, PointerButton::Primary)
    }

    /// The widget under the pointer, which is drawn highlighted, if any.
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hover.filter(|&id| self.is_visible(id))
    }

    /// The pointer moved to the point, in logical pixels: the widget under it is hovered, and a
    /// press that has moved past [`Ui::DRAG_THRESHOLD`] on either axis starts a drag.
    pub fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Event> {
        self.hover = self.hit(x, y);
        let press = self.press.as_mut()?;
        if let Kind::Slider(_) = self.widgets[press.widget].kind {
            let id = press.widget;
            return self.slide(id, x);
        }
        let moved = (x - press.x).abs().max((y - press.y).abs());
        if press.dragging || moved <= Self::DRAG_THRESHOLD {
            return None;
        }
        press.dragging = true;
        Some(Event::DragStarted(press.widget))
    }

    /// The pointer left the surface: nothing is hovered, and the press waiting for its release
    /// ends, since the release goes elsewhere, as when a grab starts with the button down. A
    /// drag it started ends over no widget.
    pub fn pointer_leave(&mut self) -> Option<Event> {
        self.hover = None;
        let press = self.press.take()?;
        press.dragging.then_some(Event::DragEnded {
            from: press.widget,
            to: None,
        })
    }

    /// A pointer button was pressed at the point, in logical pixels. The primary button focuses
    /// the widget there and waits for the release to activate it, as GTK's buttons do; the
    /// secondary button asks for its context menu.
    pub fn pointer_press(&mut self, x: f32, y: f32, button: PointerButton) -> Option<Event> {
        let id = self.hit(x, y);
        match button {
            PointerButton::Primary => {
                self.press = id.map(|widget| Press {
                    widget,
                    x,
                    y,
                    dragging: false,
                });
                self.set_focus(id.or(self.focus));
                // A slider takes its value from the press, and from the drag that follows.
                id.and_then(|id| self.slide(id, x))
            }
            PointerButton::Secondary => id.map(Event::ContextMenu),
        }
    }

    /// A pointer button was released at the point, in logical pixels: the primary button ends a
    /// drag, or activates the widget it was pressed on if it is still under the pointer.
    pub fn pointer_release(&mut self, x: f32, y: f32, button: PointerButton) -> Option<Event> {
        if button != PointerButton::Primary {
            return None;
        }
        let press = self.press.take()?;
        if let Kind::Slider(_) = self.widgets[press.widget].kind {
            return None;
        }
        let to = self.hit(x, y);
        if press.dragging {
            return Some(Event::DragEnded {
                from: press.widget,
                to,
            });
        }
        (to == Some(press.widget))
            .then(|| self.activate(press.widget))
            .flatten()
    }

    /// Handles a key: a focused field takes text, deletion and the horizontal arrows; otherwise
    /// Tab and Shift+Tab walk the logical order, the arrows the visual one, and Enter or Space
    /// activates. A focused slider takes the arrows (Right moves the knob right, which lowers the
    /// value when mirrored; Up and Down are not mirrored), Page Up and Page Down (ten steps) and
    /// Home and End (the least and the greatest value). A focused list row takes Up, Down, Page
    /// Up, Page Down (a viewport), Home and End, which choose a row of its list. A focused text
    /// area also takes Enter, which breaks the line, and Up and Down, which move between lines
    /// and never leave it, as GTK's `TextView`.
    pub fn key(&mut self, key: Key) -> Option<Event> {
        let focused = self.focus;
        if let Some(id) = focused {
            let multiline = matches!(self.widgets[id].kind, Kind::TextArea(_));
            if let Kind::Field(field) | Kind::TextArea(field) = &mut self.widgets[id].kind {
                let cx = &mut self.text;
                let edited = match &key {
                    Key::Text(s) => {
                        field.insert(cx, s);
                        true
                    }
                    Key::Enter if multiline => {
                        field.insert(cx, "\n");
                        true
                    }
                    Key::Backspace => {
                        field.backspace(cx);
                        true
                    }
                    Key::Delete => {
                        field.delete(cx);
                        true
                    }
                    Key::Left => {
                        field.left(cx);
                        false
                    }
                    Key::Right => {
                        field.right(cx);
                        false
                    }
                    Key::Up if multiline => {
                        field.up(cx);
                        false
                    }
                    Key::Down if multiline => {
                        field.down(cx);
                        false
                    }
                    _ => return self.navigate(key),
                };
                field.scroll_to_caret(cx);
                return edited.then_some(Event::Edited(id));
            }
        }
        if let Some(id) = focused {
            if let Kind::Slider(slider) = &self.widgets[id].kind {
                let (min, max) = (slider.min, slider.max);
                // What is seen: Right moves the knob right, which is toward min when mirrored.
                let along = if self.rtl { -1 } else { 1 };
                return match key {
                    Key::Right => self.step_value(id, along),
                    Key::Left => self.step_value(id, -along),
                    Key::Up => self.step_value(id, 1),
                    Key::Down => self.step_value(id, -1),
                    Key::PageUp => self.step_value(id, 10),
                    Key::PageDown => self.step_value(id, -10),
                    Key::Home => self.set_value(id, min),
                    Key::End => self.set_value(id, max),
                    key => self.navigate(key),
                };
            }
        }
        if let Some((list, id)) = focused.and_then(|id| Some((self.list_of(id)?, id))) {
            let rows = self.visible_children(list);
            let at = rows.iter().position(|&r| r == id).unwrap_or(0);
            let last = rows.len().saturating_sub(1);
            let to = match key {
                // Past either end the focus leaves the list for the widget in that direction,
                // if there is one, as GTK's ListBox does; it does not wrap round.
                Key::Up if at == 0 => return self.navigate(Key::Up),
                Key::Down if at == last => return self.navigate(Key::Down),
                Key::Up => at - 1,
                Key::Down => at + 1,
                Key::Home => 0,
                Key::End => last,
                Key::PageUp | Key::PageDown => self.page(list, &rows, at, key == Key::PageDown),
                key => return self.list_key(Some(id), key),
            };
            let unmoved = to == at && self.selected(list) == Some(id);
            return if unmoved { None } else { self.activate(rows[to]) };
        }
        self.list_key(focused, key)
    }

    /// The row a viewport above or below the middle of `rows[at]`, the viewport of the scroll
    /// that holds `list` or else the list's own height, clamped to the first and last rows.
    fn page(&self, list: WidgetId, rows: &[WidgetId], at: usize, down: bool) -> usize {
        let mut viewport = self.widgets[list].rect.h;
        let mut up = self.widgets[list].parent;
        while let Some(w) = up {
            if let Kind::Scroll { height, .. } = self.widgets[w].kind {
                viewport = height;
                break;
            }
            up = self.widgets[w].parent;
        }
        let r = self.widgets[rows[at]].rect;
        let y = r.y + r.h / 2.0 + if down { viewport } else { -viewport };
        let past = rows.iter().take_while(|&&row| self.widgets[row].rect.y <= y);
        past.count().saturating_sub(1).min(rows.len().saturating_sub(1))
    }

    /// What a key does on `focused` when no widget takes it for itself.
    fn list_key(&mut self, focused: Option<WidgetId>, key: Key) -> Option<Event> {
        match key {
            Key::Enter => focused.and_then(|id| self.activate(id)),
            Key::Text(s) if s == " " => focused.and_then(|id| self.activate(id)),
            Key::Menu => focused.map(Event::ContextMenu),
            key => self.navigate(key),
        }
    }

    /// The menu the focus is in, or the first visible one when nothing has the focus.
    fn menu(&self) -> Option<WidgetId> {
        match self.focus {
            Some(id) => self.widgets[id]
                .parent
                .filter(|&p| matches!(self.widgets[p].kind, Kind::Menu)),
            None => (0..self.widgets.len())
                .find(|&id| matches!(self.widgets[id].kind, Kind::Menu) && self.is_visible(id)),
        }
    }

    fn navigate(&mut self, key: Key) -> Option<Event> {
        let next = match (key, self.menu()) {
            // Up and down cycle through a menu's items, as in GTK's menus.
            (Key::Up, Some(menu)) => focus::cycle(self, menu, true),
            (Key::Down, Some(menu)) => focus::cycle(self, menu, false),
            (Key::Tab, _) => focus::step(self, false),
            (Key::BackTab, _) => focus::step(self, true),
            (key @ (Key::Left | Key::Right | Key::Up | Key::Down), _) => focus::arrow(self, &key),
            _ => None,
        };
        if next.is_some() {
            self.set_focus(next);
        }
        None
    }

    /// Shows the input method's preedit in the focused field, scrolled to its caret.
    pub fn compose(&mut self, preedit: &str, cursor: Option<(usize, usize)>) {
        if let Some(Kind::Field(field) | Kind::TextArea(field)) =
            self.focus.map(|id| &mut self.widgets[id].kind)
        {
            field.compose(&mut self.text, preedit, cursor);
            field.scroll_to_caret(&mut self.text);
        }
    }

    /// Commits the input method's text into the focused field, without the control
    /// characters but a text area's newlines, as typing them would.
    pub fn commit(&mut self, text: &str) -> Option<Event> {
        let id = self.focus?;
        let multiline = matches!(self.widgets[id].kind, Kind::TextArea(_));
        let (Kind::Field(field) | Kind::TextArea(field)) = &mut self.widgets[id].kind else {
            return None;
        };
        field.commit(&mut self.text, &printable(text, multiline));
        field.scroll_to_caret(&mut self.text);
        Some(Event::Edited(id))
    }

    /// Replaces a field's text, as an assistive technology's SetValue does: without the control
    /// characters but a text area's newlines, and without the bidirectional formats, which
    /// would change how the text reads without being seen.
    pub fn set_field_text(&mut self, id: WidgetId, text: &str) -> Option<Event> {
        let multiline = matches!(self.widgets[id].kind, Kind::TextArea(_));
        let (Kind::Field(field) | Kind::TextArea(field)) = &mut self.widgets[id].kind else {
            return None;
        };
        let text: String = text
            .chars()
            .filter(|&c| keeps(c, multiline) && !is_invisible_format(c))
            .take(Self::FIELD_MAX_CHARS)
            .collect();
        field.set_text(&text);
        if multiline {
            // As a GTK text view's, where a single-line field's caret stays at its start.
            field.move_to_end(&mut self.text);
        }
        field.scroll_to_caret(&mut self.text);
        Some(Event::Edited(id))
    }
}

/// The size of the box of a check button, or of the track of a switch, at text size `font`, in
/// logical pixels.
pub(crate) fn mark_size(is_switch: bool, font: f32) -> (f32, f32) {
    if is_switch {
        (font * 3.2, font * 1.6)
    } else {
        (font * 1.2, font * 1.2)
    }
}

/// Where the mark of a check button (at the start edge) or of a switch (at the end edge) sits in
/// the widget's rectangle `r`, centred vertically; the label takes the rest.
pub(crate) fn mark_rect(r: Rect, rtl: bool, (w, h): (f32, f32), at_end: bool) -> Rect {
    let left = rtl == at_end;
    Rect {
        x: if left { r.x } else { r.x + r.w - w },
        y: r.y + (r.h - h) / 2.0,
        w,
        h,
    }
}

/// The x of the label of a check button or a switch of width `width`, given its `mark`: a check's
/// label follows its box by `gap`, wherever the widget's rectangle `r` ends; a switch's label
/// takes the far edge, opposite its track.
pub(crate) fn label_x(r: Rect, mark: Rect, rtl: bool, is_switch: bool, gap: f32, width: f32) -> f32 {
    match (is_switch, rtl) {
        (false, false) => mark.x + mark.w + gap,
        (false, true) => mark.x - gap - width,
        (true, false) => r.x,
        (true, true) => r.x + r.w - width,
    }
}

/// The knob inside the `track` of a switch: at the start edge when off, at the end edge when on,
/// so that it mirrors in right-to-left.
pub(crate) fn knob_rect(track: Rect, rtl: bool, active: bool) -> Rect {
    let inset = 2.0;
    let d = track.h - 2.0 * inset;
    let left = active == rtl;
    Rect {
        x: if left {
            track.x + inset
        } else {
            track.x + track.w - inset - d
        },
        y: track.y + inset,
        w: d,
        h: d,
    }
}

/// The diameter of a slider's knob at text size `font`, in logical pixels.
pub(crate) fn slider_knob(font: f32) -> f32 {
    font * 1.4
}

/// The horizontal centre of the knob of a slider in rectangle `r` when its value is `fraction`
/// of the way from min to max: the track runs from min at the start edge to max at the end
/// edge, a knob's radius inside the rectangle so that the knob stays in it.
pub(crate) fn slider_knob_x(r: Rect, rtl: bool, font: f32, fraction: f64) -> f32 {
    let inset = slider_knob(font) / 2.0;
    let length = (r.w - 2.0 * inset).max(0.0);
    let along = fraction as f32 * length;
    if rtl {
        r.x + r.w - inset - along
    } else {
        r.x + inset + along
    }
}

/// The inverse of [`slider_knob_x`]: how far from min to max the pointer at `x` is, from 0 to 1.
fn slider_fraction(r: Rect, rtl: bool, font: f32, x: f32) -> f32 {
    let (at_min, at_max) = (slider_knob_x(r, rtl, font, 0.0), slider_knob_x(r, rtl, font, 1.0));
    if at_min == at_max {
        return 0.0;
    }
    ((x - at_min) / (at_max - at_min)).clamp(0.0, 1.0)
}

/// Bidirectional overrides, isolates and marks, and the line and paragraph separators: they
/// change how text reads without being seen.
fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toggle_group_is_exclusive() {
        let mut ui = Ui::new("Test", Theme::default(), false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        let toggle = |label: &str, active| Kind::Toggle {
            label: label.into(),
            active,
        };
        let a = ui.add(row, toggle("A", true));
        let b = ui.add(row, toggle("B", false));
        let c = ui.add(row, toggle("C", false));
        let other = ui.add(Ui::ROOT, Kind::Row);
        let d = ui.add(other, toggle("D", true));

        assert_eq!(ui.activate(b), Some(Event::Selected(b)));
        assert_eq!(
            [a, b, c, d].map(|id| ui.is_active(id)),
            [false, true, false, true]
        );
        ui.set_focus(Some(c));
        assert_eq!(ui.key(Key::Text(" ".into())), Some(Event::Selected(c)));
        assert_eq!(
            [a, b, c, d].map(|id| ui.is_active(id)),
            [false, false, true, true]
        );
        // Activating the active toggle keeps it active: a radio group is never empty.
        ui.activate(c);
        assert!(ui.is_active(c));
    }

    #[test]
    fn a_check_toggles_on_click_and_space() {
        let mut ui = Ui::new("Checks", Theme::default(), false);
        let check = |label: &str, active| Kind::Check {
            label: label.into(),
            active,
        };
        let a = ui.add(Ui::ROOT, check("A", false));
        let b = ui.add(Ui::ROOT, check("B", true));
        ui.layout();
        assert_eq!(crate::focus::chain(&ui), [a, b]);

        let r = ui.widget(a).rect;
        assert_eq!(ui.click(r.x + 1.0, r.y + 1.0), Some(Event::Toggled(a)));
        // Checks under one parent are independent, unlike toggles.
        assert_eq!([a, b].map(|id| ui.is_active(id)), [true, true]);
        assert_eq!(ui.focus(), Some(a));
        assert_eq!(ui.key(Key::Text(" ".into())), Some(Event::Toggled(a)));
        assert_eq!([a, b].map(|id| ui.is_active(id)), [false, true]);
        ui.set_focus(Some(b));
        assert_eq!(ui.key(Key::Enter), Some(Event::Toggled(b)));
        assert_eq!([a, b].map(|id| ui.is_active(id)), [false, false]);
    }

    #[test]
    fn a_switch_mirrors_its_knob() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Switch", Theme::default(), rtl);
            ui.min_width = 300.0;
            let switch = |label: &str, active| Kind::Switch {
                label: label.into(),
                active,
            };
            let off = ui.add(Ui::ROOT, switch("Off", false));
            let on = ui.add(Ui::ROOT, switch("On", true));
            let (w, _) = ui.layout();
            let parts = |id: WidgetId| {
                let widget = ui.widget(id);
                let size = mark_size(true, ui.theme.font_size);
                let track = mark_rect(widget.rect, rtl, size, true);
                (widget.rect, track, knob_rect(track, rtl, ui.is_active(id)))
            };
            let (row, track, knob) = parts(off);
            let margin = ui.theme.margin;
            assert_eq!(
                row.w,
                w - 2.0 * margin,
                "rtl {rtl}: the switch fills the width"
            );
            if rtl {
                assert_eq!(track.x, row.x, "the track at the end edge, the left");
                assert!(
                    knob.x + knob.w > track.center_x(),
                    "an off knob is at the right"
                );
            } else {
                assert_eq!(track.x + track.w, row.x + row.w);
                assert!(knob.x < track.center_x(), "an off knob is at the left");
            }
            let (_, track, knob) = parts(on);
            assert_eq!(
                knob.x + knob.w > track.center_x(),
                !rtl,
                "rtl {rtl}: an on knob is at the other side"
            );
            assert_eq!(ui.activate(off), Some(Event::Toggled(off)));
            assert!(ui.is_active(off));
        }
    }

    #[test]
    fn a_check_label_follows_its_box_in_a_row_cell() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Checks", Theme::default(), rtl);
            ui.min_width = 600.0;
            let row = ui.add(Ui::ROOT, Kind::Row);
            let check = |label: &str| Kind::Check {
                label: label.into(),
                active: false,
            };
            let bold = ui.add(row, check("Bold"));
            ui.add(row, check("A much longer label"));
            ui.layout();
            let widget = ui.widget(bold);
            let width = widget.text.as_ref().expect("a laid-out label").width();
            assert!(width > 0.0);
            let (r, gap) = (widget.rect, ui.theme.gap);
            let mark = mark_rect(r, rtl, mark_size(false, ui.theme.font_size), false);
            assert!(r.w > mark.w + gap + width, "the cell is wider than its content");
            let x = label_x(r, mark, rtl, false, gap, width);
            if rtl {
                assert!((x + width + gap - mark.x).abs() < 1e-3, "the label ends a gap before the box");
            } else {
                assert!((x - (mark.x + mark.w + gap)).abs() < 1e-3, "the label starts a gap after the box");
            }
            // A switch keeps the far edge.
            let track = mark_rect(r, rtl, mark_size(true, ui.theme.font_size), true);
            let x = label_x(r, track, rtl, true, gap, width);
            assert_eq!(if rtl { x + width } else { x }, if rtl { r.x + r.w } else { r.x });
        }
    }

    #[test]
    fn a_row_fills_its_column_with_children_of_one_width() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Row", Theme::default(), rtl);
            ui.min_width = 300.0;
            let row = ui.add(Ui::ROOT, Kind::Row);
            let short = ui.add(row, Kind::Button("A".into()));
            let long = ui.add(row, Kind::Button("A longer label".into()));
            let (w, _) = ui.layout();
            assert_eq!(w, 300.0, "rtl {rtl}: the window's minimum width");
            let (s, l) = (ui.widget(short).rect, ui.widget(long).rect);
            assert!((s.w - l.w).abs() < 0.01, "rtl {rtl}: {s:?} {l:?}");
            // In reading order from the start edge, the margin's width inside the window.
            let (left, right) = if rtl { (l, s) } else { (s, l) };
            let margin = ui.theme.margin;
            assert!((left.x - margin).abs() < 0.01, "rtl {rtl}: {left:?}");
            assert!(
                (right.x + right.w - (w - margin)).abs() < 0.01,
                "rtl {rtl}: {right:?}"
            );
        }
    }

    #[test]
    fn the_title_bar_spans_the_window_with_its_buttons_at_the_end_edge() {
        for rtl in [false, true] {
            let mut ui = crate::chooser::build(&athanor_i18n::Catalog::empty(), rtl).ui;
            let plain = ui.layout();
            ui.add_title_bar();
            ui.add_title_bar();
            let bar = ui.widget(Ui::ROOT).children[0];
            ui.set_visible(bar, false);
            assert!(
                ui.is_visible(bar),
                "rtl {rtl}: the title bar cannot be hidden"
            );
            let (w, h) = ui.layout();
            let r = ui.widget(bar).rect;
            assert_eq!((r.x, r.y, r.w), (0.0, 0.0, w), "rtl {rtl}");
            assert_eq!(h, plain.1 + r.h, "rtl {rtl}: the bar adds its height once");
            let parts = ui.widget(bar).children.clone();
            assert_eq!(parts.len(), 3, "rtl {rtl}: the title and two buttons");
            let (title, minimize, close) = (parts[0], parts[1], parts[2]);
            let (m, c) = (ui.widget(minimize).rect, ui.widget(close).rect);
            let pad = ui.theme.padding;
            if rtl {
                assert!(c.x == pad && m.x > c.x, "rtl: {m:?} {c:?}");
            } else {
                assert!(c.x + c.w == w - pad && m.x < c.x, "ltr: {m:?} {c:?}");
            }
            let t = ui.widget(title).rect;
            assert!((t.center_x() - w / 2.0).abs() < 0.01 && !t.contains(m.x, m.y + 1.0));
            assert!(ui.in_title_bar(w / 2.0, 1.0) && !ui.in_title_bar(w / 2.0, r.h + 1.0));
            assert_eq!(crate::focus::chain(&ui)[..2], [minimize, close]);
            assert_eq!(ui.activate(close), Some(Event::Window(WindowButton::Close)));
        }
    }

    /// A column with a button, a hidden row of two buttons and a field.
    fn with_hidden_row() -> (Ui, [WidgetId; 5]) {
        let mut ui = Ui::new("Hidden", Theme::default(), false);
        let top = ui.add(Ui::ROOT, Kind::Button("Top".into()));
        let row = ui.add(Ui::ROOT, Kind::Row);
        let a = ui.add(row, Kind::Button("A".into()));
        let b = ui.add(row, Kind::Button("B".into()));
        let field = ui.field(Ui::ROOT, "Name", "");
        (ui, [top, row, a, b, field])
    }

    #[test]
    fn a_hidden_widget_is_gone_everywhere() {
        use crate::a11y::{node_id, tree_update};
        let (mut ui, [top, row, a, b, field]) = with_hidden_row();
        let shown = ui.layout();
        let old = ui.widget(a).rect;
        ui.set_visible(row, false);
        let hidden = ui.layout();
        assert_eq!(
            shown.1 - hidden.1,
            ui.widget(row).rect.h + ui.theme.section_gap,
            "the row's height and its gap leave the window"
        );
        assert_eq!(crate::focus::chain(&ui), [top, field]);
        assert!(![Some(a), Some(b)].contains(&ui.hit(old.x + 1.0, old.y + 1.0)));
        let has = |ui: &Ui, id| {
            let update = tree_update(ui, 1.0);
            update.nodes.iter().any(|(n, _)| *n == node_id(id))
        };
        assert!(![row, a, b].iter().any(|&id| has(&ui, id)));
        assert!(!ui.is_visible(a) && ui.widget(a).visible);

        ui.set_visible(row, true);
        assert_eq!(ui.layout(), shown);
        assert_eq!(crate::focus::chain(&ui), [top, a, b, field]);
        assert_eq!(ui.hit(old.x + 1.0, old.y + 1.0), Some(a));
        assert!([row, a, b].iter().all(|&id| has(&ui, id)));
    }

    #[test]
    fn hiding_the_focus_moves_it() {
        use crate::a11y::{node_id, tree_update};
        use accesskit::{Action, ActionRequest, TreeId};
        let (mut ui, [_, row, a, b, field]) = with_hidden_row();
        ui.layout();
        ui.set_focus(Some(a));
        ui.set_visible(row, false);
        assert_eq!(ui.focus(), Some(field));
        let update = tree_update(&ui, 1.0);
        assert!(update.nodes.iter().any(|(n, _)| *n == update.focus));
        assert_eq!(update.focus, node_id(field));

        ui.set_focus(Some(b));
        assert_eq!(ui.focus(), Some(field), "a hidden button takes no focus");
        assert_eq!(ui.activate(b), None);
        let click = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: node_id(b),
            data: None,
        };
        assert_eq!(ui.do_action(&click), None);
        assert_eq!(ui.focus(), Some(field));
        ui.set_visible(row, true);
        assert_eq!(ui.activate(b), Some(Event::Clicked(b)));
    }

    /// A menu of three items with a separator between the first and the second.
    fn menu(rtl: bool) -> (Ui, [WidgetId; 4]) {
        let mut ui = Ui::new("Menu", Theme::default(), rtl);
        let menu = ui.add(Ui::ROOT, Kind::Menu);
        let first = ui.add(menu, Kind::MenuItem("New Window".into()));
        ui.add(menu, Kind::Separator);
        let second = ui.add(menu, Kind::MenuItem("Pin to Dock".into()));
        let third = ui.add(menu, Kind::MenuItem("Quit".into()));
        ui.layout();
        (ui, [menu, first, second, third])
    }

    #[test]
    fn the_arrows_cycle_through_a_menus_items_past_its_separators() {
        for rtl in [false, true] {
            let (mut ui, [_, first, second, third]) = menu(rtl);
            assert_eq!(ui.focus(), None);
            let mut walk = |key: Key| {
                assert_eq!(ui.key(key), None);
                ui.focus()
            };
            assert_eq!(walk(Key::Down), Some(first), "rtl {rtl}");
            assert_eq!(
                walk(Key::Down),
                Some(second),
                "rtl {rtl}: the separator is skipped"
            );
            assert_eq!(walk(Key::Down), Some(third), "rtl {rtl}");
            assert_eq!(
                walk(Key::Down),
                Some(first),
                "rtl {rtl}: down wraps, as GTK's menus"
            );
            assert_eq!(walk(Key::Up), Some(third), "rtl {rtl}: up wraps");
            assert_eq!(walk(Key::Up), Some(second), "rtl {rtl}");
            assert_eq!(walk(Key::Left), Some(second), "rtl {rtl}: no item beside");
        }
    }

    #[test]
    fn enter_space_and_a_click_activate_a_menu_item() {
        let (mut ui, [_, first, second, third]) = menu(false);
        ui.set_focus(Some(second));
        assert_eq!(ui.key(Key::Enter), Some(Event::Clicked(second)));
        ui.set_focus(Some(third));
        assert_eq!(ui.key(Key::Text(" ".into())), Some(Event::Clicked(third)));
        let r = ui.widget(first).rect;
        assert_eq!(ui.click(r.x + 1.0, r.y + 1.0), Some(Event::Clicked(first)));
        assert_eq!(ui.focus(), Some(first));
        // Every item spans the menu, so the pointer finds one anywhere across it.
        let widths: Vec<f32> = [first, second, third].map(|id| ui.widget(id).rect.w).into();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }

    /// A button with a popup and a plain button beside it.
    fn with_popup() -> (Ui, WidgetId, WidgetId) {
        let mut ui = Ui::new("Dock", Theme::default(), false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        let app = ui.add(row, Kind::Button("Files".into()));
        let other = ui.add(row, Kind::Button("Terminal".into()));
        ui.set_has_popup(app, true);
        ui.layout();
        (ui, app, other)
    }

    #[test]
    fn a_button_with_a_popup_is_collapsed_until_it_opens() {
        let (mut ui, app, other) = with_popup();
        assert!(ui.has_popup(app) && !ui.has_popup(other));
        assert!(!ui.is_expanded(app));
        ui.set_expanded(app, true);
        assert!(ui.is_expanded(app));
        ui.set_expanded(other, true);
        assert!(
            !ui.is_expanded(other),
            "a button without a popup never expands"
        );
        // Closing returns the focus to the button and collapses it, as GTK's popover does.
        ui.set_focus(Some(other));
        ui.popup_closed(app);
        assert_eq!((ui.focus(), ui.is_expanded(app)), (Some(app), false));
    }

    #[test]
    fn the_secondary_button_and_the_menu_key_ask_for_the_context_menu() {
        let (mut ui, app, other) = with_popup();
        let r = ui.widget(app).rect;
        let at = (r.x + 2.0, r.y + 2.0);
        assert_eq!(
            ui.pointer_press(at.0, at.1, PointerButton::Secondary),
            Some(Event::ContextMenu(app))
        );
        assert_eq!(
            ui.pointer_release(at.0, at.1, PointerButton::Secondary),
            None
        );
        ui.set_focus(Some(other));
        assert_eq!(ui.key(Key::Menu), Some(Event::ContextMenu(other)));
        ui.set_focus(None);
        assert_eq!(ui.key(Key::Menu), None);
    }

    #[test]
    fn the_primary_button_activates_on_release_over_the_pressed_widget() {
        let (mut ui, app, other) = with_popup();
        let (a, o) = (ui.widget(app).rect, ui.widget(other).rect);
        assert_eq!(
            ui.pointer_press(a.x + 1.0, a.y + 1.0, PointerButton::Primary),
            None
        );
        assert_eq!(
            ui.focus(),
            Some(app),
            "a press takes the focus, as GTK's buttons"
        );
        assert_eq!(
            ui.pointer_release(a.x + 3.0, a.y + 3.0, PointerButton::Primary),
            Some(Event::Clicked(app))
        );
        // Released over another widget: nothing is activated, as GTK's buttons.
        ui.pointer_press(a.x + 1.0, a.y + 1.0, PointerButton::Primary);
        assert_eq!(
            ui.pointer_release(o.x + 1.0, o.y + 1.0, PointerButton::Primary),
            None
        );
    }

    #[test]
    fn a_drag_starts_once_the_pointer_moves_past_gtks_threshold() {
        let (mut ui, app, other) = with_popup();
        let (a, o) = (ui.widget(app).rect, ui.widget(other).rect);
        let (x, y) = (a.x + 1.0, a.y + 1.0);
        ui.pointer_press(x, y, PointerButton::Primary);
        assert_eq!(
            ui.pointer_motion(x + Ui::DRAG_THRESHOLD, y),
            None,
            "8 px is not past"
        );
        assert_eq!(ui.pointer_motion(x, y - Ui::DRAG_THRESHOLD), None);
        assert_eq!(
            ui.pointer_motion(x + Ui::DRAG_THRESHOLD + 0.5, y),
            Some(Event::DragStarted(app))
        );
        assert_eq!(ui.pointer_motion(x + 30.0, y), None, "it starts once");
        assert_eq!(
            ui.pointer_release(o.x + 1.0, o.y + 1.0, PointerButton::Primary),
            Some(Event::DragEnded {
                from: app,
                to: Some(other)
            }),
            "a drag never activates"
        );
        // A press on no widget drags nothing.
        ui.pointer_press(0.5, 0.5, PointerButton::Primary);
        assert_eq!(ui.pointer_motion(40.0, 40.0), None);
        assert_eq!(ui.pointer_release(40.0, 40.0, PointerButton::Primary), None);
    }

    #[test]
    fn a_leave_with_the_button_down_ends_the_press() {
        let (mut ui, app, _) = with_popup();
        let a = ui.widget(app).rect;
        let (x, y) = (a.x + 1.0, a.y + 1.0);
        // A grab starts with the button down: the release goes elsewhere.
        ui.pointer_press(x, y, PointerButton::Primary);
        assert_eq!(ui.pointer_leave(), None, "no drag had started");
        assert_eq!(
            ui.pointer_motion(x + 30.0, y),
            None,
            "no button is down: nothing drags"
        );
        assert_eq!(ui.pointer_release(x, y, PointerButton::Primary), None);
    }

    #[test]
    fn a_leave_during_a_drag_ends_it_over_no_widget() {
        let (mut ui, app, _) = with_popup();
        let a = ui.widget(app).rect;
        let (x, y) = (a.x + 1.0, a.y + 1.0);
        ui.pointer_press(x, y, PointerButton::Primary);
        assert_eq!(
            ui.pointer_motion(x + 30.0, y),
            Some(Event::DragStarted(app))
        );
        assert_eq!(
            ui.pointer_leave(),
            Some(Event::DragEnded {
                from: app,
                to: None
            })
        );
        assert_eq!(ui.pointer_motion(x, y), None, "it ends once");
        assert_eq!(ui.pointer_release(x, y, PointerButton::Primary), None);
    }

    #[test]
    fn the_hovered_widget_follows_the_pointer() {
        let (mut ui, app, other) = with_popup();
        let (a, o) = (ui.widget(app).rect, ui.widget(other).rect);
        assert_eq!(ui.hovered(), None);
        ui.pointer_motion(a.x + 1.0, a.y + 1.0);
        assert_eq!(ui.hovered(), Some(app));
        ui.pointer_motion(o.x + 1.0, o.y + 1.0);
        assert_eq!(ui.hovered(), Some(other));
        ui.pointer_motion(0.5, 0.5);
        assert_eq!(ui.hovered(), None);
        ui.pointer_motion(o.x + 1.0, o.y + 1.0);
        ui.pointer_leave();
        assert_eq!(ui.hovered(), None);
        // A hidden widget takes no pointer input, hover included.
        ui.pointer_motion(o.x + 1.0, o.y + 1.0);
        ui.set_visible(other, false);
        assert_eq!(ui.hovered(), None);
    }

    #[test]
    fn a_tooltip_is_the_description_unless_one_is_set() {
        let (mut ui, app, _) = with_popup();
        assert_eq!((ui.tooltip(app), ui.description(app)), (None, None));
        ui.set_tooltip(app, Some("Files"));
        assert_eq!(
            (ui.tooltip(app), ui.description(app)),
            (Some("Files"), Some("Files"))
        );
        ui.set_description(app, Some("Running"));
        assert_eq!(ui.description(app), Some("Running"));
        ui.set_description(app, None);
        ui.set_tooltip(app, None);
        assert_eq!((ui.tooltip(app), ui.description(app)), (None, None));
    }

    fn slider(min: f64, max: f64, step: f64, value: f64, rtl: bool) -> (Ui, WidgetId) {
        let mut ui = Ui::new("Slider", Theme::default(), rtl);
        let id = ui
            .slider(Ui::ROOT, "Level", min, max, step, value)
            .unwrap_or(Ui::ROOT);
        ui.layout();
        ui.set_focus(Some(id));
        (ui, id)
    }

    #[test]
    fn a_slider_refuses_a_broken_range() {
        let mut ui = Ui::new("Slider", Theme::default(), false);
        let (inf, nan) = (f64::INFINITY, f64::NAN);
        for (min, max, step, value) in [
            (0.0, 10.0, 0.0, 0.0),
            (0.0, 10.0, nan, 0.0),
            (0.0, 10.0, -1.0, 0.0),
            (10.0, 0.0, 1.0, 0.0),
            (5.0, 5.0, 1.0, 5.0),
            (-inf, 10.0, 1.0, 0.0),
            (0.0, inf, 1.0, 0.0),
            (0.0, 10.0, inf, 0.0),
            (0.0, 10.0, 1.0, nan),
            (0.0, 10.0, 1.0, inf),
        ] {
            assert_eq!(ui.slider(Ui::ROOT, "Level", min, max, step, value), None);
        }
        assert_eq!(ui.len(), 1, "a refused slider adds nothing");
    }

    #[test]
    fn a_slider_refuses_a_range_it_cannot_count_or_draw() {
        let mut ui = Ui::new("Slider", Theme::default(), false);
        // The span overflows, and the steps do not fit comfortably.
        assert_eq!(ui.slider(Ui::ROOT, "L", -1e308, 1e308, 1.0, 0.0), None);
        assert_eq!(ui.slider(Ui::ROOT, "L", 0.0, 100.0, 1e-300, 0.0), None);
        assert_eq!(ui.slider(Ui::ROOT, "L", 0.0, 1e12, 1.0, 0.0), None);
        assert_eq!(ui.len(), 1);
        // The most steps is still a slider, and its keys cannot overflow.
        let id = ui.slider(Ui::ROOT, "L", 0.0, 2e9, 1.0, 2e9);
        assert!(id.is_some());
        let id = id.unwrap_or(Ui::ROOT);
        assert_eq!(ui.step_value(id, i64::MAX), None);
        assert_eq!(ui.step_value(id, i64::MAX).or(ui.step_value(id, -1)), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(2e9 - 1.0));
    }

    #[test]
    fn a_slider_ends_on_max() {
        let (mut ui, id) = slider(0.0, 10.0, 3.0, 0.0, false);
        assert_eq!(ui.key(Key::End), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(10.0));
        assert_eq!(ui.key(Key::Left), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(9.0));
        assert_eq!(ui.key(Key::Right), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(10.0));
        assert_eq!(ui.set_value(id, 5.0), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(6.0));
        assert_eq!(ui.set_value(id, 9.6), Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(10.0));
    }

    #[test]
    fn a_slider_stays_on_its_steps_within_its_range() {
        let (mut ui, id) = slider(0.0, 10.0, 2.0, 0.0, false);
        for (set, value) in [(11.0, 10.0), (-3.0, 0.0), (3.1, 4.0), (f64::INFINITY, 10.0)] {
            ui.set_value(id, set);
            assert_eq!(ui.value(id), Some(value), "set to {set}");
        }
        ui.set_value(id, 4.0);
        assert_eq!(ui.set_value(id, f64::NAN), None);
        assert_eq!(ui.value(id), Some(4.0));
        // A value that rounds to where it is moves nothing.
        assert_eq!(ui.set_value(id, 4.2), None);

        for (key, moved, value) in [
            (Key::Right, true, 6.0),
            (Key::Up, true, 8.0),
            (Key::Left, true, 6.0),
            (Key::Down, true, 4.0),
            (Key::PageUp, true, 10.0),
            (Key::PageUp, false, 10.0),
            (Key::PageDown, true, 0.0),
            (Key::PageDown, false, 0.0),
            (Key::Left, false, 0.0),
            (Key::End, true, 10.0),
            (Key::End, false, 10.0),
            (Key::Home, true, 0.0),
            (Key::Home, false, 0.0),
        ] {
            let event = ui.key(key.clone());
            let expected = moved.then_some(Event::Changed(id));
            assert_eq!(event, expected, "{key:?}");
            assert_eq!(ui.value(id), Some(value), "{key:?}");
        }
        // Only a slider has a value.
        let label = ui.add(Ui::ROOT, Kind::Label("Label".into()));
        assert_eq!((ui.value(label), ui.set_value(label, 1.0)), (None, None));
    }

    #[test]
    fn a_mirrored_slider_follows_what_is_seen() {
        let seen = |rtl| {
            let (mut ui, id) = slider(0.0, 10.0, 1.0, 5.0, rtl);
            let r = ui.widget(id).rect;
            let font = ui.theme.font_size;
            let min = slider_knob_x(r, rtl, font, 0.0);
            let max = slider_knob_x(r, rtl, font, 1.0);
            assert_eq!(min < max, !rtl, "rtl {rtl}: min is at the start edge");
            // The pointer at the min end gives min, at the max end gives max.
            ui.click(min, r.y + r.h / 2.0);
            assert_eq!(ui.value(id), Some(0.0));
            ui.click(max, r.y + r.h / 2.0);
            assert_eq!(ui.value(id), Some(10.0));
            ui.set_value(id, 5.0);
            ui.key(Key::Right);
            ui.value(id)
        };
        assert_eq!(seen(false), Some(6.0));
        assert_eq!(seen(true), Some(4.0));
    }

    #[test]
    fn a_slider_follows_a_press_and_a_drag() {
        let (mut ui, id) = slider(0.0, 100.0, 1.0, 0.0, false);
        let r = ui.widget(id).rect;
        let (y, font) = (r.y + r.h / 2.0, ui.theme.font_size);
        let at = |f| slider_knob_x(r, false, font, f);
        let press = ui.pointer_press(at(0.5), y, PointerButton::Primary);
        assert_eq!(press, Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(50.0));
        // A drag keeps moving the knob, past the drag threshold and outside the widget, and
        // never starts a drag of its own.
        let moved = ui.pointer_motion(at(0.75), y + 40.0);
        assert_eq!(moved, Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(75.0));
        let moved = ui.pointer_motion(r.x + r.w + 50.0, y);
        assert_eq!(moved, Some(Event::Changed(id)));
        assert_eq!(ui.value(id), Some(100.0));
        assert_eq!(ui.pointer_release(at(1.0), y, PointerButton::Primary), None);
        assert_eq!(ui.pointer_motion(at(0.1), y), None, "the button is up");
        assert_eq!(ui.value(id), Some(100.0));
    }

    /// A button, a list of `rows` rows in a scroll `height` logical pixels high, and a button.
    fn long_list(rows: usize, height: f32, rtl: bool) -> (Ui, [WidgetId; 4], Vec<WidgetId>) {
        let mut ui = Ui::new("List", Theme::default(), rtl);
        let before = ui.add(Ui::ROOT, Kind::Button("Before".into()));
        let scroll = ui.scroll(Ui::ROOT, height);
        let list = ui.list(scroll, "Rows");
        let rows = (1..=rows)
            .map(|n| ui.list_row(list, &format!("Row {n}")))
            .collect();
        let after = ui.add(Ui::ROOT, Kind::Button("After".into()));
        ui.layout();
        (ui, [before, scroll, list, after], rows)
    }

    /// Whether `row` lies wholly inside the viewport of `scroll`.
    fn in_view(ui: &Ui, scroll: WidgetId, row: WidgetId) -> bool {
        let (view, r) = (ui.widget(scroll).rect, ui.widget(row).rect);
        r.y >= view.y && r.bottom() <= view.bottom()
    }

    #[test]
    fn the_focused_row_stays_in_view() {
        let (mut ui, [before, scroll, list, after], rows) = long_list(1000, 200.0, false);
        assert_eq!(ui.widget(scroll).rect.h, 200.0);
        let last = rows[999];
        ui.set_focus(Some(before));
        ui.key(Key::Tab);
        assert_eq!(ui.focus(), Some(rows[0]));
        assert!(!in_view(&ui, scroll, last));
        assert_eq!(ui.key(Key::End), Some(Event::Selected(last)));
        assert_eq!((ui.focus(), ui.selected(list)), (Some(last), Some(last)));
        assert!(in_view(&ui, scroll, last), "End scrolls to the last row");
        ui.layout();
        assert!(in_view(&ui, scroll, last), "a layout keeps the scroll");
        assert_eq!(ui.key(Key::Home), Some(Event::Selected(rows[0])));
        assert!(in_view(&ui, scroll, rows[0]), "Home scrolls back");

        // Tab out, scroll the selected row away, and Tab back in: it is in view again.
        ui.key(Key::End);
        assert_eq!((ui.key(Key::Tab), ui.focus()), (None, Some(after)));
        let view = ui.widget(scroll).rect;
        assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, -1e6));
        assert!(!in_view(&ui, scroll, last) && in_view(&ui, scroll, rows[0]));
        ui.key(Key::BackTab);
        assert_eq!(ui.focus(), Some(last));
        assert!(
            in_view(&ui, scroll, last),
            "Tab back in scrolls to the selected row"
        );
    }

    #[test]
    fn a_list_is_one_tab_stop_walked_by_arrows() {
        let (mut ui, [before, scroll, list, after], rows) = long_list(50, 200.0, false);
        assert_eq!(crate::focus::chain(&ui), [before, rows[0], after]);
        ui.set_focus(Some(before));
        ui.key(Key::Tab);
        assert_eq!(ui.focus(), Some(rows[0]), "Tab enters on the first row");
        assert_eq!(ui.selected(list), None, "focus alone selects nothing");
        assert_eq!(ui.key(Key::Down), Some(Event::Selected(rows[1])));
        assert_eq!(ui.key(Key::Down), Some(Event::Selected(rows[2])));
        assert_eq!(ui.key(Key::Up), Some(Event::Selected(rows[1])));
        assert_eq!(
            (ui.focus(), ui.selected(list)),
            (Some(rows[1]), Some(rows[1]))
        );

        // Page Down moves by a viewport: to the row a viewport below the selected row's middle.
        let r = ui.widget(rows[1]).rect;
        let below = r.y + r.h / 2.0 + ui.widget(scroll).rect.h;
        let paged = rows.iter().position(|&row| {
            let c = ui.widget(row).rect;
            c.y <= below && below < c.bottom()
        });
        let paged = paged.map_or(Ui::ROOT, |i| rows[i]);
        assert!(paged != rows[1] && paged != Ui::ROOT);
        assert_eq!(ui.key(Key::PageDown), Some(Event::Selected(paged)));
        assert!(in_view(&ui, scroll, paged));
        assert_eq!(ui.key(Key::PageUp), Some(Event::Selected(rows[1])));
        assert_eq!(ui.key(Key::PageUp), Some(Event::Selected(rows[0])));
        assert_eq!(ui.key(Key::PageUp), None, "nothing above the first row");

        // Tab leaves the list, and comes back to its selected row.
        ui.key(Key::Down);
        assert_eq!(crate::focus::chain(&ui), [before, rows[1], after]);
        ui.key(Key::Tab);
        assert_eq!(ui.focus(), Some(after));
        ui.key(Key::BackTab);
        assert_eq!(ui.focus(), Some(rows[1]));
        ui.set_focus(Some(before));
        ui.key(Key::Tab);
        assert_eq!(ui.focus(), Some(rows[1]), "Tab enters on the selected row");

        // At the ends, Up and Down leave the list as Shift+Tab and Tab, as GTK's ListBox does.
        assert_eq!(ui.key(Key::End), Some(Event::Selected(rows[49])));
        assert_eq!(ui.key(Key::End), None);
        assert_eq!((ui.key(Key::Down), ui.focus()), (None, Some(after)));
        ui.key(Key::BackTab);
        ui.key(Key::Home);
        assert_eq!((ui.key(Key::Up), ui.focus()), (None, Some(before)));
        assert_eq!(ui.selected(list), Some(rows[0]));

        // Enter and Space choose the focused row, as a click does.
        ui.key(Key::Tab);
        assert_eq!(ui.key(Key::Enter), Some(Event::Selected(rows[0])));
        let r = ui.widget(rows[2]).rect;
        assert_eq!(
            ui.click(r.x + 1.0, r.y + 1.0),
            Some(Event::Selected(rows[2]))
        );
        assert_eq!(
            (ui.focus(), ui.selected(list)),
            (Some(rows[2]), Some(rows[2]))
        );
    }

    #[test]
    fn up_and_down_leave_a_list_only_toward_a_widget() {
        // The list is the window's last widget: Down on its last row stays there, as GTK's
        // directional focus does not wrap round to the first widget.
        let mut ui = Ui::new("List", Theme::default(), false);
        let before = ui.add(Ui::ROOT, Kind::Button("Before".into()));
        let scroll = ui.scroll(Ui::ROOT, 200.0);
        let list = ui.list(scroll, "Rows");
        let rows: Vec<WidgetId> = (1..=20)
            .map(|n| ui.list_row(list, &format!("Row {n}")))
            .collect();
        ui.layout();
        ui.set_focus(Some(before));
        ui.key(Key::Tab);
        assert_eq!(ui.key(Key::End), Some(Event::Selected(rows[19])));
        assert_eq!((ui.key(Key::Down), ui.focus()), (None, Some(rows[19])));
        assert_eq!(ui.key(Key::Home), Some(Event::Selected(rows[0])));
        assert_eq!((ui.key(Key::Up), ui.focus()), (None, Some(before)));

        // The list is the first widget: Up on its first row stays there.
        let mut ui = Ui::new("List", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 200.0);
        let list = ui.list(scroll, "Rows");
        let first = ui.list_row(list, "Row 1");
        ui.list_row(list, "Row 2");
        let after = ui.add(Ui::ROOT, Kind::Button("After".into()));
        ui.layout();
        ui.set_focus(Some(first));
        assert_eq!((ui.key(Key::Up), ui.focus()), (None, Some(first)));
        ui.key(Key::End);
        assert_eq!((ui.key(Key::Down), ui.focus()), (None, Some(after)));
    }

    #[test]
    fn the_arrows_reach_a_list_whose_selected_row_is_scrolled_away() {
        let (mut ui, [before, scroll, _, after], rows) = long_list(50, 200.0, false);
        assert_eq!(ui.activate(rows[39]), Some(Event::Selected(rows[39])));
        let view = ui.widget(scroll).rect;
        assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, -1e6));
        assert!(!in_view(&ui, scroll, rows[39]));
        ui.set_focus(Some(before));
        ui.key(Key::Down);
        assert_eq!(ui.focus(), Some(rows[39]), "Down from above enters the list");
        assert!(in_view(&ui, scroll, rows[39]));

        // And from below, with the first row selected and the list scrolled to its end.
        ui.activate(rows[0]);
        ui.set_focus(Some(after));
        assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, 1e6));
        assert!(!in_view(&ui, scroll, rows[0]));
        ui.key(Key::Up);
        assert_eq!(ui.focus(), Some(rows[0]), "Up from below enters the list");
        assert!(in_view(&ui, scroll, rows[0]));
    }

    #[test]
    fn the_arrows_leave_a_list_whose_focused_row_is_scrolled_away() {
        let (mut ui, [before, scroll, _, after], rows) = long_list(50, 200.0, false);
        let view = ui.widget(scroll).rect;
        ui.set_focus(Some(rows[0]));
        assert_eq!(ui.key(Key::End), Some(Event::Selected(rows[49])));
        assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, -1e6));
        assert!(!in_view(&ui, scroll, rows[49]));
        assert_eq!((ui.key(Key::Down), ui.focus()), (None, Some(after)));

        ui.set_focus(Some(rows[49]));
        assert_eq!(ui.key(Key::Home), Some(Event::Selected(rows[0])));
        assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, 1e6));
        assert!(!in_view(&ui, scroll, rows[0]));
        assert_eq!((ui.key(Key::Up), ui.focus()), (None, Some(before)));
    }

    #[test]
    fn a_scroll_clips_its_content_and_follows_the_wheel() {
        for rtl in [false, true] {
            let (mut ui, [_, scroll, list, after], rows) = long_list(50, 200.0, rtl);
            let view = ui.widget(scroll).rect;
            let a = ui.widget(after).rect;
            assert!(
                a.y >= view.bottom(),
                "rtl {rtl}: the next widget is under the viewport"
            );
            // Rows run on under the viewport, where the pointer finds none of them.
            let hidden = rows
                .iter()
                .copied()
                .find(|&r| ui.widget(r).rect.y > view.bottom());
            assert!(hidden.is_some(), "rtl {rtl}: a row lies under the viewport");
            let h = ui.widget(hidden.unwrap_or(Ui::ROOT)).rect;
            assert!(!rows.contains(&ui.hit(h.x + 1.0, h.y + 1.0).unwrap_or(Ui::ROOT)));

            // The wheel scrolls the list under the pointer, and only there.
            assert!(!ui.pointer_axis(a.x + 1.0, a.y + 1.0, 40.0), "rtl {rtl}");
            assert!(
                ui.pointer_axis(view.x + 1.0, view.y + 1.0, 40.0),
                "rtl {rtl}"
            );
            assert_eq!(ui.widget(rows[0]).rect.y, view.y - 40.0, "rtl {rtl}");
            assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, -100.0), "rtl {rtl}");
            assert_eq!(
                ui.widget(rows[0]).rect.y,
                view.y,
                "rtl {rtl}: not above the top"
            );
            assert!(!ui.pointer_axis(view.x + 1.0, view.y + 1.0, -1.0), "rtl {rtl}");
            assert!(ui.pointer_axis(view.x + 1.0, view.y + 1.0, 1e6));
            let last = ui.widget(rows[49]).rect;
            assert_eq!(
                last.bottom(),
                view.bottom(),
                "rtl {rtl}: not past the bottom"
            );
            assert!(!ui.pointer_axis(view.x + 1.0, view.y + 1.0, 10.0));
            ui.layout();
            assert_eq!(
                ui.widget(rows[49]).rect.bottom(),
                view.bottom(),
                "rtl {rtl}"
            );

            // A visible row takes a click; its list fills the width, its text at the start edge.
            let at = (last.x + 1.0, last.y + 1.0);
            assert_eq!(ui.click(at.0, at.1), Some(Event::Selected(rows[49])));
            assert_eq!(ui.selected(list), Some(rows[49]));
            let margin = ui.theme.margin;
            assert_eq!((view.x, view.w), (margin, ui.size().0 - 2.0 * margin));
        }
    }

    /// Whether the caret of the text area `id` lies inside the part of its text it shows, and
    /// the line the caret is on, counted from 0.
    fn caret_line(ui: &mut Ui, id: WidgetId) -> (bool, i32) {
        let Kind::TextArea(area) = &mut ui.widgets[id].kind else {
            return (false, -1);
        };
        let (view, line) = (area.view_height(&mut ui.text), area.line_height(&mut ui.text));
        let offset = area.offset;
        let Some(c) = area.caret(&mut ui.text, 1.5) else {
            return (false, -1);
        };
        let (top, bottom) = (c.y0 as f32 - offset, c.y1 as f32 - offset);
        let shown = top >= -0.5 && bottom <= view + 0.5;
        (shown, (c.y0 as f32 / line).round() as i32)
    }

    #[test]
    fn composing_scrolls_a_text_area_to_its_caret() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "1\n2\n3\n4\n5\n6", 3);
        ui.layout();
        ui.set_focus(Some(area));
        assert_eq!(caret_line(&mut ui, area), (false, 5));
        ui.compose("x", Some((1, 1)));
        assert_eq!(caret_line(&mut ui, area), (true, 5));
    }

    #[test]
    fn composing_with_a_hidden_caret_scrolls_to_the_preedit_end() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "1\n2\n3\n4\n5\n6", 3);
        ui.layout();
        ui.set_focus(Some(area));
        ui.compose(&"word ".repeat(40), None);
        let Kind::TextArea(field) = &mut ui.widgets[area].kind else {
            panic!("not a text area");
        };
        let most = field.layout(&mut ui.text).height() - field.view_height(&mut ui.text);
        let offset = field.offset;
        assert!(most > 0.0 && (offset - most).abs() < 0.5, "offset {offset} of {most}");
    }

    #[test]
    fn a_text_area_that_ends_a_composition_scrolls_back_into_its_text() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let name = ui.field(Ui::ROOT, "Name", "");
        let notes = ui.text_area(Ui::ROOT, "Notes", "a", 3);
        ui.layout();
        let offset = |ui: &Ui| match &ui.widgets[notes].kind {
            Kind::TextArea(area) => area.offset,
            _ => f32::NAN,
        };
        let preedit = "word ".repeat(60);
        // The area loses the focus, then its window loses the text input.
        for leave in [false, true] {
            ui.set_focus(Some(notes));
            ui.compose(&preedit, None);
            assert!(offset(&ui) > 0.0, "the preedit did not scroll");
            if leave {
                assert!(ui.end_composition());
            } else {
                ui.set_focus(Some(name));
            }
            assert_eq!(offset(&ui), 0.0, "leave {leave}");
        }
    }

    #[test]
    fn hiding_a_composing_field_ends_its_composition() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let name = ui.field(Ui::ROOT, "Name", "ab");
        let other = ui.field(Ui::ROOT, "Other", "");
        ui.layout();
        ui.set_focus(Some(name));
        ui.compose("x", Some((1, 1)));
        ui.set_visible(name, false);
        assert_eq!(ui.focus(), Some(other));
        let Kind::Field(field) = &ui.widgets[name].kind else {
            panic!("not a field");
        };
        assert!(!field.is_composing());
    }

    #[test]
    fn a_field_that_loses_the_focus_ends_its_composition() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let name = ui.field(Ui::ROOT, "Name", "ab");
        let notes = ui.text_area(Ui::ROOT, "Notes", "", 3);
        ui.layout();
        ui.set_focus(Some(name));
        ui.compose("x", Some((1, 1)));
        ui.set_focus(Some(notes));
        let Kind::Field(field) = &ui.widgets[name].kind else {
            panic!("not a field");
        };
        assert!(!field.is_composing());
        assert_eq!(field.shown(), "ab");
    }

    #[test]
    fn a_text_area_keeps_its_newlines_only() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "", 4);
        let field = ui.field(Ui::ROOT, "Name", "");
        ui.layout();
        let text = |ui: &Ui, id| ui.field_text(id).unwrap_or_default();

        // SetValue drops the controls but the newline, and the bidirectional formats.
        ui.set_field_text(area, "a\nb\u{202E}c\u{7}");
        assert_eq!(text(&ui, area), "a\nbc");
        ui.set_field_text(field, "a\nb\u{202E}c\u{7}");
        assert_eq!(text(&ui, field), "abc");

        // The input method's text keeps the newline, and the RLM as a typed one is kept;
        // Parley's other line breaks and the other controls go.
        ui.set_field_text(area, "");
        ui.set_focus(Some(area));
        assert_eq!(ui.commit("x\ny"), Some(Event::Edited(area)));
        ui.commit("\u{200F}x");
        ui.commit("a\u{2028}b\u{2029}c\t");
        assert_eq!(text(&ui, area), "x\ny\u{200F}xabc");
        ui.set_focus(Some(field));
        ui.set_field_text(field, "");
        ui.commit("x\ny\u{2028}z");
        assert_eq!(text(&ui, field), "xyz");
    }

    #[test]
    fn a_text_area_scrolls_to_its_caret() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Text", Theme::default(), rtl);
            let area = ui.text_area(Ui::ROOT, "Notes", "", 3);
            let after = ui.add(Ui::ROOT, Kind::Button("OK".into()));
            ui.layout();
            ui.set_focus(Some(area));
            assert_eq!(caret_line(&mut ui, area), (true, 0), "rtl {rtl}");

            // Enter breaks the line; the field follows the caret past its third line.
            for line in 1..=5 {
                assert_eq!(ui.key(Key::Enter), Some(Event::Edited(area)));
                assert_eq!(caret_line(&mut ui, area), (true, line), "rtl {rtl}");
            }
            assert_eq!(ui.field_text(area).as_deref(), Some("\n\n\n\n\n"));

            // Up and Down move a line at a time, and do not leave the field at its ends.
            for line in (0..5).rev() {
                assert_eq!(ui.key(Key::Up), None);
                assert_eq!(caret_line(&mut ui, area), (true, line), "rtl {rtl}");
            }
            assert_eq!((ui.key(Key::Up), ui.focus()), (None, Some(area)));
            assert_eq!(ui.key(Key::Down), None);
            assert_eq!(caret_line(&mut ui, area), (true, 1), "rtl {rtl}");

            // The wheel scrolls the text under the pointer, within it.
            let r = ui.widget(area).rect;
            assert!(ui.pointer_axis(r.x + 1.0, r.y + 1.0, 1e6), "rtl {rtl}");
            assert!(!ui.pointer_axis(r.x + 1.0, r.y + 1.0, 1.0), "rtl {rtl}");
            assert!(ui.pointer_axis(r.x + 1.0, r.y + 1.0, -1e6), "rtl {rtl}");

            // Text wraps at the field's width, and the field keeps its size.
            ui.set_field_text(area, &"word ".repeat(40));
            ui.layout();
            assert_eq!(ui.widget(area).rect, r, "rtl {rtl}");

            // Tab leaves, as a GTK TextView that does not take Tab.
            assert_eq!(ui.key(Key::Tab), None);
            assert_eq!(ui.focus(), Some(after), "rtl {rtl}");
        }
    }

    #[test]
    fn a_text_area_starts_at_the_end_of_its_text() {
        // As a GTK text view after gtk_text_buffer_set_text: the caret follows the text, where a
        // single-line field's stays at its start.
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "a\nb", 4);
        let field = ui.field(Ui::ROOT, "Name", "a");
        ui.layout();
        let text = |ui: &Ui, id| ui.field_text(id).unwrap_or_default();
        ui.set_focus(Some(area));
        ui.commit("c");
        assert_eq!(text(&ui, area), "a\nbc");
        ui.set_field_text(area, "x");
        ui.commit("y");
        assert_eq!(text(&ui, area), "xy");
        ui.set_focus(Some(field));
        ui.commit("b");
        assert_eq!(text(&ui, field), "ba");
    }
}
