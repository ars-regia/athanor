# Control center, steps 1 and 2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the bar's service models onto `zbus` in a new toolkit-free crate, then build the first control-center panel the maintainer judges on the reference laptop.

**Architecture:** `system/athanor-services` holds every model: a `Mirror` of a D-Bus service on `zbus`, the pure decoding functions the bar already tests, and one async task per service that publishes a typed state on a `tokio::sync::watch` channel and takes commands on `tokio::sync::mpsc`. Each GTK process starts one single-threaded Tokio runtime on its own thread and awaits the channels from GLib's main context. `system/athanor-controls` holds the GTK widgets of the detail pages, shared by the bar and by the new `athanor-control-center` program, a resident hidden layer-shell window opened through `os.athanor.ControlCenter1`.

**Tech Stack:** Rust 2021, workspace `zbus 5.18` (feature `tokio`), `tokio 1.53`, `gtk4 0.11` (`v4_18`), `gtk4-layer-shell 0.8`, `libpulse-binding` (threaded main loop), Python e2e under `forge/test/shell/`.

**Spec:** `docs/architecture/doc_control_center.md` (revision 2, approved 2026-10-04). Also binding: `doc_bar.md` (BR1, BR3, BR9), `doc_launcher.md` (LA1, LA8), `doc_shell.md` (SH4, SH8, SH13), `doc_shell_standard.md` (ST5, ST7).

## Global Constraints

- English in code, comments, commit messages; enterprise tone; no attribution to any assistant.
- `athanor-services` depends on no GTK, GLib or GIO crate (CC3, SH4). `cargo tree -p athanor-services | grep -E 'glib|gio|gtk'` prints nothing.
- No model calls `tokio::runtime::Handle::current()`; every model takes a `Handle` (CC3).
- No GTK widget waits on D-Bus: every reply is decoded on the runtime thread (CC3, ST5).
- A reply or signal of the wrong type is `None` or an absent field, never a panic (`props.rs` rule). `panic = "abort"` holds in every profile.
- The bar's behaviour, its unit tests and its e2e tests are unchanged by tasks 2–8; its idle PSS stays within 64 MB, baseline 30.0 MB (`docs/shell-bench/2026-10-04-1b4c5f7/results.json`).
- No `|| true`, no `continue-on-error`, no placeholder in a security path. No `chmod 777`.
- The panel stays disabled by preset (CC14). No change to polkit rules, the Gatekeeper or attestation.
- Shortcut: Super+C, written once per user (CC9, LA8).
- Commits: `type(scope): imperative summary`, as `git log -10` shows.

## Review Focus

1. A service that is absent at start and appears later (BlueZ started after login): the model must publish the state when it appears, as `Mirror` does today on `NameOwnerChanged`.
2. A service that restarts (NetworkManager restart): stale objects are dropped and the mirror reloads; no tile shows a network of the dead instance.
3. A peer that sends a property of the wrong type: the field is absent, the process lives.
4. The NetworkManager secret agent on `zbus`: a request while no prompt can be shown answers `NoSecrets`, a cancel closes the prompt, and only NetworkManager's unique name is answered.
5. Do not disturb with `until` in the past at shelld start: it reads as off, not on forever.

---

## Task order and parallelism

| Task                                          | Depends on    | Parallel group |
| --------------------------------------------- | ------------- | -------------- |
| 1 Foundations of `athanor-services`           | —             | A (alone)      |
| 2 Battery, power profile, brightness          | 1             | B              |
| 3 Bluetooth                                   | 1             | B              |
| 4 Network and secret agent                    | 1             | B              |
| 5 MPRIS                                       | 1             | C              |
| 6 Audio on the threaded main loop             | 1             | C              |
| 7 Deploy builds to the reference laptop       | —             | A              |
| 8 Bar measurement and e2e, before/after       | 2–7           | D              |
| 9 `athanor-controls`                          | 8             | E              |
| 10 shelld: do-not-disturb state and admission | —             | B or C         |
| 11 `athanor-control-center` skeleton          | 1             | C              |
| 12 `control-center.toml`                      | —             | B or C         |
| 13 The first panel                            | 9, 10, 11, 12 | F              |
| 14 Package and preset                         | 13            | F              |

At most two agents at a time, each in its own worktree branched from `control-center` at the task's BASE, all building into one `CARGO_TARGET_DIR=/var/tmp/athanor-target-cc` with `-j 4`, so builds queue on cargo's lock and the desktop keeps its CPU budget (runner at most 8 threads, local work at most 4); each task merges back into `control-center` before a task that depends on it starts.

---

### Task 1: Foundations of `athanor-services`

**Files:**

- Create: `system/athanor-services/Cargo.toml`, `system/athanor-services/src/lib.rs`, `src/runtime.rs`, `src/props.rs`, `src/mirror.rs`, `src/testbus.rs` (cfg(test) helper, `pub` under feature `testbus`)
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**

- Produces:
  - `pub struct Runtime { handle: tokio::runtime::Handle }`; `Runtime::start() -> std::io::Result<Runtime>` builds a current-thread runtime with `enable_all()` on a thread named `athanor-services` that runs `block_on(std::future::pending::<()>())`; `Runtime::handle(&self) -> &Handle`.
  - `pub enum Bus { System, Session }`; `pub struct Buses` with `Buses::new(handle: Handle) -> Buses` and `async fn connection(&self, bus: Bus) -> zbus::Result<zbus::Connection>` (one connection per bus, created once, `tokio::sync::OnceCell`); `Buses::with(conn: zbus::Connection) -> Buses` for tests (both buses answer `conn`).
  - `props`: `pub type Props = BTreeMap<String, zvariant::OwnedValue>`, `Interfaces = BTreeMap<String, Props>`, `Objects = BTreeMap<String, Interfaces>`; typed getters `get_str(&Props, &str) -> Option<&str>`, `get_u32`, `get_i32`, `get_u64`, `get_f64`, `get_bool`, `get_bytes -> Option<Vec<u8>>`, `get_path -> Option<String>`, `get_paths -> Option<Vec<String>>`, `get_strs -> Option<Vec<String>>`; each returns `None` on a wrong type.
  - `mirror`: `pub enum Source { Managed(&'static str), Fixed(Vec<(&'static str, &'static str)>) }`; `pub struct Snapshot { pub objects: Objects, pub owner: Option<String>, pub generation: u64 }`; `pub fn spawn(handle: &Handle, conn: zbus::Connection, name: &'static str, source: Source) -> watch::Receiver<Snapshot>`.
- Consumes: nothing.

Behaviour of `mirror::spawn`, ported from `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/bus.rs` (`Mirror`, lines 94–328):

- Watch `NameOwnerChanged` for `name` through `zbus::fdo::DBusProxy`. The mirror always runs on a bus; there is no peer-to-peer branch in it.
- Calls carry a 5 s timeout: a load that never answers leaves `objects` empty with the owner known, logs once at warning level, and is retried at the next owner change.
- When the owner appears: load (`GetManagedObjects` at the root, or `GetAll` per fixed pair), bump `generation`, publish. When it vanishes: clear `objects`, `owner = None`, bump, publish.
- Signals `PropertiesChanged`, `InterfacesAdded`, `InterfacesRemoved` from the current owner only (match rule with `sender = name`, then compare the message's sender with the owner) update `objects` and publish. Updates within 16 ms coalesce into one publish (`watch::Sender::send_modify` after a `tokio::time::sleep` debounce on the first change).
- Every decode goes through `OwnedValue::try_from` / `zvariant::Value::downcast_ref`; a wrong type drops that property only.

- [ ] **Step 1: Write the failing tests** in `src/mirror.rs` (`#[cfg(test)]`), using `testbus::start(name)`, which starts a private `dbus-daemon` as `forge/specs/athanor-shelld/athanor-shelld-1.0.0/tests/common/mod.rs` does (`Bus::start`) and returns `(bus, server, client)`: the server connection owns `name`. Pure decoding is tested with no bus at all. The server serves `org.freedesktop.DBus.ObjectManager` at `/root` and a test interface `os.athanor.Test1` with properties `Name: s` and `Level: u`.

```rust
#[tokio::test(flavor = "current_thread")]
async fn managed_objects_are_loaded_and_follow_signals() {
    let (_bus, server, client) = testbus::start("os.athanor.Test").await;
    testbus::add_object(&server, "/root/a", "Alpha", 10).await;
    let mut rx = spawn(&Handle::current(), client, "os.athanor.Test", Source::Managed("/root"));
    let snap = testbus::wait_for(&mut rx, |s| s.objects.contains_key("/root/a")).await;
    assert_eq!(props::get_u32(&snap.objects["/root/a"]["os.athanor.Test1"], "Level"), Some(10));
    testbus::set_level(&server, "/root/a", 42).await;
    let snap = testbus::wait_for(&mut rx, |s| {
        props::get_u32(&s.objects["/root/a"]["os.athanor.Test1"], "Level") == Some(42)
    }).await;
    assert!(snap.generation >= 1);
    testbus::remove_object(&server, "/root/a").await;
    testbus::wait_for(&mut rx, |s| !s.objects.contains_key("/root/a")).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_property_of_the_wrong_type_is_absent_not_fatal() {
    let (_bus, server, client) = testbus::start("os.athanor.Test").await;
    testbus::add_object_raw(&server, "/root/b", "Level", zvariant::Value::from("not a u32")).await;
    let mut rx = spawn(&Handle::current(), client, "os.athanor.Test", Source::Managed("/root"));
    let snap = testbus::wait_for(&mut rx, |s| s.objects.contains_key("/root/b")).await;
    assert_eq!(props::get_u32(&snap.objects["/root/b"]["os.athanor.Test1"], "Level"), None);
}
```

Two more in `src/mirror.rs`: `the_owner_leaves_and_returns` (the server connection is dropped: `objects` empties and `owner` is `None`; a new server takes the name: the objects load again) and `a_load_that_never_answers_does_not_stall` (the server's `GetManagedObjects` awaits forever: after the timeout the snapshot has the owner and no objects, and a second mirror on another name still publishes).

In `src/props.rs`, one test per getter with a right and a wrong type. In `src/runtime.rs`:

```rust
#[test]
fn a_task_spawned_on_the_handle_runs_off_the_calling_thread() {
    let rt = Runtime::start().expect("runtime");
    let caller = std::thread::current().id();
    let (tx, rx) = std::sync::mpsc::channel();
    rt.handle().spawn(async move { tx.send(std::thread::current().id()).unwrap() });
    assert_ne!(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), caller);
}
```

- [ ] **Step 2: Run them and watch them fail**
      Run: `cargo test -p athanor-services`
      Expected: FAIL to compile (`spawn`, `Runtime`, `testbus` not defined).

- [ ] **Step 3: Implement** `runtime.rs`, `props.rs`, `mirror.rs`, `testbus.rs` as specified above. `Cargo.toml`: `zbus = { workspace = true }`, `tokio = { workspace = true }`, `tracing = { workspace = true }`; `[features] testbus = []`.

- [ ] **Step 4: Run the tests and the toolkit check**
      Run: `cargo test -p athanor-services && cargo tree -p athanor-services -e normal | grep -cE '\b(glib|gio|gtk4)\b'`
      Expected: tests PASS; the count prints `0`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock system/athanor-services
git commit -m "feat(services): add the zbus mirror and runtime the shell's models share"
```

---

### Task 2: Battery, power profile and brightness

**Files:**

- Create: `system/athanor-services/src/battery.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/battery.rs` (delete after the move; `lib.rs` re-exports from `athanor_services::battery` until Task 7), `src/ui/battery.rs`, `src/main.rs` (start the runtime), bar `Cargo.toml` (+`athanor-services`, `tokio` with `sync`)
- Test: inline in `system/athanor-services/src/battery.rs`

**Interfaces:**

- Consumes: Task 1 `Runtime`, `Buses`, `mirror::spawn`, `props::*`.
- Produces:
  - `pub struct BatteryState { pub battery: Option<Battery>, pub profiles: Option<Profiles>, pub backlight: Option<Backlight> }` (the types `Battery`, `Profiles`, `Backlight` of today's `battery.rs`, decoding from `athanor_services::props::Props`).
  - `pub enum BatteryCommand { SetProfile(String), SetBrightness { device: String, raw: u32 } }`.
  - `pub fn spawn(handle: &Handle, buses: Buses, backlight_dir: PathBuf) -> (watch::Receiver<BatteryState>, mpsc::Sender<BatteryCommand>)`.
  - In the bar: `ui::bridge::follow<T: Clone + 'static>(rx: watch::Receiver<T>, apply: impl Fn(&T) + 'static)` — `glib::spawn_future_local` loop on `rx.changed()`; created in this task, reused by tasks 3–7.

Rules: brightness is set through logind `org.freedesktop.login1.Session.SetBrightness("backlight", name, raw)` on the session object `/org/freedesktop/login1/session/auto` (today's `ui/battery.rs:170-179`); the sysfs directory stays overridable by `ATHANOR_BAR_BACKLIGHT_DIR`. The profile is written as UPower PowerProfiles' `ActiveProfile` property. A refused command logs at warning level, as `bus::refused` does, and the state is re-read.

- [ ] **Step 1:** Port every test of today's `battery.rs` (from line 203) to `zvariant` values, unchanged in what it asserts. Add one `testbus` test: a fake UPower display device at `/org/freedesktop/UPower/devices/DisplayDevice` with `Percentage = 55.0` and `State = 2` publishes `battery.percent == 55`.
- [ ] **Step 2:** Run `cargo test -p athanor-services battery` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement; switch `ui/battery.rs` from `Mirror::new` (lines 101–110) and `bus::set_property` to `spawn` + `bridge::follow` + the command sender.
- [ ] **Step 4:** Run `cargo test -p athanor-services battery && cargo test -p athanor-bar` — Expected: PASS, same count of bar tests as before minus the moved ones.
- [ ] **Step 5:** Commit `refactor(bar): read the battery, power profile and brightness through athanor-services`.

---

### Task 3: Bluetooth

**Files:** Create `system/athanor-services/src/bluetooth.rs`; modify the bar's `bluetooth.rs` (moved), `ui/bluetooth.rs`.

**Interfaces:**

- Produces: `pub fn spawn(handle: &Handle, buses: Buses) -> (watch::Receiver<BluetoothState>, mpsc::Sender<BluetoothCommand>)`; `BluetoothState` as today's (`bluetooth.rs:86`, `state(&Objects)`); `BluetoothCommand::{Power(bool), Connect(String), Disconnect(String), Pair(String), Forget(String), Discoverable(bool)}` — the subset the bar's popover issues today, plus `Discoverable` used by Task 13.
- The pairing agent today's `ui/bluetooth.rs` registers (passkey, `device_of`) moves to a `zbus` `#[interface]` served on the system connection; a request is forwarded to the UI on `mpsc::Sender<PairingRequest>` with a `oneshot::Sender<PairingReply>`; no reply within BlueZ's timeout answers `org.bluez.Error.Canceled`. Only BlueZ's unique name is answered (Review Focus 4 applies by analogy).

- [ ] **Step 1:** Port the tests of `bluetooth.rs` (from line 161) to `zvariant`; add a `testbus` test with an adapter and one paired device (`org.bluez.Device1`, `Connected = true`, `Alias = "Headset"`).
- [ ] **Step 2:** `cargo test -p athanor-services bluetooth` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement; switch `ui/bluetooth.rs` (Mirror at line 139) to `spawn` + `bridge::follow`.
- [ ] **Step 4:** `cargo test -p athanor-services bluetooth && cargo test -p athanor-bar` — Expected: PASS.
- [ ] **Step 5:** Commit `refactor(bar): read Bluetooth through athanor-services`.

---

### Task 4: Network and the secret agent

**Files:** Create `system/athanor-services/src/network.rs`, `src/network_agent.rs`; modify the bar's `network.rs` (moved), `ui/network.rs`.

**Interfaces:**

- Produces: `pub fn spawn(handle: &Handle, buses: Buses) -> (watch::Receiver<NetworkState>, mpsc::Sender<NetworkCommand>, mpsc::Receiver<PasswordPrompt>)`. `NetworkState`, `Network`, `Wifi`, `Vpn`, `Link`, `Security`, `ConnectionInfo` keep today's fields (`network.rs:14-230`). `NetworkCommand::{Wireless(bool), Join { network: Network, password: Option<String> }, Activate { connection, device, specific }, Deactivate(String), Details(String, oneshot::Sender<Option<ConnectionInfo>>)}`. `PasswordPrompt { request: SecretsRequest, reply: oneshot::Sender<Option<String>> }`.
- `add_and_activate`, `activate`, `deactivate` build `zbus` arguments instead of `glib::Variant`; `wifi_settings` builds `HashMap<&str, HashMap<&str, zvariant::Value>>` with the same keys and values.
- The agent: `os.athanor.Bar` keeps its identifier; served with `#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]` at `/org/freedesktop/NetworkManager/SecretAgent`, registered with `AgentManager.RegisterWithCapabilities`. `GetSecrets` from a sender that is not NetworkManager's current owner is refused with `org.freedesktop.NetworkManager.SecretAgent.PermissionDenied`; a prompt the UI drops answers `NoSecrets`; `CancelGetSecrets` drops the pending reply and the UI closes its prompt.

- [ ] **Step 1:** Port every test of `network.rs` (from line 498) to `zvariant`. Add `testbus` tests: (a) one Wi-Fi device and two access points publish two networks sorted as today; (b) `GetSecrets` from a non-owner is refused; (c) a dropped prompt answers `NoSecrets`.
- [ ] **Step 2:** `cargo test -p athanor-services network` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement; switch `ui/network.rs` (Mirror at line 139, agent registration, `bus::call` sites) to the channels.
- [ ] **Step 4:** `cargo test -p athanor-services network && cargo test -p athanor-bar` — Expected: PASS.
- [ ] **Step 5:** Commit `refactor(bar): read the network and answer NetworkManager's secrets through athanor-services`.

---

### Task 5: MPRIS

**Files:** Create `system/athanor-services/src/media.rs`; modify `ui/mpris.rs`, the bar's `audio.rs` (MPRIS helpers `Track`, `track`, `is_player`, `playing` move to `media.rs`).

**Interfaces:**

- Produces: `pub struct MediaState { pub players: Vec<Player>, pub current: Option<usize> }`, `Player { bus_name, identity, track: Option<Track>, playing: bool, can_seek: bool, position_us: Option<i64>, length_us: Option<i64>, art: Option<Art> }`, `Art::{File(PathBuf), Data(Vec<u8>)}` — `https:` artwork is `None` (CC8). `MediaCommand::{PlayPause, Next, Previous, Seek(i64), Choose(String)}`. `pub fn spawn(handle, buses) -> (watch::Receiver<MediaState>, mpsc::Sender<MediaCommand>)`.
- Players are found by `ListNames` and followed by `NameOwnerChanged` for names starting `org.mpris.MediaPlayer2.`; `Media::now()` / `settled()` behaviour of today's `ui/mpris.rs:35-175` is kept.

- [ ] **Step 1:** Port the MPRIS tests of `audio.rs` (from line 101); add `testbus` tests for a player with `file:` artwork and one with `https:` artwork (absent).
- [ ] **Step 2:** `cargo test -p athanor-services media` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement; switch `ui/mpris.rs`.
- [ ] **Step 4:** `cargo test -p athanor-services media && cargo test -p athanor-bar` — Expected: PASS.
- [ ] **Step 5:** Commit `refactor(bar): follow media players through athanor-services`.

---

### Task 6: Audio on the threaded main loop

**Files:** Create `system/athanor-services/src/audio.rs`; modify `ui/audio.rs`, the bar's `audio.rs` (moved), bar `Cargo.toml` (drop `libpulse-glib-binding`).

**Interfaces:**

- Produces: `AudioState { outputs: Vec<Device>, inputs: Vec<Device>, default_output: Option<String>, default_input: Option<String> }` with today's `Device`, `percent`/`raw` mapping and `chosen`; `AudioCommand::{Volume { sink: bool, name: String, percent: u32 }, Mute { sink: bool, name: String, on: bool }, Default { sink: bool, name: String }}`; `pub fn spawn(handle: &Handle) -> (watch::Receiver<AudioState>, mpsc::Sender<AudioCommand>)`.
- `libpulse_binding::mainloop::threaded::Mainloop` on its own thread; callbacks only copy data into the `watch::Sender`; commands from the `mpsc` are executed under the main loop's lock. A server that disconnects publishes the empty state and reconnects with backoff 1 s doubling to 30 s.

- [ ] **Step 1:** Port the tests of `audio.rs` (from line 101, non-MPRIS part). Add a test of the reconnect backoff schedule as a pure function `backoff(attempt: u32) -> Duration`.
- [ ] **Step 2:** `cargo test -p athanor-services audio` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement; switch `ui/audio.rs`.
- [ ] **Step 4:** `cargo test -p athanor-services audio && cargo test -p athanor-bar && cargo tree -p athanor-bar | grep -c libpulse-glib` — Expected: PASS and `0`.
- [ ] **Step 5:** Commit `refactor(bar): read audio through athanor-services, off the GLib main loop`.

---

### Task 7: Deploy builds to the reference laptop

Runs first, beside Task 1: every later build is installed with it (CC14).

**Files:** Create `scripts/shell-bench/deploy.py` (Python, like the rest of `scripts/shell-bench/`, reusing `machine.Machine`).

**Interfaces:**

- Produces: `python3 scripts/shell-bench/deploy.py --host athanor-ref <crate>...` — builds the named crates in release mode in the podman builder the bar's spec uses (`cargo build --release --locked -j 4 -p <crate>`), copies each binary, unit and D-Bus activation file to the laptop, makes `/usr` writable for this boot only with `sudo -n bootc usr-overlay` (skipped when already writable), installs them, runs `systemctl --user daemon-reload` and restarts the crate's unit, and prints the commit installed. A reboot returns the laptop to its image. Refuses to run while `soak.py` or `bench.py` is running on this host (`pgrep -f`), so it never touches a measurement; neither script changes.
- Consumes: `machine.Machine.run`, `machine.Machine.systemctl`.

- [ ] **Step 1:** Test in `scripts/shell-bench/tests/test_deploy.py`: `plan(crate, built_dir)` returns the `(source, destination)` pairs for `athanor-bar` (binary to `/usr/bin/athanor-bar`, unit to `/usr/lib/systemd/user/athanor-bar.service`) and refuses an unknown crate.
- [ ] **Step 2:** `python3 -B -m unittest discover -s scripts/shell-bench/tests` — Expected: FAIL (`deploy` not found).
- [ ] **Step 3:** Implement `deploy.py`.
- [ ] **Step 4:** Same command — Expected: PASS. Then `python3 scripts/shell-bench/deploy.py --host athanor-ref athanor-bar` on today's bar — Expected: the bar restarts and `systemctl --user show -p ExecMainStartTimestamp athanor-bar` on the laptop moves.
- [ ] **Step 5:** Commit `feat(shell-bench): install a build on the reference laptop for the maintainer to judge`.

The bar's power and session actions (`ui/power.rs`, `ui/logind.rs`) and the GIO `Mirror` stay: CC3 covers the four models, and `Mirror` still serves the notifications, the shield and the tray, which move when they are next changed.

---

### Task 8: The bar before and after

**Files:** none changed unless a check fails.

- [ ] **Step 1:** Run the bar's e2e in the rig: `forge/test/shell/rig.sh` with `bar_e2e.py` and `bar_modules_e2e.py` (the commands in `forge/test/shell/README` or the rig script's usage). Expected: the same passes as on BASE of Task 2.
- [ ] **Step 2:** Install the branch's bar on the reference laptop (Task 7's `deploy.py --host athanor-ref athanor-bar`) and run `python3 scripts/shell-bench/bench.py --host athanor-ref --stages memory,idle,response --commit <sha> --out /var/tmp/cc-bench/<sha>`. Expected: `athanor-bar` PSS ≤ 64 MB; record the delta from 30.0 MB in the ledger; response p95 ≤ 100 ms.
- [ ] **Step 3:** The maintainer looks at the bar on the laptop: every popover behaves as before.

---

### Task 9: `athanor-controls`

**Files:** Create `system/athanor-controls/` (lib); move the popover page builders out of the bar's `ui/network.rs`, `ui/bluetooth.rs`, `ui/audio.rs`, `ui/battery.rs` into `athanor-controls/src/{network,bluetooth,audio,battery}.rs`; the bar calls them.

**Interfaces:**

- Produces, per page: `pub fn page(services: &Services) -> gtk::Widget` where `pub struct Services { battery: (watch::Receiver<BatteryState>, mpsc::Sender<BatteryCommand>), network: …, bluetooth: …, audio: …, media: … }` is built once per process by `Services::start(&Runtime) -> Services`. A page subscribes through `bridge::follow` (moved into `athanor-controls`).
- `athanor-controls` is a GTK crate: add it to the workspace `members` next to the bar (root `Cargo.toml` lines 48–51 explain why GTK crates move together).

- [ ] Steps: test first — a `#[test]` per page that builds it under `gtk::init()` skipped without a display (`if gtk::init().is_err() { return }`) and asserts its accessible name; run, fail, move, pass; `cargo test -p athanor-controls -p athanor-bar`; commit `refactor(bar): move the detail pages to athanor-controls`.

---

### Task 10: shelld — do-not-disturb state and admission (superseded by `docs/superpowers/plans/2026-10-05-notification-center.md`, Tasks 4 and 5)

**Files:** Modify `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/dnd.rs`, `sender.rs`, `notifications.rs`, `store.rs`.

**Interfaces:**

- Produces:
  - `dnd::State { on: bool, until: Option<i64> /* unix seconds */ }`; `dnd::load(dir) -> io::Result<State>`: no file → off; the legacy content `on\n` → `{ on, until: None }`; the new content `on\nuntil=<secs>\n`; an `until` in the past → off and the file removed. `dnd::save(dir, &State)`.
  - `sender::Admitted` replacing `BarUnit`: `Admitted::from_proc(&["athanor-bar.service", "athanor-control-center.service"])`, `admits(pid) -> Option<&str>` returning the unit. `admits_path` unchanged.
  - Private interface additions: `DoNotDisturb() -> (b on, x until)` (until `0` = none; records the caller's unique name as that unit's destination), `SetDoNotDisturbUntil(b on, x until)`, signal `DoNotDisturbChanged(b, x)` unicast to every recorded destination. `SetDoNotDisturb(b)` stays and means `until = None`. Notification signals stay unicast to the bar's destination only.
  - A Tokio timer turns DND off at `until`, re-armed after a start.
  - `schedule` (CC7) is not built here: the notification center's specification sets it. The file is `key=value` lines, so it arrives as one more key with no migration.
- [ ] **Step 1:** Tests: legacy file reads as on; past `until` reads off and removes the file; `admits` returns the control center's unit for a cgroup ending `athanor-control-center.service` and `None` for `app-foo.service`; a DND set by the control center emits `DoNotDisturbChanged` to the bar's destination too — an integration test in `tests/` on the private `dbus-daemon` that `tests/common/mod.rs` already starts (`Bus::start`, `BAR_CGROUP`; add a `CONTROL_CENTER_CGROUP` beside it).
- [ ] **Step 2:** `cargo test -p athanor-shelld` — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p athanor-shelld` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(shelld): keep do not disturb with an end time and admit the control center`.

---

### Task 11: `athanor-control-center` skeleton

**Files:** Create `forge/specs/athanor-control-center/athanor-control-center-1.0.0/` (bin crate: `src/main.rs`, `src/bus.rs`, `src/surface.rs`, `data/athanor-control-center.service`, `data/os.athanor.ControlCenter1.service`, `data/80-athanor-control-center.preset` with `disable`), `forge/specs/athanor-control-center/athanor-control-center.spec`; modify the bar's `order.rs` (`Module::ControlCenter`, name `"control-center"`, default last at the end edge) and `ui/mod.rs:247-258` (a button calling `Toggle()`).

**Interfaces:**

- Consumes: `athanor-launcher`'s pattern — `src/bus.rs:13-55` (`Show`, name loss), `src/surface.rs:73` (resident hidden window), `src/main.rs:90-104` (the once-per-user marker).
- Super+C is a key binding, not a COSMIC system action, and `athanor-compositor-client/src/shortcuts.rs` writes only `system_actions`. Add `set_custom_binding_once(modifiers: &[&str], key: &str, command: &str, marker: &Path)` beside `set_system_action_once`, writing COSMIC's `custom` key of `com.system76.CosmicSettings.Shortcuts` and leaving an existing binding of that key combination alone (logged, marker written). The command is `busctl --user call os.athanor.ControlCenter1 /os/athanor/ControlCenter1 os.athanor.ControlCenter1 Toggle`. Run `codegraph_impact` on `shortcuts.rs` first.
- Produces: `os.athanor.ControlCenter1` at `/os/athanor/ControlCenter1` with `Show(s page)` and `Toggle()`; Super+C written once with the state file `state/athanor/control-center/super-c-bound`; the window on the focused output, anchored under the bar's button (above it when the bar is at the bottom), closing on outside click, Escape and focus loss (CC9). A property `Open: b` with `PropertiesChanged`; the bar's `ui/notifications.rs` follows it and keeps its popups hidden while it is true (CC9). Landlock as CC2. Unit `Type=notify`, `MemoryHigh=96M`, the sandbox lines of `athanor-bar.service`.
- [ ] Steps: test first for the pure parts (page id parsing, the anchor computation for top and bottom bars, Landlock ruleset construction, the `custom` binding writer on an empty file, on a file with another binding, and on a file already binding Super+C) → fail → implement → `cargo test -p athanor-control-center` PASS → commit `feat(control-center): add the resident panel, its D-Bus name, Super+C and the bar's button`.

---

### Task 12: `control-center.toml`

**Files:** Modify `system/athanor-layout` (new module `src/control_center.rs`): a loader `control_center::load(layers) -> Tiles` with `schema = 1`, whole-file rejection on any error and the lower layer used instead (SH8); preset defaults per SH7 preset.

- [ ] Steps: tests for a valid file, an unknown tile id (rejected, lower layer used, warning logged), a wrong schema; fail; implement; `cargo test -p athanor-layout` PASS; commit `feat(layout): load the control center's tiles`.

---

### Task 13: The first panel

**Files:** `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/panel.rs`, `src/tiles.rs`, `src/sliders.rs`, `src/media.rs`, `src/footer.rs`, `src/rfkill.rs`, `src/theme.rs`.

Contents (CC14 step 2): Wi-Fi, Bluetooth and airplane-mode tiles; output and input volume; display brightness; power profile; do not disturb (on, off, one hour, until 08:00); dark mode; full media controls; battery level; Settings button opening `cosmic-settings`; each tile with an arrow opens its `athanor-controls` page on a stack with a back button. A tile whose service or hardware is absent is not built (BR3). Airplane mode soft-blocks every radio through `/dev/rfkill` (`struct rfkill_event`, `RFKILL_OP_CHANGE_ALL`), absent when the device is.

- [ ] Steps: pure tests first (rfkill event encoding, the DND menu's `until` for "one hour" and "until 08:00" across midnight, tile presence from a `Services` snapshot); fail; implement; `cargo test -p athanor-control-center` PASS; commit `feat(control-center): the first panel`.
- [ ] Accessibility (ST7) in the same task: every tile and slider reachable with Tab and arrows, with an accessible name and state, and the panel announced when it opens.

---

### Task 14: Package and preset

**Files:** `forge/specs/athanor-control-center/athanor-control-center.spec` (from `athanor-bar.spec`: `cargo build --release --locked -p %{name}`, `%check` with `check_shim_link_order.py`), the preset with `disable`, and the image's package list.

- [ ] Steps: `python3 scripts/verify.py shipped` and `python3 scripts/verify.py specs` — Expected: no new failure; deploy with Task 7's script; the maintainer opens the panel with the bar's button and with Super+C and judges it. Commit `build(control-center): package the panel, disabled by preset`.

---

## Acceptance for this plan

1. `cargo test -p athanor-services -p athanor-controls -p athanor-bar -p athanor-shelld -p athanor-control-center -p athanor-layout` passes.
2. `cargo tree -p athanor-services` contains no `glib`, `gio` or `gtk4`.
3. The bar's e2e passes as before; its idle PSS on the laptop ≤ 64 MB, with the delta from 30.0 MB recorded.
4. On the laptop, the bar's button and Super+C open the panel with the contents of Task 13; the maintainer has judged it.
5. `python3 scripts/verify.py` shows no new failure against BASE.
