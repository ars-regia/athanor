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

/// The colours and sizes every widget draws with, in logical pixels. New fields are added over
/// time: start from [`Theme::default`] and set the fields that differ.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Theme {
    /// The window's background.
    pub background: Rgba,
    /// The fill of controls: buttons, window buttons, unselected toggles and the marks of
    /// unchecked checks and switches.
    pub surface: Rgba,
    /// The colour of text on the background.
    pub text: Rgba,
    /// The colour of the active toggle and of the primary marks.
    pub accent: Rgba,
    /// Text and marks drawn on the accent, and the sheet of what holds content: text fields,
    /// lists and the rows they fill, a slider's knob.
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
    /// The width of a text field, which clips its text, of a text area and of a slider.
    pub field_width: f32,
    /// The fill behind an island's children: Calmo's `bar` token, light variant.
    pub island: Rgba,
    /// The corner radius of an island's fill.
    pub island_radius: f32,
    /// Between an island's fill and its children.
    pub island_padding: f32,
    /// The smallest width and height of an icon button.
    pub button_min: f32,
    /// The part of a level beyond its full mark: Calmo's `warn` token, light variant.
    pub warning: Rgba,
    /// The font family text is set in, ahead of the system's sans-serif, as GTK's
    /// `font-family`. `None`, the default, keeps the system's sans-serif; a family the system
    /// lacks is skipped.
    pub font_family: Option<String>,
    /// The width of the outline of buttons and toggles, drawn in the border colour and in the
    /// accent for the active toggle, as GTK's `border` on a button. 0, the default, draws none.
    pub control_border: f32,
    /// The smallest height of a button or a toggle inside its outline, as GTK's `min-height`.
    /// 0, the default, lets its text and padding set it.
    pub control_min_height: f32,
    /// How the title bar draws its title and its buttons.
    pub title_bar_style: TitleBarStyle,
}

/// How the title bar the window draws itself shows its title and its buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TitleBarStyle {
    /// A regular title and round, filled buttons: the toolkit's own look.
    #[default]
    Round,
    /// A bold title and bare marks, filled only under the pointer: GTK's header bar.
    Bare,
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
            warning: Rgba::rgb(0xa3, 0x6a, 0x00),
            font_family: None,
            control_border: 0.0,
            control_min_height: 0.0,
            title_bar_style: TitleBarStyle::Round,
        }
    }
}
