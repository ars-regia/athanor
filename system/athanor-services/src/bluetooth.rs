//! BlueZ as the Bluetooth module shows it (doc_bar.md, BR3; doc_control_center.md, CC3): the
//! adapter's power, the paired devices, the devices nearby while discovering, and pairing with
//! a confirmation. One model per process mirrors BlueZ, is its default agent, and publishes a
//! [`BluetoothState`]; the shell's surfaces only draw it. Device names come from the radio:
//! they are sanitised and bounded here.
//!
//! The agent answers only the current owner of `org.bluez` (the mirror follows the owner
//! across a BlueZ restart), and only about the device whose pairing the person started from
//! the shell: every other request is rejected before anything shows. It never types a code:
//! the capability is DisplayYesNo, so the person compares six digits on both screens. A code
//! is shown to the person and never logged.
//!
//! The adapter is bondable only while that pairing runs. Registering an agent makes BlueZ set
//! the adapter bondable (`adapter_set_io_capability`), and a bondable adapter accepts a remote
//! device's bonding request. Where neither side asks for MITM protection, the kernel confirms
//! that request itself, asking no agent, whenever the local IO capability is NoInputNoOutput
//! (`hci_user_confirm_request_evt`, `smp.c` alike), and any process may register the default
//! agent that sets it. `Pairable` false clears the kernel's bondable flag
//! (`MGMT_OP_SET_BONDABLE`), which refuses every bonding request the adapter did not start
//! (`hci_io_capa_request_evt`, `smp_cmd_pairing_req`) whoever the default agent is: the model
//! keeps it false outside a pairing, and turns it off again whenever BlueZ or another process
//! turns it on.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use athanor_unit::text::{line, NAME_CHARS};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio::time::timeout;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, Value};
use zbus::{Connection, DBusError, Proxy};

use crate::mirror::{self, Snapshot, Source, TIMEOUT};
use crate::props::{self, Objects, Props, PROPERTIES};
use crate::runtime::{Bus, Buses};

pub const BLUEZ: &str = "org.bluez";
/// BlueZ's object manager.
pub const ROOT: &str = "/";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
pub const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
pub const AGENT_MANAGER_PATH: &str = "/org/bluez";
pub const AGENT_IFACE: &str = "org.bluez.Agent1";
pub const AGENT_PATH: &str = "/os/athanor/Bar/BluezAgent";
/// The shell shows a passkey and asks yes or no; it never types one.
pub const CAPABILITY: &str = "DisplayYesNo";
pub const MAX_DEVICES: usize = 16;
const MAX_CODE_CHARS: usize = 16;
/// How long a request waits for the person: inside BlueZ's own 60 s, so BlueZ hears
/// `Canceled` from the agent rather than giving up on it.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(50);
/// `Pair` waits for the person, so it is allowed longer than any other call.
const PAIR_TIMEOUT: Duration = Duration::from_secs(120);
/// What the shell has not taken yet: a request is rejected rather than queued behind one.
const REQUESTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub label: String,
    pub icon: &'static str,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothState {
    pub adapter: String,
    pub powered: bool,
    pub discovering: bool,
    /// The adapter accepts bonding (BlueZ's `Pairable`, the kernel's bondable flag).
    pub pairable: bool,
    /// Other devices find this adapter (BlueZ's `Discoverable`).
    pub discoverable: bool,
    pub paired: Vec<Device>,
    /// Unpaired devices with a name, only while discovering.
    pub nearby: Vec<Device>,
    /// Counts the commands of the person that BlueZ refused or did not answer: the state is
    /// published again after each, so a reader that sees this change knows its last action
    /// did not complete. Beyond the plan's fields, so the bar keeps its "action did not
    /// complete" note. Always 0 in the value of [`state`]; the model fills it in.
    pub refused: u32,
}

/// BlueZ's `Icon` property, mapped to an icon the theme has; anything else is generic.
pub fn icon(bluez: &str) -> &'static str {
    match bluez {
        "audio-headphones" | "audio-headset" => "audio-headphones-symbolic",
        "audio-card" | "audio-speakers" => "audio-speakers-symbolic",
        "input-keyboard" => "input-keyboard-symbolic",
        "input-mouse" | "input-tablet" => "input-mouse-symbolic",
        "input-gaming" => "input-gaming-symbolic",
        "phone" => "phone-symbolic",
        "computer" => "computer-symbolic",
        _ => "bluetooth-symbolic",
    }
}

fn name(props: &Props, key: &str) -> Option<String> {
    props::get_str(props, key)
        .map(|text| line(text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
}

fn device(path: &str, props: &Props) -> Option<Device> {
    let paired = props::get_bool(props, "Paired").unwrap_or(false);
    // BlueZ fills `Alias` with the address when a device has no name: an unpaired device
    // needs a real `Name`, or the list fills with addresses.
    let label = if paired {
        name(props, "Alias").or_else(|| name(props, "Name"))
    } else {
        name(props, "Name")
    }?;
    Some(Device {
        path: path.to_owned(),
        label,
        icon: icon(props::get_str(props, "Icon").unwrap_or_default()),
        paired,
        connected: props::get_bool(props, "Connected").unwrap_or(false),
    })
}

/// The module's state; `None` without an adapter, and then the module hides. With several
/// adapters the first by path is shown.
pub fn state(objects: &Objects) -> Option<BluetoothState> {
    let (adapter, adapter_props) = objects
        .iter()
        .find_map(|(path, interfaces)| Some((path, interfaces.get(ADAPTER)?)))?;
    let discovering = props::get_bool(adapter_props, "Discovering").unwrap_or(false);
    let (mut paired, mut nearby): (Vec<Device>, Vec<Device>) = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let props = interfaces.get(DEVICE)?;
            (props::get_path(props, "Adapter").as_deref() == Some(adapter.as_str()))
                .then_some(())?;
            device(path, props)
        })
        .partition(|device| device.paired);
    paired.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.label.cmp(&b.label))
    });
    paired.truncate(MAX_DEVICES);
    if discovering {
        nearby.sort_by(|a, b| a.label.cmp(&b.label));
        nearby.truncate(MAX_DEVICES);
    } else {
        nearby.clear();
    }
    Some(BluetoothState {
        adapter: adapter.clone(),
        powered: props::get_bool(adapter_props, "Powered").unwrap_or(false),
        discovering,
        pairable: props::get_bool(adapter_props, "Pairable").unwrap_or(false),
        discoverable: props::get_bool(adapter_props, "Discoverable").unwrap_or(false),
        paired,
        nearby,
        refused: 0,
    })
}

pub fn module_icon(state: &BluetoothState) -> &'static str {
    if !state.powered {
        "bluetooth-disabled-symbolic"
    } else if state.paired.iter().any(|device| device.connected) {
        "bluetooth-active-symbolic"
    } else {
        "bluetooth-symbolic"
    }
}

/// A passkey as six digits; `None` beyond six digits, which Bluetooth never sends.
pub fn passkey_label(passkey: u32) -> Option<String> {
    (passkey <= 999_999).then(|| format!("{passkey:06}"))
}

/// A device's label for the confirmation page.
pub fn device_label(objects: &Objects, path: &str) -> Option<String> {
    let props = props::lookup(objects, path, DEVICE)?;
    name(props, "Alias").or_else(|| name(props, "Name"))
}

/// What the person can ask of BlueZ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothCommand {
    /// Registers the model's pairing agent with BlueZ as the default, now and again with every
    /// new owner of it. BlueZ lets one agent be the default, so the process that owns the
    /// pairing requests sends this and no other: a model that never gets it registers nothing.
    RegisterAgent,
    /// Registers the model's pairing agent with BlueZ without asking to be the default, for a
    /// second process that pairs devices the person picked in its own surface. BlueZ routes
    /// the pairing of a device to the agent of the process that called `Pair`, so the default
    /// agent stays the bar's. A model that got this command never writes the adapter's
    /// `Pairable`: that stays with the process that owns the default agent, and an outgoing
    /// `Pair` does not need it.
    RegisterGuestAgent,
    Power(bool),
    /// Other devices may find this adapter.
    Discoverable(bool),
    /// Look for devices nearby, for as long as a popover shows them. The model starts it
    /// again when the adapter powers on or BlueZ restarts under it. Never counted as refused:
    /// no one acts on a discovery that did not start.
    Discovery(bool),
    Connect(String),
    Disconnect(String),
    /// Pairs, then trusts and connects. A second `Pair` while one is queued or running is
    /// dropped.
    Pair(String),
    /// Removes the device (BlueZ's `RemoveDevice`).
    Forget(String),
}

/// The person's answer to a [`PairingRequest::Confirm`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingReply {
    Accept,
    Decline,
}

/// What the pairing agent asks the shell to show. A request nobody takes up is rejected, so
/// the shell takes them from the receiver `spawn` returns for as long as it runs.
#[derive(Debug)]
pub enum PairingRequest {
    /// Both devices show `code`: pair only if they agree. No reply within the timeout is
    /// `org.bluez.Error.Canceled` to BlueZ, and a dropped `reply` is a decline.
    Confirm {
        label: String,
        code: String,
        reply: oneshot::Sender<PairingReply>,
    },
    /// A code to type on the other device; nothing to answer.
    Display { label: String, code: String },
    /// BlueZ, or the loss of BlueZ, ended the pairing the page shows: close it.
    Withdrawn,
}

/// What the agent and the model share.
struct Shared {
    /// The device the person pressed to pair, until `Pair` returns.
    pairing: Mutex<Option<String>>,
    cancel: Notify,
    /// A request is waiting for the person.
    busy: AtomicBool,
    /// The model is a guest (`RegisterGuestAgent`): it never writes the adapter's `Pairable`.
    guest: AtomicBool,
    /// The model was told it owns the default agent (`RegisterAgent`): only then does it own
    /// the adapter's `Pairable`, also on exit.
    owner: AtomicBool,
    /// The shell was sent a page and has not been told to close it.
    shown: AtomicBool,
    requests: mpsc::Sender<PairingRequest>,
}

impl Shared {
    fn pairing(&self) -> Option<String> {
        self.pairing.lock().ok().and_then(|pairing| pairing.clone())
    }

    fn set_pairing(&self, device: Option<String>) {
        if let Ok(mut pairing) = self.pairing.lock() {
            *pairing = device;
        }
    }

    /// Ends the pairing of `device` unless a newer owner of BlueZ already cleared it and let
    /// another start; true when no pairing is left.
    fn finish_pairing(&self, device: &str) -> bool {
        match self.pairing.lock() {
            Ok(mut pairing) => {
                if pairing.as_deref() == Some(device) {
                    *pairing = None;
                }
                pairing.is_none()
            }
            Err(_) => true,
        }
    }

    /// Ends the request in progress, and tells the shell to close its page.
    fn withdraw(&self) {
        self.cancel.notify_waiters();
        if self.shown.swap(false, Ordering::SeqCst) {
            // A full queue or a shell that is gone: nothing is left to close.
            self.requests.try_send(PairingRequest::Withdrawn).ok();
        }
    }
}

/// Clears `busy` however the request ends.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "org.bluez.Error", impl_display = true)]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

fn rejected(why: &str) -> AgentError {
    AgentError::Rejected(why.to_owned())
}

/// `org.bluez.Agent1`, served on the system connection.
struct Agent {
    bluez: watch::Receiver<Snapshot>,
    shared: Arc<Shared>,
    timeout: Duration,
}

impl Agent {
    /// Only the unique name that owns `org.bluez` now may call the agent.
    fn require_bluez(&self, header: &Header<'_>, method: &str) -> Result<(), AgentError> {
        let sender = header.sender().map(|sender| sender.as_str());
        if sender.is_some() && sender == self.bluez.borrow().owner.as_deref() {
            return Ok(());
        }
        tracing::warn!(
            method,
            sender = sender.unwrap_or(""),
            "a Bluetooth agent call from a process that is not BlueZ was refused"
        );
        Err(rejected("only BlueZ may call this agent"))
    }

    /// Only the device whose pairing the person started from the shell.
    fn started_here(&self, device: &ObjectPath<'_>, method: &str) -> Result<(), AgentError> {
        if self.shared.pairing().as_deref() == Some(device.as_str()) {
            return Ok(());
        }
        tracing::info!(
            method,
            "a Bluetooth request the person did not start from the shell was rejected"
        );
        Err(rejected(
            "the person did not start this pairing from the shell",
        ))
    }

    fn label(&self, device: &ObjectPath<'_>) -> Option<String> {
        device_label(&self.bluez.borrow().objects, device.as_str())
    }

    fn show(&self, request: PairingRequest) -> Result<(), AgentError> {
        self.shared.shown.store(true, Ordering::SeqCst);
        self.shared
            .requests
            .try_send(request)
            .map_err(|_| rejected("the shell shows no Bluetooth module"))
    }

    async fn confirm(&self, label: String, code: String) -> Result<(), AgentError> {
        if self.shared.busy.swap(true, Ordering::SeqCst) {
            return Err(rejected("another request is open"));
        }
        let _busy = Busy(&self.shared.busy);
        let (reply, answer) = oneshot::channel();
        let cancel = self.shared.cancel.notified();
        tokio::pin!(cancel);
        cancel.as_mut().enable();
        self.show(PairingRequest::Confirm { label, code, reply })?;
        let outcome = tokio::select! {
            answer = timeout(self.timeout, answer) => Some(answer),
            () = &mut cancel => None,
        };
        match outcome {
            Some(Ok(Ok(PairingReply::Accept))) => Ok(()),
            // A dropped reply is the shell going away with the page open: no.
            Some(Ok(Ok(PairingReply::Decline) | Err(_))) => Err(rejected("the person declined")),
            Some(Err(_)) => {
                self.shared.withdraw();
                Err(AgentError::Canceled("no answer in time".to_owned()))
            }
            None => Err(AgentError::Canceled("the request was withdrawn".to_owned())),
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self, #[zbus(header)] header: Header<'_>) -> Result<(), AgentError> {
        self.require_bluez(&header, "Release")
    }

    fn cancel(&self, #[zbus(header)] header: Header<'_>) -> Result<(), AgentError> {
        self.require_bluez(&header, "Cancel")?;
        self.shared.withdraw();
        Ok(())
    }

    fn request_pin_code(
        &self,
        _device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, AgentError> {
        self.require_bluez(&header, "RequestPinCode")?;
        Err(rejected("this agent does not type codes"))
    }

    fn request_passkey(
        &self,
        _device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<u32, AgentError> {
        self.require_bluez(&header, "RequestPasskey")?;
        Err(rejected("this agent does not type codes"))
    }

    fn display_pin_code(
        &self,
        device: ObjectPath<'_>,
        pincode: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_bluez(&header, "DisplayPinCode")?;
        self.started_here(&device, "DisplayPinCode")?;
        let code: String = pincode
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(MAX_CODE_CHARS)
            .collect();
        if let Some(label) = self.label(&device) {
            self.show(PairingRequest::Display { label, code })?;
        }
        Ok(())
    }

    fn display_passkey(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        _entered: u16,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_bluez(&header, "DisplayPasskey")?;
        self.started_here(&device, "DisplayPasskey")?;
        if let (Some(label), Some(code)) = (self.label(&device), passkey_label(passkey)) {
            self.show(PairingRequest::Display { label, code })?;
        }
        Ok(())
    }

    async fn request_confirmation(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_bluez(&header, "RequestConfirmation")?;
        self.started_here(&device, "RequestConfirmation")?;
        let (Some(label), Some(code)) = (self.label(&device), passkey_label(passkey)) else {
            return Err(rejected("unknown device or invalid passkey"));
        };
        self.confirm(label, code).await
    }

    // The guard admits only the device of the pairing in progress.
    fn request_authorization(
        &self,
        device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_bluez(&header, "RequestAuthorization")?;
        self.started_here(&device, "RequestAuthorization")
    }

    fn authorize_service(
        &self,
        device: ObjectPath<'_>,
        _uuid: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_bluez(&header, "AuthorizeService")?;
        self.started_here(&device, "AuthorizeService")
    }
}

/// Starts the model on `handle`'s runtime. The state receiver holds `None` until BlueZ has an
/// adapter, and ends when the model does: without the system bus it never leaves `None`.
/// Commands wait in an unbounded queue and run one at a time; a send fails only when the model
/// has ended. Take the pairing requests from the last receiver, or pairing from the shell
/// fails.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
) -> (
    watch::Receiver<Option<BluetoothState>>,
    mpsc::UnboundedSender<BluetoothCommand>,
    mpsc::Receiver<PairingRequest>,
) {
    start(handle, buses, REPLY_TIMEOUT)
}

fn start(
    handle: &Handle,
    buses: Buses,
    reply_timeout: Duration,
) -> (
    watch::Receiver<Option<BluetoothState>>,
    mpsc::UnboundedSender<BluetoothCommand>,
    mpsc::Receiver<PairingRequest>,
) {
    let (state, rx) = watch::channel(None);
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (requests, request_rx) = mpsc::channel(REQUESTS);
    let model = handle.clone();
    handle.spawn(async move {
        let connection = match buses.connection(Bus::System).await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!(error = %err, "no system bus; the Bluetooth module is hidden");
                return;
            }
        };
        run(
            &model,
            connection,
            state,
            command_rx,
            requests,
            reply_timeout,
        )
        .await;
    });
    (rx, commands, request_rx)
}

/// One step of the queue.
#[derive(Clone)]
enum Job {
    Command(BluetoothCommand),
    /// Sets the adapter's `Pairable`, so that it is on only while the shell pairs.
    Pairable(bool),
    /// `RegisterAgent` and, for the default agent, `RequestDefaultAgent`, to this unique name.
    Register(String, bool),
}

impl Job {
    fn is_pair(&self) -> bool {
        matches!(self, Job::Command(BluetoothCommand::Pair(_)))
    }

    /// Whether a failure is the person's action not completing.
    fn counts(&self) -> bool {
        matches!(self, Job::Command(command) if !matches!(command, BluetoothCommand::Discovery(_)))
    }
}

/// A job being carried out.
type Running = Pin<Box<dyn Future<Output = zbus::Result<()>> + Send>>;

async fn run(
    handle: &Handle,
    connection: Connection,
    state: watch::Sender<Option<BluetoothState>>,
    mut commands: mpsc::UnboundedReceiver<BluetoothCommand>,
    requests: mpsc::Sender<PairingRequest>,
    reply_timeout: Duration,
) {
    let mut bluez = mirror::spawn(handle, connection.clone(), BLUEZ, Source::Managed(ROOT));
    let shared = Arc::new(Shared {
        pairing: Mutex::new(None),
        cancel: Notify::new(),
        busy: AtomicBool::new(false),
        guest: AtomicBool::new(false),
        owner: AtomicBool::new(false),
        shown: AtomicBool::new(false),
        requests,
    });
    let agent = Agent {
        bluez: bluez.clone(),
        shared: shared.clone(),
        timeout: reply_timeout,
    };
    // The exit guard below removes it when the model ends.
    if let Err(err) = connection.object_server().at(AGENT_PATH, agent).await {
        tracing::error!(error = %err, "cannot export the Bluetooth agent; pairing will fail");
    }
    let mut refused = 0;
    let mut waiting: VecDeque<Job> = VecDeque::new();
    let mut running: Option<(Job, Running)> = None;
    let mut seen = 0;
    let mut was_powered = false;
    // Whether `RegisterAgent` came, and the generation of the owner the agent was queued for.
    let mut registering = false;
    let mut registered_for: Option<u64> = None;
    let mut had_adapter = false;
    let mut discovery = false;
    // The value of `Pairable` last asked of BlueZ and not yet seen to change anything: asked
    // again only after the mirror changes, so a BlueZ that refuses is not asked in a loop.
    let mut pairable_tried: Option<bool> = None;
    // The running Pair was cancelled by the model at the person's request: its failure is
    // no refusal.
    let mut cancelled = false;
    let mut exit = ExitGuard {
        handle: handle.clone(),
        connection: connection.clone(),
        shared: shared.clone(),
        adapter: None,
    };
    loop {
        // The mirror ends only after it has emptied itself, so its last state is published too.
        let mut alive = true;
        tokio::select! {
            changed = bluez.changed() => {
                alive = changed.is_ok();
                pairable_tried = None;
            }
            command = commands.recv() => match command {
                Some(BluetoothCommand::RegisterAgent) => {
                    shared.owner.store(true, Ordering::SeqCst);
                    registering = true;
                }
                Some(BluetoothCommand::RegisterGuestAgent) => {
                    shared.guest.store(true, Ordering::SeqCst);
                    registering = true;
                }
                Some(command) => {
                    // A pairing waits for the person for minutes: whatever would queue
                    // behind it and ends it anyway cancels it, outside the queue.
                    let paired_now = running.as_ref().is_some_and(|(job, _)| job.is_pair());
                    if let Some(device) = shared.pairing().filter(|_| paired_now) {
                        let ends_it = match &command {
                            BluetoothCommand::Power(on) => !on,
                            BluetoothCommand::Disconnect(other) | BluetoothCommand::Forget(other) => *other == device,
                            _ => false,
                        };
                        if ends_it {
                            cancelled = true;
                            handle.spawn(cancel_pairing(connection.clone(), device));
                        }
                    }
                    let job = Job::Command(command);
                    let busy = waiting.iter().chain(running.iter().map(|(job, _)| job)).any(Job::is_pair);
                    match &job {
                        Job::Command(BluetoothCommand::Discovery(on)) => {
                            discovery = *on;
                            waiting.retain(|queued| !matches!(queued, Job::Command(BluetoothCommand::Discovery(_))));
                            waiting.push_back(job);
                        }
                        _ if job.is_pair() && busy => {}
                        _ => waiting.push_back(job),
                    }
                }
                None => return,
            },
            result = async {
                match running.as_mut() {
                    Some((_, call)) => call.await,
                    None => std::future::pending().await,
                }
            }, if running.is_some() => {
                let counts = running.take().is_some_and(|(job, _)| job.counts() && !(job.is_pair() && cancelled));
                cancelled = false;
                if let Err(err) = result {
                    tracing::warn!(error = %err, "BlueZ refused or did not answer");
                    if counts {
                        refused += 1;
                    }
                }
            }
        }
        let (current, owner, generation) = {
            let snapshot = bluez.borrow();
            (
                state_of(&snapshot),
                snapshot.owner.clone(),
                snapshot.generation,
            )
        };
        if generation != seen {
            seen = generation;
            // A new owner of BlueZ knows nothing of the pairing, the agent or the discovery.
            shared.withdraw();
            shared.set_pairing(None);
            was_powered = false;
            waiting.retain(|job| !matches!(job, Job::Register(..) | Job::Pairable(_)));
            registered_for = None;
        }
        if registering && registered_for != Some(seen) {
            if let Some(owner) = owner {
                registered_for = Some(seen);
                waiting.push_back(Job::Register(owner, !shared.guest.load(Ordering::SeqCst)));
            }
        }
        if had_adapter && current.is_none() {
            shared.withdraw();
        }
        had_adapter = current.is_some();
        exit.adapter = current.as_ref().map(|state| state.adapter.clone());
        // bluetoothd stops discovery when the adapter powers off: start it again when the
        // adapter comes back on under an open popover.
        let powered = current.as_ref().is_some_and(|state| state.powered);
        if powered && !was_powered && discovery {
            waiting
                .retain(|queued| !matches!(queued, Job::Command(BluetoothCommand::Discovery(_))));
            waiting.push_back(Job::Command(BluetoothCommand::Discovery(true)));
        }
        was_powered = powered;
        // Runs after every wake with nothing queued: at the start, when an adapter appears,
        // when BlueZ comes back under a new owner (a bar that died during a pairing left it
        // true), and when registering the agent or another process turned it on.
        // Only a model that was told it owns the default agent keeps the adapter closed: one that
        // was not told yet, or is a guest, leaves `Pairable` to the bar.
        if running.is_none()
            && waiting.is_empty()
            && registering
            && !shared.guest.load(Ordering::SeqCst)
        {
            let wanted = shared.pairing().is_some();
            if pairable_tried != Some(wanted)
                && current
                    .as_ref()
                    .is_some_and(|state| state.pairable != wanted)
            {
                pairable_tried = Some(wanted);
                if !wanted {
                    tracing::info!("the Bluetooth adapter is bondable with no pairing from the shell; turning it off");
                }
                waiting.push_back(Job::Pairable(wanted));
            }
        }
        if running.is_none() {
            if let Some(job) = waiting.pop_front() {
                let context = Context {
                    connection: connection.clone(),
                    adapter: current.as_ref().map(|state| state.adapter.clone()),
                    powered,
                    shared: shared.clone(),
                };
                running = Some((job.clone(), Box::pin(execute(context, job))));
            }
        }
        let next = current.map(|state| BluetoothState { refused, ..state });
        state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
        if !alive {
            return;
        }
    }
}

/// Best effort: a pairing BlueZ cannot cancel ends at its own timeout.
async fn cancel_pairing(connection: Connection, device: String) {
    let call = async {
        let proxy = Proxy::new(&connection, BLUEZ, device.as_str(), DEVICE).await?;
        no_answer(timeout(TIMEOUT, proxy.call_method("CancelPairing", &())).await)
    };
    if let Err(err) = call.await {
        tracing::warn!(error = %err, "BlueZ did not cancel the pairing");
    }
}

/// However `run` ends, the agent stops answering and the adapter stops bonding: a pairing
/// future dropped half way would otherwise leave `pairing` set, the agent served and
/// `Pairable` true, and the agent would accept the device's authorisation requests.
struct ExitGuard {
    handle: Handle,
    connection: Connection,
    shared: Arc<Shared>,
    adapter: Option<String>,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.shared.set_pairing(None);
        let connection = self.connection.clone();
        // A model that registered no agent, or a guest, leaves `Pairable` to the bar.
        let guest = self.shared.guest.load(Ordering::SeqCst)
            || !self.shared.owner.load(Ordering::SeqCst);
        let adapter = self.adapter.take();
        self.handle.spawn(async move {
            // Nothing to remove when the agent never was served.
            connection.object_server().remove::<Agent, _>(AGENT_PATH).await.ok();
            let Some(adapter) = adapter.filter(|_| !guest) else { return };
            let set = async {
                let proxy = Proxy::new(&connection, BLUEZ, adapter.as_str(), PROPERTIES).await?;
                let body = (ADAPTER, "Pairable", Value::from(false));
                no_answer(timeout(TIMEOUT, proxy.call_method("Set", &body)).await)
            };
            if let Err(err) = set.await {
                tracing::warn!(error = %err, "the Bluetooth adapter could not be made unbondable on exit");
            }
        });
    }
}

fn state_of(snapshot: &Snapshot) -> Option<BluetoothState> {
    state(&snapshot.objects)
}

/// What a job needs of the world as it was when the job started.
struct Context {
    connection: Connection,
    adapter: Option<String>,
    powered: bool,
    shared: Arc<Shared>,
}

fn no_answer<T>(result: Result<zbus::Result<T>, tokio::time::error::Elapsed>) -> zbus::Result<T> {
    result.unwrap_or_else(|_| Err(zbus::Error::Failure("no answer in time".into())))
}

impl Context {
    fn adapter(&self) -> zbus::Result<&str> {
        self.adapter
            .as_deref()
            .ok_or_else(|| zbus::Error::Failure("no Bluetooth adapter".into()))
    }

    async fn call(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        limit: Duration,
    ) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, BLUEZ, path, interface).await?;
        no_answer(timeout(limit, proxy.call_method(method, &())).await).map(|_| ())
    }

    async fn set_property(
        &self,
        path: &str,
        interface: &str,
        property: &str,
        on: bool,
    ) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, BLUEZ, path, PROPERTIES).await?;
        let body = (interface, property, Value::from(on));
        no_answer(timeout(TIMEOUT, proxy.call_method("Set", &body)).await).map(|_| ())
    }

    async fn set_adapter(&self, property: &str, on: bool) -> zbus::Result<()> {
        self.set_property(self.adapter()?, ADAPTER, property, on)
            .await
    }
}

async fn execute(context: Context, job: Job) -> zbus::Result<()> {
    match job {
        Job::Command(command) => command_of(&context, command).await,
        Job::Pairable(on) => context.set_adapter("Pairable", on).await,
        Job::Register(owner, default) => register(&context.connection, &owner, default).await,
    }
}

async fn command_of(context: &Context, command: BluetoothCommand) -> zbus::Result<()> {
    match command {
        // Taken by the model's loop, which never queues it.
        BluetoothCommand::RegisterAgent | BluetoothCommand::RegisterGuestAgent => Ok(()),
        BluetoothCommand::Power(on) => context.set_adapter("Powered", on).await,
        BluetoothCommand::Discoverable(on) => context.set_adapter("Discoverable", on).await,
        BluetoothCommand::Discovery(on) => {
            if !context.powered {
                return Ok(());
            }
            let method = if on {
                "StartDiscovery"
            } else {
                "StopDiscovery"
            };
            context
                .call(context.adapter()?, ADAPTER, method, TIMEOUT)
                .await
        }
        BluetoothCommand::Connect(device) => {
            context.call(&device, DEVICE, "Connect", TIMEOUT).await
        }
        BluetoothCommand::Disconnect(device) => {
            context.call(&device, DEVICE, "Disconnect", TIMEOUT).await
        }
        BluetoothCommand::Forget(device) => {
            let proxy = Proxy::new(&context.connection, BLUEZ, context.adapter()?, ADAPTER).await?;
            let device = ObjectPath::try_from(device.as_str())?;
            no_answer(timeout(TIMEOUT, proxy.call_method("RemoveDevice", &(device,))).await)
                .map(|_| ())
        }
        BluetoothCommand::Pair(device) => pair(context, &device).await,
    }
}

/// Makes the adapter bondable, pairs, makes it unbondable again, then trusts and connects:
/// the order GNOME and COSMIC use. A pairing with an unbondable adapter would store no key.
/// Trusting lets the device reconnect later without asking again.
async fn pair(context: &Context, device: &str) -> zbus::Result<()> {
    context.adapter()?;
    context.shared.set_pairing(Some(device.to_owned()));
    let guest = context.shared.guest.load(Ordering::SeqCst);
    let opened = if guest {
        Ok(())
    } else {
        context.set_adapter("Pairable", true).await
    };
    let paired = match opened {
        Ok(()) => context.call(device, DEVICE, "Pair", PAIR_TIMEOUT).await,
        Err(err) => Err(err),
    };
    // Every way out of the pairing ends here: success, failure, Cancel, a declined page, BlueZ
    // leaving (the call fails). A new owner of BlueZ may have cleared `pairing` and let
    // another pairing start: that one is left alone.
    if context.shared.finish_pairing(device) && !guest {
        if let Err(err) = context.set_adapter("Pairable", false).await {
            tracing::warn!(error = %err, "the Bluetooth adapter could not be made unbondable again");
        }
    }
    paired?;
    if let Err(err) = context.set_property(device, DEVICE, "Trusted", true).await {
        tracing::warn!(error = %err, "the paired device could not be trusted");
    }
    context.call(device, DEVICE, "Connect", TIMEOUT).await
}

/// `RegisterAgent`, then `RequestDefaultAgent` when `default`, to this owner.
async fn register(connection: &Connection, owner: &str, default: bool) -> zbus::Result<()> {
    let proxy = Proxy::new(connection, owner, AGENT_MANAGER_PATH, AGENT_MANAGER).await?;
    let path = ObjectPath::try_from(AGENT_PATH)?;
    no_answer(
        timeout(
            TIMEOUT,
            proxy.call_method("RegisterAgent", &(&path, CAPABILITY)),
        )
        .await,
    )?;
    if !default {
        return Ok(());
    }
    no_answer(timeout(TIMEOUT, proxy.call_method("RequestDefaultAgent", &(&path,))).await)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::runtime::Handle;
    use zbus::message::Header;
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
    use zbus::{Connection, Proxy};

    use super::*;
    use crate::testbus::{self, TestBus};

    const ADAPTER_PATH: &str = "/org/bluez/hci0";
    const HEADSET: &str = "/org/bluez/hci0/dev_1";
    const PHONE: &str = "/org/bluez/hci0/dev_2";

    fn owned(value: Value<'_>) -> OwnedValue {
        value.try_to_owned().expect("owned value")
    }

    fn add(objects: &mut Objects, path: &str, interface: &str, entries: Vec<(&str, Value<'_>)>) {
        let props: Props = entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), owned(value)))
            .collect();
        objects
            .entry(path.to_owned())
            .or_default()
            .insert(interface.to_owned(), props);
    }

    fn add_device(objects: &mut Objects, path: &str, mut entries: Vec<(&str, Value<'_>)>) {
        entries.push((
            "Adapter",
            Value::from(ObjectPath::try_from(ADAPTER_PATH).expect("path")),
        ));
        add(objects, path, DEVICE, entries);
    }

    fn world(discovering: bool) -> Objects {
        let mut objects = Objects::new();
        add(
            &mut objects,
            ADAPTER_PATH,
            ADAPTER,
            vec![
                ("Powered", Value::from(true)),
                ("Discovering", Value::from(discovering)),
                ("Pairable", Value::from(true)),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_1",
            vec![
                ("Alias", Value::from("Keyboard")),
                ("Paired", Value::from(true)),
                ("Icon", Value::from("input-keyboard")),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_2",
            vec![
                ("Alias", Value::from("Headphones")),
                ("Paired", Value::from(true)),
                ("Connected", Value::from(true)),
                ("Icon", Value::from("audio-headset")),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_3",
            vec![
                ("Alias", Value::from("AA-BB")),
                ("Name", Value::from("Phone")),
                ("Icon", Value::from("phone")),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_4",
            vec![("Alias", Value::from("CC-DD"))],
        );
        objects
    }

    #[test]
    fn no_adapter_no_module() {
        assert_eq!(state(&Objects::new()), None);
    }

    #[test]
    fn connected_first_and_nearby_only_while_discovering() {
        let idle = state(&world(false)).unwrap();
        let labels: Vec<_> = idle.paired.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["Headphones", "Keyboard"]);
        assert_eq!(idle.paired[0].icon, "audio-headphones-symbolic");
        assert!(idle.nearby.is_empty());
        assert_eq!(module_icon(&idle), "bluetooth-active-symbolic");
        assert!(idle.pairable);
        let scanning = state(&world(true)).unwrap();
        let nearby: Vec<_> = scanning.nearby.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(nearby, ["Phone"], "a device with no name is not listed");
    }

    #[test]
    fn a_device_name_is_sanitised() {
        let mut objects = world(false);
        let long = format!("Evil\u{202e}{}\n", "x".repeat(200));
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_5",
            vec![
                ("Alias", Value::from(long)),
                ("Paired", Value::from(true)),
                ("Icon", Value::from("../../etc/passwd")),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_6",
            vec![
                ("Alias", Value::from("\u{1b}\u{7}")),
                ("Paired", Value::from(true)),
            ],
        );
        let paired = state(&objects).unwrap().paired;
        let evil = paired.iter().find(|d| d.path.ends_with("dev_5")).unwrap();
        assert!(evil.label.starts_with("Evilxxx"));
        assert_eq!(evil.label.chars().count(), NAME_CHARS);
        assert_eq!(evil.icon, "bluetooth-symbolic");
        assert!(
            paired.iter().all(|d| !d.path.ends_with("dev_6")),
            "an empty name is left out"
        );
    }

    #[test]
    fn a_device_of_another_adapter_is_not_listed() {
        let mut objects = world(false);
        add(
            &mut objects,
            "/org/bluez/hci1/dev_9",
            DEVICE,
            vec![
                ("Alias", Value::from("Other")),
                ("Paired", Value::from(true)),
                (
                    "Adapter",
                    Value::from(ObjectPath::try_from("/org/bluez/hci1").unwrap()),
                ),
            ],
        );
        assert!(state(&objects)
            .unwrap()
            .paired
            .iter()
            .all(|d| d.label != "Other"));
    }

    #[test]
    fn a_property_of_the_wrong_type_reads_as_absent_instead_of_panicking() {
        let mut objects = world(false);
        add(
            &mut objects,
            ADAPTER_PATH,
            ADAPTER,
            vec![
                ("Powered", Value::from("yes")),
                ("Discovering", Value::from(3u32)),
            ],
        );
        add_device(
            &mut objects,
            "/org/bluez/hci0/dev_7",
            vec![("Alias", Value::from(7u32)), ("Paired", Value::from("no"))],
        );
        let state = state(&objects).unwrap();
        assert!(!state.powered && !state.discovering && !state.pairable);
    }

    #[test]
    fn passkeys_are_six_digits() {
        assert_eq!(passkey_label(42).as_deref(), Some("000042"));
        assert_eq!(passkey_label(999_999).as_deref(), Some("999999"));
        assert_eq!(passkey_label(1_000_000), None);
        assert_eq!(
            device_label(&world(false), "/org/bluez/hci0/dev_3").as_deref(),
            Some("AA-BB")
        );
    }

    /// What the fake BlueZ saw, in order, and where the agent registered.
    #[derive(Default)]
    struct Log {
        /// The unique name and path of the agent that registered, and its capability.
        registered: Option<(String, String, String)>,
        calls: Vec<String>,
        /// The error name, or `ok`, that BlueZ got for each request it made of the agent.
        agent_results: Vec<String>,
        /// The adapter starts bondable, and refuses to change `Pairable`.
        start_pairable: bool,
        refuse_pairable: bool,
        /// `Pair` of a device waits for `CancelPairing` instead of asking the agent.
        hold_pair: bool,
        cancel: Arc<tokio::sync::Notify>,
    }

    type Shared = Arc<Mutex<Log>>;

    fn note(log: &Shared, line: String) {
        log.lock().unwrap().calls.push(line);
    }

    struct FakeAdapter {
        powered: bool,
        pairable: bool,
        discoverable: bool,
        log: Shared,
    }

    #[zbus::interface(name = "org.bluez.Adapter1")]
    impl FakeAdapter {
        fn start_discovery(&self) {
            note(&self.log, "StartDiscovery".into());
        }
        fn stop_discovery(&self) {
            note(&self.log, "StopDiscovery".into());
        }
        fn remove_device(&self, device: ObjectPath<'_>) {
            note(&self.log, format!("RemoveDevice {device}"));
        }
        #[zbus(property)]
        fn powered(&self) -> bool {
            self.powered
        }
        #[zbus(property)]
        fn set_powered(&mut self, on: bool) {
            self.powered = on;
        }
        #[zbus(property)]
        fn discovering(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn pairable(&self) -> bool {
            self.pairable
        }
        #[zbus(property)]
        fn set_pairable(&mut self, on: bool) -> zbus::fdo::Result<()> {
            note(&self.log, format!("Pairable {on}"));
            if self.log.lock().unwrap().refuse_pairable {
                return Err(zbus::fdo::Error::Failed("refused".into()));
            }
            self.pairable = on;
            Ok(())
        }
        #[zbus(property)]
        fn discoverable(&self) -> bool {
            self.discoverable
        }
        #[zbus(property)]
        fn set_discoverable(&mut self, on: bool) {
            self.discoverable = on;
        }
    }

    struct FakeDevice {
        path: &'static str,
        alias: &'static str,
        paired: bool,
        connected: bool,
        trusted: bool,
        log: Shared,
    }

    #[zbus::interface(name = "org.bluez.Device1")]
    impl FakeDevice {
        fn connect(&self) {
            note(&self.log, format!("Connect {}", self.path));
        }
        fn disconnect(&self) {
            note(&self.log, format!("Disconnect {}", self.path));
        }
        fn cancel_pairing(&self) {
            note(&self.log, format!("CancelPairing {}", self.path));
            let cancel = self.log.lock().unwrap().cancel.clone();
            cancel.notify_one();
        }
        /// Asks the agent to confirm 123456, as BlueZ does, and fails when it is refused.
        async fn pair(&self, #[zbus(connection)] connection: &Connection) -> zbus::fdo::Result<()> {
            let (registered, hold, cancel) = {
                let log = self.log.lock().unwrap();
                (log.registered.clone(), log.hold_pair, log.cancel.clone())
            };
            if hold {
                note(&self.log, format!("Pair started {}", self.path));
                cancel.notified().await;
                return Err(zbus::fdo::Error::Failed("canceled".into()));
            }
            let Some((agent, path, _)) = registered else {
                return Err(zbus::fdo::Error::Failed("no agent".into()));
            };
            let proxy = Proxy::new(connection, agent, path, AGENT_IFACE).await?;
            let result = proxy
                .call_method(
                    "RequestConfirmation",
                    &(ObjectPath::try_from(self.path).expect("path"), 123_456u32),
                )
                .await;
            let seen = match &result {
                Ok(_) => "ok".to_owned(),
                Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
                Err(err) => err.to_string(),
            };
            self.log.lock().unwrap().agent_results.push(seen);
            result
                .map(|_| ())
                .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))
        }
        #[zbus(property)]
        fn alias(&self) -> String {
            self.alias.to_owned()
        }
        #[zbus(property)]
        fn paired(&self) -> bool {
            self.paired
        }
        #[zbus(property)]
        fn connected(&self) -> bool {
            self.connected
        }
        #[zbus(property)]
        fn trusted(&self) -> bool {
            self.trusted
        }
        #[zbus(property)]
        fn set_trusted(&mut self, on: bool) {
            note(&self.log, format!("Trusted {on}"));
            self.trusted = on;
        }
        #[zbus(property)]
        fn icon(&self) -> String {
            "audio-headset".to_owned()
        }
        #[zbus(property)]
        fn adapter(&self) -> OwnedObjectPath {
            OwnedObjectPath::try_from(ADAPTER_PATH).expect("path")
        }
    }

    struct FakeAgentManager {
        log: Shared,
    }

    #[zbus::interface(name = "org.bluez.AgentManager1")]
    impl FakeAgentManager {
        fn register_agent(
            &self,
            agent: ObjectPath<'_>,
            capability: String,
            #[zbus(header)] header: Header<'_>,
        ) {
            let sender = header.sender().map(ToString::to_string).unwrap_or_default();
            self.log.lock().unwrap().registered = Some((sender, agent.to_string(), capability));
        }
        fn request_default_agent(&self, _agent: ObjectPath<'_>) {
            note(&self.log, "RequestDefaultAgent".into());
        }
    }

    /// BlueZ with an adapter, a paired and connected `Headset`, and an unpaired `Phone`.
    async fn serve(bus: &TestBus, log: &Shared) -> Connection {
        let device = |path, alias, paired| FakeDevice {
            path,
            alias,
            paired,
            connected: paired,
            trusted: false,
            log: log.clone(),
        };
        let build = bus
            .builder()
            .serve_at("/", zbus::fdo::ObjectManager)
            .expect("object manager")
            .serve_at("/org/bluez", FakeAgentManager { log: log.clone() })
            .expect("agent manager")
            .serve_at(
                ADAPTER_PATH,
                FakeAdapter {
                    powered: true,
                    pairable: log.lock().unwrap().start_pairable,
                    discoverable: false,
                    log: log.clone(),
                },
            )
            .expect("adapter")
            .serve_at(HEADSET, device(HEADSET, "Headset", true))
            .expect("headset")
            .serve_at(PHONE, device(PHONE, "Phone", false))
            .expect("phone")
            .name(BLUEZ)
            .expect("name")
            .build();
        tokio::time::timeout(testbus::WAIT, build)
            .await
            .expect("bluez connected in time")
            .expect("bluez")
    }

    type States = watch::Receiver<Option<BluetoothState>>;

    async fn wait_for(
        rx: &mut States,
        predicate: impl Fn(&BluetoothState) -> bool,
    ) -> BluetoothState {
        tokio::time::timeout(
            testbus::WAIT,
            rx.wait_for(|state| state.as_ref().is_some_and(&predicate)),
        )
        .await
        .expect("the model reached the expected state in time")
        .expect("the model is running")
        .clone()
        .expect("a state")
    }

    async fn until(log: &Shared, predicate: impl Fn(&Log) -> bool) {
        tokio::time::timeout(testbus::WAIT, async {
            while !predicate(&log.lock().unwrap()) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("BlueZ saw what the test waits for");
    }

    struct Rig {
        _bus: TestBus,
        bluez: Connection,
        client: Connection,
        log: Shared,
        states: States,
        commands: mpsc::UnboundedSender<BluetoothCommand>,
        requests: mpsc::Receiver<PairingRequest>,
    }

    async fn rig(reply_timeout: Duration) -> Rig {
        rig_with(reply_timeout, |_| {}).await
    }

    async fn rig_with(reply_timeout: Duration, setup: impl FnOnce(&mut Log)) -> Rig {
        let rig = started(reply_timeout, setup, true).await;
        until(&rig.log, |log| log.registered.is_some()).await;
        rig
    }

    async fn started(reply_timeout: Duration, setup: impl FnOnce(&mut Log), register: bool) -> Rig {
        let bus = TestBus::start();
        let log = Shared::default();
        setup(&mut log.lock().unwrap());
        let bluez = serve(&bus, &log).await;
        let client = bus.client().await;
        let (states, commands, requests) = start(
            &Handle::current(),
            Buses::with(client.clone()),
            reply_timeout,
        );
        if register {
            commands.send(BluetoothCommand::RegisterAgent).unwrap();
        }
        let mut rig = Rig {
            _bus: bus,
            bluez,
            client,
            log,
            states,
            commands,
            requests,
        };
        wait_for(&mut rig.states, |_| true).await;
        rig
    }

    async fn next_request(requests: &mut mpsc::Receiver<PairingRequest>) -> PairingRequest {
        tokio::time::timeout(testbus::WAIT, requests.recv())
            .await
            .expect("a request in time")
            .expect("the model is running")
    }

    /// The error name of `method` called on the agent of `rig`'s model from `caller`, `ok`
    /// when it answers, about `device`.
    async fn agent_call(
        client: &Connection,
        caller: &Connection,
        method: &str,
        device: &str,
    ) -> String {
        let name = client.unique_name().expect("unique name").to_string();
        let proxy = Proxy::new(caller, name, AGENT_PATH, AGENT_IFACE)
            .await
            .expect("proxy");
        let path = ObjectPath::try_from(device).expect("path");
        let result = match method {
            "RequestConfirmation" => proxy.call_method(method, &(path, 1u32)).await,
            "AuthorizeService" => proxy.call_method(method, &(path, "uuid")).await,
            _ => proxy.call_method(method, &()).await,
        };
        match result {
            Ok(_) => "ok".to_owned(),
            Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
            Err(err) => err.to_string(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_adapter_and_a_paired_device_are_published() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let state = wait_for(&mut rig.states, |state| !state.paired.is_empty()).await;
        assert_eq!(state.adapter, ADAPTER_PATH);
        assert!(state.powered && !state.discovering && !state.discoverable);
        assert_eq!(state.paired.len(), 1);
        assert_eq!(state.paired[0].label, "Headset");
        assert!(state.paired[0].connected);
        assert_eq!(state.refused, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_model_that_was_not_asked_registers_no_agent() {
        let rig = started(REPLY_TIMEOUT, |_| {}, false).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(rig.log.lock().unwrap().registered.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_agent_is_registered_with_bluez_as_the_default() {
        let rig = rig(REPLY_TIMEOUT).await;
        let (sender, path, capability) = rig.log.lock().unwrap().registered.clone().unwrap();
        assert_eq!(
            Some(sender.as_str()),
            rig.client.unique_name().map(|n| n.as_str())
        );
        assert_eq!(
            (path.as_str(), capability.as_str()),
            (AGENT_PATH, CAPABILITY)
        );
        until(&rig.log, |log| {
            log.calls.iter().any(|call| call == "RequestDefaultAgent")
        })
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_guest_agent_registers_but_never_asks_to_be_the_default() {
        let rig = started(REPLY_TIMEOUT, |_| {}, false).await;
        rig.commands
            .send(BluetoothCommand::RegisterGuestAgent)
            .unwrap();
        until(&rig.log, |log| log.registered.is_some()).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let log = rig.log.lock().unwrap();
        assert!(
            !log.calls.iter().any(|call| call == "RequestDefaultAgent"),
            "{:?}",
            log.calls
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_model_that_registered_no_agent_leaves_pairable_alone_on_exit() {
        let rig = started(REPLY_TIMEOUT, |log| log.start_pairable = true, false).await;
        let log = rig.log.clone();
        drop(rig.commands);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let log = log.lock().unwrap();
        assert!(
            !log.calls.iter().any(|call| call.starts_with("Pairable")),
            "{:?}",
            log.calls
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_guest_never_writes_pairable_not_even_to_pair() {
        // The adapter starts bondable: a default-agent model would turn it off at once.
        let mut rig = started(REPLY_TIMEOUT, |log| log.start_pairable = true, false).await;
        rig.commands
            .send(BluetoothCommand::RegisterGuestAgent)
            .unwrap();
        until(&rig.log, |log| log.registered.is_some()).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        let PairingRequest::Confirm { reply, .. } = next_request(&mut rig.requests).await else {
            panic!("a confirmation is asked");
        };
        reply.send(PairingReply::Accept).unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Connect {PHONE}"))
        })
        .await;
        let log = rig.log.lock().unwrap();
        assert!(
            !log.calls.iter().any(|call| call.starts_with("Pairable")),
            "{:?}",
            log.calls
        );
        assert!(!log.calls.iter().any(|call| call == "RequestDefaultAgent"));
        assert_eq!(log.agent_results, ["ok"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn power_and_discoverable_are_set_and_a_refused_command_is_counted() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.commands.send(BluetoothCommand::Power(false)).unwrap();
        wait_for(&mut rig.states, |state| !state.powered).await;
        rig.commands
            .send(BluetoothCommand::Discoverable(true))
            .unwrap();
        wait_for(&mut rig.states, |state| state.discoverable).await;
        rig.commands
            .send(BluetoothCommand::Connect("/org/bluez/hci0/dev_9".into()))
            .unwrap();
        let state = wait_for(&mut rig.states, |state| state.refused == 1).await;
        assert!(!state.powered && state.discoverable);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn devices_are_connected_disconnected_and_forgotten() {
        let rig = rig(REPLY_TIMEOUT).await;
        for command in [
            BluetoothCommand::Connect(PHONE.into()),
            BluetoothCommand::Disconnect(HEADSET.into()),
            BluetoothCommand::Forget(PHONE.into()),
        ] {
            rig.commands.send(command).unwrap();
        }
        until(&rig.log, |log| log.calls.len() >= 4).await;
        let calls = rig.log.lock().unwrap().calls.clone();
        assert!(calls.ends_with(&[
            format!("Connect {PHONE}"),
            format!("Disconnect {HEADSET}"),
            format!("RemoveDevice {PHONE}"),
        ]));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_call_from_a_process_that_is_not_bluez_is_rejected_and_ends_nothing() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        let PairingRequest::Confirm { reply, .. } = next_request(&mut rig.requests).await else {
            panic!("a confirmation is asked");
        };
        // The pairing of PHONE runs, so only the sender can be what rejects these.
        let stranger = rig._bus.client().await;
        for method in ["RequestConfirmation", "AuthorizeService", "Cancel"] {
            assert_eq!(
                agent_call(&rig.client, &stranger, method, PHONE).await,
                "org.bluez.Error.Rejected",
                "{method}"
            );
        }
        assert!(
            rig.requests.try_recv().is_err(),
            "the pending confirmation was not withdrawn"
        );
        reply.send(PairingReply::Accept).unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Connect {PHONE}"))
        })
        .await;
        assert_eq!(rig.log.lock().unwrap().agent_results, ["ok"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_request_for_a_device_that_is_not_being_paired_is_rejected() {
        let rig = rig(REPLY_TIMEOUT).await;
        assert_eq!(
            agent_call(&rig.client, &rig.bluez, "RequestConfirmation", HEADSET).await,
            "org.bluez.Error.Rejected"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_pairable_is_tried_once_not_in_a_loop() {
        let rig = rig_with(REPLY_TIMEOUT, |log| {
            log.start_pairable = true;
            log.refuse_pairable = true;
        })
        .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let attempts = rig
            .log
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| *call == "Pairable false")
            .count();
        assert_eq!(attempts, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn turning_the_adapter_off_cancels_the_pairing_that_would_hold_the_queue() {
        let mut rig = rig_with(REPLY_TIMEOUT, |log| log.hold_pair = true).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Pair started {PHONE}"))
        })
        .await;
        rig.commands.send(BluetoothCommand::Power(false)).unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("CancelPairing {PHONE}"))
        })
        .await;
        // The switch-off runs after the cancelled Pair returned: that Pair is the person's own
        // doing, not a refusal.
        let state = wait_for(&mut rig.states, |state| !state.powered).await;
        assert_eq!(state.refused, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forgetting_the_device_being_paired_cancels_the_pairing() {
        let rig = rig_with(REPLY_TIMEOUT, |log| log.hold_pair = true).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Pair started {PHONE}"))
        })
        .await;
        rig.commands
            .send(BluetoothCommand::Forget(PHONE.into()))
            .unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("CancelPairing {PHONE}"))
        })
        .await;
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("RemoveDevice {PHONE}"))
        })
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_model_that_ends_during_a_pairing_leaves_no_agent_and_no_bondable_adapter() {
        let rig = rig_with(REPLY_TIMEOUT, |log| log.hold_pair = true).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Pair started {PHONE}"))
        })
        .await;
        drop(rig.commands);
        until(&rig.log, |log| {
            let mut pairable = log.calls.iter().filter(|call| call.starts_with("Pairable"));
            pairable.next().is_some() && pairable.any(|call| call == "Pairable false")
        })
        .await;
        let gone = tokio::time::timeout(testbus::WAIT, async {
            loop {
                let seen = agent_call(&rig.client, &rig.bluez, "AuthorizeService", PHONE).await;
                if seen != "ok" && seen != "org.bluez.Error.Rejected" {
                    return seen;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the agent is no longer served");
        assert!(gone.contains("Unknown"), "{gone}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_pairing_is_confirmed_through_the_agent_then_trusted_and_connected() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        let PairingRequest::Confirm { label, code, reply } = next_request(&mut rig.requests).await
        else {
            panic!("a confirmation is asked");
        };
        assert_eq!((label.as_str(), code.as_str()), ("Phone", "123456"));
        reply.send(PairingReply::Accept).unwrap();
        until(&rig.log, |log| {
            log.calls
                .iter()
                .any(|call| call == &format!("Connect {PHONE}"))
        })
        .await;
        let log = rig.log.lock().unwrap();
        assert_eq!(log.agent_results, ["ok"]);
        let calls: Vec<&str> = log.calls.iter().map(String::as_str).collect();
        let at = |what: &str| calls.iter().position(|call| *call == what).expect(what);
        assert!(at("Pairable true") < at("Trusted true"));
        assert!(at("Trusted true") < at(&format!("Connect {PHONE}")));
        assert!(
            calls.contains(&"Pairable false"),
            "the adapter is closed to bonding again"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_declined_pairing_is_rejected_and_counted() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        let PairingRequest::Confirm { reply, .. } = next_request(&mut rig.requests).await else {
            panic!("a confirmation is asked");
        };
        reply.send(PairingReply::Decline).unwrap();
        wait_for(&mut rig.states, |state| state.refused == 1).await;
        assert_eq!(
            rig.log.lock().unwrap().agent_results,
            ["org.bluez.Error.Rejected"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unanswered_request_is_canceled_after_the_timeout() {
        let mut rig = rig(Duration::from_millis(200)).await;
        rig.commands
            .send(BluetoothCommand::Pair(PHONE.into()))
            .unwrap();
        let PairingRequest::Confirm { reply, .. } = next_request(&mut rig.requests).await else {
            panic!("a confirmation is asked");
        };
        until(&rig.log, |log| !log.agent_results.is_empty()).await;
        assert_eq!(
            rig.log.lock().unwrap().agent_results,
            ["org.bluez.Error.Canceled"]
        );
        assert!(matches!(
            next_request(&mut rig.requests).await,
            PairingRequest::Withdrawn
        ));
        drop(reply);
    }
}
