//! athanor-compositor: Athanor's Wayland compositor on Smithay (doc_own_stack.md, milestone
//! C1). It runs nested, on the winit backend, in a parent compositor, or headless, on virtual
//! outputs drawn off screen (OS6).

#![forbid(unsafe_code)]

#[cfg(test)]
mod fixture;
mod handlers;
mod headless;
mod input;
mod state;
mod winit;

use headless::VirtualOutput;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use state::State;
use std::path::PathBuf;
use std::process::ExitCode;

pub type Error = Box<dyn std::error::Error>;

const USAGE: &str = "usage: athanor-compositor [--scale <factor>]\n       \
    athanor-compositor --headless [--render-node <path>] [--output <width>x<height>[@<scale>]]...";

/// How the compositor runs, from its arguments.
#[derive(Debug, PartialEq)]
enum Options {
    /// In a window of a parent compositor, its output at `scale`.
    Nested { scale: f64 },
    /// On virtual outputs, drawn on a render node or, without one, on the CPU.
    Headless {
        node: Option<PathBuf>,
        outputs: Vec<VirtualOutput>,
    },
}

/// A scale from 1 to 4.
fn factor(value: &str) -> Result<f64, Error> {
    value
        .parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && (1.0..=4.0).contains(s))
        .ok_or_else(|| format!("{value}: a scale from 1 to 4").into())
}

/// `<width>x<height>[@<scale>]`, each side from 1 to 16384 pixels.
fn virtual_output(value: &str) -> Result<VirtualOutput, Error> {
    let (size, scale) = match value.split_once('@') {
        Some((size, scale)) => (size, factor(scale)?),
        None => (value, 1.0),
    };
    let side = |s: &str| {
        s.parse::<i32>()
            .ok()
            .filter(|n| (1..=16_384).contains(n))
            .ok_or_else(|| format!("--output {value}: <width>x<height>[@<scale>]"))
    };
    let (width, height) = size
        .split_once('x')
        .ok_or_else(|| format!("--output {value}: <width>x<height>[@<scale>]"))?;
    Ok(VirtualOutput {
        width: side(width)?,
        height: side(height)?,
        scale,
    })
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Options, Error> {
    let (mut headless, mut scale, mut node, mut outputs) = (false, None, None, Vec::new());
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--headless" => headless = true,
            "--scale" => scale = Some(factor(&value()?)?),
            "--render-node" => node = Some(PathBuf::from(value()?)),
            "--output" => outputs.push(virtual_output(&value()?)?),
            other => return Err(format!("unknown argument {other}\n{USAGE}").into()),
        }
    }
    match (headless, scale) {
        (false, scale) if node.is_none() && outputs.is_empty() => Ok(Options::Nested {
            scale: scale.unwrap_or(1.0),
        }),
        (true, None) => {
            if outputs.is_empty() {
                outputs.push(VirtualOutput {
                    width: 1920,
                    height: 1080,
                    scale: 1.0,
                });
            }
            Ok(Options::Headless { node, outputs })
        }
        _ => Err(format!(
            "--scale is for the nested compositor, --render-node and --output for --headless\n{USAGE}"
        )
        .into()),
    }
}

fn run() -> Result<(), Error> {
    let options = options(std::env::args().skip(1))?;
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let mut state = match &options {
        Options::Nested { scale } => winit::start(&mut event_loop, display, *scale)?,
        Options::Headless { node, outputs } => {
            headless::start(&mut event_loop, display, node.as_deref(), outputs)?
        }
    };
    let socket = state::listen(&event_loop.handle())?;
    tracing::info!(?socket, ?options, "listening");
    event_loop.run(None, &mut state, |_| {})?;
    Ok(())
}

fn main() -> ExitCode {
    if let Err(error) = tracing_subscriber::fmt().try_init() {
        eprintln!("athanor-compositor: no log: {error}");
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{options, Options, VirtualOutput};
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Result<Options, String> {
        options(args.iter().map(|a| a.to_string())).map_err(|e| e.to_string())
    }

    #[test]
    fn the_scale_is_one_by_default_and_bounded() {
        assert_eq!(parse(&[]), Ok(Options::Nested { scale: 1.0 }));
        assert_eq!(
            parse(&["--scale", "1.5"]),
            Ok(Options::Nested { scale: 1.5 })
        );
        for bad in ["0", "0.5", "-1", "NaN", "inf", "5", "x"] {
            assert!(parse(&["--scale", bad]).is_err(), "{bad}");
        }
        assert!(parse(&["--scale"]).is_err());
        assert!(parse(&["--other"]).is_err());
    }

    #[test]
    fn headless_takes_a_node_and_outputs_and_nothing_nested() {
        let output = |width, height, scale| VirtualOutput {
            width,
            height,
            scale,
        };
        assert_eq!(
            parse(&["--headless"]),
            Ok(Options::Headless {
                node: None,
                outputs: vec![output(1920, 1080, 1.0)]
            })
        );
        assert_eq!(
            parse(&[
                "--headless",
                "--render-node",
                "/dev/dri/renderD128",
                "--output",
                "1280x800",
                "--output",
                "800x600@1.5"
            ]),
            Ok(Options::Headless {
                node: Some(PathBuf::from("/dev/dri/renderD128")),
                outputs: vec![output(1280, 800, 1.0), output(800, 600, 1.5)]
            })
        );
        for bad in ["0x600", "800x", "800", "800x600@5", "99999x10", "x600@1"] {
            assert!(parse(&["--headless", "--output", bad]).is_err(), "{bad}");
        }
        assert!(parse(&["--headless", "--scale", "2"]).is_err());
        assert!(parse(&["--output", "800x600"]).is_err());
        assert!(parse(&["--render-node", "/dev/dri/renderD128"]).is_err());
    }
}
