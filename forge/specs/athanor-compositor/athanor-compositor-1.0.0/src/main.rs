//! athanor-compositor: Athanor's Wayland compositor on Smithay (doc_own_stack.md, milestone
//! C1). It runs nested, on the winit backend, in a parent compositor, headless, on virtual
//! outputs drawn off screen (OS6), or on the monitors of the seat's GPU through DRM and KMS.

// One function writes a client's wl_shm buffer through a raw pointer (`capture::copy_into`);
// one reads a socket option the safe crates lack (`identity::so_peerpidfd`).
#![deny(unsafe_code)]

mod bindings;
mod capture;
mod cursor;
mod grabs;
mod handlers;
mod headless;
mod identity;
mod idle_inhibit;
mod input;
mod input_method;
mod keyboard_group;
mod layer;
mod lock;
mod output_management;
mod output_power;
mod screen_filter;
mod session_calls;
mod shutdown;
mod state;
mod toplevel_state;
mod xkb_config;
mod udev;
mod winit;
mod workspace;
mod xwayland;

use headless::VirtualOutput;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use state::State;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub type Error = Box<dyn std::error::Error>;

const USAGE: &str = "usage: athanor-compositor [--scale <factor>]\n       \
    athanor-compositor --headless [--render-node <path>] [--output <width>x<height>[@<scale>]]...\n       \
    athanor-compositor --drm\n       \
    athanor-compositor --screen-filter-on";

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
    /// On the monitors of the seat's primary GPU, in a session of the seat.
    Drm,
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
    let (mut headless, mut drm, mut scale, mut node, mut outputs) =
        (false, false, None, None, Vec::new());
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--headless" => headless = true,
            "--drm" => drm = true,
            "--scale" => scale = Some(factor(&value()?)?),
            "--render-node" => node = Some(PathBuf::from(value()?)),
            "--output" => outputs.push(virtual_output(&value()?)?),
            other => return Err(format!("unknown argument {other}\n{USAGE}").into()),
        }
    }
    match (headless, scale) {
        (false, None) if drm && node.is_none() && outputs.is_empty() => Ok(Options::Drm),
        (false, scale) if !drm && node.is_none() && outputs.is_empty() => Ok(Options::Nested {
            scale: scale.unwrap_or(1.0),
        }),
        (true, None) if !drm => {
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
            "--scale is for the nested compositor, --render-node and --output for --headless, \
             --drm takes no other option\n{USAGE}"
        )
        .into()),
    }
}

fn run() -> Result<(), Error> {
    let options = options(std::env::args().skip(1))?;
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    // First, before any thread: see `shutdown::watch`.
    let signals = shutdown::watch(&event_loop)?;
    let display: Display<State> = Display::new()?;
    let mut state = match &options {
        Options::Nested { scale } => winit::start(&mut event_loop, display, *scale)?,
        Options::Headless { node, outputs } => {
            headless::start(&mut event_loop, display, node.as_deref(), outputs)?
        }
        Options::Drm => udev::start(&mut event_loop, display)?,
    };
    // The layouts of the file before the first client, which never sees the environment's.
    xkb_config::start(&mut state, &event_loop.handle());
    let socket = state::listen(&event_loop.handle())?;
    // Under the user manager, as `athanor-compositor.service` runs it, the satellite gets a
    // scope of its own (OS4).
    let managed = std::env::var_os("NOTIFY_SOCKET").is_some();
    let x11 = if Path::new(xwayland::SATELLITE).exists() {
        let program = PathBuf::from(xwayland::SATELLITE);
        let started = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| Error::from("XDG_RUNTIME_DIR is not set"))
            .and_then(|runtime| {
                let (tmp, runtime) = (Path::new("/tmp"), Path::new(&runtime));
                xwayland::start(&mut state, tmp, runtime, program, managed)
            });
        match started {
            Ok(display) => Some(display),
            Err(error) => {
                tracing::warn!(%error, "no X11 display: X11 applications cannot start");
                None
            }
        }
    } else {
        tracing::info!("xwayland-satellite is not installed: no X11 display");
        None
    };
    // `x11`, the display's number, is what the session's readiness announces as `DISPLAY`.
    tracing::info!(?socket, ?x11, ?options, "listening");
    let ran = event_loop.run(None, &mut state, State::refresh_idle_inhibit);
    // From here a second SIGTERM or SIGINT takes the default action: see `shutdown::watch`.
    event_loop.handle().remove(signals);
    ran?;
    match state.fatal.take() {
        Some(reason) => Err(reason.into()),
        None => Ok(()),
    }
}

fn main() -> ExitCode {
    // athanor-compositor-session's question, answered before any log or display.
    if std::env::args().skip(1).eq(["--screen-filter-on"]) {
        return screen_filter::answer();
    }
    // RUST_LOG adds to the log, as `smithay::backend::drm::compositor=trace` for where each
    // element of a frame goes; what it does not name stays at info. Its directives follow
    // `info`, and a later directive replaces an earlier one of the same target, so a bare level
    // in RUST_LOG replaces `info` too. (EnvFilter's own default applies only to an empty one.)
    let directives = match std::env::var(tracing_subscriber::EnvFilter::DEFAULT_ENV) {
        Ok(directives) => Ok(format!("info,{directives}")),
        Err(std::env::VarError::NotPresent) => Ok(String::from("info")),
        Err(error) => Err(error.to_string()),
    };
    let filter = directives.and_then(|directives| {
        tracing_subscriber::EnvFilter::try_new(directives).map_err(|error| error.to_string())
    });
    let (filter, malformed) = match filter {
        Ok(filter) => (filter, None),
        Err(error) => (tracing_subscriber::EnvFilter::new("info"), Some(error)),
    };
    if let Err(error) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        eprintln!("athanor-compositor: no log: {error}");
    }
    if let Some(error) = malformed {
        tracing::warn!(%error, "RUST_LOG is malformed: the log is at info");
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

// Test-only code comes last: `verify.py panics` stops reading a file where it begins.
#[cfg(test)]
mod fixture;

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

    #[test]
    fn drm_takes_no_other_option() {
        assert_eq!(parse(&["--drm"]), Ok(Options::Drm));
        for other in [
            &["--drm", "--headless"][..],
            &["--drm", "--scale", "2"],
            &["--drm", "--output", "800x600"],
            &["--drm", "--render-node", "/dev/dri/renderD128"],
        ] {
            assert!(parse(other).is_err(), "{other:?}");
        }
    }
}
