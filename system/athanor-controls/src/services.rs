//! The models the pages draw, started once per process.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use athanor_services::battery::{self, BatteryCommand, BatteryState};
use athanor_services::bluetooth::{self, BluetoothCommand, BluetoothState, PairingRequest};
use athanor_services::media::{self, MediaCommand, MediaState};
use athanor_services::network::{self, NetworkCommand, NetworkState, PasswordPrompt};
use athanor_services::{audio, Buses};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

/// Each model as its `spawn` returns it: the state it publishes and the queue of commands it
/// takes. A send fails only when the model has ended.
pub struct Services {
    pub battery: (
        watch::Receiver<BatteryState>,
        mpsc::UnboundedSender<BatteryCommand>,
    ),
    pub network: (
        watch::Receiver<Option<NetworkState>>,
        mpsc::UnboundedSender<NetworkCommand>,
    ),
    pub bluetooth: (
        watch::Receiver<Option<BluetoothState>>,
        mpsc::UnboundedSender<BluetoothCommand>,
    ),
    pub audio: (
        watch::Receiver<audio::AudioState>,
        mpsc::UnboundedSender<audio::AudioCommand>,
    ),
    pub media: (
        watch::Receiver<MediaState>,
        mpsc::UnboundedSender<MediaCommand>,
    ),
    /// The sysfs directory the backlight is read from again when the battery page opens.
    pub backlight_root: PathBuf,
    /// The password prompts of NetworkManager: the first network page takes the receiver.
    pub password_prompts: Rc<RefCell<Option<mpsc::Receiver<PasswordPrompt>>>>,
    /// The pairing requests of BlueZ's agent: the first Bluetooth page takes the receiver.
    pub pairing_requests: Rc<RefCell<Option<mpsc::Receiver<PairingRequest>>>>,
}

impl Clone for Services {
    fn clone(&self) -> Services {
        Services {
            battery: self.battery.clone(),
            network: self.network.clone(),
            bluetooth: self.bluetooth.clone(),
            audio: self.audio.clone(),
            media: self.media.clone(),
            backlight_root: self.backlight_root.clone(),
            password_prompts: self.password_prompts.clone(),
            pairing_requests: self.pairing_requests.clone(),
        }
    }
}

impl Services {
    /// Starts every model on `handle`'s runtime, sharing the one set of bus connections.
    pub fn start(handle: &Handle, buses: &Buses, backlight_root: PathBuf) -> Services {
        let battery = battery::spawn(handle, buses.clone(), backlight_root.clone());
        let (network_state, network_commands, prompts) = network::spawn(handle, buses.clone());
        let (bluetooth_state, bluetooth_commands, requests) =
            bluetooth::spawn(handle, buses.clone());
        Services {
            battery,
            network: (network_state, network_commands),
            bluetooth: (bluetooth_state, bluetooth_commands),
            audio: audio::spawn(handle),
            media: media::spawn(handle, buses.clone()),
            backlight_root,
            password_prompts: Rc::new(RefCell::new(Some(prompts))),
            pairing_requests: Rc::new(RefCell::new(Some(requests))),
        }
    }
}
