//! A picture on each output's background layer, as the wallpaper opens it (doc_session_daemons.md
//! SD6): anchored on all four edges, an exclusive zone of -1, no keyboard and an empty input
//! region, so the surface takes the size the compositor configures and the picture fills it
//! with "zoom". The rig's check of the full-size layer surface (`toolkit_picture_check.py`).

use athanor_sal::driver::{self, Anchor, Application, Layer, LayerOptions, OutputInfo, Shell};
use athanor_sal::{Event, Image, SurfaceId, Theme, Ui};
use std::process::ExitCode;

/// The namespace of the surfaces.
const PICTURE: &str = "athanor-sal-picture";

/// Four columns of colour on two rows, opaque.
const PIXELS: [[u8; 3]; 8] = [
    [0x81, 0x3d, 0x9c],
    [0x24, 0x1f, 0x31],
    [0xf6, 0xf5, 0xf4],
    [0xc0, 0xbf, 0xbc],
    [0x24, 0x1f, 0x31],
    [0x81, 0x3d, 0x9c],
    [0xc0, 0xbf, 0xbc],
    [0xf6, 0xf5, 0xf4],
];

struct Pictures(Image);

impl Application for Pictures {
    fn output(&mut self, _: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        let mut theme = Theme::default();
        theme.margin = 0.0;
        let mut ui = Ui::new(PICTURE, theme, false);
        ui.picture(Ui::ROOT, self.0.clone(), "");
        let mut options = LayerOptions::new(Layer::Background);
        options.namespace = PICTURE.into();
        options.anchor = Anchor::all();
        options.exclusive_zone = -1;
        options.input_region = Some(Vec::new());
        Some((ui, options))
    }

    fn event(&mut self, _: &mut Shell, _: SurfaceId, _: Event) {}
}

fn main() -> ExitCode {
    let rgba = PIXELS
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    let outcome = Image::from_rgba8(4, 2, rgba).and_then(|image| driver::run(Pictures(image)));
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-picture: {error}");
            ExitCode::FAILURE
        }
    }
}
