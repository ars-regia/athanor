//! The widget tree: a flat arena of widgets, laid out in logical pixels and mirrored in
//! right-to-left.

use crate::focus;
use crate::text::{Brush, TextContext, TextField};
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
    /// A single-line text field.
    Field(Box<TextField>),
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
    /// An image drawn as a square of `size` logical pixels, scaled from its own pixels. It takes
    /// no input.
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
                | Kind::Field(_)
                | Kind::WindowButton(_)
                | Kind::MenuItem(_)
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

    /// The text of the field `id`, or `None` if it is not a field.
    pub fn field_text(&self, id: WidgetId) -> Option<String> {
        match &self.widgets[id].kind {
            Kind::Field(field) => Some(field.text()),
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
    /// it where it is.
    pub fn set_focus(&mut self, id: Option<WidgetId>) {
        match id {
            Some(id) if !self.widgets[id].is_focusable() || !self.is_visible(id) => {}
            _ => self.focus = id,
        }
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
            self.focus = next.or(previous).copied();
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

    fn visible_children(&self, id: WidgetId) -> Vec<WidgetId> {
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
            Kind::Menu => {
                let w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
                (w, sizes.iter().map(|s| s.1).sum::<f32>())
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
            Kind::Button(text) | Kind::Toggle { label: text, .. } | Kind::MenuItem(text) => {
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
            Kind::Field(field) => {
                let line = field.layout(&mut self.text).height().max(size * 1.25);
                (field_width, line + pad)
            }
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
            Kind::Menu => {
                let mut cy = y;
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
                        Kind::Row | Kind::Group(_) | Kind::Menu | Kind::Switch { .. }
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
    /// its group, a check button or a switch turns over. Each takes the focus.
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
            _ => None,
        }
    }

    /// The focusable widget under the point, in logical pixels.
    pub fn hit(&self, x: f32, y: f32) -> Option<WidgetId> {
        (0..self.widgets.len()).find(|&id| {
            self.widgets[id].is_focusable()
                && self.widgets[id].rect.contains(x, y)
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
                None
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
    /// activates.
    pub fn key(&mut self, key: Key) -> Option<Event> {
        let focused = self.focus;
        if let Some(id) = focused {
            if let Kind::Field(field) = &mut self.widgets[id].kind {
                let cx = &mut self.text;
                match &key {
                    Key::Text(s) => field.insert(cx, s),
                    Key::Backspace => field.backspace(cx),
                    Key::Delete => field.delete(cx),
                    Key::Left => {
                        field.left(cx);
                        return None;
                    }
                    Key::Right => {
                        field.right(cx);
                        return None;
                    }
                    _ => return self.navigate(key),
                }
                return Some(Event::Edited(id));
            }
        }
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
            self.focus = next;
        }
        None
    }

    /// Shows the input method's preedit in the focused field.
    pub fn compose(&mut self, preedit: &str, cursor: Option<(usize, usize)>) {
        if let Some(Kind::Field(field)) = self.focus.map(|id| &mut self.widgets[id].kind) {
            field.compose(&mut self.text, preedit, cursor);
        }
    }

    /// Commits the input method's text into the focused field.
    pub fn commit(&mut self, text: &str) -> Option<Event> {
        let id = self.focus?;
        let Kind::Field(field) = &mut self.widgets[id].kind else {
            return None;
        };
        field.commit(&mut self.text, text);
        Some(Event::Edited(id))
    }

    /// Replaces a field's text, as an assistive technology's SetValue does.
    pub fn set_field_text(&mut self, id: WidgetId, text: &str) -> Option<Event> {
        let Kind::Field(field) = &mut self.widgets[id].kind else {
            return None;
        };
        let text: String = text
            .chars()
            .filter(|&c| !c.is_control() && !is_invisible_format(c))
            .take(Self::FIELD_MAX_CHARS)
            .collect();
        field.set_text(&text);
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
}
