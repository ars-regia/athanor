//! athanor-compositor: Athanor's Wayland compositor on Smithay (doc_own_stack.md, milestone
//! C1). Task 1 runs nested, on the winit backend, in a parent compositor.

#![forbid(unsafe_code)]

mod handlers;
mod input;
mod state;
mod winit;

use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use state::State;
use std::process::ExitCode;

pub type Error = Box<dyn std::error::Error>;

/// The output's scale from `--scale <factor>`, 1 without it.
fn scale(mut args: impl Iterator<Item = String>) -> Result<f64, Error> {
    let mut scale = 1.0;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scale" => {
                let value = args.next().ok_or("--scale needs a factor")?;
                scale = value
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && (1.0..=4.0).contains(s))
                    .ok_or_else(|| format!("--scale {value}: a factor from 1 to 4"))?;
            }
            other => return Err(format!("unknown argument {other}; usage: athanor-compositor [--scale <factor>]").into()),
        }
    }
    Ok(scale)
}

fn run() -> Result<(), Error> {
    let scale = scale(std::env::args().skip(1))?;
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let (backend, events) = smithay::backend::winit::init()?;
    let mut state = State::new(&mut event_loop, display, backend, scale)?;
    winit::listen(&event_loop.handle(), events)?;
    tracing::info!(socket = ?state.socket_name, scale, "listening");
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
    use super::scale;

    fn parse(args: &[&str]) -> Result<f64, String> {
        scale(args.iter().map(|a| a.to_string())).map_err(|e| e.to_string())
    }

    #[test]
    fn the_scale_is_one_by_default_and_bounded() {
        assert_eq!(parse(&[]), Ok(1.0));
        assert_eq!(parse(&["--scale", "1.5"]), Ok(1.5));
        for bad in ["0", "0.5", "-1", "NaN", "inf", "5", "x"] {
            assert!(parse(&["--scale", bad]).is_err(), "{bad}");
        }
        assert!(parse(&["--scale"]).is_err());
        assert!(parse(&["--other"]).is_err());
    }
}
