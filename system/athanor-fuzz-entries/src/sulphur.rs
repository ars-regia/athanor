//! The requests of a Wayland client: any application writes what it likes on its socket.

use crate::Setup;

/// `athanor_sulphur::fuzz::run`: the input as one client's requests, to a compositor of its own.
pub fn protocol(data: &[u8]) -> Setup {
    athanor_sulphur::fuzz::run(data).map_err(|error| std::io::Error::other(error.to_string()))
}
