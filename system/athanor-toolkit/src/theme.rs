//! Colours and metrics. The toolkit does not read any desktop setting itself: the application
//! fills a [`Theme`] (Athanor's shell from `athanor-style`) and hands it to the [`crate::Ui`].

/// A straight-alpha sRGB colour, 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    /// An opaque colour from its red, green and blue channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }
}

/// The colours and sizes every widget draws with, in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The window's background.
    pub background: Rgba,
    /// The fill of controls: buttons, fields and unselected toggles.
    pub surface: Rgba,
    /// The colour of text on the background.
    pub text: Rgba,
    /// The colour of the active toggle and of the primary marks.
    pub accent: Rgba,
    /// Text and marks drawn on the accent.
    pub on_accent: Rgba,
    /// The outline of controls.
    pub border: Rgba,
    /// The title bar the window draws itself: Calmo's `bar` token, light variant.
    pub title_bar: Rgba,
    /// The focus ring around the focused widget.
    pub focus: Rgba,
    /// The size of text, in logical pixels.
    pub font_size: f32,
    /// The space between a control's edge and its content.
    pub padding: f32,
    /// Around the window's content, under the title bar.
    pub margin: f32,
    /// Between the children of a column: the window's sections.
    pub section_gap: f32,
    /// Between the children of a group or a row.
    pub gap: f32,
    /// The corner radius of controls.
    pub radius: f32,
    /// The thickness of the focus ring.
    pub focus_width: f32,
    /// The width of a text field, before its text widens it.
    pub field_width: f32,
    /// The fill behind an island's children: Calmo's `bar` token, light variant.
    pub island: Rgba,
    /// The corner radius of an island's fill.
    pub island_radius: f32,
    /// Between an island's fill and its children.
    pub island_padding: f32,
    /// The smallest width and height of an icon button.
    pub button_min: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Rgba::rgb(0xf6, 0xf5, 0xf4),
            surface: Rgba::rgb(0xe8, 0xe6, 0xe3),
            text: Rgba::rgb(0x24, 0x1f, 0x31),
            accent: Rgba::rgb(0x81, 0x3d, 0x9c),
            on_accent: Rgba::rgb(0xff, 0xff, 0xff),
            border: Rgba::rgb(0xc0, 0xbf, 0xbc),
            title_bar: Rgba::rgb(0xfb, 0xfc, 0xfd),
            focus: Rgba::rgb(0x81, 0x3d, 0x9c),
            font_size: 15.0,
            padding: 10.0,
            margin: 10.0,
            section_gap: 8.0,
            gap: 8.0,
            radius: 9.0,
            focus_width: 2.0,
            field_width: 240.0,
            island: Rgba::rgb(0xfb, 0xfc, 0xfd),
            island_radius: 10.0,
            island_padding: 4.0,
            button_min: 32.0,
        }
    }
}
