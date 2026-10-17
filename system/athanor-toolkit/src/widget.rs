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
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

pub enum Kind {
    /// Children stacked top to bottom, aligned to the start edge.
    Column,
    /// Children side by side in reading order: left to right, or right to left.
    Row,
    /// A column with an accessible name, for a heading and the controls it names.
    Group(String),
    Label(String),
    Button(String),
    /// A toggle button. The toggles that share a parent form a radio group: activating one
    /// releases the others.
    Toggle {
        label: String,
        active: bool,
    },
    Field(Box<TextField>),
}

pub struct Widget {
    pub kind: Kind,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub rect: Rect,
    pub(crate) text: Option<Layout<Brush>>,
}

impl Widget {
    pub fn is_focusable(&self) -> bool {
        matches!(
            self.kind,
            Kind::Button(_) | Kind::Toggle { .. } | Kind::Field(_)
        )
    }
}

/// What the user did, for the application to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Clicked(WidgetId),
    /// A toggle became the active one of its group.
    Selected(WidgetId),
    /// A field's committed text changed.
    Edited(WidgetId),
}

/// A key, already translated from the keyboard layout by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Enter,
    Backspace,
    Delete,
    Text(String),
}

/// One window's widgets, theme, reading direction and keyboard focus.
pub struct Ui {
    pub(crate) widgets: Vec<Widget>,
    pub title: String,
    pub theme: Theme,
    pub rtl: bool,
    pub(crate) focus: Option<WidgetId>,
    pub(crate) text: TextContext,
    size: (f32, f32),
}

impl Ui {
    pub fn new(title: &str, theme: Theme, rtl: bool) -> Self {
        Self {
            widgets: vec![Widget {
                kind: Kind::Column,
                parent: None,
                children: Vec::new(),
                rect: Rect::default(),
                text: None,
            }],
            title: title.to_owned(),
            theme,
            rtl,
            focus: None,
            text: TextContext::default(),
            size: (0.0, 0.0),
        }
    }

    pub const ROOT: WidgetId = 0;
    /// The longest text a field takes from outside the keyboard, in characters.
    pub const FIELD_MAX_CHARS: usize = 1024;

    /// Adds `kind` as the last child of `parent`.
    pub fn add(&mut self, parent: WidgetId, kind: Kind) -> WidgetId {
        let id = self.widgets.len();
        self.widgets.push(Widget {
            kind,
            parent: Some(parent),
            children: Vec::new(),
            rect: Rect::default(),
            text: None,
        });
        self.widgets[parent].children.push(id);
        id
    }

    /// Adds a text field named `name` for assistive technologies.
    pub fn field(&mut self, parent: WidgetId, name: &str, text: &str) -> WidgetId {
        let field = TextField::new(name, text, self.theme.font_size, self.rtl);
        self.add(parent, Kind::Field(Box::new(field)))
    }

    pub fn widget(&self, id: WidgetId) -> &Widget {
        &self.widgets[id]
    }

    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    pub fn is_active(&self, id: WidgetId) -> bool {
        matches!(self.widgets[id].kind, Kind::Toggle { active: true, .. })
    }

    pub fn field_text(&self, id: WidgetId) -> Option<String> {
        match &self.widgets[id].kind {
            Kind::Field(field) => Some(field.text()),
            _ => None,
        }
    }

    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// Moves the focus to `id`, or clears it with `None`; a widget that takes no focus leaves
    /// it where it is.
    pub fn set_focus(&mut self, id: Option<WidgetId>) {
        match id {
            Some(id) if !self.widgets[id].is_focusable() => {}
            _ => self.focus = id,
        }
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

    fn measure(&mut self, id: WidgetId) -> (f32, f32) {
        let t = &self.theme;
        let (pad, gap, size, field_width) = (t.padding, t.gap, t.font_size, t.field_width);
        let rtl = self.rtl;
        let children = self.widgets[id].children.clone();
        let sizes: Vec<(f32, f32)> = children.iter().map(|&c| self.measure(c)).collect();
        let spacing = gap * children.len().saturating_sub(1) as f32;
        let widget = &mut self.widgets[id];
        let (w, h) = match &mut widget.kind {
            Kind::Column | Kind::Group(_) => {
                let w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
                let h = sizes.iter().map(|s| s.1).sum::<f32>() + spacing;
                let edge = if id == Self::ROOT { 2.0 * pad } else { 0.0 };
                (w + edge, h + edge)
            }
            Kind::Row => {
                let w = sizes.iter().map(|s| s.0).sum::<f32>() + spacing;
                (w, sizes.iter().map(|s| s.1).fold(0.0, f32::max))
            }
            Kind::Label(text) => {
                let layout = self.text.layout(text, size, rtl);
                let wh = (layout.width(), layout.height());
                widget.text = Some(layout);
                wh
            }
            Kind::Button(text) | Kind::Toggle { label: text, .. } => {
                let layout = self.text.layout(text, size, rtl);
                let wh = (layout.width() + 2.0 * pad, layout.height() + pad);
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
            self.theme.padding
        } else {
            0.0
        };
        let gap = self.theme.gap;
        let outer = self.widgets[id].rect;
        let children = self.widgets[id].children.clone();
        match self.widgets[id].kind {
            Kind::Row => {
                let mut cx = if self.rtl { x + outer.w } else { x };
                for c in children {
                    let w = self.widgets[c].rect.w;
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
            Kind::Column | Kind::Group(_) => {
                let mut cy = y + edge;
                for c in children {
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
    /// its group. Either takes the focus.
    pub fn activate(&mut self, id: WidgetId) -> Option<Event> {
        self.set_focus(Some(id));
        match self.widgets[id].kind {
            Kind::Button(_) => Some(Event::Clicked(id)),
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
        (0..self.widgets.len())
            .find(|&id| self.widgets[id].is_focusable() && self.widgets[id].rect.contains(x, y))
    }

    pub fn click(&mut self, x: f32, y: f32) -> Option<Event> {
        self.hit(x, y).and_then(|id| self.activate(id))
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
            key => self.navigate(key),
        }
    }

    fn navigate(&mut self, key: Key) -> Option<Event> {
        let next = match key {
            Key::Tab => focus::step(self, false),
            Key::BackTab => focus::step(self, true),
            Key::Left | Key::Right | Key::Up | Key::Down => focus::arrow(self, &key),
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
            .filter(|c| !c.is_control())
            .take(Self::FIELD_MAX_CHARS)
            .collect();
        field.set_text(&text);
        Some(Event::Edited(id))
    }
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
}
