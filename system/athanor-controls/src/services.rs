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
#[derive(Clone)]
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

impl Services {
    /// Starts every model on `handle`'s runtime, sharing the one set of bus connections.
    ///
    /// This registers no agent: NetworkManager takes one secret agent and BlueZ one default
    /// pairing agent per session, and those belong to the bar (doc_control_center.md). A
    /// surface that reuses the pages calls `start` alone and its pages work. A surface other
    /// than the bar does not pair yet: its Bluetooth page is [`bluetooth::Page::without_pairing`],
    /// because a pairing needs a bondable adapter and the bar's model clears `Pairable`.
    /// [`Services::register_bluetooth_guest_agent`] exists for the design that will let one;
    /// nothing calls it. A NetworkManager password is answered by the bar's agent when the bar
    /// runs.
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

    /// Registers the NetworkManager secret agent. Only the bar calls this.
    pub fn register_network_agent(&self) {
        // A send fails only when the model has ended, and then there is nothing to register.
        self.network.1.send(NetworkCommand::RegisterAgent).ok();
    }

    /// Registers the BlueZ pairing agent as the default. Only the bar calls this.
    pub fn register_bluetooth_agent(&self) {
        self.bluetooth.1.send(BluetoothCommand::RegisterAgent).ok();
    }

    /// Registers the BlueZ pairing agent without asking to be the default, and leaves the
    /// adapter's `Pairable` alone. For a surface other than the bar: BlueZ routes a pairing to
    /// the agent of the process that started it.
    pub fn register_bluetooth_guest_agent(&self) {
        self.bluetooth
            .1
            .send(BluetoothCommand::RegisterGuestAgent)
            .ok();
    }
}
