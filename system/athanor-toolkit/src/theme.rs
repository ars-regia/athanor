//! Colours and metrics. The toolkit does not read any desktop setting itself: the application
//! fills a [`Theme`] (Athanor's shell from `athanor-style`) and hands it to the [`crate::Ui`].

/// A straight-alpha sRGB colour, 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }
}

/// The colours and sizes every widget draws with, in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Rgba,
    pub surface: Rgba,
    pub text: Rgba,
    pub accent: Rgba,
    /// Text and marks drawn on the accent.
    pub on_accent: Rgba,
    pub border: Rgba,
    /// The focus ring around the focused widget.
    pub focus: Rgba,
    pub font_size: f32,
    pub padding: f32,
    pub gap: f32,
    pub radius: f32,
    pub focus_width: f32,
    pub field_width: f32,
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
            focus: Rgba::rgb(0x81, 0x3d, 0x9c),
            font_size: 15.0,
            padding: 10.0,
            gap: 8.0,
            radius: 9.0,
            focus_width: 2.0,
            field_width: 240.0,
        }
    }
}
