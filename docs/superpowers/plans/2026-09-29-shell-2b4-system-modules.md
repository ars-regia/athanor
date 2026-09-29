# Shell 2b.4: the network, Bluetooth, audio and battery modules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The four system modules of `athanor-bar`:

- **network**: the wired state, the Wi-Fi list, joining with a password, VPN and airplane mode;
- **Bluetooth**: power, the paired devices, connect and disconnect, and pairing that shows the passkey for confirmation;
- **audio**: the volume and the choice of output and input device, plus media controls over MPRIS;
- **battery**: the percentage, the time left, the power profile and the screen brightness.

They are tested end to end in the rig against python3-dbusmock and a real PipeWire, and captured as four scenes of BR9.

**Architecture:** The bar crate keeps its split.

- **The logic is pure Rust in the library target.** It covers property maps, network and Bluetooth state, the password and passkey rules, volume arithmetic and the battery. The tests need no display and no bus.
- **Each module in `ui/` is a view per surface of one service per process.** Each service holds:
  - one mirror of the service's objects, fed by `GetManagedObjects` or `GetAll` and kept current by the service's signals;
  - one agent where the protocol needs one;
  - a list of weak views.

  The views are driven by signals, not by the bar's `refresh`. When a module's source comes or goes, the service redraws its views and asks the bar to refit its groups.
- **D-Bus goes through GIO** on the bar's GLib main loop, as logind already does.
- **Audio goes through libpulse**, the PulseAudio protocol that `pipewire-pulse` serves, on the same main loop through `libpulse-glib-binding`.

**Tech Stack:**

- Rust 2021.
- gtk4 0.11 (`v4_18`), glib and gio 0.22 (`v2_68`).
- libpulse-binding 2.30 and libpulse-glib-binding 2.29.
- The rig adds:
  - python3-dbusmock 0.38.0, with the templates networkmanager, bluez5, upower and upower_power_profiles_daemon;
  - pipewire 1.4.11, wireplumber 0.5.14 and pipewire-pulseaudio;
  - a Gio MPRIS player;
  - a fake backlight directory.

**Spec:** `docs/architecture/doc_bar.md` rev 1:

- BR3: the rows for Network, Bluetooth, Audio and Battery;
- BR6: one popover at a time;
- BR9: fixtures, scenes, and unit tests of parsers;
- open doubt 4: the Fedora 43 templates;
- section 5, items 15, 17 and 18. Item 14 covers the modules of 2b.2 and is not part of this plan.

Read it with `docs/architecture/doc_shell.md` rev 5: SH1 (no facades) and SH13 (12 cases per scene, and a popover scene starts open).

**Where 2b.4 sits.** Package 2b is delivered in five plans:

| Plan                 | Delivers                                                                                                                             |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| 2b.1                 | `athanor-unit`, `athanor-shelld`                                                                                                     |
| 2b.2                 | `athanor-bar`: the surfaces, the presets, the compositor modules, the clock and the power menu                                       |
| 2b.3                 | the notification popups and list, the tray host and dbusmenu, in the bar                                                             |
| **2b.4 (this plan)** | the network, Bluetooth, audio and battery modules on dbusmock fixtures. It confirms the Fedora 43 templates (BR9 and open doubt 4). |
| 2b.5                 | the shield and its sheet, and the BR8 signals                                                                                        |

**Order.** 2b.4 runs after Tasks 1 to 3 of the dock plan (2c Unit A, branch `shell-2c-dock` at `b648f633`) and after 2b.3 on the bar. Before Task 1, merge the Unit A branch into `shell-2b4-modules`, then the 2b.3 branch, each with `git merge`. Never rebase: those branches are shared, and their commits stay as they were published. What they change under this plan:
- Unit A moves the applications row, the favourites store and the openers into `system/athanor-apps` (`athanor_apps::{row, favorites, openers}`), `athanor_bar::running` into `athanor_apps::model`, `dirs.rs` into `athanor_unit::dirs`, and the layout `Source` into `athanor_layout::loader::Source`. The bar's `lib.rs` keeps `clock`, `keyboard`, `order`, `power` and `tiling`. `po/POTFILES.in` lists athanor-apps' `openers.rs` and `row.rs` after the bar's own sources.
- The bar keeps `ui/popup.rs`. `Popup::new(bar, child, name)`, `Popup::open` and `switch_row(text)` are unchanged; `popup::attach(bar, button)` wraps `athanor_apps::menu::attach(button, popup::towards_inside(bar))`, and `popup::towards_inside(bar)` is the side facing the inside of the screen. The bar also keeps `crate::i18n::{tr, tr_with}`, which hands its catalogue to `athanor_apps::i18n::set_catalog`. Every popover of this plan is built by `Popup::new`, so the modules import nothing from athanor-apps. A popover built elsewhere would be attached with `athanor_apps::menu::attach_popover(button, popover, popup::towards_inside(bar))`.
- 2b.3 adds `Bar::popovers_changed` and calls it when a popover of the bar shows or closes (BR6, "Stacking"), so the modules' popovers reach it through `Popup::new`. 2b.3 also adds `dbusmenu`, `notices`, `popups` and `tray` to `lib.rs` and `menu`, `notifications`, `popups` and `tray` to `ui/mod.rs`, rewrites `bar_session.py` with argparse, adds three scenes, and edits `rig.sh`, the translations and the workflow.

The code of this plan is written against Unit A: every signature it uses from the bar, athanor-apps and athanor-unit was checked on `b648f633`. A step that inserts at an anchor where 2b.3 also inserts holds as written. Two steps rewrite lines that 2b.3 rewrote: Task 3, Step 2 (`bar_session.py`) and Task 8, Step 5 (the scenes). Their anchors were checked against the 2b.3 code at `29d9293f`: the argparse `parse`, `helpers = []` after the `RequestName` check, the `bar-tray)` arm of `capture_bar`, and the nine-scene lists in `cases.py`, `test_cases.py` and the workflow matrix. This plan brings 4 of the 15 scenes of BR9, which is 48 of the 180 surface cases: `bar-network`, `bar-bluetooth`, `bar-audio` and `bar-battery`.

**Rulings this plan makes.** The spec leaves these open, or says them differently:

- **GIO, not zbus.** BR3 names `zbus` for the network row. The bar already runs GDBus on GLib's main loop for logind. zbus would bring a second D-Bus stack and an async executor into a process with a 64 MB budget (item 17), and what goes over the bus would be the same. The pull request description records the deviation, under "Deviations from doc_bar.md" (Task 9, Step 5).
- **The NetworkManager secret agent is registered by `athanor-bar` itself,** on its own system-bus connection, at `/org/freedesktop/NetworkManager/SecretAgent` with the identifier `os.athanor.Bar`. The bar is the process that shows the prompt, so no second process ever holds the password. It is safe because:
  - it refuses every call that does not come from the current unique owner of `org.freedesktop.NetworkManager`;
  - it answers only interactive requests for a Wi-Fi personal password (WPA-PSK or SAE), one at a time, and `NoSecrets` to everything else;
  - the password goes from the `gtk::PasswordEntry` into the reply to NetworkManager and nowhere else. It is never logged, never written by the bar, and the entry is cleared as the reply leaves.
- **The BlueZ agent is registered by `athanor-bar` itself,** at `/os/athanor/Bar/BluezAgent` with the capability `DisplayYesNo`, and requested as the default agent. It is safe because:
  - it refuses every call that does not come from the current owner of `org.bluez`;
  - **it answers only the pairing the person started from the bar** (maintainer decision D8, 2026-09-29). The agent keeps one field, `pairing`: the object path of the device whose row the person pressed, set before `Pair` is called and cleared when `Pair` returns. `RequestConfirmation`, `DisplayPasskey`, `DisplayPinCode` and `RequestAuthorization` for any other device, or with no pairing in progress, get `org.bluez.Error.Rejected` and show nothing: no page opens and no open popover closes;
  - it confirms a passkey only after a press on Pair on that device's page;
  - **`AuthorizeService` follows the same rule:** it is granted only for the device of the pairing in progress. After a pairing from the bar, the bar sets the device `Trusted`, and bluetoothd never asks the agent again for a trusted device. A device paired elsewhere and not trusted is refused; the person trusts it in Settings;
  - it refuses a legacy PIN request and a passkey entry request.
- **Joining a new network sends the password inline.** `AddAndActivateConnection` carries the `psk` in `802-11-wireless-security`. NetworkManager stores the password in its own connection profile, and the bar stores nothing. The connection is system-wide, and polkit decides: the call allows interactive authorisation. Task 9 records the image's default for `org.freedesktop.NetworkManager.settings.modify.system`, and the item 15 procedure records whether a prompt appeared. The secret agent covers the other path, a saved network whose password NetworkManager asks for again.
- **Out of scope; they need Settings:**
  - Enterprise (802.1X) and WEP networks show as rows that cannot be pressed, labelled "needs Settings".
  - Hidden networks are not listed.
  - Legacy Bluetooth PIN entry is refused.
- **Airplane mode is NetworkManager's `WirelessEnabled` and `WwanEnabled`.** Bluetooth keeps its own power switch.
- **The battery module shows only when UPower reports a present battery** (SH1). The power profile and the screen brightness live in its popover, so a desktop without a battery shows neither.
- **Power profiles go over `org.freedesktop.UPower.PowerProfiles`.** BR3 names `tuned-ppd`, and power-profiles-daemon 0.20 and later owns the same name, so either daemon works.
- **Brightness goes through logind's `Session.SetBrightness`** on the display session (`session/auto`, as the power menu does). The level is read from sysfs, and nothing goes through COSMIC's settings daemon.
- **The media controls live in the audio popover** and hide when no player is present.
- **There is no CSS change.** The rows reuse `bar-row`, `bar-confirm`, `bar-popover-title` and `bar-popover-note`.
- **There is one service per process, whatever the number of outputs.** A prompt, whether a password or a passkey, shows on one output: the one whose module popover was opened last, else the first on which the module shows.
- **The existing scenes run without fixtures.** The new modules find no service there and hide, so the goldens of 2b.2 and 2b.3 stay unchanged. The rig image is layered on the published one for the same reason: fonts, Mesa and GTK do not move.
- **dbusmock in Fedora 43 (open doubt 4), confirmed on `registry.fedoraproject.org/fedora:43`.** python3-dbusmock is 0.38.0-1.fc43 and ships these templates:
  - bluez5, bluez5-obex, gnome_screensaver, gsd_rfkill, logind, low_memory_monitor, modemmanager, networkmanager, notification_daemon, ofono, polkitd, power_profiles_daemon, systemd, timedated, upower, upower_power_profiles_daemon, urfkill.

  The four this plan needs are present. The fixtures cover four gaps:
  - The NetworkManager template has no AgentManager: the fixture adds one with `AddObject`.
  - The NetworkManager template does not check the `psk` it is sent: the end-to-end test reads it back from the mock's in-memory call log (`GetMethodCalls`), never from a file.
  - The NetworkManager template never calls an agent: a method only the mock has calls `GetSecrets` on the bar.
  - The BlueZ template's `Pair` pairs at once and never calls an agent: the fixture overrides `org.bluez.Device1.Pair` on the nearby devices with a method that calls the bar's `RequestConfirmation`, as bluetoothd does, and marks the device paired only when the bar confirms. A second fixture method, `RequestConfirmation`, calls the agent with no pairing in progress, to prove that an unsolicited request is rejected.

  logind, the fifth template BR9 names, is present too. The rig keeps `bar_session.py`'s own logind, which 2b.2 needs for a method that never answers, and adds `SetBrightness` to it.
- **dbusmock logs every call with its arguments,** so the NetworkManager mock runs with `-l /dev/null`. Without it a password would reach the logs. dbusmock also emits every call, arguments included, as the `org.freedesktop.DBus.Mock.MethodCalled` signal on the rig's private system bus. No file results, so never run `dbus-monitor` or a bus capture in the rig while a password test runs.
- **Item 15 cannot run in the dev VM,** which has no Wi-Fi and no Bluetooth radio. It is a procedure the maintainer follows on the desktop, written out at the end of Task 9.

## Global Constraints

- English in code, comments, commits and docs. There is no attribution line anywhere and no model name.
- No `|| true`, no `continue-on-error`, and no `let _ =` on a result in non-test code. An error is logged at the right priority or returned.
- `panic = "abort"` on dev and release. Anything that comes from a D-Bus peer, from libpulse or from sysfs is untrusted: no `unwrap`, `expect` or indexing on it in non-test code, and every variant is type-checked before it is read.
- **A password or a passkey is never logged, never formatted into an error, and never written to a file** by the bar, the fixtures or the tests. The NetworkManager mock runs with `-l /dev/null`. The end-to-end test fails if the typed password appears in any file the bar can write or the rig keeps: `/out` recursively, the scene's `$XDG_CONFIG_HOME`, `$XDG_CACHE_HOME`, `$XDG_STATE_HOME`, `$XDG_DATA_HOME` and `$XDG_RUNTIME_DIR`, `/run/user/1000` and `/tmp`.
- The texts shown come from peers (SSIDs, device names, connection ids, media titles). They go through `athanor_unit::text::line` with `NAME_CHARS`, which strips control and bidirectional characters and truncates, before any widget shows them.
- Names, verbatim:
  - the NetworkManager agent path is `/org/freedesktop/NetworkManager/SecretAgent` and its identifier is `os.athanor.Bar`;
  - the BlueZ agent path is `/os/athanor/Bar/BluezAgent`, with the capability `DisplayYesNo`;
  - the module ids are `network`, `bluetooth`, `audio` and `battery`, as `order.rs` has them;
  - the scenes are `bar-network`, `bar-bluetooth`, `bar-audio` and `bar-battery`.
- A module whose source is absent is not shown (BR3, SH1).
- At most one popover of the bar is open at a time (BR6). A popover scene starts open (SH13).
- Memory: `athanor-bar` stays at most 64 MB PSS with every module loaded (section 5, item 17).
- New code goes in `forge/specs/athanor-bar` and the rig. No new library crate. Each module keeps its own files, and shared files change by the lines this plan shows.
- Never edit `scripts/verify.py`, `forge/config/packages.json` or `docs/architecture/*.md` with the Edit or Write tool: the formatter rewrites the whole file. Edit them with a short `python3` script through Bash, then check that `git diff --stat` shows only your lines.
- Never prefix a command with `cd`. Run podman, git writes and gh unsandboxed.
- **`RIG_CARGO`** below stands for this command, run from the repository root. It is `rig.sh build-bar`'s container with a filter:

  ```bash
  podman run --rm --memory 6g --security-opt label=disable \
      -v "$(git rev-parse --show-toplevel):/repo:ro" -v "$(git rev-parse --show-toplevel)/.scratch/shell-rig:/out" \
      -v athanor-cargo-registry:/root/.cargo/registry -e CARGO_TARGET_DIR=/out/target -w /repo \
      localhost/athanor-shell-rig:build cargo test --locked -p athanor-bar
  ```

  It takes extra arguments, for example `RIG_CARGO --lib network::`.
- Commit messages follow `git log -10`: `feat(bar): …`, `build(bar): …`, `test(shell): …`, `ci(shell): …`, `test(devvm): …`, `docs(bar): …`.

## Review Focus

1. **The sound server restarts, or libpulse calls back while the bar is inside libpulse.** `pipewire-pulse` can restart under a running bar. libpulse can also fire the state callback inside `connect()`, and the introspection callbacks run inside its dispatch. A reasonable person expects the audio module to hide, come back within seconds, and never abort. The two rules:
   - a libpulse callback only schedules work on the main loop and never touches the `Context`;
   - every setter checks that the context is `Ready`, because libpulse-binding panics on the null operation a setter gets from a context that is not ready.

   Pinned in Task 6 by the "sound server restarts" steps of `bar_modules_e2e.py`.
2. **NetworkManager or BlueZ restarts while a prompt is open.** The prompt must close. A new owner must get the agent registered again, without the bar restarting. Pinned in Task 4 by the "NetworkManager restarts" step of `bar_modules_e2e.py`, which checks that the new mock sees `Register`.
3. **A hostile SSID or device name.** Examples: non-UTF-8 bytes, a U+202E override, control characters, a thousand access points, or an empty SSID. A reasonable person expects a readable row, no reordered text, a bounded list, and hidden networks left out. Pinned in Task 2:
   - `a_hostile_ssid_is_sanitised_and_an_empty_one_is_left_out`;
   - `the_list_is_bounded`;
   - `a_device_name_is_sanitised`.
4. **A second request while a prompt is open, a call from a process that is not the service, or a Bluetooth request the person did not start.** A second `GetSecrets` must get `NoSecrets` and leave the first prompt alone. A `GetSecrets` or `RequestConfirmation` from any other bus peer must be refused without a prompt. A BlueZ request about a device the person did not press in the bar must be rejected with nothing shown and no popover closed (decision D8). Pinned by the "refused sender" and "second request" steps in Task 4, and the "unsolicited" and "another process" steps in Task 5.
5. **Two outputs, and memory with libpulse loaded.** A prompt must appear once, not on every surface. The popovers of the new modules must not push PSS over 64 MB. The single prompt is pinned by `prompt_view` choosing one view (Task 4); the dev VM's two-head stage of 2b.2 runs with the new modules built. Memory is pinned by the PSS check at the end of `bar_modules_e2e.py` (Task 3) and by the dev VM's `memory` stage.

---

## File Structure

```
forge/specs/athanor-bar/
  athanor-bar.spec                        MODIFY: BuildRequires pulseaudio-libs-devel, changelog (Task 9)
  athanor-bar-1.0.0/
    Cargo.toml                            MODIFY (shared): libpulse-binding, libpulse-glib-binding
    src/lib.rs                            MODIFY (shared): pub mod props, network, bluetooth, audio, battery
    src/props.rs                          NEW: property maps, and the signals of D-Bus services, type-checked
    src/network.rs                        NEW: NetworkManager state, Wi-Fi settings, secret-agent requests
    src/bluetooth.rs                      NEW: BlueZ state and the passkey
    src/audio.rs                          NEW: volume arithmetic, icons, MPRIS metadata
    src/battery.rs                        NEW: UPower battery, power profiles, backlight
    src/ui/mod.rs                         MODIFY (shared): six mod lines, four build arms, Bar::fit_groups
    src/ui/bus.rs                         NEW: calls with a timeout, and a mirror of a service's objects
    src/ui/network.rs                     NEW: the network module and NetworkManager's secret agent
    src/ui/bluetooth.rs                   NEW: the Bluetooth module and the BlueZ agent
    src/ui/audio.rs                       NEW: the audio module over libpulse
    src/ui/mpris.rs                       NEW: media controls in the audio popover
    src/ui/battery.rs                     NEW: the battery module, profiles and brightness
    po/POTFILES.in, po/athanor-bar.pot, po/en.po, po/it.po   MODIFY (shared, Task 8)

Cargo.lock                                MODIFY (shared): the libpulse crates only

forge/test/shell/Containerfile            MODIFY (shared): rig-fresh, RIG_BASE, the fixtures' packages
forge/test/shell/rig-image.digest         MODIFY (shared): the republished rig, pinned by the executor (Task 1, Step 6)
forge/test/shell/rig.sh                   MODIFY (shared): build-image/publish-image with RIG_BASE, bar-modules-e2e, atspi bar-modules, capture_bar, surface list
forge/test/shell/system_fixtures.py       NEW: dbusmock services, PipeWire with null sinks, an MPRIS player, a backlight
forge/test/shell/bar_session.py           MODIFY (shared): --fixtures, logind SetBrightness
forge/test/shell/bar_modules_e2e.py       NEW: the four modules end to end
forge/test/shell/cases.py                 MODIFY (shared): four scenes
forge/test/shell/tests/test_cases.py      MODIFY (shared): thirteen scenes, 156 cases
forge/test/shell/locale/bar-de.po         MODIFY (shared): the new messages
forge/test/shell/golden/bar-{network,bluetooth,audio,battery}/*.png   NEW: 48 goldens
.github/workflows/shell-surfaces.yml      MODIFY (shared): a bar step, four matrix entries

scripts/devvm/bar-acceptance.sh           MODIFY (shared): stage modules, a library check in deploy
scripts/devvm/bar_modules.py              NEW: the modules in the VM over AT-SPI
forge/config/packages.json                MODIFY (shared): pulseaudio-libs-glib2 in upstream_desktop (Task 9)
```

"Shared" marks the files 2b.3 or 2c may also change. Merge conflicts are expected there and nowhere else.

---

### Task 1: The rig image gains the fixtures' packages, layered on the published one

**Files:**
- Modify: `forge/test/shell/Containerfile`
- Modify: `forge/test/shell/rig.sh` (header comment; `build-image`, `publish-image`)
- Modify: `forge/test/shell/rig-image.digest` (by the executor, after `publish-image`, Step 6)

**Interfaces:**
- Consumes: the published rig at `forge/test/shell/rig-image.digest` (today `sha256:85c2909d…`).
- Produces:
  - a rig image with python3-dbusmock 0.38.0, pipewire, wireplumber, pipewire-pulse, `pactl` and `libpulse-mainloop-glib.so.0`;
  - a build image with `pulseaudio-libs-devel`.

  Every later task runs in them.

The published rig carries the pixels of every golden: fonts, Mesa, GTK, COSMIC. Rebuilding it from Fedora would move all of them. The new packages are therefore a layer on top of the published image, and only a first build from scratch (no digest file) builds the base.

- [ ] **Step 1: Layer the Containerfile**

Replace the first line of the stage header, and add the new `rig` stage between the base and the build stage. Apply with Edit, in three places.

(a) Before the first `FROM`, and on that `FROM`, the stage name:

```dockerfile
# RIG_BASE: the image the fixtures' layer goes on. rig.sh passes the published rig, pinned by
# digest, so the pixels of every golden stay; with no published rig it is the base below.
ARG RIG_BASE=rig-fresh
FROM registry.fedoraproject.org/fedora@sha256:0b52d7c65426cdb567000481d0d1b056040b4d80c1a479bc967383c41aae5809 AS rig-fresh
```

(b) Between `RUN mkdir -p /run/user/1000 && chmod 700 /run/user/1000` and `# The build stage compiles…`:

```dockerfile

# The fixtures of the system modules (doc_bar.md, BR9): python3-dbusmock for NetworkManager,
# BlueZ, UPower and the power profiles, and PipeWire with its PulseAudio server for the audio
# module. libpulse-mainloop-glib is what athanor-bar links for audio.
FROM ${RIG_BASE} AS rig
RUN dnf5 -y install --setopt=install_weak_deps=False \
      python3-dbusmock pipewire wireplumber pipewire-pulseaudio pipewire-utils \
      pulseaudio-utils pulseaudio-libs-glib2 \
  && dnf5 clean all
```

(c) In the build stage's package list, after `systemd-devel`:

```dockerfile
      speech-dispatcher-devel upower-devel pam-devel tpm2-tss-devel systemd-devel \
      pulseaudio-libs-devel \
```

- [ ] **Step 2: Pass the published rig as the base in `rig.sh`**

After `rig_image() { … }`, add:

```bash
# The base of the fixtures' layer: the published rig when there is one (see the Containerfile).
rig_base=()
if [ -s "$rig/rig-image.digest" ]; then
    rig_base=(--build-arg "RIG_BASE=$registry/athanor-shell-rig@$(cat "$rig/rig-image.digest")")
fi
```

Replace the `build-image` and `publish-image` arms:

```bash
build-image)
    podman build "${rig_base[@]}" --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    podman build "${rig_base[@]}" --target build -t "$local_image:build" -f "$rig/Containerfile" "$rig"
    ;;
publish-image)
    podman build "${rig_base[@]}" --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    podman push --digestfile "$out/rig-image.digest" "$local_image:rig" "docker://$registry/athanor-shell-rig:latest"
    echo "published $registry/athanor-shell-rig@$(cat "$out/rig-image.digest")"
    echo "commit that digest as forge/test/shell/rig-image.digest together with the goldens it changes"
    ;;
```

In the header comment, replace the `build-image` line:

```bash
#   rig.sh build-image      build the rig and build stages locally, layered on the published rig
```

- [ ] **Step 3: Build and check the layer**

Run:

```bash
bash forge/test/shell/rig.sh build-image
podman run --rm localhost/athanor-shell-rig:rig bash -c \
  'rpm -q python3-dbusmock pipewire wireplumber pipewire-pulseaudio pulseaudio-utils pulseaudio-libs-glib2 \
   && python3 -c "import dbusmock; print(dbusmock.__version__)" \
   && ls /usr/lib/python3*/site-packages/dbusmock/templates/ | grep -E "^(networkmanager|bluez5|upower|upower_power_profiles_daemon)\.py$" | wc -l'
podman run --rm localhost/athanor-shell-rig:build pkg-config --modversion libpulse-mainloop-glib
```

Expected:
- `rpm -q` prints six installed packages, and python3-dbusmock is `0.38.0-1.fc43` or later;
- then `0.38.0`, then `4`;
- the build image prints a libpulse version (17.0 on Fedora 43).

- [ ] **Step 4: The existing scenes are unchanged in the layered image**

Run each of the nine bar scenes of 2b.2 and 2b.3, and `greeter`, against the local image:

```bash
for scene in bar bar-power bar-input bar-calendar bar-accessibility bar-tiling \
             bar-popups bar-notifications bar-tray greeter; do
  ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface "$scene" || exit 1
done
```

Run `bash forge/test/shell/rig.sh build-bar` first if `.scratch/shell-rig/bin/athanor-bar` is missing, and `build-shelld` if `.scratch/shell-rig/bin/athanor-shelld` is missing: `bar-tray` runs the real watcher.

Expected: every case PASS.

A failure means dnf upgraded a shared library under the layer. Stop and report it: the layer must add packages, never replace them.

- [ ] **Step 5: Commit**

```bash
git add forge/test/shell/Containerfile forge/test/shell/rig.sh
git commit -m "test(shell): layer the dbusmock and PipeWire fixtures on the published rig image"
```

- [ ] **Step 6: Publish the rig image and pin its digest (gate for every later push)**

From Task 6 on, `athanor-bar` links `libpulse-mainloop-glib.so.0`. The published rig lacks `pulseaudio-libs-glib2`, so CI would fail every bar job, the existing ones included, until the new image is published and pinned. The maintainer consented on 2026-09-29 (decision D9): the executor publishes the image now, from the host, and it stays public as today. Run these outside the sandbox:

```bash
gh auth token | podman login ghcr.io -u hr-mes --password-stdin
bash forge/test/shell/rig.sh publish-image
cp .scratch/shell-rig/rig-image.digest forge/test/shell/rig-image.digest
podman pull "ghcr.io/hr-mes/athanor-shell-rig@$(cat forge/test/shell/rig-image.digest)"
bash forge/test/shell/rig.sh surface bar-power
git add forge/test/shell/rig-image.digest
git commit -m "test(shell): pin the rig image with the system modules' fixtures"
```

Expected:
- `publish-image` prints `published ghcr.io/hr-mes/athanor-shell-rig@sha256:…`;
- the pull by digest succeeds, and `surface bar-power`, now on the pinned digest without `ATHANOR_RIG_IMAGE`, passes every case.

The new image is the old one plus a layer, so no golden changes with the digest. The digest commit is its own commit, and it comes before any commit that links libpulse is pushed. If 2b.3 or 2c pinned a newer rig in the meantime, rebuild on top of their digest (`build-image` layers on whatever `rig-image.digest` names) and publish again; never merge two digests by hand.

---

### Task 2: The logic of the four modules, in the library, with unit tests

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml` (two dependencies)
- Modify: `Cargo.lock` (regenerated by cargo, the libpulse crates only)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs` (five `pub mod` lines)
- Create: `src/props.rs`, `src/network.rs`, `src/bluetooth.rs`, `src/audio.rs`, `src/battery.rs` (under `forge/specs/athanor-bar/athanor-bar-1.0.0/`)

**Interfaces:**
- Consumes: `athanor_unit::text::{line, NAME_CHARS}`; `glib::Variant`.
- Produces, for Tasks 4 to 7. These are exact signatures:
  - `props`:
    - the types `Props = BTreeMap<String, Variant>`, `Interfaces = BTreeMap<String, Props>` and `Objects = BTreeMap<String, Interfaces>`;
    - `enum Change { Properties { path, interface, changed: Props, invalidated: Vec<String> }, Added { path, interfaces: Interfaces }, Removed { path, interfaces: Vec<String> } }`;
    - `fn has_type(&Variant, &str) -> bool`;
    - `fn managed_objects(&Variant) -> Option<Objects>` and `fn get_all(&Variant) -> Option<Props>`;
    - `fn change(path: &str, interface: &str, member: &str, params: &Variant) -> Option<Change>` and `fn apply(&mut Objects, Change)`;
    - `fn lookup<'a>(&'a Objects, path: &str, interface: &str) -> Option<&'a Props>`;
    - `fn value<T: FromVariant>(&Props, &str) -> Option<T>`, `fn path(&Props, &str) -> Option<String>` and `fn paths(&Props, &str) -> Vec<String>`;
    - `fn object_path(&str) -> Option<Variant>`.
  - `network`:
    - the constants listed in the file;
    - the types `Security`, `Link`, `Network`, `Wifi`, `Vpn`, `NetworkState`, `ConnectionInfo`, `SecretsRequest` and `PasswordPrompt`;
    - `fn connection_info(&Variant) -> Option<ConnectionInfo>` and `fn state(&Objects, &BTreeMap<String, ConnectionInfo>) -> Option<NetworkState>`;
    - `fn icon(&NetworkState) -> &'static str`;
    - `fn add_and_activate(&Network, Option<&str>, device: &str) -> Option<Variant>`, `fn activate(connection: &str, device: &str, specific: &str) -> Option<Variant>` and `fn deactivate(active: &str) -> Option<Variant>`;
    - `fn password_acceptable(Security, &str) -> bool`;
    - `fn secrets_request(&Variant) -> Option<SecretsRequest>`, `fn secrets_reply(&str) -> Variant` and `fn cancel_request(&Variant) -> Option<(String, String)>`.
  - `bluetooth`:
    - the types `Device` and `BluetoothState`;
    - `fn state(&Objects) -> Option<BluetoothState>` and `fn module_icon(&BluetoothState) -> &'static str`;
    - `fn passkey_label(u32) -> Option<String>`, `fn device_and_passkey(&Variant) -> Option<(String, u32)>` and `fn device_of(&Variant) -> Option<String>`;
    - `fn device_label(&Objects, &str) -> Option<String>`.
  - `audio`:
    - `NORMAL` and `MAX_DEVICES`;
    - `fn percent(u32) -> f64` and `fn raw(f64) -> u32`;
    - `fn output_icon(f64, bool) -> &'static str` and `fn input_icon(bool) -> &'static str`;
    - `fn device_label(Option<&str>, &str) -> String`;
    - the types `Device` and `Track`;
    - `fn chosen(&[Device], Option<&str>) -> Option<usize>`;
    - `fn track(&Props) -> Option<Track>`, `fn is_player(&str) -> bool` and `fn playing(&Props) -> bool`.
  - `battery`:
    - the types `Charge`, `Battery` and `Backlight`;
    - `fn battery(&Props) -> Option<Battery>`, `fn icon(&Battery) -> &'static str` and `fn hours_minutes(u64) -> (u64, u64)`;
    - `fn profiles(&Props) -> Option<(Vec<&'static str>, String)>`;
    - `fn read_backlight(&Path) -> Option<Backlight>`, `Backlight::percent(&self) -> f64` and `Backlight::raw(&self, f64) -> u32`.

- [ ] **Step 1: Add the libpulse crates and regenerate the lock file**

In `forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml`, after the `gtk4-layer-shell` line, add:

```toml
# The audio module (doc_bar.md, BR3): the PulseAudio protocol that pipewire-pulse serves, on
# the bar's GLib main loop. Not in the workspace table: only the bar uses them.
libpulse-binding = "2.30"
libpulse-glib-binding = "2.29"
```

`libpulse-glib-binding` 2.29 depends on `glib = "^0"`, so cargo unifies it with the workspace's glib 0.22. The `links` key of `glib-sys` would refuse a second copy.

Regenerate `Cargo.lock` in the build image (unsandboxed):

```bash
podman run --rm --security-opt label=disable -v "$(git rev-parse --show-toplevel):/repo" \
    -v athanor-cargo-registry:/root/.cargo/registry -w /repo \
    localhost/athanor-shell-rig:build cargo metadata --format-version 1 >/dev/null
git diff --stat Cargo.lock
git diff Cargo.lock | grep -c '^-[^-]'
```

Expected:
- the stat shows only `Cargo.lock` with insertions;
- the count is `0`: no existing package changed version;
- the new packages are `libpulse-binding`, `libpulse-sys`, `libpulse-glib-binding`, `libpulse-mainloop-glib-sys` and their small dependencies (`num-derive`, `num-traits` if absent).

If a `-` line appears, stop. Cargo moved an existing crate, and that is a separate change.

- [ ] **Step 2: Declare the modules**

In `src/lib.rs`, add these five lines, each in its alphabetical place:

```rust
pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod network;
pub mod props;
```

With Unit A and 2b.3 merged, the list then reads `audio`, `battery`, `bluetooth`, `clock`, `dbusmenu`, `keyboard`, `network`, `notices`, `order`, `popups`, `power`, `props`, `tiling`, `tray`.

Replace the doc comment with:

```rust
//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, what logind offers, the time zone, and the state of NetworkManager,
//! BlueZ, the sound server and UPower. The binary draws it.
```

- [ ] **Step 3: Write `src/props.rs` with its tests**

```rust
//! The objects and properties of a D-Bus service, mirrored from `GetManagedObjects`, `GetAll`
//! and their signals. Everything a peer sends is type-checked here before anything reads it:
//! a reply or a signal of the wrong type is `None`, never a panic.

use std::collections::BTreeMap;

use glib::prelude::*;
use glib::variant::{FromVariant, ObjectPath};
use glib::{Variant, VariantTy};

pub type Props = BTreeMap<String, Variant>;
pub type Interfaces = BTreeMap<String, Props>;
pub type Objects = BTreeMap<String, Interfaces>;

/// One signal of `org.freedesktop.DBus.Properties` or `org.freedesktop.DBus.ObjectManager`.
#[derive(Debug)]
pub enum Change {
    Properties {
        path: String,
        interface: String,
        changed: Props,
        invalidated: Vec<String>,
    },
    Added {
        path: String,
        interfaces: Interfaces,
    },
    Removed {
        path: String,
        interfaces: Vec<String>,
    },
}

pub fn has_type(variant: &Variant, signature: &str) -> bool {
    VariantTy::new(signature).is_ok_and(|ty| variant.is_type(ty))
}

/// The pair `(o, a{sa{sv}})` of a managed object; `get` cannot read it, since `String` is `s`.
fn object_entry(entry: &Variant) -> Option<(String, Interfaces)> {
    let path = entry.try_child_value(0)?.str()?.to_owned();
    let interfaces = entry.try_child_value(1)?.get::<Interfaces>()?;
    Some((path, interfaces))
}

/// The reply of `GetManagedObjects`, `(a{oa{sa{sv}}})`.
pub fn managed_objects(reply: &Variant) -> Option<Objects> {
    if !has_type(reply, "(a{oa{sa{sv}}})") {
        return None;
    }
    reply.try_child_value(0)?.iter().map(|entry| object_entry(&entry)).collect()
}

/// The reply of `GetAll`, `(a{sv})`.
pub fn get_all(reply: &Variant) -> Option<Props> {
    reply.get::<(Props,)>().map(|(props,)| props)
}

/// A signal as a change of the mirror; `None` for any other member or a wrong type.
pub fn change(path: &str, interface: &str, member: &str, params: &Variant) -> Option<Change> {
    match (interface, member) {
        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
            let (interface, changed, invalidated) = params.get::<(String, Props, Vec<String>)>()?;
            Some(Change::Properties {
                path: path.to_owned(),
                interface,
                changed,
                invalidated,
            })
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
            if !has_type(params, "(oa{sa{sv}})") {
                return None;
            }
            let (path, interfaces) = object_entry(params)?;
            Some(Change::Added { path, interfaces })
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
            if !has_type(params, "(oas)") {
                return None;
            }
            let path = params.try_child_value(0)?.str()?.to_owned();
            let interfaces = params.try_child_value(1)?.get::<Vec<String>>()?;
            Some(Change::Removed { path, interfaces })
        }
        _ => None,
    }
}

/// Applies `change`. A property change of an object or interface the mirror does not hold is
/// ignored: the mirror learns of objects from `GetManagedObjects` and `InterfacesAdded` only.
pub fn apply(objects: &mut Objects, change: Change) {
    match change {
        Change::Properties {
            path,
            interface,
            changed,
            invalidated,
        } => {
            if let Some(props) = objects
                .get_mut(&path)
                .and_then(|interfaces| interfaces.get_mut(&interface))
            {
                props.extend(changed);
                for name in invalidated {
                    props.remove(&name);
                }
            }
        }
        Change::Added { path, interfaces } => {
            objects.entry(path).or_default().extend(interfaces);
        }
        Change::Removed { path, interfaces } => {
            if let Some(held) = objects.get_mut(&path) {
                for name in interfaces {
                    held.remove(&name);
                }
                if held.is_empty() {
                    objects.remove(&path);
                }
            }
        }
    }
}

pub fn lookup<'a>(objects: &'a Objects, path: &str, interface: &str) -> Option<&'a Props> {
    objects.get(path)?.get(interface)
}

/// A property of type `T`; `None` when it is absent or of another type.
pub fn value<T: FromVariant>(props: &Props, name: &str) -> Option<T> {
    props.get(name)?.get::<T>()
}

/// An object-path property; `None` for `/`, which D-Bus services use for "none".
pub fn path(props: &Props, name: &str) -> Option<String> {
    let value = props.get(name)?;
    if !value.is_type(VariantTy::OBJECT_PATH) {
        return None;
    }
    value.str().filter(|path| *path != "/").map(str::to_owned)
}

/// An `ao` property; empty when it is absent or of another type.
pub fn paths(props: &Props, name: &str) -> Vec<String> {
    match props.get(name) {
        Some(value) if value.is_type(VariantTy::OBJECT_PATH_ARRAY) => value
            .iter()
            .filter_map(|path| path.str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// `path` as an `o` value; `None` when it is not a valid object path.
pub fn object_path(path: &str) -> Option<Variant> {
    ObjectPath::try_from(path).ok().map(|path| path.to_variant())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(signature: &str, text: &str) -> Variant {
        Variant::parse(Some(VariantTy::new(signature).unwrap()), text).unwrap()
    }

    fn objects() -> Objects {
        managed_objects(&parse(
            "(a{oa{sa{sv}}})",
            "({objectpath '/a': {'x.Y': {'On': <true>, 'Name': <'a'>}}},)",
        ))
        .unwrap()
    }

    #[test]
    fn managed_objects_are_read_and_a_wrong_type_is_none() {
        let objects = objects();
        assert_eq!(value::<bool>(lookup(&objects, "/a", "x.Y").unwrap(), "On"), Some(true));
        assert!(managed_objects(&parse("(a{sv})", "({'On': <true>},)")).is_none());
    }

    #[test]
    fn a_property_change_updates_and_invalidates() {
        let mut objects = objects();
        let params = parse("(sa{sv}as)", "('x.Y', {'On': <false>}, ['Name'])");
        apply(&mut objects, change("/a", "org.freedesktop.DBus.Properties", "PropertiesChanged", &params).unwrap());
        let props = lookup(&objects, "/a", "x.Y").unwrap();
        assert_eq!(value::<bool>(props, "On"), Some(false));
        assert!(props.get("Name").is_none());
    }

    #[test]
    fn a_change_of_an_unknown_object_is_ignored() {
        let mut objects = objects();
        let params = parse("(sa{sv}as)", "('x.Y', {'On': <false>}, @as [])");
        apply(&mut objects, change("/b", "org.freedesktop.DBus.Properties", "PropertiesChanged", &params).unwrap());
        assert_eq!(objects.len(), 1);
        assert!(objects.get("/b").is_none());
    }

    #[test]
    fn objects_come_and_go_with_their_interfaces() {
        let mut objects = objects();
        let added = parse("(oa{sa{sv}})", "(objectpath '/b', {'x.Z': @a{sv} {}})");
        apply(&mut objects, change("/", "org.freedesktop.DBus.ObjectManager", "InterfacesAdded", &added).unwrap());
        assert!(lookup(&objects, "/b", "x.Z").is_some());
        let removed = parse("(oas)", "(objectpath '/b', ['x.Z'])");
        apply(&mut objects, change("/", "org.freedesktop.DBus.ObjectManager", "InterfacesRemoved", &removed).unwrap());
        assert!(objects.get("/b").is_none());
    }

    #[test]
    fn a_signal_of_the_wrong_type_is_none() {
        let wrong = parse("(s)", "('x',)");
        for (interface, member) in [
            ("org.freedesktop.DBus.Properties", "PropertiesChanged"),
            ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded"),
            ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved"),
            ("x.Y", "Other"),
        ] {
            assert!(change("/a", interface, member, &wrong).is_none(), "{member}");
        }
    }

    #[test]
    fn paths_are_type_checked_and_root_is_none() {
        let props = get_all(&parse(
            "(a{sv})",
            "({'One': <objectpath '/x'>, 'Root': <objectpath '/'>, 'Text': <'/x'>, 'Many': <@ao ['/x', '/y']>},)",
        ))
        .unwrap();
        assert_eq!(path(&props, "One").as_deref(), Some("/x"));
        assert_eq!(path(&props, "Root"), None);
        assert_eq!(path(&props, "Text"), None);
        assert_eq!(paths(&props, "Many"), ["/x", "/y"]);
        assert!(paths(&props, "Text").is_empty());
        assert!(object_path("not a path").is_none());
    }
}
```

- [ ] **Step 4: Run the props tests**

Run: `RIG_CARGO --lib props::`

Expected: 6 passed.

- [ ] **Step 5: Write `src/network.rs` with its tests**

```rust
//! NetworkManager as the network module shows it (doc_bar.md, BR3): the wired state, the
//! Wi-Fi networks, the VPNs and airplane mode; the settings of a network to join; and the
//! requests of NetworkManager's secret agent. Names and SSIDs come from the network itself:
//! they are sanitised and bounded here, and the list of networks is bounded too.

use std::collections::BTreeMap;

use athanor_unit::text::{line, NAME_CHARS};
use glib::prelude::*;
use glib::Variant;

use crate::props::{self, Objects, Props};

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// NetworkManager's object manager.
pub const NM_ROOT: &str = "/org/freedesktop";
pub const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const SETTINGS_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub const AGENT_MANAGER: &str = "org.freedesktop.NetworkManager.AgentManager";
pub const AGENT_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager/AgentManager";
pub const AGENT_IFACE: &str = "org.freedesktop.NetworkManager.SecretAgent";
/// Fixed by NetworkManager: it calls every agent at this path.
pub const AGENT_PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
pub const AGENT_ID: &str = "os.athanor.Bar";
pub const SECURITY_SETTING: &str = "802-11-wireless-security";
pub const MAX_NETWORKS: usize = 24;

const TYPE_ETHERNET: u32 = 1;
const TYPE_WIFI: u32 = 2;
const DEVICE_ACTIVATED: u32 = 100;
const ACTIVE_ACTIVATING: u32 = 1;
const ACTIVE_ACTIVATED: u32 = 2;
const AP_PRIVACY: u32 = 0x1;
const KEY_MGMT_PSK: u32 = 0x100;
const KEY_MGMT_8021X: u32 = 0x200;
const KEY_MGMT_SAE: u32 = 0x400;
const ALLOW_INTERACTION: u32 = 0x1;
const REQUEST_NEW: u32 = 0x2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA or WPA2 with a password; `sae` for WPA3 Personal.
    Personal { sae: bool },
    /// Enterprise, WEP, OWE: the bar does not join these; Settings does.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Link {
    Idle,
    Connecting,
    Connected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub ssid: Vec<u8>,
    pub label: String,
    pub strength: u8,
    pub security: Security,
    pub access_point: String,
    pub link: Link,
    /// The active connection, to disconnect.
    pub active: Option<String>,
    /// A saved connection for this SSID, to activate without a password.
    pub saved: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wifi {
    pub device: String,
    pub enabled: bool,
    pub networks: Vec<Network>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vpn {
    pub connection: String,
    pub label: String,
    pub active: Option<String>,
    pub link: Link,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkState {
    /// `None` without an Ethernet device; `Some(true)` when one is activated.
    pub wired: Option<bool>,
    pub wifi: Option<Wifi>,
    pub vpns: Vec<Vpn>,
    pub airplane: bool,
}

/// What the bar needs of a saved connection, from `GetSettings`. Never a secret:
/// `GetSettings` does not return them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub id: String,
    pub kind: String,
    pub ssid: Option<Vec<u8>>,
}

/// The reply of `Settings.Connection.GetSettings`, `(a{sa{sv}})`.
pub fn connection_info(reply: &Variant) -> Option<ConnectionInfo> {
    let (settings,) = reply.get::<(BTreeMap<String, Props>,)>()?;
    let connection = settings.get("connection")?;
    Some(ConnectionInfo {
        id: props::value::<String>(connection, "id").unwrap_or_default(),
        kind: props::value::<String>(connection, "type")?,
        ssid: settings
            .get("802-11-wireless")
            .and_then(|wireless| props::value::<Vec<u8>>(wireless, "ssid")),
    })
}

pub fn security(flags: u32, wpa: u32, rsn: u32) -> Security {
    let keys = wpa | rsn;
    if keys & KEY_MGMT_8021X != 0 {
        Security::Other
    } else if keys & (KEY_MGMT_PSK | KEY_MGMT_SAE) != 0 {
        Security::Personal {
            sae: keys & KEY_MGMT_PSK == 0,
        }
    } else if keys == 0 && flags & AP_PRIVACY == 0 {
        Security::Open
    } else {
        Security::Other
    }
}

fn link(state: u32) -> Link {
    match state {
        ACTIVE_ACTIVATING => Link::Connecting,
        ACTIVE_ACTIVATED => Link::Connected,
        _ => Link::Idle,
    }
}

/// The active connections: connection path -> (active path, link).
fn active_connections(objects: &Objects, manager: &Props) -> BTreeMap<String, (String, Link)> {
    props::paths(manager, "ActiveConnections")
        .into_iter()
        .filter_map(|active| {
            let props = props::lookup(objects, &active, ACTIVE)?;
            let connection = props::path(props, "Connection")?;
            let state = props::value::<u32>(props, "State").unwrap_or(0);
            Some((connection, (active, link(state))))
        })
        .collect()
}

fn device_type(objects: &Objects, device: &str) -> Option<u32> {
    props::value::<u32>(props::lookup(objects, device, DEVICE)?, "DeviceType")
}

fn networks(
    objects: &Objects,
    device: &str,
    connections: &BTreeMap<String, ConnectionInfo>,
    active: &BTreeMap<String, (String, Link)>,
) -> Vec<Network> {
    let saved_for = |ssid: &[u8]| {
        connections
            .iter()
            .find(|(_, info)| info.kind == "802-11-wireless" && info.ssid.as_deref() == Some(ssid))
            .map(|(path, _)| path.clone())
    };
    let Some(wireless) = props::lookup(objects, device, WIRELESS) else {
        return Vec::new();
    };
    let mut by_ssid: BTreeMap<Vec<u8>, Network> = BTreeMap::new();
    for access_point in props::paths(wireless, "AccessPoints") {
        let Some(ap) = props::lookup(objects, &access_point, ACCESS_POINT) else {
            continue;
        };
        let Some(ssid) = props::value::<Vec<u8>>(ap, "Ssid").filter(|ssid| !ssid.is_empty()) else {
            continue;
        };
        let label = line(&String::from_utf8_lossy(&ssid), NAME_CHARS);
        if label.trim().is_empty() {
            continue;
        }
        let saved = saved_for(&ssid);
        let (active_path, link) = saved
            .as_ref()
            .and_then(|path| active.get(path))
            .map_or((None, Link::Idle), |(path, link)| (Some(path.clone()), *link));
        let network = Network {
            label,
            strength: props::value::<u8>(ap, "Strength").unwrap_or(0).min(100),
            security: security(
                props::value::<u32>(ap, "Flags").unwrap_or(0),
                props::value::<u32>(ap, "WpaFlags").unwrap_or(0),
                props::value::<u32>(ap, "RsnFlags").unwrap_or(0),
            ),
            access_point,
            link,
            active: active_path,
            saved,
            ssid: ssid.clone(),
        };
        // Several access points of one network: one row, the strongest.
        match by_ssid.get(&ssid) {
            Some(held) if held.strength >= network.strength => {}
            _ => {
                by_ssid.insert(ssid, network);
            }
        }
    }
    let mut networks: Vec<Network> = by_ssid.into_values().collect();
    networks.sort_by(|a, b| {
        b.link
            .cmp(&a.link)
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.label.cmp(&b.label))
    });
    networks.truncate(MAX_NETWORKS);
    networks
}

/// The module's state; `None` when NetworkManager is absent, and then the module hides.
pub fn state(objects: &Objects, connections: &BTreeMap<String, ConnectionInfo>) -> Option<NetworkState> {
    let manager = props::lookup(objects, NM_PATH, NM)?;
    let wireless_enabled = props::value::<bool>(manager, "WirelessEnabled").unwrap_or(false);
    let wwan_enabled = props::value::<bool>(manager, "WwanEnabled").unwrap_or(false);
    let devices = props::paths(manager, "Devices");
    let active = active_connections(objects, manager);
    let wired: Vec<bool> = devices
        .iter()
        .filter(|device| device_type(objects, device) == Some(TYPE_ETHERNET))
        .map(|device| {
            props::lookup(objects, device, DEVICE)
                .and_then(|props| props::value::<u32>(props, "State"))
                == Some(DEVICE_ACTIVATED)
        })
        .collect();
    let wifi = devices
        .iter()
        .find(|device| device_type(objects, device) == Some(TYPE_WIFI))
        .map(|device| Wifi {
            device: device.clone(),
            enabled: wireless_enabled,
            networks: if wireless_enabled {
                networks(objects, device, connections, &active)
            } else {
                Vec::new()
            },
        });
    let mut vpns: Vec<Vpn> = connections
        .iter()
        .filter(|(_, info)| info.kind == "vpn" || info.kind == "wireguard")
        .map(|(path, info)| {
            let (active_path, link) = active
                .get(path)
                .map_or((None, Link::Idle), |(active, link)| (Some(active.clone()), *link));
            Vpn {
                connection: path.clone(),
                label: line(&info.id, NAME_CHARS),
                active: active_path,
                link,
            }
        })
        .filter(|vpn| !vpn.label.trim().is_empty())
        .collect();
    vpns.sort_by(|a, b| a.label.cmp(&b.label));
    vpns.truncate(MAX_NETWORKS);
    Some(NetworkState {
        wired: (!wired.is_empty()).then(|| wired.iter().any(|&on| on)),
        wifi,
        vpns,
        airplane: !wireless_enabled && !wwan_enabled,
    })
}

pub fn signal_icon(strength: u8) -> &'static str {
    match strength {
        81.. => "network-wireless-signal-excellent-symbolic",
        56..=80 => "network-wireless-signal-good-symbolic",
        31..=55 => "network-wireless-signal-ok-symbolic",
        6..=30 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// The module's icon: the best link wins, wired over Wi-Fi.
pub fn icon(state: &NetworkState) -> &'static str {
    if state.airplane {
        return "airplane-mode-symbolic";
    }
    if state.wired == Some(true) {
        return "network-wired-symbolic";
    }
    let best = state
        .wifi
        .as_ref()
        .and_then(|wifi| wifi.networks.iter().find(|network| network.link != Link::Idle));
    match (best, &state.wifi) {
        (Some(network), _) if network.link == Link::Connected => signal_icon(network.strength),
        (Some(_), _) => "network-wireless-acquiring-symbolic",
        (None, Some(wifi)) if !wifi.enabled => "network-wireless-disabled-symbolic",
        (None, Some(_)) => "network-wireless-offline-symbolic",
        (None, None) => "network-wired-disconnected-symbolic",
    }
}

/// The settings of a network to join. The password, when there is one, goes to
/// NetworkManager inside these settings and nowhere else.
fn wifi_settings(network: &Network, password: Option<&str>) -> Variant {
    let mut settings: BTreeMap<String, Props> = BTreeMap::new();
    settings.insert(
        "connection".into(),
        Props::from([
            ("type".into(), "802-11-wireless".to_variant()),
            ("id".into(), network.label.to_variant()),
        ]),
    );
    settings.insert(
        "802-11-wireless".into(),
        Props::from([
            ("ssid".into(), Variant::array_from_fixed_array(&network.ssid)),
            ("mode".into(), "infrastructure".to_variant()),
        ]),
    );
    if let (Security::Personal { sae }, Some(password)) = (network.security, password) {
        settings.insert(
            SECURITY_SETTING.into(),
            Props::from([
                ("key-mgmt".into(), if sae { "sae" } else { "wpa-psk" }.to_variant()),
                ("psk".into(), password.to_variant()),
            ]),
        );
    }
    settings.to_variant()
}

/// The arguments of `AddAndActivateConnection`, `(a{sa{sv}}oo)`.
pub fn add_and_activate(network: &Network, password: Option<&str>, device: &str) -> Option<Variant> {
    Some(Variant::tuple_from_iter([
        wifi_settings(network, password),
        props::object_path(device)?,
        props::object_path(&network.access_point)?,
    ]))
}

/// The arguments of `ActivateConnection`, `(ooo)`. A VPN passes `/` for the device and the
/// specific object: NetworkManager picks them.
pub fn activate(connection: &str, device: &str, specific: &str) -> Option<Variant> {
    Some(Variant::tuple_from_iter([
        props::object_path(connection)?,
        props::object_path(device)?,
        props::object_path(specific)?,
    ]))
}

/// The arguments of `DeactivateConnection`, `(o)`.
pub fn deactivate(active: &str) -> Option<Variant> {
    Some(Variant::tuple_from_iter([props::object_path(active)?]))
}

/// WPA2 Personal: 8 to 63 printable ASCII characters, or 64 hexadecimal digits. WPA3
/// Personal: any non-empty password of at most 128 characters without a control character.
pub fn password_acceptable(security: Security, password: &str) -> bool {
    match security {
        Security::Personal { sae: false } => {
            let printable = password.chars().all(|c| c.is_ascii_graphic() || c == ' ');
            (printable && (8..=63).contains(&password.len()))
                || (password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()))
        }
        Security::Personal { sae: true } => {
            !password.is_empty() && password.chars().count() <= 128 && !password.chars().any(char::is_control)
        }
        Security::Open | Security::Other => false,
    }
}

/// A `GetSecrets` call of NetworkManager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsRequest {
    pub connection: String,
    pub setting: String,
    pub flags: u32,
    pub label: String,
    pub key_mgmt: Option<String>,
}

/// What the password prompt shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordPrompt {
    pub label: String,
    pub security: Security,
    /// NetworkManager asks again because the password it had was refused.
    pub retry: bool,
}

/// The arguments of `GetSecrets`, `(a{sa{sv}}osasu)`.
pub fn secrets_request(params: &Variant) -> Option<SecretsRequest> {
    if !props::has_type(params, "(a{sa{sv}}osasu)") {
        return None;
    }
    let settings = params.try_child_value(0)?.get::<BTreeMap<String, Props>>()?;
    let connection = params.try_child_value(1)?.str()?.to_owned();
    let setting = params.try_child_value(2)?.str()?.to_owned();
    let flags = params.try_child_value(4)?.get::<u32>()?;
    let ssid = settings
        .get("802-11-wireless")
        .and_then(|wireless| props::value::<Vec<u8>>(wireless, "ssid"));
    let id = settings
        .get("connection")
        .and_then(|connection| props::value::<String>(connection, "id"))
        .unwrap_or_default();
    let label = match ssid {
        Some(ssid) => line(&String::from_utf8_lossy(&ssid), NAME_CHARS),
        None => line(&id, NAME_CHARS),
    };
    let key_mgmt = settings
        .get(SECURITY_SETTING)
        .and_then(|security| props::value::<String>(security, "key-mgmt"));
    Some(SecretsRequest {
        connection,
        setting,
        flags,
        label,
        key_mgmt,
    })
}

impl SecretsRequest {
    /// The prompt, when the bar can answer this request: a WPA or WPA3 Personal password, and
    /// NetworkManager allows interaction. Anything else is answered `NoSecrets`.
    pub fn prompt(&self) -> Option<PasswordPrompt> {
        if self.setting != SECURITY_SETTING || self.flags & ALLOW_INTERACTION == 0 || self.label.trim().is_empty() {
            return None;
        }
        let security = match self.key_mgmt.as_deref() {
            Some("wpa-psk") => Security::Personal { sae: false },
            Some("sae") => Security::Personal { sae: true },
            _ => return None,
        };
        Some(PasswordPrompt {
            label: self.label.clone(),
            security,
            retry: self.flags & REQUEST_NEW != 0,
        })
    }
}

/// The reply of `GetSecrets`, `(a{sa{sv}})`: the password and nothing else.
pub fn secrets_reply(password: &str) -> Variant {
    let secrets: BTreeMap<String, Props> =
        BTreeMap::from([(SECURITY_SETTING.to_owned(), Props::from([("psk".to_owned(), password.to_variant())]))]);
    (secrets,).to_variant()
}

/// The arguments of `CancelGetSecrets`, `(os)`: the connection and the setting.
pub fn cancel_request(params: &Variant) -> Option<(String, String)> {
    if !props::has_type(params, "(os)") {
        return None;
    }
    Some((
        params.try_child_value(0)?.str()?.to_owned(),
        params.try_child_value(1)?.str()?.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::VariantTy;

    fn op(path: &str) -> Variant {
        props::object_path(path).unwrap()
    }

    fn ops(paths: &[&str]) -> Variant {
        Variant::array_from_iter_with_type(VariantTy::OBJECT_PATH, paths.iter().map(|path| op(path)))
    }

    fn map(pairs: Vec<(&str, Variant)>) -> Props {
        pairs.into_iter().map(|(name, value)| (name.to_owned(), value)).collect()
    }

    fn add(objects: &mut Objects, path: &str, interface: &str, pairs: Vec<(&str, Variant)>) {
        objects
            .entry(path.to_owned())
            .or_default()
            .insert(interface.to_owned(), map(pairs));
    }

    fn ap(objects: &mut Objects, path: &str, ssid: &[u8], strength: u8, rsn: u32) {
        add(
            objects,
            path,
            ACCESS_POINT,
            vec![
                ("Ssid", ssid.to_vec().to_variant()),
                ("Strength", strength.to_variant()),
                ("Flags", u32::from(rsn != 0).to_variant()),
                ("WpaFlags", 0u32.to_variant()),
                ("RsnFlags", rsn.to_variant()),
            ],
        );
    }

    /// A manager with a wired device, a Wi-Fi device and the access points `aps`.
    fn world(aps: &[&str], active: &[&str], wireless: bool) -> Objects {
        let mut objects = Objects::new();
        add(
            &mut objects,
            NM_PATH,
            NM,
            vec![
                ("WirelessEnabled", wireless.to_variant()),
                ("WwanEnabled", false.to_variant()),
                ("Devices", ops(&["/d/eth0", "/d/wlan0"])),
                ("ActiveConnections", ops(active)),
            ],
        );
        add(&mut objects, "/d/eth0", DEVICE, vec![("DeviceType", 1u32.to_variant()), ("State", 100u32.to_variant())]);
        add(&mut objects, "/d/wlan0", DEVICE, vec![("DeviceType", 2u32.to_variant()), ("State", 30u32.to_variant())]);
        add(&mut objects, "/d/wlan0", WIRELESS, vec![("AccessPoints", ops(aps))]);
        objects
    }

    fn saved(ssid: &[u8]) -> ConnectionInfo {
        ConnectionInfo {
            id: String::from_utf8_lossy(ssid).into_owned(),
            kind: "802-11-wireless".into(),
            ssid: Some(ssid.to_vec()),
        }
    }

    #[test]
    fn no_manager_no_module() {
        assert_eq!(state(&Objects::new(), &BTreeMap::new()), None);
    }

    #[test]
    fn the_active_network_comes_first_then_the_strongest() {
        let mut objects = world(&["/ap/1", "/ap/2", "/ap/3"], &["/active/1"], true);
        ap(&mut objects, "/ap/1", b"Cafe", 60, 0);
        ap(&mut objects, "/ap/2", b"Home", 40, KEY_MGMT_PSK);
        ap(&mut objects, "/ap/3", b"Lab", 90, KEY_MGMT_PSK);
        add(
            &mut objects,
            "/active/1",
            ACTIVE,
            vec![("Connection", op("/s/1")), ("State", 2u32.to_variant())],
        );
        let connections = BTreeMap::from([("/s/1".to_owned(), saved(b"Home"))]);
        let state = state(&objects, &connections).unwrap();
        assert_eq!(state.wired, Some(true));
        let wifi = state.wifi.unwrap();
        let labels: Vec<_> = wifi.networks.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Home", "Lab", "Cafe"]);
        assert_eq!(wifi.networks[0].link, Link::Connected);
        assert_eq!(wifi.networks[0].active.as_deref(), Some("/active/1"));
        assert_eq!(wifi.networks[0].saved.as_deref(), Some("/s/1"));
        assert_eq!(wifi.networks[2].security, Security::Open);
    }

    #[test]
    fn a_hostile_ssid_is_sanitised_and_an_empty_one_is_left_out() {
        let mut objects = world(&["/ap/1", "/ap/2", "/ap/3"], &[], true);
        ap(&mut objects, "/ap/1", b"\xffEvil\xe2\x80\xaeNet\n", 50, 0);
        ap(&mut objects, "/ap/2", b"", 99, 0);
        ap(&mut objects, "/ap/3", b"\x07\x1b\n", 98, 0);
        let networks = state(&objects, &BTreeMap::new()).unwrap().wifi.unwrap().networks;
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].label, "\u{fffd}EvilNet");
        assert_eq!(networks[0].ssid, b"\xffEvil\xe2\x80\xaeNet\n");
    }

    #[test]
    fn the_list_is_bounded() {
        let names: Vec<String> = (0..1000).map(|n| format!("/ap/{n}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut objects = world(&refs, &[], true);
        for (n, path) in names.iter().enumerate() {
            ap(&mut objects, path, format!("net{n}").as_bytes(), (n % 100) as u8, 0);
        }
        let long = "x".repeat(500);
        ap(&mut objects, "/ap/0", long.as_bytes(), 100, 0);
        let networks = state(&objects, &BTreeMap::new()).unwrap().wifi.unwrap().networks;
        assert_eq!(networks.len(), MAX_NETWORKS);
        assert_eq!(networks[0].label.chars().count(), NAME_CHARS);
    }

    #[test]
    fn duplicates_keep_the_strongest_access_point() {
        let mut objects = world(&["/ap/1", "/ap/2"], &[], true);
        ap(&mut objects, "/ap/1", b"Mesh", 30, 0);
        ap(&mut objects, "/ap/2", b"Mesh", 70, 0);
        let networks = state(&objects, &BTreeMap::new()).unwrap().wifi.unwrap().networks;
        assert_eq!(networks.len(), 1);
        assert_eq!((networks[0].access_point.as_str(), networks[0].strength), ("/ap/2", 70));
    }

    #[test]
    fn security_from_the_flags() {
        assert_eq!(security(0, 0, 0), Security::Open);
        assert_eq!(security(1, 0, KEY_MGMT_PSK), Security::Personal { sae: false });
        assert_eq!(security(1, KEY_MGMT_PSK, 0), Security::Personal { sae: false });
        assert_eq!(security(1, 0, KEY_MGMT_SAE), Security::Personal { sae: true });
        assert_eq!(security(1, 0, KEY_MGMT_PSK | KEY_MGMT_SAE), Security::Personal { sae: false });
        assert_eq!(security(1, 0, KEY_MGMT_8021X), Security::Other);
        assert_eq!(security(1, 0, 0), Security::Other, "WEP");
    }

    #[test]
    fn airplane_is_both_radios_off_and_hides_the_list() {
        let mut objects = world(&["/ap/1"], &[], false);
        ap(&mut objects, "/ap/1", b"Cafe", 60, 0);
        let state = state(&objects, &BTreeMap::new()).unwrap();
        assert!(state.airplane);
        assert!(state.wifi.as_ref().unwrap().networks.is_empty());
        assert_eq!(icon(&state), "airplane-mode-symbolic");
    }

    #[test]
    fn vpns_come_from_the_saved_connections() {
        let mut objects = world(&[], &["/active/9"], true);
        add(&mut objects, "/active/9", ACTIVE, vec![("Connection", op("/s/9")), ("State", 1u32.to_variant())]);
        let connections = BTreeMap::from([
            ("/s/9".to_owned(), ConnectionInfo { id: "Office\u{202e}VPN".into(), kind: "vpn".into(), ssid: None }),
            ("/s/8".to_owned(), ConnectionInfo { id: "Home WG".into(), kind: "wireguard".into(), ssid: None }),
            ("/s/7".to_owned(), saved(b"Cafe")),
        ]);
        let vpns = state(&objects, &connections).unwrap().vpns;
        assert_eq!(vpns.len(), 2);
        assert_eq!((vpns[0].label.as_str(), vpns[0].link), ("Home WG", Link::Idle));
        assert_eq!((vpns[1].label.as_str(), vpns[1].link), ("OfficeVPN", Link::Connecting));
        assert_eq!(vpns[1].active.as_deref(), Some("/active/9"));
    }

    #[test]
    fn the_settings_carry_the_ssid_as_bytes_and_the_key_only_when_secured() {
        let mut network = Network {
            ssid: b"Lab\xff".to_vec(),
            label: "Lab\u{fffd}".into(),
            strength: 80,
            security: Security::Personal { sae: true },
            access_point: "/ap/1".into(),
            link: Link::Idle,
            active: None,
            saved: None,
        };
        let args = add_and_activate(&network, Some("correct horse"), "/d/wlan0").unwrap();
        assert_eq!(args.type_().as_str(), "(a{sa{sv}}oo)");
        let settings = args.try_child_value(0).unwrap().get::<BTreeMap<String, Props>>().unwrap();
        assert_eq!(props::value::<Vec<u8>>(&settings["802-11-wireless"], "ssid").unwrap(), b"Lab\xff");
        assert_eq!(props::value::<String>(&settings[SECURITY_SETTING], "key-mgmt").as_deref(), Some("sae"));
        network.security = Security::Open;
        let open = add_and_activate(&network, None, "/d/wlan0").unwrap();
        let settings = open.try_child_value(0).unwrap().get::<BTreeMap<String, Props>>().unwrap();
        assert!(!settings.contains_key(SECURITY_SETTING));
        assert_eq!(activate("/s/1", "/", "/").unwrap().type_().as_str(), "(ooo)");
        assert!(deactivate("bad path").is_none());
    }

    #[test]
    fn password_rules() {
        let wpa2 = Security::Personal { sae: false };
        assert!(password_acceptable(wpa2, "12345678"));
        assert!(password_acceptable(wpa2, &"a".repeat(63)));
        assert!(password_acceptable(wpa2, &"0f".repeat(32)));
        assert!(!password_acceptable(wpa2, "1234567"));
        assert!(!password_acceptable(wpa2, &"g".repeat(64)));
        assert!(!password_acceptable(wpa2, "pässwörd1"));
        assert!(!password_acceptable(wpa2, "tab\there1"));
        let wpa3 = Security::Personal { sae: true };
        assert!(password_acceptable(wpa3, "pässwörd"));
        assert!(!password_acceptable(wpa3, ""));
        assert!(!password_acceptable(wpa3, "new\nline"));
        assert!(!password_acceptable(Security::Open, "12345678"));
    }

    fn request(key_mgmt: &str, setting: &str, flags: u32) -> Variant {
        let text = format!(
            "({{'connection': {{'id': <'Home'>, 'type': <'802-11-wireless'>}}, \
              '802-11-wireless': {{'ssid': <@ay [72, 111, 109, 101]>}}, \
              '802-11-wireless-security': {{'key-mgmt': <'{key_mgmt}'>}}}}, \
              objectpath '/s/1', '{setting}', @as [], uint32 {flags})"
        );
        Variant::parse(Some(VariantTy::new("(a{sa{sv}}osasu)").unwrap()), &text).unwrap()
    }

    #[test]
    fn a_secrets_request_prompts_only_for_a_personal_network_with_interaction() {
        let asked = secrets_request(&request("wpa-psk", SECURITY_SETTING, 0x3)).unwrap();
        assert_eq!(asked.connection, "/s/1");
        assert_eq!(
            asked.prompt(),
            Some(PasswordPrompt { label: "Home".into(), security: Security::Personal { sae: false }, retry: true })
        );
        assert_eq!(secrets_request(&request("sae", SECURITY_SETTING, 0x1)).unwrap().prompt().unwrap().security, Security::Personal { sae: true });
        assert_eq!(secrets_request(&request("wpa-psk", SECURITY_SETTING, 0x0)).unwrap().prompt(), None, "no interaction");
        assert_eq!(secrets_request(&request("wpa-eap", SECURITY_SETTING, 0x1)).unwrap().prompt(), None);
        assert_eq!(secrets_request(&request("wpa-psk", "vpn", 0x1)).unwrap().prompt(), None);
        assert!(secrets_request(&("x",).to_variant()).is_none());
    }

    #[test]
    fn the_reply_carries_the_psk_and_nothing_else() {
        let reply = secrets_reply("hunter2hunter2");
        assert_eq!(reply.type_().as_str(), "(a{sa{sv}})");
        let (secrets,) = reply.get::<(BTreeMap<String, Props>,)>().unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[SECURITY_SETTING].len(), 1);
        assert_eq!(props::value::<String>(&secrets[SECURITY_SETTING], "psk").as_deref(), Some("hunter2hunter2"));
    }

    #[test]
    fn a_cancel_names_the_connection_and_the_setting() {
        let params = Variant::tuple_from_iter([op("/s/1"), SECURITY_SETTING.to_variant()]);
        assert_eq!(cancel_request(&params), Some(("/s/1".into(), SECURITY_SETTING.into())));
        assert_eq!(cancel_request(&("/s/1", "x").to_variant()), None, "s is not o");
    }
}
```

- [ ] **Step 6: Run the network tests**

Run: `RIG_CARGO --lib network::`

Expected: 13 passed. If `the_list_is_bounded` fails on the count, check `truncate` runs after the sort, not before.

- [ ] **Step 7: Write `src/bluetooth.rs` with its tests**

```rust
//! BlueZ as the Bluetooth module shows it (doc_bar.md, BR3): the adapter's power, the paired
//! devices, the devices nearby while discovering, and the passkey a pairing asks to confirm.
//! Device names come from the radio: they are sanitised and bounded here.

use athanor_unit::text::{line, NAME_CHARS};
use glib::Variant;

use crate::props::{self, Objects, Props};

pub const BLUEZ: &str = "org.bluez";
/// BlueZ's object manager.
pub const ROOT: &str = "/";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
pub const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
pub const AGENT_MANAGER_PATH: &str = "/org/bluez";
pub const AGENT_IFACE: &str = "org.bluez.Agent1";
pub const AGENT_PATH: &str = "/os/athanor/Bar/BluezAgent";
/// The bar shows a passkey and asks yes or no; it never types one.
pub const CAPABILITY: &str = "DisplayYesNo";
pub const REJECTED: &str = "org.bluez.Error.Rejected";
pub const CANCELED: &str = "org.bluez.Error.Canceled";
pub const MAX_DEVICES: usize = 16;

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
    pub paired: Vec<Device>,
    /// Unpaired devices with a name, only while discovering.
    pub nearby: Vec<Device>,
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
    props::value::<String>(props, key)
        .map(|text| line(&text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
}

fn device(path: &str, props: &Props) -> Option<Device> {
    let paired = props::value::<bool>(props, "Paired").unwrap_or(false);
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
        icon: icon(&props::value::<String>(props, "Icon").unwrap_or_default()),
        paired,
        connected: props::value::<bool>(props, "Connected").unwrap_or(false),
    })
}

/// The module's state; `None` without an adapter, and then the module hides. With several
/// adapters the first by path is shown.
pub fn state(objects: &Objects) -> Option<BluetoothState> {
    let (adapter, adapter_props) = objects
        .iter()
        .find_map(|(path, interfaces)| Some((path, interfaces.get(ADAPTER)?)))?;
    let discovering = props::value::<bool>(adapter_props, "Discovering").unwrap_or(false);
    let (mut paired, mut nearby): (Vec<Device>, Vec<Device>) = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let props = interfaces.get(DEVICE)?;
            (props::path(props, "Adapter").as_deref() == Some(adapter.as_str())).then_some(())?;
            device(path, props)
        })
        .partition(|device| device.paired);
    paired.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.label.cmp(&b.label)));
    paired.truncate(MAX_DEVICES);
    if discovering {
        nearby.sort_by(|a, b| a.label.cmp(&b.label));
        nearby.truncate(MAX_DEVICES);
    } else {
        nearby.clear();
    }
    Some(BluetoothState {
        adapter: adapter.clone(),
        powered: props::value::<bool>(adapter_props, "Powered").unwrap_or(false),
        discovering,
        paired,
        nearby,
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

/// The device and the passkey of `RequestConfirmation` `(ou)` or `DisplayPasskey` `(ouq)`.
pub fn device_and_passkey(params: &Variant) -> Option<(String, u32)> {
    if !(props::has_type(params, "(ou)") || props::has_type(params, "(ouq)")) {
        return None;
    }
    Some((
        params.try_child_value(0)?.str()?.to_owned(),
        params.try_child_value(1)?.get::<u32>()?,
    ))
}

/// The device of `RequestAuthorization` `(o)` or `AuthorizeService` `(os)`.
pub fn device_of(params: &Variant) -> Option<String> {
    if !(props::has_type(params, "(o)") || props::has_type(params, "(os)")) {
        return None;
    }
    Some(params.try_child_value(0)?.str()?.to_owned())
}

/// A device's label for the confirmation page.
pub fn device_label(objects: &Objects, path: &str) -> Option<String> {
    let props = props::lookup(objects, path, DEVICE)?;
    name(props, "Alias").or_else(|| name(props, "Name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;

    fn add_device(objects: &mut Objects, path: &str, pairs: Vec<(&str, Variant)>) {
        let mut props: Props = pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        props.insert("Adapter".into(), props::object_path("/org/bluez/hci0").unwrap());
        objects.entry(path.to_owned()).or_default().insert(DEVICE.into(), props);
    }

    fn world(discovering: bool) -> Objects {
        let mut objects = Objects::new();
        objects.entry("/org/bluez/hci0".into()).or_default().insert(
            ADAPTER.into(),
            Props::from([
                ("Powered".into(), true.to_variant()),
                ("Discovering".into(), discovering.to_variant()),
            ]),
        );
        add_device(&mut objects, "/org/bluez/hci0/dev_1", vec![
            ("Alias", "Keyboard".to_variant()), ("Paired", true.to_variant()), ("Icon", "input-keyboard".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_2", vec![
            ("Alias", "Headphones".to_variant()), ("Paired", true.to_variant()), ("Connected", true.to_variant()),
            ("Icon", "audio-headset".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_3", vec![
            ("Alias", "AA-BB".to_variant()), ("Name", "Phone".to_variant()), ("Icon", "phone".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_4", vec![("Alias", "CC-DD".to_variant())]);
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
        let scanning = state(&world(true)).unwrap();
        let nearby: Vec<_> = scanning.nearby.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(nearby, ["Phone"], "a device with no name is not listed");
    }

    #[test]
    fn a_device_name_is_sanitised() {
        let mut objects = world(false);
        add_device(&mut objects, "/org/bluez/hci0/dev_5", vec![
            ("Alias", format!("Evil\u{202e}{}\n", "x".repeat(200)).to_variant()), ("Paired", true.to_variant()),
            ("Icon", "../../etc/passwd".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_6", vec![("Alias", "\u{1b}\u{7}".to_variant()), ("Paired", true.to_variant())]);
        let paired = state(&objects).unwrap().paired;
        let evil = paired.iter().find(|d| d.path.ends_with("dev_5")).unwrap();
        assert!(evil.label.starts_with("Evilxxx"));
        assert_eq!(evil.label.chars().count(), NAME_CHARS);
        assert_eq!(evil.icon, "bluetooth-symbolic");
        assert!(paired.iter().all(|d| !d.path.ends_with("dev_6")), "an empty name is left out");
    }

    #[test]
    fn a_device_of_another_adapter_is_not_listed() {
        let mut objects = world(false);
        let mut props = Props::from([("Alias".into(), "Other".to_variant()), ("Paired".into(), true.to_variant())]);
        props.insert("Adapter".into(), props::object_path("/org/bluez/hci1").unwrap());
        objects.entry("/org/bluez/hci1/dev_9".into()).or_default().insert(DEVICE.into(), props);
        assert!(state(&objects).unwrap().paired.iter().all(|d| d.label != "Other"));
    }

    #[test]
    fn passkeys_are_six_digits() {
        assert_eq!(passkey_label(42).as_deref(), Some("000042"));
        assert_eq!(passkey_label(999_999).as_deref(), Some("999999"));
        assert_eq!(passkey_label(1_000_000), None);
        let dev = props::object_path("/org/bluez/hci0/dev_3").unwrap();
        let confirm = Variant::tuple_from_iter([dev.clone(), 123_456u32.to_variant()]);
        assert_eq!(device_and_passkey(&confirm), Some(("/org/bluez/hci0/dev_3".into(), 123_456)));
        let display = Variant::tuple_from_iter([dev.clone(), 7u32.to_variant(), 2u16.to_variant()]);
        assert_eq!(device_and_passkey(&display).map(|(_, key)| key), Some(7));
        assert_eq!(device_and_passkey(&("x", 1u32).to_variant()), None);
        assert_eq!(device_of(&Variant::tuple_from_iter([dev])).as_deref(), Some("/org/bluez/hci0/dev_3"));
        assert_eq!(device_label(&world(false), "/org/bluez/hci0/dev_3").as_deref(), Some("AA-BB"));
    }
}
```

- [ ] **Step 8: Run the Bluetooth tests**

Run: `RIG_CARGO --lib bluetooth::`

Expected: 5 passed.

- [ ] **Step 9: Write `src/audio.rs` with its tests**

```rust
//! The audio module's arithmetic (doc_bar.md, BR3): volumes between the sound server's scale
//! and a percentage, the icons, device labels, and the MPRIS metadata of the media controls.
//! Descriptions and track titles come from other processes: they are sanitised here.

use athanor_unit::text::{line, NAME_CHARS};

use crate::props::{self, Props};

/// The sound server's 100 %, `PA_VOLUME_NORM`.
pub const NORMAL: u32 = 0x10000;
pub const MAX_DEVICES: usize = 16;
pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
pub const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// A volume as a percentage. Above 100 % the slider shows 100 %: the bar does not amplify.
pub fn percent(raw: u32) -> f64 {
    (f64::from(raw) * 100.0 / f64::from(NORMAL)).clamp(0.0, 100.0)
}

pub fn raw(percent: f64) -> u32 {
    let percent = if percent.is_finite() { percent.clamp(0.0, 100.0) } else { 0.0 };
    // At most NORMAL, so the cast cannot truncate.
    (percent * f64::from(NORMAL) / 100.0).round() as u32
}

pub fn output_icon(percent: f64, muted: bool) -> &'static str {
    match percent {
        _ if muted || percent <= 0.0 => "audio-volume-muted-symbolic",
        p if p < 34.0 => "audio-volume-low-symbolic",
        p if p < 67.0 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

pub fn input_icon(muted: bool) -> &'static str {
    if muted {
        "microphone-disabled-symbolic"
    } else {
        "audio-input-microphone-symbolic"
    }
}

/// The description when there is a usable one, else the device's name.
pub fn device_label(description: Option<&str>, name: &str) -> String {
    description
        .map(|text| line(text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| line(name, NAME_CHARS))
}

/// One output or input device.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The sound server's name, to address it.
    pub name: String,
    pub label: String,
    pub percent: f64,
    pub muted: bool,
}

/// The device the module controls: the default one, else the first.
pub fn chosen(devices: &[Device], default: Option<&str>) -> Option<usize> {
    default
        .and_then(|name| devices.iter().position(|device| device.name == name))
        .or_else(|| (!devices.is_empty()).then_some(0))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: Option<String>,
}

/// The track from a player's `Metadata`; `None` without a title.
pub fn track(player: &Props) -> Option<Track> {
    let metadata = props::value::<Props>(player, "Metadata")?;
    let title = props::value::<String>(&metadata, "xesam:title")
        .map(|title| line(&title, NAME_CHARS))
        .filter(|title| !title.trim().is_empty())?;
    let artist = props::value::<Vec<String>>(&metadata, "xesam:artist")
        .map(|artists| line(&artists.join(", "), NAME_CHARS))
        .filter(|artist| !artist.trim().is_empty());
    Some(Track { title, artist })
}

/// A well-known name of an MPRIS player.
pub fn is_player(name: &str) -> bool {
    name.strip_prefix(MPRIS_PREFIX).is_some_and(|rest| !rest.is_empty())
}

pub fn playing(player: &Props) -> bool {
    props::value::<String>(player, "PlaybackStatus").as_deref() == Some("Playing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;
    use glib::{Variant, VariantTy};

    #[test]
    fn volumes_round_trip_and_are_bounded() {
        assert_eq!(percent(NORMAL), 100.0);
        assert_eq!(percent(NORMAL / 2), 50.0);
        assert_eq!(percent(NORMAL * 3 / 2), 100.0, "amplified shows as 100");
        assert_eq!(raw(100.0), NORMAL);
        assert_eq!(raw(150.0), NORMAL);
        assert_eq!(raw(-3.0), 0);
        assert_eq!(raw(f64::NAN), 0);
        for p in [0.0, 1.0, 33.0, 50.0, 99.0] {
            assert!((percent(raw(p)) - p).abs() < 0.01, "{p}");
        }
    }

    #[test]
    fn icons_follow_the_level_and_the_mute() {
        assert_eq!(output_icon(80.0, true), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(10.0, false), "audio-volume-low-symbolic");
        assert_eq!(output_icon(50.0, false), "audio-volume-medium-symbolic");
        assert_eq!(output_icon(100.0, false), "audio-volume-high-symbolic");
        assert_eq!(input_icon(true), "microphone-disabled-symbolic");
    }

    #[test]
    fn the_default_device_is_chosen_else_the_first() {
        let device = |name: &str| Device { name: name.into(), label: name.into(), percent: 0.0, muted: false };
        let devices = [device("a"), device("b")];
        assert_eq!(chosen(&devices, Some("b")), Some(1));
        assert_eq!(chosen(&devices, Some("gone")), Some(0));
        assert_eq!(chosen(&[], Some("b")), None);
        assert_eq!(device_label(Some("\u{202e}Speakers\n"), "sink.0"), "Speakers");
        assert_eq!(device_label(Some(" "), "sink.0"), "sink.0");
    }

    #[test]
    fn the_track_is_sanitised_and_needs_a_title() {
        let player = |metadata: &str| -> Props {
            let v = Variant::parse(Some(VariantTy::new("a{sv}").unwrap()), metadata).unwrap();
            Props::from([("Metadata".into(), v), ("PlaybackStatus".into(), "Playing".to_variant())])
        };
        let full = player("{'xesam:title': <'Night\u{202e} Drive'>, 'xesam:artist': <['Calmo', 'Duo']>}");
        assert_eq!(track(&full), Some(Track { title: "Night Drive".into(), artist: Some("Calmo, Duo".into()) }));
        assert!(playing(&full));
        assert_eq!(track(&player("{'xesam:artist': <['Calmo']>}")), None);
        assert_eq!(track(&player("{'xesam:title': <42>}")), None);
        assert!(is_player("org.mpris.MediaPlayer2.athanor"));
        assert!(!is_player("org.mpris.MediaPlayer2."));
        assert!(!is_player("org.example.Player"));
    }
}
```

- [ ] **Step 10: Run the audio tests**

Run: `RIG_CARGO --lib audio::`

Expected: 4 passed.

- [ ] **Step 11: Write `src/battery.rs` with its tests**

```rust
//! UPower, the power profiles and the backlight, as the battery module shows them (doc_bar.md,
//! BR3). The backlight is read from sysfs and written through logind, so the bar needs no
//! write access to sysfs and no group.

use std::fs;
use std::path::Path;

use crate::props::{self, Props};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
pub const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
/// Owned by tuned-ppd and by power-profiles-daemon 0.20 and later.
pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
pub const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
/// In the order the popover lists them.
pub const PROFILE_NAMES: [&str; 3] = ["power-saver", "balanced", "performance"];
pub const BACKLIGHT_ROOT: &str = "/sys/class/backlight";

const TYPE_BATTERY: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: f64,
    pub charge: Charge,
    /// Until empty while discharging, until full while charging; `None` when UPower does
    /// not know yet.
    pub seconds: Option<u64>,
}

/// The display device's battery; `None` when it is not a present battery, and then the
/// module hides (SH1).
pub fn battery(props: &Props) -> Option<Battery> {
    if props::value::<u32>(props, "Type") != Some(TYPE_BATTERY) || props::value::<bool>(props, "IsPresent") != Some(true) {
        return None;
    }
    let percent = props::value::<f64>(props, "Percentage").filter(|p| p.is_finite())?.clamp(0.0, 100.0);
    let charge = match props::value::<u32>(props, "State") {
        Some(1) => Charge::Charging,
        Some(2 | 3) => Charge::Discharging,
        Some(4) => Charge::Full,
        _ => Charge::Unknown,
    };
    let key = match charge {
        Charge::Charging => Some("TimeToFull"),
        Charge::Discharging => Some("TimeToEmpty"),
        Charge::Full | Charge::Unknown => None,
    };
    let seconds = key
        .and_then(|key| props::value::<i64>(props, key))
        .and_then(|seconds| u64::try_from(seconds).ok())
        .filter(|&seconds| seconds > 0);
    Some(Battery { percent, charge, seconds })
}

pub fn icon(battery: &Battery) -> &'static str {
    let charging = battery.charge == Charge::Charging;
    match (battery.charge, battery.percent) {
        (Charge::Full, _) => "battery-full-charged-symbolic",
        (_, p) if p >= 80.0 => if charging { "battery-full-charging-symbolic" } else { "battery-full-symbolic" },
        (_, p) if p >= 50.0 => if charging { "battery-good-charging-symbolic" } else { "battery-good-symbolic" },
        (_, p) if p >= 20.0 => if charging { "battery-low-charging-symbolic" } else { "battery-low-symbolic" },
        (_, p) if p >= 5.0 => if charging { "battery-caution-charging-symbolic" } else { "battery-caution-symbolic" },
        _ => if charging { "battery-empty-charging-symbolic" } else { "battery-empty-symbolic" },
    }
}

pub fn hours_minutes(seconds: u64) -> (u64, u64) {
    let minutes = seconds / 60;
    (minutes / 60, minutes % 60)
}

/// The profiles the daemon offers, in `PROFILE_NAMES` order, and the active one; `None`
/// when the reply names no profile the bar knows.
pub fn profiles(props: &Props) -> Option<(Vec<&'static str>, String)> {
    let offered: Vec<String> = props::value::<Vec<Props>>(props, "Profiles")?
        .iter()
        .filter_map(|profile| props::value::<String>(profile, "Profile"))
        .collect();
    let known: Vec<&'static str> = PROFILE_NAMES
        .into_iter()
        .filter(|name| offered.iter().any(|offered| offered == name))
        .collect();
    let active = props::value::<String>(props, "ActiveProfile").filter(|active| known.contains(&active.as_str()))?;
    Some((known, active))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backlight {
    /// The directory's name under `/sys/class/backlight`, as logind's `SetBrightness` takes it.
    pub name: String,
    pub max: u32,
    pub level: u32,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn read_u32(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The panel's backlight: firmware first, then platform, then raw, as the kernel documents
/// the preference; `None` without one, and then the popover shows no brightness.
pub fn read_backlight(root: &Path) -> Option<Backlight> {
    let rank = |kind: &str| match kind {
        "firmware" => Some(0),
        "platform" => Some(1),
        "raw" => Some(2),
        _ => None,
    };
    let mut found: Vec<(u8, Backlight)> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok().filter(|name| valid_name(name))?;
            let dir = entry.path();
            let kind = fs::read_to_string(dir.join("type")).ok()?;
            let rank = rank(kind.trim())?;
            let max = read_u32(&dir.join("max_brightness")).filter(|&max| max > 0)?;
            let level = read_u32(&dir.join("brightness"))?.min(max);
            Some((rank, Backlight { name, max, level }))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().next().map(|(_, backlight)| backlight)
}

impl Backlight {
    pub fn percent(&self) -> f64 {
        f64::from(self.level) * 100.0 / f64::from(self.max)
    }

    /// The level for `percent`, never 0: a slider at the left end must not turn the panel
    /// black.
    pub fn raw(&self, percent: f64) -> u32 {
        let percent = if percent.is_finite() { percent.clamp(0.0, 100.0) } else { 100.0 };
        // At most `max`, so the cast cannot truncate.
        ((percent * f64::from(self.max) / 100.0).round() as u32).clamp(1, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::{Variant, VariantTy};

    fn device(text: &str) -> Props {
        crate::props::get_all(&Variant::parse(Some(VariantTy::new("(a{sv})").unwrap()), text).unwrap()).unwrap()
    }

    #[test]
    fn a_present_battery_is_read_and_anything_else_hides_the_module() {
        let on_battery = device("({'Type': <uint32 2>, 'IsPresent': <true>, 'Percentage': <72.0>, 'State': <uint32 2>, 'TimeToEmpty': <int64 12300>, 'TimeToFull': <int64 0>},)");
        assert_eq!(battery(&on_battery), Some(Battery { percent: 72.0, charge: Charge::Discharging, seconds: Some(12300) }));
        assert_eq!(icon(&battery(&on_battery).unwrap()), "battery-good-symbolic");
        assert_eq!(hours_minutes(12300), (3, 25));
        let absent = device("({'Type': <uint32 2>, 'IsPresent': <false>, 'Percentage': <0.0>},)");
        assert_eq!(battery(&absent), None);
        let ups = device("({'Type': <uint32 3>, 'IsPresent': <true>, 'Percentage': <50.0>},)");
        assert_eq!(battery(&ups), None);
        let charging = device("({'Type': <uint32 2>, 'IsPresent': <true>, 'Percentage': <130.0>, 'State': <uint32 1>, 'TimeToFull': <int64 -5>},)");
        let charging = battery(&charging).unwrap();
        assert_eq!((charging.percent, charging.seconds), (100.0, None));
        assert_eq!(icon(&charging), "battery-full-charging-symbolic");
    }

    #[test]
    fn profiles_keep_the_known_ones_in_order() {
        let props = device("({'ActiveProfile': <'balanced'>, 'Profiles': <[{'Profile': <'performance'>}, {'Profile': <'balanced'>}, {'Profile': <'turbo'>}]>},)");
        assert_eq!(profiles(&props), Some((vec!["balanced", "performance"], "balanced".into())));
        let unknown = device("({'ActiveProfile': <'turbo'>, 'Profiles': <[{'Profile': <'turbo'>}]>},)");
        assert_eq!(profiles(&unknown), None);
        assert_eq!(profiles(&device("({'ActiveProfile': <'balanced'>},)")), None);
    }

    #[test]
    fn the_backlight_prefers_firmware_and_never_goes_to_zero() {
        let root = std::env::temp_dir().join(format!("athanor-bar-backlight-{}", std::process::id()));
        let make = |name: &str, kind: &str, max: &str, level: &str| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("type"), kind).unwrap();
            fs::write(dir.join("max_brightness"), max).unwrap();
            fs::write(dir.join("brightness"), level).unwrap();
        };
        make("intel_backlight", "raw\n", "1000\n", "600\n");
        make("acpi_video0", "firmware\n", "0\n", "0\n");
        make(".hidden", "firmware\n", "10\n", "5\n");
        make("nv_backlight", "platform\n", "100\n", "250\n");
        let backlight = read_backlight(&root).unwrap();
        assert_eq!(backlight, Backlight { name: "nv_backlight".into(), max: 100, level: 100 });
        assert_eq!(backlight.raw(0.0), 1);
        assert_eq!(backlight.raw(50.0), 50);
        assert_eq!(backlight.raw(f64::NAN), 100);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(read_backlight(&root), None);
    }
}
```

- [ ] **Step 12: Run the battery tests, then the whole library, then build the binary**

Run: `RIG_CARGO --lib battery::`

Expected: 3 passed.

Run: `RIG_CARGO --lib`

Expected: every library test passes. That is the earlier tests plus 31 new ones: 6 props, 13 network, 5 bluetooth, 4 audio, 3 battery.

Run: `bash forge/test/shell/rig.sh build-bar`

Expected: the binary builds. The five new modules are unused by the binary so far, which is not a warning for a `pub` library item.

- [ ] **Step 13: Commit**

```bash
git add Cargo.lock forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/src/{props,network,bluetooth,audio,battery}.rs
git commit -m "feat(bar): model NetworkManager, BlueZ, the sound server and UPower for the system modules"
```

---
### Task 3: The fixtures, and the skeleton of the modules' end-to-end test

**Files:**
- Create: `forge/test/shell/system_fixtures.py`
- Modify: `forge/test/shell/bar_session.py` (docstring, `NODE`, `logind`, `parse`, `main`)
- Create: `forge/test/shell/bar_modules_e2e.py`
- Modify: `forge/test/shell/rig.sh` (header line, a `bar-modules-e2e` command)
- Modify: `.github/workflows/shell-surfaces.yml` (one step in job `bar`)

**Interfaces:**
- Consumes: the rig image of Task 1.
- Produces:
  - `system_fixtures.py`:
    - the constants `SYSTEM_BUS`, `BACKLIGHT_DIR`, `MOCK`, `FIXTURE`, `NM`, `NM_MOCK_PATH`, `NM_AGENT_MANAGER`, `NM_SETTINGS`, `BLUEZ`, `BLUEZ_MOCK`, `UPOWER`, `PROFILES`, `PLAYER` and `MPRIS_PATH`;
    - `system_bus()`, `call(bus, name, path, interface, method, signature=None, args=None, reply=None, timeout=5000)`, `wait_for_name(bus, name, seconds=10)`, `has_owner(bus, name)`;
    - `networkmanager(bus) -> Popen`, `bluez(bus, log) -> Popen`, `upower(bus, log) -> Popen`, `profiles(bus, log) -> Popen`;
    - `pipewire(log) -> list[Popen]`, `pipewire_pulse(log) -> Popen`, `player(log) -> Popen`, `backlight()`;
    - `pids_of(comm) -> list[int]`, and `start(tag) -> list[Popen]`.
  - The fixture methods on interface `os.athanor.Fixture`:
    - on NetworkManager's agent manager: `Registrations() -> u`, `AskSecrets(s agent, o connection, s ssid, u flags) -> u`, `SecretsResult(u) -> s`, `CancelSecrets(s agent, o connection)`;
    - on BlueZ's `/org/bluez`: `DefaultAgent() -> s`, `AgentOwner(s)`, `RequestConfirmation(s agent, o device, u passkey) -> u`, `AgentRequest(s agent, s method, o device) -> u`, `ConfirmationResult(u) -> s`;
    - on the nearby devices, `org.bluez.Device1.Pair` replaced by `PAIR_WITH_AGENT`, which confirms `fx.PAIRING_PASSKEY` with the agent (the bar shows `fx.PAIRING_CODE`).
  - `bar_session.py --fixtures`: the fixtures before the bar, `ATHANOR_BAR_BACKLIGHT_DIR` for the bar, and logind's `Session.SetBrightness`, logged as `SetBrightness <subsystem> <name> <level>` and written to the fake sysfs file.
  - `bar_modules_e2e.py`: a `SECTIONS` list that Tasks 4 to 7 extend, each section a function of one `SimpleNamespace` with `app`, `Atspi`, `bus`, `bar` (the bar's unique name on the system bus) and `pid`.
  - `rig.sh bar-modules-e2e`.

- [ ] **Step 1: Write `forge/test/shell/system_fixtures.py`**

```python
#!/usr/bin/python3
"""system_fixtures.py - the system services of the bar's modules in the rig (doc_bar.md,
BR9). python3-dbusmock provides NetworkManager, BlueZ, UPower and the power profiles on the
private system bus of bar_session.py. PipeWire runs with two null sinks and a virtual source,
an MPRIS player sits on the session bus, and a fake backlight lives under /tmp.
bar_session.py --fixtures starts them before the bar. bar_modules_e2e.py imports this module
to drive the mocks and to restart a service under a running bar.

The NetworkManager mock logs to /dev/null: dbusmock logs every call with its arguments, and
a secret agent's reply carries a password. The other mocks log to /out/<tag>-<service>.log.

Fixture methods live on the interface os.athanor.Fixture, added to the mocks:
- NetworkManager's agent manager (templates lack one): Register counts, Registrations()
  returns the count; AskSecrets(agent, connection, ssid, flags) calls the agent's GetSecrets
  as NetworkManager would and returns an index; SecretsResult(index) returns "pending",
  "reply:<psk>" or "error:<D-Bus error name>"; CancelSecrets(agent, connection).
- BlueZ's /org/bluez: DefaultAgent() returns the default agent's path; AgentOwner(name)
  records the bus name that registered it, which the template does not keep;
  RequestConfirmation(agent, device, passkey) calls the agent with no pairing in progress
  and returns an index; AgentRequest(agent, method, device) does the same for
  RequestAuthorization, AuthorizeService, RequestPinCode, RequestPasskey and DisplayPasskey;
  ConfirmationResult(index) returns "pending", "confirmed" or "error:<name>".
- The nearby devices' org.bluez.Device1.Pair is replaced: like bluetoothd, it calls the
  default agent's RequestConfirmation with PAIRING_PASSKEY and pairs the device only when the
  agent confirms. It blocks the BlueZ mock until the agent answers, so a test must not call
  the BlueZ mock while a confirmation page is open.
"""

import os
import subprocess
import time
from pathlib import Path

from gi.repository import Gio, GLib

SYSTEM_BUS = "/tmp/athanor-system-bus"
BACKLIGHT_DIR = Path("/tmp/athanor-backlight")
MOCK = "org.freedesktop.DBus.Mock"
FIXTURE = "os.athanor.Fixture"
NM = "org.freedesktop.NetworkManager"
NM_MOCK_PATH = "/org/freedesktop"
NM_AGENT_MANAGER = "/org/freedesktop/NetworkManager/AgentManager"
NM_SETTINGS = "/org/freedesktop/NetworkManager/Settings"
BLUEZ = "org.bluez"
BLUEZ_MOCK = "org.bluez.Mock"
UPOWER = "org.freedesktop.UPower"
PROFILES = "org.freedesktop.UPower.PowerProfiles"
PLAYER = "org.mpris.MediaPlayer2.athanor"
MPRIS_PATH = "/org/mpris/MediaPlayer2"
MPRIS_PLAYER = "org.mpris.MediaPlayer2.Player"
# (object name, SSID, strength, security flags): WPA2, open, WPA2, 802.1X.
ACCESS_POINTS = [
    ("lab", "Athanor Lab", 82, 0x100),
    ("cafe", "Corner Café", 60, 0),
    ("home", "Home Network", 45, 0x100),
    ("campus", "Campus", 38, 0x200),
]
# (address, alias, paired, icon)
DEVICES = [
    ("11:22:33:44:55:01", "Headphones", True, "audio-headset"),
    ("11:22:33:44:55:02", "Keyboard", True, "input-keyboard"),
    ("11:22:33:44:55:03", "Phone", False, "phone"),
    ("11:22:33:44:55:04", "Speaker", False, "audio-card"),
]
# The passkey the fixture's Pair asks the agent to confirm, and the six digits the bar shows.
PAIRING_PASSKEY = 482916
PAIRING_CODE = "482916"
# bluetoothd's Pair: ask the default agent, pair only on its confirmation.
PAIR_WITH_AGENT = f"""
bluez = get_object('/org/bluez')
owner = bluez.__dict__.get('agent_owner')
if not owner or not bluez.default_agent:
    raise dbus.exceptions.DBusException('no agent', name='org.bluez.Error.AuthenticationFailed')
try:
    self.connection.call_blocking(
        owner, str(bluez.default_agent), 'org.bluez.Agent1', 'RequestConfirmation', 'ou',
        [dbus.ObjectPath(self.__dbus_object_path__), dbus.UInt32({PAIRING_PASSKEY})], timeout=60)
except dbus.exceptions.DBusException as error:
    raise dbus.exceptions.DBusException(
        'the agent refused: ' + error.get_dbus_name(), name='org.bluez.Error.AuthenticationRejected')
self.paired = True
self.UpdateProperties('org.bluez.Device1', {{'Paired': dbus.Boolean(True)}})
"""

NM_AGENT_METHODS = [
    ("Register", "s", "", "self.registered = getattr(self, 'registered', 0) + 1"),
    (
        "RegisterWithCapabilities",
        "su",
        "",
        "self.registered = getattr(self, 'registered', 0) + 1",
    ),
    ("Unregister", "", "", ""),
]
NM_FIXTURE_METHODS = [
    ("Registrations", "", "u", "ret = getattr(self, 'registered', 0)"),
    (
        "AskSecrets",
        "sosu",
        "u",
        """
results = self.__dict__.setdefault('secrets_results', [])
index = len(results)
results.append('pending')
settings = {
    'connection': {'id': args[2], 'type': '802-11-wireless'},
    '802-11-wireless': {'ssid': dbus.ByteArray(args[2].encode())},
    '802-11-wireless-security': {'key-mgmt': 'wpa-psk'},
}
def done(secrets, results=results, index=index):
    results[index] = 'reply:' + str(secrets.get('802-11-wireless-security', {}).get('psk', ''))
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], '/org/freedesktop/NetworkManager/SecretAgent',
    'org.freedesktop.NetworkManager.SecretAgent', 'GetSecrets', 'a{sa{sv}}osasu',
    [settings, args[1], '802-11-wireless-security', [], args[3]], done, failed, timeout=300)
ret = index
""",
    ),
    ("SecretsResult", "u", "s", "ret = self.__dict__.get('secrets_results', [])[args[0]]"),
    (
        "CancelSecrets",
        "so",
        "",
        "self.connection.call_blocking(args[0], '/org/freedesktop/NetworkManager/SecretAgent', "
        "'org.freedesktop.NetworkManager.SecretAgent', 'CancelGetSecrets', 'os', "
        "[args[1], '802-11-wireless-security'], timeout=10)",
    ),
]
BLUEZ_FIXTURE_METHODS = [
    ("DefaultAgent", "", "s", "ret = str(self.default_agent or '')"),
    (
        "RequestConfirmation",
        "sou",
        "u",
        """
results = self.__dict__.setdefault('confirmations', [])
index = len(results)
results.append('pending')
def done(results=results, index=index, device=str(args[1])):
    results[index] = 'confirmed'
    # get_object is dbusmock.mockobject's own, in the globals the method code runs with.
    get_object(device).UpdateProperties('org.bluez.Device1', {'Paired': True})
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], str(self.default_agent), 'org.bluez.Agent1', 'RequestConfirmation', 'ou',
    [args[1], args[2]], done, failed, timeout=300)
ret = index
""",
    ),
    (
        "AgentRequest",
        "sso",
        "u",
        """
requests = {
    'RequestAuthorization': ('o', [args[2]]),
    'AuthorizeService': ('os', [args[2], '0000110b-0000-1000-8000-00805f9b34fb']),
    'RequestPinCode': ('o', [args[2]]),
    'RequestPasskey': ('o', [args[2]]),
    'DisplayPasskey': ('ouq', [args[2], dbus.UInt32(222333), dbus.UInt16(0)]),
}
signature, arguments = requests[args[1]]
results = self.__dict__.setdefault('confirmations', [])
index = len(results)
results.append('pending')
def done(*_, results=results, index=index):
    results[index] = 'confirmed'
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], str(self.default_agent), 'org.bluez.Agent1', args[1], signature, arguments,
    done, failed, timeout=30)
ret = index
""",
    ),
    ("AgentOwner", "s", "", "self.agent_owner = args[0]"),
    ("ConfirmationResult", "u", "s", "ret = self.__dict__.get('confirmations', [])[args[0]]"),
]


def system_bus():
    return Gio.DBusConnection.new_for_address_sync(
        f"unix:path={SYSTEM_BUS}",
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )


def call(bus, name, path, interface, method, signature=None, args=None, reply=None, timeout=5000):
    """A synchronous call; the reply unpacked, as a tuple."""
    result = bus.call_sync(
        name,
        path,
        interface,
        method,
        GLib.Variant(signature, args) if signature else None,
        GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE,
        timeout,
        None,
    )
    return result.unpack()


def has_owner(bus, name):
    (owned,) = call(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        "(s)",
        (name,),
        "(b)",
    )
    return owned


def wait_for_name(bus, name, seconds=10):
    deadline = time.monotonic() + seconds
    while not has_owner(bus, name):
        if time.monotonic() > deadline:
            raise SystemExit(f"system_fixtures.py: {name} did not appear within {seconds} s")
        time.sleep(0.1)


def real_time_env(**extra):
    """The environment without scene.sh's faketime: the services keep real clocks, as they
    do at login, and PipeWire's timers are not skewed."""
    env = {
        key: value
        for key, value in os.environ.items()
        if key != "LD_PRELOAD" and not key.startswith("FAKETIME")
    }
    env.update(extra)
    return env


def spawn_mock(template, log):
    return subprocess.Popen(
        ["python3", "-m", "dbusmock", "--system", "--template", template, "--logfile", log],
        env=real_time_env(DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={SYSTEM_BUS}"),
    )


def networkmanager(bus):
    process = spawn_mock("networkmanager", "/dev/null")
    wait_for_name(bus, NM)

    def mock(method, signature, args):
        return call(bus, NM, NM_MOCK_PATH, MOCK, method, signature, args, "(s)")[0]

    call(bus, NM, NM_MOCK_PATH, MOCK, "AddEthernetDevice", "(ssi)", ("eth0", "eth0", 100))
    wlan = mock("AddWiFiDevice", "(ssi)", ("wlan0", "wlan0", 100))
    points = {}
    for index, (name, ssid, strength, security) in enumerate(ACCESS_POINTS):
        points[name] = mock(
            "AddAccessPoint",
            "(ssssuuuyu)",
            (wlan, name, ssid, f"00:11:22:33:44:{index:02x}", 2, 2412, 54000, strength, security),
        )
    lab = mock("AddWiFiConnection", "(ssss)", (wlan, "lab", "Athanor Lab", "wpa-psk"))
    mock("AddActiveConnection", "(assssu)", ([wlan], lab, points["lab"], "lab", 2))
    vpn = GLib.Variant(
        "(a{sa{sv}})",
        (
            {
                "connection": {
                    "id": GLib.Variant("s", "Office VPN"),
                    "type": GLib.Variant("s", "vpn"),
                    "uuid": GLib.Variant("s", "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0"),
                },
                "vpn": {"service-type": GLib.Variant("s", "org.freedesktop.NetworkManager.openvpn")},
            },
        ),
    )
    bus.call_sync(
        NM, NM_SETTINGS, "org.freedesktop.NetworkManager.Settings", "AddConnection",
        vpn, GLib.VariantType("(o)"), Gio.DBusCallFlags.NONE, 5000, None,
    )
    call(
        bus, NM, NM_MOCK_PATH, MOCK, "AddObject", "(ssa{sv}a(ssss))",
        (NM_AGENT_MANAGER, "org.freedesktop.NetworkManager.AgentManager", {}, NM_AGENT_METHODS),
    )
    call(bus, NM, NM_AGENT_MANAGER, MOCK, "AddMethods", "(sa(ssss))", (FIXTURE, NM_FIXTURE_METHODS))
    return process


def bluez(bus, log):
    process = spawn_mock("bluez5", log)
    wait_for_name(bus, BLUEZ)
    call(bus, BLUEZ, "/", BLUEZ_MOCK, "AddAdapter", "(ss)", ("hci0", "athanor"), "(s)")
    for address, alias, paired, icon in DEVICES:
        (path,) = call(bus, BLUEZ, "/", BLUEZ_MOCK, "AddDevice", "(sss)", ("hci0", address, alias), "(s)")
        call(
            bus, BLUEZ, path, MOCK, "UpdateProperties", "(sa{sv})",
            ("org.bluez.Device1", {"Icon": GLib.Variant("s", icon)}),
        )
        if paired:
            call(bus, BLUEZ, "/", BLUEZ_MOCK, "PairDevice", "(ss)", ("hci0", address))
        else:
            call(bus, BLUEZ, path, MOCK, "AddMethod", "(sssss)", ("org.bluez.Device1", "Pair", "", "", PAIR_WITH_AGENT))
    (headphones,) = [
        f"/org/bluez/hci0/dev_{address.replace(':', '_')}"
        for address, alias, _, _ in DEVICES
        if alias == "Headphones"
    ]
    call(bus, BLUEZ, headphones, "org.bluez.Device1", "Connect")
    # The template's Connect only signals: the property itself must say so for GetManagedObjects.
    call(
        bus, BLUEZ, headphones, MOCK, "UpdateProperties", "(sa{sv})",
        ("org.bluez.Device1", {"Connected": GLib.Variant("b", True)}),
    )
    call(bus, BLUEZ, "/org/bluez", MOCK, "AddMethods", "(sa(ssss))", (FIXTURE, BLUEZ_FIXTURE_METHODS))
    return process


def upower(bus, log):
    process = spawn_mock("upower", log)
    wait_for_name(bus, UPOWER)
    # Type battery, discharging, 72 %, 3 h 25 min left, present.
    call(
        bus, UPOWER, "/org/freedesktop/UPower", MOCK, "SetupDisplayDevice", "(uuddddxxbsu)",
        (2, 2, 72.0, 36.0, 50.0, 10.0, 12300, 0, True, "battery-good-symbolic", 1),
    )
    return process


def profiles(bus, log):
    process = spawn_mock("upower_power_profiles_daemon", log)
    wait_for_name(bus, PROFILES)
    return process


def pactl(*args):
    return subprocess.run(["pactl", *args], check=True, capture_output=True, text=True).stdout


def pipewire_pulse(log):
    process = subprocess.Popen(
        ["pipewire-pulse"], stdout=log, stderr=subprocess.STDOUT, env=real_time_env()
    )
    deadline = time.monotonic() + 10
    while subprocess.run(["pactl", "info"], capture_output=True).returncode != 0:
        if time.monotonic() > deadline:
            raise SystemExit("system_fixtures.py: pipewire-pulse did not answer within 10 s")
        time.sleep(0.2)
    return process


def null_node(name, description, media_class, positions):
    """A node owned by PipeWire itself (object.linger), so it outlives pipewire-pulse."""
    subprocess.run(
        [
            "pw-cli", "create-node", "adapter",
            f"{{ factory.name=support.null-audio-sink node.name={name} "
            f'node.description="{description}" media.class={media_class} '
            f"audio.position=[ {positions} ] object.linger=true }}",
        ],
        check=True,
        capture_output=True,
    )


def pipewire(log):
    processes = [
        subprocess.Popen([command], stdout=log, stderr=subprocess.STDOUT, env=real_time_env())
        for command in ("pipewire", "wireplumber")
    ]
    processes.append(pipewire_pulse(log))
    null_node("speakers", "Speakers", "Audio/Sink", "FL FR")
    null_node("headphones", "Headphones", "Audio/Sink", "FL FR")
    null_node("microphone", "Microphone", "Audio/Source/Virtual", "MONO")
    deadline = time.monotonic() + 10
    while not all(name in pactl("list", "short", "sinks") for name in ("speakers", "headphones")) or (
        "microphone" not in pactl("list", "short", "sources")
    ):
        if time.monotonic() > deadline:
            raise SystemExit("system_fixtures.py: the null devices did not appear within 10 s")
        time.sleep(0.2)
    pactl("set-default-sink", "speakers")
    pactl("set-sink-volume", "speakers", "40%")
    pactl("set-sink-volume", "headphones", "70%")
    pactl("set-default-source", "microphone")
    pactl("set-source-volume", "microphone", "55%")
    return processes


def player(log):
    process = subprocess.Popen(
        ["python3", "-m", "dbusmock", "--logfile", log, PLAYER, MPRIS_PATH, MPRIS_PLAYER],
        env=real_time_env(),
    )
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    wait_for_name(session, PLAYER)
    metadata = {
        "xesam:title": GLib.Variant("s", "Night Drive"),
        "xesam:artist": GLib.Variant("as", ["Calmo"]),
    }
    call(
        session, PLAYER, MPRIS_PATH, MOCK, "AddProperties", "(sa{sv})",
        (
            MPRIS_PLAYER,
            {
                "PlaybackStatus": GLib.Variant("s", "Playing"),
                "Metadata": GLib.Variant("a{sv}", metadata),
                "CanGoNext": GLib.Variant("b", True),
                "CanGoPrevious": GLib.Variant("b", True),
                "CanControl": GLib.Variant("b", True),
            },
        ),
    )
    toggle = (
        f"self.Set('{MPRIS_PLAYER}', 'PlaybackStatus', "
        f"'Paused' if self.Get('{MPRIS_PLAYER}', 'PlaybackStatus') == 'Playing' else 'Playing')"
    )
    call(
        session, PLAYER, MPRIS_PATH, MOCK, "AddMethods", "(sa(ssss))",
        (MPRIS_PLAYER, [("PlayPause", "", "", toggle), ("Next", "", "", ""), ("Previous", "", "", "")]),
    )
    return process


def backlight():
    device = BACKLIGHT_DIR / "intel_backlight"
    device.mkdir(parents=True, exist_ok=True)
    (device / "type").write_text("raw\n", encoding="utf-8")
    (device / "max_brightness").write_text("1000\n", encoding="utf-8")
    (device / "brightness").write_text("600\n", encoding="utf-8")


def pids_of(comm):
    """The processes whose /proc/<pid>/comm is `comm`."""
    found = []
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit():
            try:
                if (proc / "comm").read_text(encoding="utf-8").strip() == comm:
                    found.append(int(proc.name))
            except OSError:
                continue
    return found


def start(tag):
    """Every fixture, ready before the bar starts; the processes to stop afterwards."""
    backlight()
    bus = system_bus()
    out = Path("/out")
    audio_log = (out / f"{tag}-pipewire.log").open("a", encoding="utf-8")
    return [
        networkmanager(bus),
        bluez(bus, str(out / f"{tag}-bluez.log")),
        upower(bus, str(out / f"{tag}-upower.log")),
        profiles(bus, str(out / f"{tag}-profiles.log")),
        *pipewire(audio_log),
        player(str(out / f"{tag}-mpris.log")),
    ]
```

- [ ] **Step 2: Give `bar_session.py` the fixtures and `SetBrightness`**

Apply these edits to `forge/test/shell/bar_session.py`:

1. In the docstring's usage lines, `[--respawn]` becomes `[--respawn] [--fixtures]`; rewrap the two lines within 100 columns. Append this paragraph after the paragraph that starts "--notifications starts":

```text
--fixtures starts system_fixtures.py's services before the bar (NetworkManager, BlueZ, UPower
and the power profiles on the private system bus, PipeWire, an MPRIS player, a backlight)
and points the bar at the fake backlight. Session.SetBrightness is logged like the other
acting calls, "SetBrightness backlight intel_backlight 300", and writes the fake sysfs file.
```

2. Below `from gi.repository import Gio, GLib`, add:

```python
import system_fixtures
```

3. In `NODE`, the Session interface becomes:

```xml
  <interface name="org.freedesktop.login1.Session">
    <method name="Lock"/>
    <method name="SetBrightness">
      <arg type="s" direction="in"/>
      <arg type="s" direction="in"/>
      <arg type="u" direction="in"/>
    </method>
  </interface>
```

4. In `logind`'s `on_call`, the `else` branch becomes:

```python
        else:
            words = [method] + [str(value) for value in parameters.unpack()]
            with log.open("a", encoding="utf-8") as out:
                out.write(" ".join(words) + "\n")
            if method == "SetBrightness":
                subsystem, name, level = parameters.unpack()
                device = system_fixtures.BACKLIGHT_DIR / name
                if subsystem == "backlight" and "/" not in name and device.is_dir():
                    (device / "brightness").write_text(f"{level}\n", encoding="utf-8")
            invocation.return_value(None)
```

5. In `parse`, after `parser.add_argument("--respawn", action="store_true")`, add:

```python
    parser.add_argument("--fixtures", action="store_true")
```

6. In `main`:
   - replace `helpers = []`, after the `RequestName` check, with:

   ```python
       # The services exist before the bar starts, as they do at login. The loop at the end
       # of main terminates them with the other helpers.
       helpers = system_fixtures.start(os.environ.get("RIG_TAG", "bar")) if args.fixtures else []
   ```

   - after the `env = dict(...)` call, add the backlight directory when there are fixtures:

   ```python
       if args.fixtures:
           env["ATHANOR_BAR_BACKLIGHT_DIR"] = str(system_fixtures.BACKLIGHT_DIR)
   ```

- [ ] **Step 3: Write the skeleton of `forge/test/shell/bar_modules_e2e.py`**

```python
#!/usr/bin/python3
"""bar_modules_e2e.py - the network, Bluetooth, audio and battery modules of athanor-bar
against the fixtures of system_fixtures.py (doc_bar.md, BR3, BR9, section 5 item 17).
rig.sh bar-modules-e2e runs it as scene.sh's RIG_HOLD, with `bar_session.py --fixtures` as
the scene's client.

Each module is one section in SECTIONS. A section drives the bar over AT-SPI and checks what
reached the mocks, never the reverse only. The last checks are the bar's memory with every
module loaded, and that no password typed here reached a file the bar can write or the rig
keeps.
"""

import os
import stat
import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

import system_fixtures as fx  # noqa: E402
from atspi_check import find_application  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    check,
    failures,
    pss_kb,
    wait_for,
)

# Typed into the password entries. Searched for in every written file at the end.
PASSWORDS = ["correct horse battery", "staple-9-orbit"]
# The sections; Tasks 4 to 7 of the 2b.4 plan add network, bluetooth, audio and battery.
SECTIONS = []


def bar_name(bus, pid):
    """The bar's unique name on the private system bus: the connection whose process is
    the bar."""
    (names,) = fx.call(
        bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "ListNames", reply="(as)",
    )
    for name in names:
        if not name.startswith(":"):
            continue
        (owner_pid,) = fx.call(
            bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
            "GetConnectionUnixProcessID", "(s)", (name,), "(u)",
        )
        if owner_pid == pid:
            return name
    return None


# Where the bar can write under its Landlock rules and the rig keeps files: /out (logs,
# goldens, digests), the scene's XDG directories, the runtime directory and /tmp. The build
# tree and the binaries under /out are not the bar's output and are skipped, as are PNGs.
SKIPPED_UNDER_OUT = ("target", "bin")


def written_roots():
    roots = [Path("/out"), Path("/run/user/1000"), Path("/tmp")]
    for variable in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"):
        if os.environ.get(variable):
            roots.append(Path(os.environ[variable]))
    return roots


def regular_files(root):
    """Every regular file under root, recursively, without following links. Sockets, FIFOs,
    devices and links are left out by type; a file this process cannot read is reported."""
    for directory, subdirectories, names in os.walk(root):
        if Path(directory) == Path("/out"):
            subdirectories[:] = [name for name in subdirectories if name not in SKIPPED_UNDER_OUT]
        for name in names:
            path = Path(directory) / name
            if path.suffix == ".png" or not stat.S_ISREG(path.lstat().st_mode):
                continue
            yield path


def no_password_written():
    """No typed password in any regular file the bar can write or the rig keeps."""
    clean = True
    for root in written_roots():
        for path in regular_files(root):
            if not os.access(path, os.R_OK):
                print(f"cannot read {path} to scan it for a password", file=sys.stderr)
                clean = False
                continue
            data = path.read_bytes()
            if any(password.encode() in data for password in PASSWORDS):
                print(f"a password is in {path}", file=sys.stderr)
                clean = False
    return clean


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1
    bus = fx.system_bus()
    for service in (fx.NM, fx.BLUEZ, fx.UPOWER, fx.PROFILES):
        check(f"the fixture {service} is up", fx.has_owner(bus, service))
    sinks = fx.pactl("list", "short", "sinks")
    check("the sound server lists both null sinks", "speakers" in sinks and "headphones" in sinks, sinks)
    name = bar_name(bus, pid)
    if not check("the bar is on the private system bus", name is not None):
        return 1
    # `spawned`: the services a section restarts, stopped here; bar_session.py stops the rest.
    context = SimpleNamespace(app=app, Atspi=Atspi, bus=bus, bar=name, pid=pid, spawned=[])
    for section in SECTIONS:
        section(context)
    for process in context.spawned:
        process.terminate()
    check("the bar is still running", alive(pid))
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with every module loaded: {pss} kB")
    check(
        "PSS at rest within 64 MB with every module loaded (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    check("no typed password in any file the bar can write or the rig keeps", no_password_written())
    if failures:
        print(f"bar-modules-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("bar-modules-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Add `rig.sh bar-modules-e2e`**

In `forge/test/shell/rig.sh`, add this header line after the `bar-e2e` line:

```bash
#   rig.sh bar-modules-e2e  athanor-bar's network, Bluetooth, audio and battery modules against dbusmock, PipeWire and an MPRIS player; memory with every module loaded
```

After the `bar-e2e)` case (its `;;`), add:

```bash
bar-modules-e2e)
    seed_bar "$out/seed-bar-modules-e2e" float top visible light
    rm -f "$out"/bar-modules-e2e-*.log
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-bar-modules-e2e \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/bar_modules_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 bar-modules-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --fixtures"
    ;;
```

- [ ] **Step 5: Add the CI step**

In `.github/workflows/shell-surfaces.yml`, job `bar`, after the step `Live layout, mandatory keys, running windows, the power menu and memory`, add:

```yaml
      - name: The network, Bluetooth, audio and battery modules against their fixtures
        run: bash forge/test/shell/rig.sh bar-modules-e2e
```

Extend the upload's `path` with `.scratch/shell-rig/bar-modules-e2e.png`.

Run: `python3 scripts/verify.py workflows`

Expected: PASS.

- [ ] **Step 6: Run the skeleton**

```bash
bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e
```

Expected:
- every check PASSes, with no section yet;
- the PSS line prints (the modules are not built yet, so it is the 2b.2 figure);
- `.scratch/shell-rig/bar-modules-e2e-bluez.log` lists `AddAdapter`, `AddDevice` and `PairDevice`;
- no `bar-modules-e2e-networkmanager*.log` file exists.

If `pw-cli create-node` fails, read `.scratch/shell-rig/bar-modules-e2e-pipewire.log`. The usual cause is a missing session bus for wireplumber, and scene.sh runs under `dbus-run-session`, so check that first.

Run the existing gates too, since `bar_session.py` changed:

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-e2e
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface bar-power
```

Expected: both pass unchanged.

- [ ] **Step 7: Commit**

```bash
git add forge/test/shell/system_fixtures.py forge/test/shell/bar_session.py forge/test/shell/bar_modules_e2e.py \
    forge/test/shell/rig.sh .github/workflows/shell-surfaces.yml
git commit -m "test(shell): run NetworkManager, BlueZ, UPower, PipeWire and an MPRIS player as fixtures for the bar"
```

---
### Task 4: The bus helpers, and the network module with NetworkManager's secret agent

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/bus.rs`
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/network.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (two `mod` lines, one `build` arm, `Bar::fit_groups`)
- Modify: `forge/test/shell/bar_modules_e2e.py` (helpers, the `network` section)

**Interfaces:**
- Consumes:
  - `athanor_bar::props::{self, Objects}` and `athanor_bar::network::*` (Task 2);
  - `popup::{Popup, switch_row}`, `Bar::popover_opened`, and `crate::i18n::{tr, tr_with}`;
  - the fixtures of Task 3.
- Produces:
  - `ui/bus.rs`, for Tasks 5 to 7:
    - `TIMEOUT_MS: i32 = 5000` and `INTERACTIVE_TIMEOUT_MS: i32 = 120_000`;
    - `fn call(&gio::DBusConnection, name: &str, path: &str, interface: &str, method: &str, args: Option<&glib::Variant>, timeout: i32) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static`;
    - `fn set_property(&gio::DBusConnection, name, path, interface, property: &str, value: glib::Variant) -> impl Future<…> + 'static`;
    - `fn spawn(action: &'static str, future: impl Future<Output = Result<glib::Variant, glib::Error>> + 'static)`;
    - `enum Source { Managed(&'static str), Fixed(Vec<(&'static str, &'static str)>) }`;
    - `Mirror::new(&gio::DBusConnection, name: &str, Source, notify: impl Fn() + 'static) -> Rc<Mirror>`, and the accessors `objects() -> Ref<Objects>`, `owner() -> Option<String>`, `generation() -> u64`, `is_owner(&str) -> bool` and `connection() -> &gio::DBusConnection`.
  - `Bar::fit_groups(&self)`: refits every surface's groups after a module shows or hides outside `refresh`.
  - `bar_modules_e2e.py`: the helpers `showing_role`, `type_password`, `press_confirm`, `mock_calls`, `mock_pids` and `property_of`, and the `network` section.

The design, which Tasks 5 to 7 repeat:
- **One service per process** in a `thread_local!`, created by the first surface that builds the module. It holds the mirror, the agent, the last state, and weak references to the views.
- **One view per surface.** The service pushes a state into the views only when it changes, then calls `Bar::fit_groups`. The views never poll.
- **A mirror follows the unique owner.** A restart of the service is a new owner, hence a new generation: the objects are cleared, the service is loaded again, and the agent is registered again. A reply that arrives for an older generation is dropped.

- [ ] **Step 1: Write `src/ui/bus.rs`**

```rust
//! D-Bus for the system modules (doc_bar.md, BR3): calls with a timeout, and a mirror of one
//! service's objects kept current by its signals. The mirror follows the service's unique
//! owner: a restart is a new owner, a new generation and a fresh load, and a signal from any
//! other sender is ignored. Every call allows interactive authorisation, so polkit can ask.

use std::cell::{Cell, Ref, RefCell};
use std::future::Future;
use std::rc::Rc;

use athanor_bar::props::{self, Objects};
use gtk4::prelude::*;
use gtk4::{gio, glib};

pub const TIMEOUT_MS: i32 = 5000;
/// A call that waits for the person: a pairing, or a connection polkit asks about.
pub const INTERACTIVE_TIMEOUT_MS: i32 = 120_000;
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

pub fn call(
    connection: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
    timeout: i32,
) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static {
    connection.call_future(
        Some(name),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
        timeout,
    )
}

pub fn set_property(
    connection: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    property: &str,
    value: glib::Variant,
) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static {
    let args = (interface, property, value).to_variant();
    call(connection, name, path, PROPERTIES, "Set", Some(&args), TIMEOUT_MS)
}

/// Runs a call whose reply nobody reads; a failure is logged with the action's name only.
/// The arguments are never logged: one of them may be a password.
pub fn spawn(
    action: &'static str,
    future: impl Future<Output = Result<glib::Variant, glib::Error>> + 'static,
) {
    glib::spawn_future_local(async move {
        if let Err(err) = future.await {
            tracing::warn!(error = %err, action, "a system service refused or did not answer");
        }
    });
}

/// What the mirror loads.
pub enum Source {
    /// `GetManagedObjects` at this path, then `InterfacesAdded`, `InterfacesRemoved` and
    /// `PropertiesChanged`.
    Managed(&'static str),
    /// `GetAll` of each (path, interface), then `PropertiesChanged`.
    Fixed(Vec<(&'static str, &'static str)>),
}

pub struct Mirror {
    connection: gio::DBusConnection,
    name: String,
    source: Source,
    objects: RefCell<Objects>,
    owner: RefCell<Option<String>>,
    generation: Cell<u64>,
    scheduled: Cell<bool>,
    notify: Box<dyn Fn()>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    watcher: RefCell<Option<gio::WatcherId>>,
}

impl Mirror {
    /// Mirrors `name` on `connection`. `notify` runs on an idle callback after a change,
    /// coalesced, never inside a signal handler.
    pub fn new(
        connection: &gio::DBusConnection,
        name: &str,
        source: Source,
        notify: impl Fn() + 'static,
    ) -> Rc<Mirror> {
        let mirror = Rc::new(Mirror {
            connection: connection.clone(),
            name: name.to_owned(),
            source,
            objects: RefCell::new(Objects::new()),
            owner: RefCell::new(None),
            generation: Cell::new(0),
            scheduled: Cell::new(false),
            notify: Box::new(notify),
            subscription: RefCell::new(None),
            watcher: RefCell::new(None),
        });
        let weak = Rc::downgrade(&mirror);
        let subscription = connection.subscribe_to_signal(
            Some(name),
            None,
            None,
            None,
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(mirror) = weak.upgrade() {
                    mirror.signal(&signal);
                }
            },
        );
        mirror.subscription.replace(Some(subscription));
        let (appeared, vanished) = (Rc::downgrade(&mirror), Rc::downgrade(&mirror));
        let watcher = gio::bus_watch_name_on_connection(
            connection,
            name,
            gio::BusNameWatcherFlags::NONE,
            move |_, _, owner| {
                if let Some(mirror) = appeared.upgrade() {
                    mirror.appeared(owner);
                }
            },
            move |_, _| {
                if let Some(mirror) = vanished.upgrade() {
                    mirror.vanished();
                }
            },
        );
        mirror.watcher.replace(Some(watcher));
        mirror
    }

    pub fn objects(&self) -> Ref<'_, Objects> {
        self.objects.borrow()
    }

    pub fn owner(&self) -> Option<String> {
        self.owner.borrow().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Whether `sender` is the service's current unique name.
    pub fn is_owner(&self, sender: &str) -> bool {
        self.owner.borrow().as_deref() == Some(sender)
    }

    pub fn connection(&self) -> &gio::DBusConnection {
        &self.connection
    }

    fn signal(self: &Rc<Self>, signal: &gio::DBusSignalRef<'_>) {
        if !self.is_owner(signal.sender_name) {
            return;
        }
        let Some(change) = props::change(
            signal.object_path,
            signal.interface_name,
            signal.signal_name,
            signal.parameters,
        ) else {
            return;
        };
        props::apply(&mut self.objects.borrow_mut(), change);
        self.schedule();
    }

    fn appeared(self: &Rc<Self>, owner: &str) {
        self.owner.replace(Some(owner.to_owned()));
        self.generation.set(self.generation.get() + 1);
        self.objects.borrow_mut().clear();
        self.schedule();
        self.load(owner);
    }

    fn vanished(self: &Rc<Self>) {
        if self.owner.replace(None).is_some() {
            tracing::info!(name = %self.name, "the service left the bus");
        }
        self.generation.set(self.generation.get() + 1);
        self.objects.borrow_mut().clear();
        self.schedule();
    }

    /// Loads the objects from `owner`, the unique name: a reply always comes from the owner
    /// this generation belongs to.
    fn load(self: &Rc<Self>, owner: &str) {
        let generation = self.generation.get();
        let requests: Vec<(&'static str, Option<&'static str>)> = match &self.source {
            Source::Managed(root) => vec![(*root, None)],
            Source::Fixed(list) => list.iter().map(|(path, interface)| (*path, Some(*interface))).collect(),
        };
        for (path, interface) in requests {
            let reply = match interface {
                None => call(&self.connection, owner, path, OBJECT_MANAGER, "GetManagedObjects", None, TIMEOUT_MS),
                Some(interface) => {
                    let args = (interface,).to_variant();
                    call(&self.connection, owner, path, PROPERTIES, "GetAll", Some(&args), TIMEOUT_MS)
                }
            };
            let weak = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let reply = reply.await;
                let Some(mirror) = weak.upgrade() else { return };
                if mirror.generation.get() != generation {
                    return;
                }
                let reply = match reply {
                    Ok(reply) => reply,
                    Err(err) => {
                        tracing::warn!(error = %err, name = %mirror.name, path, "the service did not answer; it is shown as absent");
                        return;
                    }
                };
                match interface {
                    None => match props::managed_objects(&reply) {
                        Some(objects) => {
                            mirror.objects.replace(objects);
                        }
                        None => tracing::error!(name = %mirror.name, "GetManagedObjects answered with an unexpected type"),
                    },
                    Some(interface) => match props::get_all(&reply) {
                        Some(values) => {
                            mirror
                                .objects
                                .borrow_mut()
                                .entry(path.to_owned())
                                .or_default()
                                .insert(interface.to_owned(), values);
                        }
                        None => tracing::error!(name = %mirror.name, path, "GetAll answered with an unexpected type"),
                    },
                }
                mirror.schedule();
            });
        }
    }

    fn schedule(self: &Rc<Self>) {
        if self.scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(mirror) = weak.upgrade() {
                mirror.scheduled.set(false);
                (mirror.notify)();
            }
        });
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        // The subscription unsubscribes itself when dropped with the struct.
        if let Some(watcher) = self.watcher.take() {
            gio::bus_unwatch_name(watcher);
        }
    }
}
```

- [ ] **Step 2: Add `Bar::fit_groups` and declare the modules in `src/ui/mod.rs`**

After `mod accessibility;` add `mod bus;`, and add `mod network;` between `mod menu;` and `mod notifications;` (both from 2b.3), so that the list stays alphabetical.

In `build`, before the `_ => None` arm:

```rust
        Module::Network => network::new(bar),
```

In `impl Bar`, after `refresh`:

```rust
    /// Refits every surface's groups after a module showed or hid itself outside `refresh`:
    /// the system modules follow their service's signals, not the bar's events.
    pub fn fit_groups(&self) {
        for surface in self.surfaces.borrow().iter() {
            surface.fit_groups();
        }
    }
```

- [ ] **Step 3: Write `src/ui/network.rs`**

```rust
//! The network module (doc_bar.md, BR3): the wired state, the Wi-Fi list, joining with a
//! password, VPN and airplane mode, over NetworkManager on the system bus. One service per
//! process mirrors NetworkManager and is its secret agent; each surface has a view.
//!
//! The secret agent (`/org/freedesktop/NetworkManager/SecretAgent`, `os.athanor.Bar`)
//! answers only the current owner of `org.freedesktop.NetworkManager`, one request at a
//! time, and only for a Wi-Fi personal password the person types. The password goes from
//! the entry into the reply and nowhere else: it is never logged, never stored by the bar,
//! and the entry is cleared as the reply leaves.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use athanor_bar::network::{
    self, ConnectionInfo, Link, Network, NetworkState, PasswordPrompt, SecretsRequest, Security, Vpn,
};
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{gio, glib, pango};

use super::bus::{self, Mirror, Source};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const ERROR: &str = "org.freedesktop.NetworkManager.SecretAgent.";
const MAX_LIST_HEIGHT: i32 = 320;
const AGENT_XML: &str = r#"<node>
  <interface name="org.freedesktop.NetworkManager.SecretAgent">
    <method name="GetSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
      <arg name="hints" type="as" direction="in"/>
      <arg name="flags" type="u" direction="in"/>
      <arg name="secrets" type="a{sa{sv}}" direction="out"/>
    </method>
    <method name="CancelGetSecrets">
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
    </method>
    <method name="SaveSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
    <method name="DeleteSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
  </interface>
</node>"#;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

fn error(name: &str) -> String {
    format!("{ERROR}{name}")
}

/// What the password page answers.
enum Ask {
    /// A network with no saved profile: the password goes into `AddAndActivateConnection`.
    Join { network: Network, device: String },
    /// NetworkManager's `GetSecrets`, answered through the pending invocation.
    Secrets,
}

struct Pending {
    request: SecretsRequest,
    invocation: gio::DBusMethodInvocation,
    view: Weak<View>,
}

struct Service {
    bar: Weak<Bar>,
    mirror: RefCell<Option<Rc<Mirror>>>,
    /// The mirror's generation the caches and the registration belong to.
    seen: Cell<u64>,
    connections: RefCell<BTreeMap<String, ConnectionInfo>>,
    fetching: RefCell<BTreeSet<String>>,
    state: RefCell<Option<NetworkState>>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    agent: RefCell<Option<gio::RegistrationId>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            mirror: RefCell::new(None),
            seen: Cell::new(0),
            connections: RefCell::new(BTreeMap::new()),
            fetching: RefCell::new(BTreeSet::new()),
            state: RefCell::new(None),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            agent: RefCell::new(None),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        service.start();
        service
    }

    fn start(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let connection = match gio::bus_get_future(gio::BusType::System).await {
                Ok(connection) => connection,
                Err(err) => {
                    tracing::warn!(error = %err, "no system bus; the network module is hidden");
                    return;
                }
            };
            let Some(service) = weak.upgrade() else { return };
            service.register_agent(&connection);
            let notify = weak.clone();
            let mirror = Mirror::new(&connection, network::NM, Source::Managed(network::NM_ROOT), move || {
                if let Some(service) = notify.upgrade() {
                    service.changed();
                }
            });
            service.mirror.replace(Some(mirror));
        });
    }

    fn register_agent(self: &Rc<Self>, connection: &gio::DBusConnection) {
        let interface = gio::DBusNodeInfo::for_xml(AGENT_XML)
            .ok()
            .and_then(|node| node.lookup_interface(network::AGENT_IFACE));
        let Some(interface) = interface else {
            tracing::error!("the secret agent's interface does not parse; joining a saved network that needs a password will fail");
            return;
        };
        let weak = Rc::downgrade(self);
        let registered = connection
            .register_object(network::AGENT_PATH, &interface)
            .method_call(move |_, sender, _, _, method, params, invocation| match weak.upgrade() {
                Some(service) => service.agent_call(sender, method, &params, invocation),
                None => invocation.return_dbus_error(&error("NoSecrets"), "the agent is gone"),
            })
            .build();
        match registered {
            Ok(id) => {
                self.agent.replace(Some(id));
            }
            Err(err) => tracing::error!(error = %err, "cannot export the NetworkManager secret agent"),
        }
    }

    fn mirror(&self) -> Option<Rc<Mirror>> {
        self.mirror.borrow().clone()
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    /// The mirror changed: a new owner registers the agent again, then the state is
    /// recomputed and pushed into the views when it differs.
    fn changed(self: &Rc<Self>) {
        let Some(mirror) = self.mirror() else { return };
        let generation = mirror.generation();
        if self.seen.replace(generation) != generation {
            self.connections.borrow_mut().clear();
            self.fetching.borrow_mut().clear();
            self.cancel_pending("AgentCanceled");
            if let Some(owner) = mirror.owner() {
                let args = (network::AGENT_ID,).to_variant();
                bus::spawn(
                    "register the NetworkManager secret agent",
                    bus::call(
                        mirror.connection(),
                        &owner,
                        network::AGENT_MANAGER_PATH,
                        network::AGENT_MANAGER,
                        "Register",
                        Some(&args),
                        bus::TIMEOUT_MS,
                    ),
                );
            }
        }
        self.fetch_connections(&mirror);
        let state = network::state(&mirror.objects(), &self.connections.borrow());
        if *self.state.borrow() == state {
            return;
        }
        if state.is_none() {
            self.cancel_pending("AgentCanceled");
        }
        self.state.replace(state);
        for view in self.views() {
            view.show(self.state.borrow().as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    /// The settings of each saved profile, read once: they tell a saved network from a new
    /// one, and name the VPNs.
    // ponytail: read once per profile; follow Settings.Connection's Updated signal if renamed
    // profiles must show their new name without a restart of NetworkManager.
    fn fetch_connections(self: &Rc<Self>, mirror: &Rc<Mirror>) {
        let Some(owner) = mirror.owner() else { return };
        let paths: Vec<String> = mirror
            .objects()
            .iter()
            .filter(|(_, interfaces)| interfaces.contains_key(network::SETTINGS_CONNECTION))
            .map(|(path, _)| path.clone())
            .collect();
        self.connections.borrow_mut().retain(|path, _| paths.contains(path));
        let generation = mirror.generation();
        for path in paths {
            if self.connections.borrow().contains_key(&path) || !self.fetching.borrow_mut().insert(path.clone()) {
                continue;
            }
            let reply = bus::call(
                mirror.connection(),
                &owner,
                &path,
                network::SETTINGS_CONNECTION,
                "GetSettings",
                None,
                bus::TIMEOUT_MS,
            );
            let weak = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let reply = reply.await;
                let Some(service) = weak.upgrade() else { return };
                if service.seen.get() != generation {
                    return;
                }
                service.fetching.borrow_mut().remove(&path);
                match reply.as_ref().ok().and_then(network::connection_info) {
                    Some(info) => {
                        service.connections.borrow_mut().insert(path, info);
                        service.changed();
                    }
                    None => tracing::warn!(path, "a NetworkManager profile has unreadable settings; it is not listed"),
                }
            });
        }
    }

    fn agent_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        let from_owner = self
            .mirror()
            .is_some_and(|mirror| sender.is_some_and(|sender| mirror.is_owner(sender)));
        if !from_owner {
            tracing::warn!(method, sender = sender.unwrap_or(""), "a secret agent call from a process that is not NetworkManager was refused");
            invocation.return_dbus_error(&error("PermissionDenied"), "only NetworkManager may call this agent");
            return;
        }
        match method {
            "GetSecrets" => self.get_secrets(params, invocation),
            "CancelGetSecrets" => {
                if let Some((connection, setting)) = network::cancel_request(params) {
                    let matches = self
                        .pending
                        .borrow()
                        .as_ref()
                        .is_some_and(|pending| pending.request.connection == connection && pending.request.setting == setting);
                    if matches {
                        self.cancel_pending("AgentCanceled");
                    }
                }
                invocation.return_value(None);
            }
            // Nothing is ever stored here, so there is nothing to delete; and nothing is saved.
            "DeleteSecrets" => invocation.return_value(None),
            "SaveSecrets" => invocation.return_dbus_error(&error("Failed"), "this agent keeps no secrets"),
            _ => invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    fn get_secrets(self: &Rc<Self>, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
        let Some(request) = network::secrets_request(params) else {
            invocation.return_dbus_error(&error("InvalidConnection"), "the request cannot be read");
            return;
        };
        let Some(prompt) = request.prompt() else {
            invocation.return_dbus_error(&error("NoSecrets"), "this agent answers only Wi-Fi personal passwords");
            return;
        };
        if self.pending.borrow().is_some() {
            invocation.return_dbus_error(&error("NoSecrets"), "another request is open");
            return;
        }
        let Some(view) = self.prompt_view() else {
            invocation.return_dbus_error(&error("NoSecrets"), "the bar shows no network module");
            return;
        };
        self.pending.replace(Some(Pending {
            request,
            invocation,
            view: Rc::downgrade(&view),
        }));
        view.ask(Ask::Secrets, &prompt);
    }

    /// The view a prompt shows on: the one whose popover opened last, else the first shown.
    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.popup.button.is_mapped();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// Answers the pending request: the password, or `UserCanceled`.
    fn answer(&self, password: Option<&str>) {
        let Some(pending) = self.pending.take() else { return };
        match password {
            Some(password) => pending.invocation.return_value(Some(&network::secrets_reply(password))),
            None => pending.invocation.return_dbus_error(&error("UserCanceled"), "the person canceled"),
        }
    }

    fn cancel_pending(&self, name: &str) {
        let Some(pending) = self.pending.take() else { return };
        pending.invocation.return_dbus_error(&error(name), "the request was withdrawn");
        if let Some(view) = pending.view.upgrade() {
            view.close_prompt();
        }
    }

    fn manager_call(&self, method: &'static str, args: Option<glib::Variant>) {
        let (Some(mirror), Some(args)) = (self.mirror(), args) else { return };
        bus::spawn(
            method,
            bus::call(
                mirror.connection(),
                network::NM,
                network::NM_PATH,
                network::NM,
                method,
                Some(&args),
                bus::INTERACTIVE_TIMEOUT_MS,
            ),
        );
    }

    fn set_manager(&self, values: &[(&'static str, bool)]) {
        let Some(mirror) = self.mirror() else { return };
        for (property, value) in values {
            bus::spawn(
                property,
                bus::set_property(
                    mirror.connection(),
                    network::NM,
                    network::NM_PATH,
                    network::NM,
                    property,
                    value.to_variant(),
                ),
            );
        }
    }

    fn pressed(&self, view: &Rc<View>, network: &Network, device: &str) {
        if let Some(active) = &network.active {
            self.manager_call("DeactivateConnection", network::deactivate(active));
        } else if let Some(saved) = &network.saved {
            self.manager_call("ActivateConnection", network::activate(saved, device, &network.access_point));
        } else {
            match network.security {
                Security::Open => {
                    self.manager_call("AddAndActivateConnection", network::add_and_activate(network, None, device))
                }
                Security::Personal { .. } => view.ask(
                    Ask::Join {
                        network: network.clone(),
                        device: device.to_owned(),
                    },
                    &PasswordPrompt {
                        label: network.label.clone(),
                        security: network.security,
                        retry: false,
                    },
                ),
                Security::Other => {}
            }
        }
    }

    fn switch_vpn(&self, vpn: &Vpn, on: bool) {
        match (&vpn.active, on) {
            (None, true) => self.manager_call("ActivateConnection", network::activate(&vpn.connection, "/", "/")),
            (Some(active), false) => self.manager_call("DeactivateConnection", network::deactivate(active)),
            _ => {}
        }
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    stack: gtk4::Stack,
    wired: gtk4::Label,
    wifi_row: gtk4::Box,
    wifi: gtk4::Switch,
    scroller: gtk4::ScrolledWindow,
    networks: gtk4::Box,
    vpns: gtk4::Box,
    airplane: gtk4::Switch,
    title: gtk4::Label,
    retry: gtk4::Label,
    entry: gtk4::PasswordEntry,
    connect: gtk4::Button,
    asking: RefCell<Option<Ask>>,
    security: Cell<Security>,
    /// A switch is being set from NetworkManager, not by the person.
    updating: Cell<bool>,
    /// `open` came before the module could show (the captures of BR9).
    pending_open: Cell<bool>,
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("network-wireless-offline-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Network"));
        popup.button.set_visible(false);

        let heading = gtk4::Label::new(Some(&tr("Network")));
        heading.add_css_class("bar-popover-title");
        heading.set_xalign(0.0);
        let wired = gtk4::Label::new(None);
        wired.add_css_class("bar-popover-note");
        wired.set_xalign(0.0);
        let (wifi_row, wifi) = switch_row(&tr("Wi-Fi"));
        let networks = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let scroller = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .max_content_height(MAX_LIST_HEIGHT)
            .propagate_natural_height(true)
            .child(&networks)
            .build();
        let vpns = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let (airplane_row, airplane) = switch_row(&tr("Airplane mode"));
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        for widget in [
            heading.upcast_ref::<gtk4::Widget>(),
            wired.upcast_ref(),
            wifi_row.upcast_ref(),
            scroller.upcast_ref(),
            vpns.upcast_ref(),
            airplane_row.upcast_ref(),
        ] {
            list.append(widget);
        }

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_wrap(true);
        title.set_xalign(0.0);
        let retry = gtk4::Label::new(Some(&tr("The password was not accepted. Try again.")));
        retry.add_css_class("bar-popover-note");
        retry.set_wrap(true);
        retry.set_xalign(0.0);
        let entry = gtk4::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        entry.update_relation(&[Relation::LabelledBy(&[title.upcast_ref()])]);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.add_css_class("bar-row");
        let connect = gtk4::Button::with_label(&tr("Connect"));
        connect.add_css_class("bar-confirm");
        connect.set_sensitive(false);
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&connect);
        let password = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        password.append(&title);
        password.append(&retry);
        password.append(&entry);
        password.append(&buttons);

        let stack = gtk4::Stack::new();
        stack.add_named(&list, Some("list"));
        stack.add_named(&password, Some("password"));
        popup.popover.set_child(Some(&stack));

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            stack,
            wired,
            wifi_row,
            wifi,
            scroller,
            networks,
            vpns,
            airplane,
            title,
            retry,
            entry,
            connect,
            asking: RefCell::new(None),
            security: Cell::new(Security::Open),
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers(&cancel);
        view
    }

    /// Every handler holds the view weakly: the view owns the widgets, and a strong
    /// reference from a widget would be a cycle.
    fn connect_handlers(self: &Rc<Self>, cancel: &gtk4::Button) {
        let weak = Rc::downgrade(self);
        self.wifi.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.set_manager(&[("WirelessEnabled", on)]);
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.airplane.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.set_manager(&[("WirelessEnabled", !on), ("WwanEnabled", !on)]);
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_changed(move |entry| {
            if let Some(view) = weak.upgrade() {
                view.connect
                    .set_sensitive(network::password_acceptable(view.security.get(), &entry.text()));
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(view) = weak.upgrade() {
                view.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.connect.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.submit();
            }
        });
        let popover = self.popup.popover.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_closed(move |_| {
            if let Some(view) = weak.upgrade() {
                view.abandon();
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
            }
        });
    }

    fn show(self: &Rc<Self>, state: Option<&NetworkState>) {
        let Some(state) = state else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.icon.set_icon_name(Some(network::icon(state)));
        match state.wired {
            Some(true) => self.wired.set_text(&tr("Wired: connected")),
            Some(false) => self.wired.set_text(&tr("Wired: not connected")),
            None => {}
        }
        self.wired.set_visible(state.wired.is_some());
        self.updating.set(true);
        self.wifi_row.set_visible(state.wifi.is_some());
        self.wifi.set_active(state.wifi.as_ref().is_some_and(|wifi| wifi.enabled));
        self.airplane.set_active(state.airplane);
        self.updating.set(false);
        clear(&self.networks);
        if let Some(wifi) = &state.wifi {
            for network in &wifi.networks {
                self.networks.append(&self.network_row(network, &wifi.device));
            }
        }
        self.scroller.set_visible(self.networks.first_child().is_some());
        clear(&self.vpns);
        for vpn in &state.vpns {
            self.vpns.append(&vpn_row(vpn));
        }
        self.vpns.set_visible(!state.vpns.is_empty());
        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }

    fn network_row(self: &Rc<Self>, network: &Network, device: &str) -> gtk4::Button {
        let usable = network.security != Security::Other || network.saved.is_some() || network.active.is_some();
        let name = match (network.link, network.security) {
            (Link::Connected, _) => tr_with("{network}, connected", "network", &network.label),
            (Link::Connecting, _) => tr_with("{network}, connecting", "network", &network.label),
            _ if !usable => tr_with("{network}, needs Settings", "network", &network.label),
            (_, Security::Personal { .. }) => tr_with("{network}, secured", "network", &network.label),
            _ => network.label.clone(),
        };
        let icon = gtk4::Image::from_icon_name(network::signal_icon(network.strength));
        let text = gtk4::Label::new(Some(&name));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(pango::EllipsizeMode::End);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.append(&icon);
        content.append(&text);
        let button = gtk4::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("bar-row");
        button.update_property(&[Property::Label(&name)]);
        button.set_sensitive(usable);
        let (weak, network, device) = (Rc::downgrade(self), network.clone(), device.to_owned());
        button.connect_clicked(move |_| {
            if let (Some(view), Some(service)) = (weak.upgrade(), service()) {
                service.pressed(&view, &network, &device);
            }
        });
        button
    }

    /// Shows the password page, opening the popover when it is closed.
    fn ask(self: &Rc<Self>, ask: Ask, prompt: &PasswordPrompt) {
        self.asking.replace(Some(ask));
        self.security.set(prompt.security);
        self.title
            .set_text(&tr_with("Password for {network}", "network", &prompt.label));
        self.retry.set_visible(prompt.retry);
        self.entry.set_text("");
        self.connect.set_sensitive(false);
        self.stack.set_visible_child_name("password");
        if !self.popup.popover.is_visible() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
        self.entry.grab_focus();
    }

    fn submit(self: &Rc<Self>) {
        let password = self.entry.text();
        if !network::password_acceptable(self.security.get(), &password) {
            return;
        }
        let Some(ask) = self.asking.take() else { return };
        self.entry.set_text("");
        self.stack.set_visible_child_name("list");
        if let Some(service) = service() {
            match ask {
                Ask::Join { network, device } => service.manager_call(
                    "AddAndActivateConnection",
                    network::add_and_activate(&network, Some(password.as_str()), &device),
                ),
                Ask::Secrets => service.answer(Some(password.as_str())),
            }
        }
        self.popup.popover.popdown();
    }

    /// The popover closed with the page open: a NetworkManager request is canceled.
    fn abandon(&self) {
        let asked = self.asking.take();
        self.entry.set_text("");
        self.stack.set_visible_child_name("list");
        if matches!(asked, Some(Ask::Secrets)) {
            if let Some(service) = service() {
                service.answer(None);
            }
        }
    }

    /// NetworkManager withdrew the request, or left.
    fn close_prompt(&self) {
        if self.asking.take().is_some() {
            self.entry.set_text("");
            self.stack.set_visible_child_name("list");
            self.popup.popover.popdown();
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // A rebuild removed the surface while NetworkManager waited for this view's answer.
        if matches!(self.asking.get_mut(), Some(Ask::Secrets)) {
            if let Some(service) = service() {
                service.answer(None);
            }
        }
    }
}

fn vpn_row(vpn: &Vpn) -> gtk4::Box {
    let (row, switch) = switch_row(&vpn.label);
    switch.set_active(vpn.link != Link::Idle);
    let vpn = vpn.clone();
    switch.connect_state_set(move |_, on| {
        if let Some(service) = service() {
            service.switch_vpn(&vpn, on);
        }
        glib::Propagation::Proceed
    });
    row
}

struct NetworkUi {
    view: Rc<View>,
}

impl ModuleUi for NetworkUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    view.show(service.state.borrow().as_ref());
    Some(Box::new(NetworkUi { view }))
}
```

Two points to check while writing it:
- `Vpn` must derive `Clone`, and `Network` too. Task 2 derives both.
- `service.answer` runs inside the `Drop` of a `View`. `service()` borrows the thread-local only for the clone, so a drop during `rebuild` is safe.

- [ ] **Step 4: Build**

Run: `ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar`

Expected: clippy passes with `-D warnings`, the tests pass, and the release build and the DT_NEEDED check pass.

A clippy `too_many_arguments` on `bus::call` is expected with seven arguments. Put `#[allow(clippy::too_many_arguments)]` on that one function, as gio does on `call_future`, rather than bundling the arguments into a struct used once.

- [ ] **Step 5: Add the network section to `bar_modules_e2e.py`**

The imports at the top become:

```python
import os
import signal
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

import system_fixtures as fx  # noqa: E402
from atspi_check import find_application  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    buttons,
    check,
    confirm_button,
    failures,
    labelled,
    press,
    pss_kb,
    showing,
    wait_for,
)
from gi.repository import Gio, GLib  # noqa: E402
```

Gio and GLib have a single typelib version, so the last import needs no `gi.require_version`. Tasks 5 to 7 import nothing more.

Below `no_password_written`, add:

```python
NM_PATH = "/org/freedesktop/NetworkManager"
LAB_CONNECTION = f"{fx.NM_SETTINGS}/lab"
SECRET_AGENT = "/org/freedesktop/NetworkManager/SecretAgent"
AGENT_ERROR = "org.freedesktop.NetworkManager.SecretAgent."
RETRY_NOTE = "The password was not accepted. Try again."
ALLOW_INTERACTION, REQUEST_NEW = 0x1, 0x2


def showing_role(app, Atspi, role):
    """Every showing accessible of `role`, found afresh."""
    found = []

    def visit(accessible):
        try:
            if accessible.get_role_name() == role and showing(accessible, Atspi):
                found.append(accessible)
            children = [accessible.get_child_at_index(index) for index in range(accessible.get_child_count())]
        except GLib.Error:
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def type_password(app, Atspi, password):
    """Types into the showing password entry through AT-SPI's EditableText."""
    entries = showing_role(app, Atspi, "password text")
    return bool(entries) and entries[0].set_text_contents(password)


def press_confirm(app, Atspi, name):
    """Presses the confirmation's button `name` once it is sensitive."""
    _, button = confirm_button(app, Atspi, name)
    if button is None or not button.get_state_set().contains(Atspi.StateType.SENSITIVE):
        return False
    button.do_action(0)
    return True


def mock_calls(bus, name, path, method):
    """The arguments of each call of `method` a dbusmock object received, from the mock's
    memory: nothing here goes through a log file."""
    (calls,) = fx.call(bus, name, path, fx.MOCK, "GetMethodCalls", "(s)", (method,), "(a(tav))")
    return [arguments for _, arguments in calls]


def mock_pids(template):
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            argv = (proc / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        if b"dbusmock" in argv and template.encode() in argv:
            found.append(int(proc.name))
    return found


def property_of(bus, name, path, interface, prop):
    (value,) = fx.call(
        bus, name, path, "org.freedesktop.DBus.Properties", "Get", "(ss)", (interface, prop), "(v)"
    )
    return value


def open_popover(app, Atspi, button, content):
    """Opens the module's popover unless `content` already shows."""
    if not content():
        press(app, Atspi, button)
    return wait_for(content, 5)


def network(ctx):
    app, Atspi, bus = ctx.app, ctx.Atspi, ctx.bus

    def registrations():
        (count,) = fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "Registrations", reply="(u)")
        return count

    def ask(flags):
        (index,) = fx.call(
            bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "AskSecrets", "(sosu)",
            (ctx.bar, LAB_CONNECTION, "Athanor Lab", flags), "(u)",
        )
        return index

    def result(index):
        (value,) = fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "SecretsResult", "(u)", (index,), "(s)")
        return value

    def prompt_open():
        return bool(showing_role(app, Atspi, "password text"))

    check("the network module shows", wait_for(lambda: buttons(app, Atspi, "Network"), 10))
    check("the bar registered its secret agent once", wait_for(lambda: registrations() == 1, 5))
    check(
        "the network list opens with the connected network",
        open_popover(app, Atspi, "Network", lambda: buttons(app, Atspi, "Athanor Lab, connected")),
    )
    check("the wired state is shown", bool(labelled(app, Atspi, "label", "Wired: connected")))
    check("an open network is listed by name", bool(buttons(app, Atspi, "Corner Café")))
    campus = buttons(app, Atspi, "Campus, needs Settings")
    check(
        "an 802.1X network says it needs Settings and cannot be pressed",
        bool(campus) and not campus[0].get_state_set().contains(Atspi.StateType.SENSITIVE),
    )

    # A new WPA2 network: the password goes inline to AddAndActivateConnection.
    press(app, Atspi, "Home Network, secured")
    check("a secured network asks for its password", wait_for(prompt_open, 5))
    type_password(app, Atspi, "short")
    check("Connect stays off for a password WPA2 refuses", not press_confirm(app, Atspi, "Connect"))
    type_password(app, Atspi, PASSWORDS[0])
    check("Connect sends the password", wait_for(lambda: press_confirm(app, Atspi, "Connect"), 5))

    def joined():
        return mock_calls(bus, fx.NM, NM_PATH, "AddAndActivateConnection")

    check("NetworkManager received one AddAndActivateConnection", wait_for(lambda: len(joined()) == 1, 10))
    security = joined()[0][0].get("802-11-wireless-security", {}) if joined() else {}
    check("the connection carries the typed password and WPA-PSK", security.get("psk") == PASSWORDS[0]
          and security.get("key-mgmt") == "wpa-psk")
    check("the password page closed", wait_for(lambda: not prompt_open(), 5))

    # The secret agent, as NetworkManager calls it.
    first = ask(ALLOW_INTERACTION)
    check("GetSecrets opens the password prompt", wait_for(prompt_open, 5))
    second = ask(ALLOW_INTERACTION)
    check("a second request while one is open gets NoSecrets",
          wait_for(lambda: result(second) == f"error:{AGENT_ERROR}NoSecrets", 5))
    check("and the first stays open", result(first) == "pending" and prompt_open())
    fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "CancelSecrets", "(so)", (ctx.bar, LAB_CONNECTION))
    check("CancelGetSecrets withdraws the prompt",
          wait_for(lambda: result(first) == f"error:{AGENT_ERROR}AgentCanceled" and not prompt_open(), 5))
    third = ask(ALLOW_INTERACTION | REQUEST_NEW)
    check("a request for a new password says the old one was refused",
          wait_for(lambda: labelled(app, Atspi, "label", RETRY_NOTE), 5))
    type_password(app, Atspi, PASSWORDS[1])
    check("Connect answers GetSecrets with the typed password",
          wait_for(lambda: press_confirm(app, Atspi, "Connect"), 5)
          and wait_for(lambda: result(third) == f"reply:{PASSWORDS[1]}", 5))
    fourth = ask(ALLOW_INTERACTION)
    wait_for(prompt_open, 5)
    press(app, Atspi, "Cancel")
    check("Cancel answers UserCanceled", wait_for(lambda: result(fourth) == f"error:{AGENT_ERROR}UserCanceled", 5))

    # The agent refuses any process that is not NetworkManager, without a prompt.
    request = GLib.Variant(
        "(a{sa{sv}}osasu)",
        (
            {"802-11-wireless-security": {"key-mgmt": GLib.Variant("s", "wpa-psk")}},
            LAB_CONNECTION, "802-11-wireless-security", [], ALLOW_INTERACTION,
        ),
    )
    try:
        bus.call_sync(ctx.bar, SECRET_AGENT, "org.freedesktop.NetworkManager.SecretAgent", "GetSecrets",
                      request, None, Gio.DBusCallFlags.NONE, 5000, None)
        refused = False
    except GLib.Error as err:
        refused = Gio.DBusError.get_remote_error(err) == f"{AGENT_ERROR}PermissionDenied"
    check("a GetSecrets from another process is refused", refused)
    check("and opens no prompt", not prompt_open())

    # Airplane mode is NetworkManager's two radio switches.
    def airplane():
        return labelled(app, Atspi, "check box", "Airplane mode")

    open_popover(app, Atspi, "Network", airplane)
    airplane()[0].do_action(0)
    check("airplane mode turns Wi-Fi and mobile broadband off", wait_for(
        lambda: property_of(bus, fx.NM, NM_PATH, fx.NM, "WirelessEnabled") is False
        and property_of(bus, fx.NM, NM_PATH, fx.NM, "WwanEnabled") is False, 5))
    wait_for(lambda: airplane() and airplane()[0].get_state_set().contains(Atspi.StateType.CHECKED), 5)
    airplane()[0].do_action(0)
    check("and back on", wait_for(lambda: property_of(bus, fx.NM, NM_PATH, fx.NM, "WirelessEnabled") is True, 5))

    # The VPN switch activates the saved VPN profile.
    vpn = labelled(app, Atspi, "check box", "Office VPN")
    check("the VPN is listed", bool(vpn))
    if vpn:
        vpn[0].do_action(0)
    check("the VPN switch asks NetworkManager to activate the VPN", wait_for(
        lambda: any(str(args[0]).startswith(fx.NM_SETTINGS) and args[1] == "/"
                    for args in mock_calls(bus, fx.NM, NM_PATH, "ActivateConnection")), 5))
    press(app, Atspi, "Network")

    # NetworkManager restarts: the module hides, comes back, and the agent registers again.
    for pid in mock_pids("networkmanager"):
        os.kill(pid, signal.SIGTERM)
    check("without NetworkManager the module hides", wait_for(lambda: not buttons(app, Atspi, "Network"), 10))
    ctx.spawned.append(fx.networkmanager(bus))
    check("NetworkManager back: the module shows again", wait_for(lambda: buttons(app, Atspi, "Network"), 10))
    check("the agent registered with the new NetworkManager", wait_for(lambda: registrations() == 1, 10))


SECTIONS.append(network)
```

- [ ] **Step 6: Run the section**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e
grep -c . .scratch/shell-rig/bar-modules-e2e.log
grep -rl -e 'correct horse battery' -e 'staple-9-orbit' .scratch/shell-rig/ --include='*.log'
```

Expected:
- every check PASSes;
- the grep over the logs prints nothing and exits 1.

If `set_text_contents` returns False, GTK's EditableText is not reached: check that the entry is showing, and see the doubt in the handback. Do not fall back to a keyboard injector.

- [ ] **Step 7: Check that the existing gates are unchanged**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-e2e
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh atspi bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface bar-power
```

Expected: all pass. Without fixtures the network module finds no NetworkManager on the private bus and stays hidden.

- [ ] **Step 8: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/bus.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/network.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/test/shell/bar_modules_e2e.py
git commit -m "feat(bar): the network module, with NetworkManager's secret agent in the bar"
```

---
### Task 5: The Bluetooth module, with BlueZ's pairing agent in the bar

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/bluetooth.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (one `mod` line, one `build` arm)
- Modify: `forge/test/shell/bar_modules_e2e.py` (the `bluetooth` section)

**Interfaces:**
- Consumes:
  - `athanor_bar::bluetooth::*` (Task 2);
  - `bus::{call, set_property, spawn, Mirror, Source, TIMEOUT_MS, INTERACTIVE_TIMEOUT_MS}` and `Bar::fit_groups` (Task 4);
  - `popup::{Popup, switch_row}`;
  - in the e2e: `open_popover`, `property_of`, `press_confirm` and the imports (Task 4); `fx.BLUEZ`, `fx.FIXTURE`, and the fixture methods `DefaultAgent`, `AgentOwner(s)`, `RequestConfirmation(sou) -> u`, `AgentRequest(sso) -> u` and `ConfirmationResult(u) -> s`, and `fx.PAIRING_CODE` (Task 3).
- Produces: `Module::Bluetooth` built, and the `bluetooth` section.

The agent:
- **Who registers it.** `athanor-bar`, the person's own session process, exports `org.bluez.Agent1` at `/os/athanor/Bar/BluezAgent` on the system bus. For each new owner of `org.bluez`, it calls `RegisterAgent(path, "DisplayYesNo")` and then `RequestDefaultAgent`.
- **Why it is safe:**
  - it answers only calls whose sender is the current unique owner of `org.bluez`, and any other sender gets `org.bluez.Error.Rejected`;
  - it never types a PIN or a passkey: `RequestPinCode` and `RequestPasskey` are rejected. `DisplayYesNo` tells BlueZ to use numeric comparison or display, which the person confirms on both devices;
  - **it answers only the pairing the person started from the bar** (maintainer decision D8). `pairing` holds the device whose row the person pressed, from just before `Pair` is called until it returns. `RequestConfirmation`, `DisplayPasskey`, `DisplayPinCode`, `RequestAuthorization` and `AuthorizeService` for any other device, or with no pairing in progress, get `org.bluez.Error.Rejected` before anything is shown: no page opens and no open popover closes. A device nearby cannot start a pairing, and cannot choose the name a prompt shows;
  - `AuthorizeService` follows the same rule. After a pairing from the bar the device is set `Trusted`, and bluetoothd does not ask the agent about a trusted device again; a device paired elsewhere and not trusted is refused until the person trusts it in Settings;
  - one request is open at a time.

- [ ] **Step 1: Write `src/ui/bluetooth.rs`**

```rust
//! The Bluetooth module (doc_bar.md, BR3): the adapter's power, the paired devices, the
//! devices nearby while a popover is open, and pairing with a confirmation. One service per
//! process mirrors BlueZ and is its default agent; each surface has a view.
//!
//! The agent answers only the current owner of `org.bluez`, and only about the device whose
//! pairing the person started from the bar: every other request is rejected before anything
//! shows. It never types a code: the capability is DisplayYesNo, so the person compares six
//! digits on both screens.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_bar::bluetooth::{self, BluetoothState, Device};
use athanor_bar::props;
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gio, glib, pango};

use super::bus::{self, Mirror, Source};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const MAX_CODE_CHARS: usize = 16;
const AGENT_XML: &str = r#"<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode">
      <arg type="o" direction="in"/><arg type="s" direction="out"/>
    </method>
    <method name="DisplayPinCode">
      <arg type="o" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="RequestPasskey">
      <arg type="o" direction="in"/><arg type="u" direction="out"/>
    </method>
    <method name="DisplayPasskey">
      <arg type="o" direction="in"/><arg type="u" direction="in"/><arg type="q" direction="in"/>
    </method>
    <method name="RequestConfirmation">
      <arg type="o" direction="in"/><arg type="u" direction="in"/>
    </method>
    <method name="RequestAuthorization">
      <arg type="o" direction="in"/>
    </method>
    <method name="AuthorizeService">
      <arg type="o" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="Cancel"/>
  </interface>
</node>"#;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

struct Pending {
    invocation: gio::DBusMethodInvocation,
    view: Weak<View>,
}

struct Service {
    bar: Weak<Bar>,
    mirror: RefCell<Option<Rc<Mirror>>>,
    seen: Cell<u64>,
    state: RefCell<Option<BluetoothState>>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    /// The device the person pressed to pair, until `Pair` returns.
    pairing: RefCell<Option<String>>,
    /// Open popovers: discovery runs while at least one is open.
    open_popovers: Cell<u32>,
    agent: RefCell<Option<gio::RegistrationId>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            mirror: RefCell::new(None),
            seen: Cell::new(0),
            state: RefCell::new(None),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            pairing: RefCell::new(None),
            open_popovers: Cell::new(0),
            agent: RefCell::new(None),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        service.start();
        service
    }

    fn start(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let connection = match gio::bus_get_future(gio::BusType::System).await {
                Ok(connection) => connection,
                Err(err) => {
                    tracing::warn!(error = %err, "no system bus; the Bluetooth module is hidden");
                    return;
                }
            };
            let Some(service) = weak.upgrade() else { return };
            service.register_agent(&connection);
            let notify = weak.clone();
            let mirror = Mirror::new(&connection, bluetooth::BLUEZ, Source::Managed(bluetooth::ROOT), move || {
                if let Some(service) = notify.upgrade() {
                    service.changed();
                }
            });
            service.mirror.replace(Some(mirror));
        });
    }

    fn register_agent(self: &Rc<Self>, connection: &gio::DBusConnection) {
        let interface = gio::DBusNodeInfo::for_xml(AGENT_XML)
            .ok()
            .and_then(|node| node.lookup_interface(bluetooth::AGENT_IFACE));
        let Some(interface) = interface else {
            tracing::error!("the Bluetooth agent's interface does not parse; pairing will fail");
            return;
        };
        let weak = Rc::downgrade(self);
        let registered = connection
            .register_object(bluetooth::AGENT_PATH, &interface)
            .method_call(move |_, sender, _, _, method, params, invocation| match weak.upgrade() {
                Some(service) => service.agent_call(sender, method, &params, invocation),
                None => invocation.return_dbus_error(bluetooth::REJECTED, "the agent is gone"),
            })
            .build();
        match registered {
            Ok(id) => {
                self.agent.replace(Some(id));
            }
            Err(err) => tracing::error!(error = %err, "cannot export the Bluetooth agent"),
        }
    }

    fn mirror(&self) -> Option<Rc<Mirror>> {
        self.mirror.borrow().clone()
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    fn changed(self: &Rc<Self>) {
        let Some(mirror) = self.mirror() else { return };
        let generation = mirror.generation();
        if self.seen.replace(generation) != generation {
            self.cancel_pending();
            self.pairing.replace(None);
            if let Some(owner) = mirror.owner() {
                register_with(mirror.connection().clone(), owner);
                if self.open_popovers.get() > 0 {
                    self.discovery(true);
                }
            }
        }
        let state = bluetooth::state(&mirror.objects());
        if *self.state.borrow() == state {
            return;
        }
        if state.is_none() {
            self.cancel_pending();
        }
        // bluetoothd stops discovery when the adapter powers off: start it again when the
        // adapter comes back on under an open popover.
        let powered_on = state.as_ref().is_some_and(|state| state.powered)
            && !self.state.borrow().as_ref().is_some_and(|state| state.powered);
        self.state.replace(state);
        if powered_on && self.open_popovers.get() > 0 {
            self.discovery(true);
        }
        for view in self.views() {
            view.show(self.state.borrow().as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    fn agent_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        let Some(mirror) = self.mirror() else {
            invocation.return_dbus_error(bluetooth::REJECTED, "BlueZ is not running");
            return;
        };
        if !sender.is_some_and(|sender| mirror.is_owner(sender)) {
            tracing::warn!(method, sender = sender.unwrap_or(""), "a Bluetooth agent call from a process that is not BlueZ was refused");
            invocation.return_dbus_error(bluetooth::REJECTED, "only BlueZ may call this agent");
            return;
        }
        match method {
            "RequestConfirmation" | "DisplayPasskey" | "DisplayPinCode" | "RequestAuthorization" | "AuthorizeService"
                if !self.is_bar_pairing(params) =>
            {
                tracing::info!(method, "a Bluetooth request the person did not start from the bar was rejected");
                invocation.return_dbus_error(bluetooth::REJECTED, "the person did not start this pairing from the bar");
            }
            "RequestConfirmation" => {
                let request = bluetooth::device_and_passkey(params).and_then(|(device, passkey)| {
                    Some((bluetooth::device_label(&mirror.objects(), &device)?, bluetooth::passkey_label(passkey)?))
                });
                let Some((label, code)) = request else {
                    invocation.return_dbus_error(bluetooth::REJECTED, "unknown device or invalid passkey");
                    return;
                };
                self.ask(invocation, &label, &code);
            }
            "DisplayPasskey" | "DisplayPinCode" => {
                let code = if method == "DisplayPasskey" {
                    bluetooth::device_and_passkey(params).and_then(|(_, passkey)| bluetooth::passkey_label(passkey))
                } else if props::has_type(params, "(os)") {
                    params.try_child_value(1).and_then(|pin| {
                        pin.str().map(|pin| {
                            pin.chars().filter(char::is_ascii_alphanumeric).take(MAX_CODE_CHARS).collect::<String>()
                        })
                    })
                } else {
                    None
                };
                let label = bluetooth::device_of(params)
                    .or_else(|| bluetooth::device_and_passkey(params).map(|(device, _)| device))
                    .and_then(|device| bluetooth::device_label(&mirror.objects(), &device));
                if let (Some(label), Some(code), Some(view)) = (label, code, self.prompt_view()) {
                    view.display(&label, &code);
                }
                invocation.return_value(None);
            }
            // The guard above admitted only the device of the pairing in progress.
            "RequestAuthorization" | "AuthorizeService" => invocation.return_value(None),
            "RequestPinCode" | "RequestPasskey" => {
                invocation.return_dbus_error(bluetooth::REJECTED, "this agent does not type codes")
            }
            "Cancel" => {
                self.cancel_pending();
                for view in self.views() {
                    view.close_page();
                }
                invocation.return_value(None);
            }
            "Release" => invocation.return_value(None),
            _ => invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    /// Whether a request names the device of the pairing the person started from the bar
    /// (maintainer decision D8). Every agent request names its device first: `(o)`, `(os)`,
    /// `(ou)` or `(ouq)`.
    fn is_bar_pairing(&self, params: &glib::Variant) -> bool {
        bluetooth::device_of(params)
            .or_else(|| bluetooth::device_and_passkey(params).map(|(device, _)| device))
            .is_some_and(|device| self.pairing.borrow().as_deref() == Some(device.as_str()))
    }

    fn ask(self: &Rc<Self>, invocation: gio::DBusMethodInvocation, label: &str, code: &str) {
        if self.pending.borrow().is_some() {
            invocation.return_dbus_error(bluetooth::REJECTED, "another request is open");
            return;
        }
        let Some(view) = self.prompt_view() else {
            invocation.return_dbus_error(bluetooth::REJECTED, "the bar shows no Bluetooth module");
            return;
        };
        self.pending.replace(Some(Pending {
            invocation,
            view: Rc::downgrade(&view),
        }));
        view.confirm(label, code);
    }

    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.popup.button.is_mapped();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// The person's answer to `RequestConfirmation`.
    fn answer(&self, confirmed: bool) {
        let Some(pending) = self.pending.take() else { return };
        if confirmed {
            pending.invocation.return_value(None);
        } else {
            pending.invocation.return_dbus_error(bluetooth::REJECTED, "the person declined");
        }
    }

    fn cancel_pending(&self) {
        let Some(pending) = self.pending.take() else { return };
        pending.invocation.return_dbus_error(bluetooth::CANCELED, "the request was withdrawn");
        if let Some(view) = pending.view.upgrade() {
            view.close_page();
        }
    }

    fn device_call(&self, device: &str, method: &'static str) {
        let Some(mirror) = self.mirror() else { return };
        bus::spawn(
            method,
            bus::call(mirror.connection(), bluetooth::BLUEZ, device, bluetooth::DEVICE, method, None, bus::TIMEOUT_MS),
        );
    }

    fn pressed(self: &Rc<Self>, device: &Device) {
        if device.connected {
            self.device_call(&device.path, "Disconnect");
        } else if device.paired {
            self.device_call(&device.path, "Connect");
        } else {
            self.pair(device.path.clone());
        }
    }

    /// Pairs, trusts, connects: the order GNOME and COSMIC use. Trusting lets the device
    /// reconnect later without asking again.
    fn pair(self: &Rc<Self>, device: String) {
        let Some(mirror) = self.mirror() else { return };
        if self.pairing.borrow().is_some() {
            return;
        }
        self.pairing.replace(Some(device.clone()));
        let connection = mirror.connection().clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let paired = bus::call(
                &connection, bluetooth::BLUEZ, &device, bluetooth::DEVICE, "Pair", None, bus::INTERACTIVE_TIMEOUT_MS,
            )
            .await;
            if let Some(service) = weak.upgrade() {
                service.pairing.replace(None);
            }
            if let Err(err) = paired {
                tracing::warn!(error = %err, "pairing failed");
                return;
            }
            let trusted =
                bus::set_property(&connection, bluetooth::BLUEZ, &device, bluetooth::DEVICE, "Trusted", true.to_variant())
                    .await;
            if let Err(err) = trusted {
                tracing::warn!(error = %err, "the paired device could not be trusted");
            }
            bus::spawn(
                "Connect",
                bus::call(&connection, bluetooth::BLUEZ, &device, bluetooth::DEVICE, "Connect", None, bus::TIMEOUT_MS),
            );
        });
    }

    fn set_powered(&self, on: bool) {
        let (Some(mirror), Some(adapter)) = (self.mirror(), self.state.borrow().as_ref().map(|s| s.adapter.clone()))
        else {
            return;
        };
        bus::spawn(
            "Powered",
            bus::set_property(mirror.connection(), bluetooth::BLUEZ, &adapter, bluetooth::ADAPTER, "Powered", on.to_variant()),
        );
    }

    fn discovery(&self, on: bool) {
        let Some(mirror) = self.mirror() else { return };
        let Some(adapter) = self
            .state
            .borrow()
            .as_ref()
            .filter(|state| state.powered)
            .map(|state| state.adapter.clone())
        else {
            return;
        };
        let method = if on { "StartDiscovery" } else { "StopDiscovery" };
        bus::spawn(
            method,
            bus::call(mirror.connection(), bluetooth::BLUEZ, &adapter, bluetooth::ADAPTER, method, None, bus::TIMEOUT_MS),
        );
    }

    fn popover_toggled(&self, open: bool) {
        let before = self.open_popovers.get();
        let after = if open { before + 1 } else { before.saturating_sub(1) };
        self.open_popovers.set(after);
        if before == 0 && after == 1 {
            self.discovery(true);
        } else if before == 1 && after == 0 {
            self.discovery(false);
        }
    }
}

/// `RegisterAgent`, then `RequestDefaultAgent`, to this owner.
fn register_with(connection: gio::DBusConnection, owner: String) {
    glib::spawn_future_local(async move {
        // A constant, valid path: the `else` is unreachable, and it is not an unwrap.
        let Ok(path) = glib::variant::ObjectPath::try_from(bluetooth::AGENT_PATH) else { return };
        let register = (path.clone(), bluetooth::CAPABILITY).to_variant();
        if let Err(err) = bus::call(
            &connection, &owner, bluetooth::AGENT_MANAGER_PATH, bluetooth::AGENT_MANAGER, "RegisterAgent",
            Some(&register), bus::TIMEOUT_MS,
        )
        .await
        {
            tracing::warn!(error = %err, "BlueZ refused the pairing agent; pairing from the bar will fail");
            return;
        }
        let default = (path,).to_variant();
        bus::spawn(
            "RequestDefaultAgent",
            bus::call(
                &connection, &owner, bluetooth::AGENT_MANAGER_PATH, bluetooth::AGENT_MANAGER, "RequestDefaultAgent",
                Some(&default), bus::TIMEOUT_MS,
            ),
        );
    });
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    stack: gtk4::Stack,
    power: gtk4::Switch,
    paired: gtk4::Box,
    nearby_title: gtk4::Label,
    nearby: gtk4::Box,
    title: gtk4::Label,
    code: gtk4::Label,
    note: gtk4::Label,
    confirm: gtk4::Button,
    /// The page shows a `RequestConfirmation` this view must answer.
    asking: Cell<bool>,
    updating: Cell<bool>,
    pending_open: Cell<bool>,
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("bluetooth-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Bluetooth"));
        popup.button.set_visible(false);

        let (power_row, power) = switch_row(&tr("Bluetooth"));
        power_row.add_css_class("bar-popover-title");
        let paired = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let nearby_title = gtk4::Label::new(Some(&tr("Nearby devices")));
        nearby_title.add_css_class("bar-popover-note");
        nearby_title.set_xalign(0.0);
        let nearby = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        list.append(&power_row);
        list.append(&paired);
        list.append(&nearby_title);
        list.append(&nearby);

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_wrap(true);
        title.set_xalign(0.0);
        let code = gtk4::Label::new(None);
        // The digits in the title's size: large enough to compare across the room, with
        // no new class in the shared stylesheet (athanor-style), which 2c also edits.
        code.add_css_class("bar-popover-title");
        let note = gtk4::Label::new(None);
        note.add_css_class("bar-popover-note");
        note.set_wrap(true);
        note.set_xalign(0.0);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.add_css_class("bar-row");
        let confirm = gtk4::Button::with_label(&tr("Pair"));
        confirm.add_css_class("bar-confirm");
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&confirm);
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        page.append(&title);
        page.append(&code);
        page.append(&note);
        page.append(&buttons);

        let stack = gtk4::Stack::new();
        stack.add_named(&list, Some("list"));
        stack.add_named(&page, Some("confirm"));
        popup.popover.set_child(Some(&stack));

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            stack,
            power,
            paired,
            nearby_title,
            nearby,
            title,
            code,
            note,
            confirm,
            asking: Cell::new(false),
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers(&cancel);
        view
    }

    fn connect_handlers(self: &Rc<Self>, cancel: &gtk4::Button) {
        let weak = Rc::downgrade(self);
        self.power.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.set_powered(on);
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.confirm.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.finish(true);
            }
        });
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.finish(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
                service.popover_toggled(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_closed(move |_| {
            if let Some(view) = weak.upgrade() {
                if view.asking.get() {
                    view.finish(false);
                }
                view.stack.set_visible_child_name("list");
            }
            if let Some(service) = service() {
                service.popover_toggled(false);
            }
        });
    }

    fn show(self: &Rc<Self>, state: Option<&BluetoothState>) {
        let Some(state) = state else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.icon.set_icon_name(Some(bluetooth::module_icon(state)));
        self.updating.set(true);
        self.power.set_active(state.powered);
        self.updating.set(false);
        clear(&self.paired);
        for device in &state.paired {
            self.paired.append(&self.device_row(device));
        }
        clear(&self.nearby);
        for device in &state.nearby {
            self.nearby.append(&self.device_row(device));
        }
        self.paired.set_visible(state.powered);
        self.nearby_title.set_visible(state.powered && state.discovering);
        self.nearby.set_visible(state.powered && state.discovering);
        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }

    fn device_row(self: &Rc<Self>, device: &Device) -> gtk4::Button {
        let name = if device.connected {
            tr_with("{device}, connected", "device", &device.label)
        } else {
            device.label.clone()
        };
        let text = gtk4::Label::new(Some(&name));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(pango::EllipsizeMode::End);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.append(&gtk4::Image::from_icon_name(device.icon));
        content.append(&text);
        let button = gtk4::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("bar-row");
        button.update_property(&[Property::Label(&name)]);
        let device = device.clone();
        button.connect_clicked(move |_| {
            if let Some(service) = service() {
                service.pressed(&device);
            }
        });
        button
    }

    fn open_page(&self) {
        self.stack.set_visible_child_name("confirm");
        if !self.popup.popover.is_visible() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }

    /// `RequestConfirmation`: the six digits both devices show, and Pair or Cancel.
    fn confirm(&self, label: &str, code: &str) {
        self.asking.set(true);
        self.title.set_text(&tr_with("Pair with {device}?", "device", label));
        self.code.set_text(code);
        self.note
            .set_text(&tr_with("Pair only if {device} shows the same number.", "device", label));
        self.confirm.set_visible(true);
        self.open_page();
        self.confirm.grab_focus();
    }

    /// `DisplayPasskey` or `DisplayPinCode`: a code to type on the other device.
    fn display(&self, label: &str, code: &str) {
        self.asking.set(false);
        self.title.set_text(&tr_with("Pairing with {device}", "device", label));
        self.code.set_text(code);
        self.note
            .set_text(&tr_with("Type this code on {device}, then press Enter there.", "device", label));
        self.confirm.set_visible(false);
        self.open_page();
    }

    fn finish(&self, confirmed: bool) {
        if self.asking.replace(false) {
            if let Some(service) = service() {
                service.answer(confirmed);
            }
        }
        self.stack.set_visible_child_name("list");
        if confirmed {
            self.popup.popover.popdown();
        }
    }

    fn close_page(&self) {
        self.asking.set(false);
        self.stack.set_visible_child_name("list");
    }
}

impl Drop for View {
    fn drop(&mut self) {
        let Some(service) = service() else { return };
        if self.asking.get() {
            service.answer(false);
        }
        // A surface that leaves with its popover open never emits `closed`: count it closed,
        // or discovery would run on with no popover to show it.
        if self.popup.popover.is_visible() {
            service.popover_toggled(false);
        }
    }
}

struct BluetoothUi {
    view: Rc<View>,
}

impl ModuleUi for BluetoothUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    view.show(service.state.borrow().as_ref());
    Some(Box::new(BluetoothUi { view }))
}
```

- [ ] **Step 2: Declare the module and build it**

In `src/ui/mod.rs`, add `mod bluetooth;` after `mod accessibility;`. Then add this arm next to `Module::Network`:

```rust
        Module::Bluetooth => bluetooth::new(bar),
```

- [ ] **Step 3: Build**

Run: `ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar`

Expected: PASS.

- [ ] **Step 4: Add the `bluetooth` section to `bar_modules_e2e.py`**

```python
BLUEZ_AGENT = "/os/athanor/Bar/BluezAgent"
ADAPTER = "/org/bluez/hci0"
REJECTED = "org.bluez.Error.Rejected"


def device_path(alias):
    (address,) = [address for address, name, _, _ in fx.DEVICES if name == alias]
    return f"{ADAPTER}/dev_{address.replace(':', '_')}"


def bluetooth(ctx):
    app, Atspi, bus = ctx.app, ctx.Atspi, ctx.bus
    device1 = "org.bluez.Device1"

    def confirm(alias, passkey):
        (index,) = fx.call(
            bus, fx.BLUEZ, "/org/bluez", fx.FIXTURE, "RequestConfirmation", "(sou)",
            (ctx.bar, device_path(alias), passkey), "(u)",
        )
        return index

    def request(method, alias):
        (index,) = fx.call(
            bus, fx.BLUEZ, "/org/bluez", fx.FIXTURE, "AgentRequest", "(sso)",
            (ctx.bar, method, device_path(alias)), "(u)",
        )
        return index

    def result(index):
        (value,) = fx.call(bus, fx.BLUEZ, "/org/bluez", fx.FIXTURE, "ConfirmationResult", "(u)", (index,), "(s)")
        return value

    def adapter(prop):
        return property_of(bus, fx.BLUEZ, ADAPTER, "org.bluez.Adapter1", prop)

    check("the Bluetooth module shows", wait_for(lambda: buttons(app, Atspi, "Bluetooth"), 10))
    check(
        "the bar is BlueZ's default agent",
        wait_for(lambda: fx.call(bus, fx.BLUEZ, "/org/bluez", fx.FIXTURE, "DefaultAgent", reply="(s)")[0] == BLUEZ_AGENT, 5),
    )
    # The template does not keep who registered the agent; the fixture's Pair needs it.
    fx.call(bus, fx.BLUEZ, "/org/bluez", fx.FIXTURE, "AgentOwner", "(s)", (ctx.bar,))
    check(
        "the list shows the connected headphones first",
        open_popover(app, Atspi, "Bluetooth", lambda: buttons(app, Atspi, "Headphones, connected")),
    )
    check("a paired, disconnected device is listed", bool(buttons(app, Atspi, "Keyboard")))
    check("an open popover discovers", wait_for(lambda: adapter("Discovering") is True, 5))

    def power():
        return labelled(app, Atspi, "check box", "Bluetooth")

    power()[0].do_action(0)
    check("the switch turns the adapter off", wait_for(lambda: adapter("Powered") is False, 5))
    wait_for(lambda: power() and not power()[0].get_state_set().contains(Atspi.StateType.CHECKED), 5)
    power()[0].do_action(0)
    check("and on again", wait_for(lambda: adapter("Powered") is True, 5))

    # Requests the person did not start from the bar (maintainer decision D8): rejected by the
    # agent before anything shows, and no open popover closes.
    unsolicited = confirm("Speaker", 111111)
    check(
        "an unsolicited RequestConfirmation from BlueZ is rejected",
        wait_for(lambda: result(unsolicited) == f"error:{REJECTED}", 5),
    )
    check("and shows no digits", not labelled(app, Atspi, "label", "111111"))
    check("and the popover stays open", bool(buttons(app, Atspi, "Headphones, connected")))
    check(
        "the device stays unpaired",
        property_of(bus, fx.BLUEZ, device_path("Speaker"), device1, "Paired") is False,
    )
    for method in ("RequestAuthorization", "AuthorizeService", "DisplayPasskey", "RequestPinCode", "RequestPasskey"):
        index = request(method, "Speaker")
        check(f"an unsolicited {method} is rejected", wait_for(lambda: result(index) == f"error:{REJECTED}", 5))
    check("DisplayPasskey showed nothing", not labelled(app, Atspi, "label", "222333"))

    # Pairing a device the person pressed: bluetoothd asks for the six digits (the fixture's
    # Pair), the person confirms, then the bar trusts and connects it. The BlueZ mock is
    # blocked while the page is open, so nothing below calls it until Pair is pressed.
    check("a nearby device is listed while discovering", wait_for(lambda: buttons(app, Atspi, "Phone"), 10))
    press(app, Atspi, "Phone")
    check("pressing it shows the digits BlueZ sent", wait_for(lambda: labelled(app, Atspi, "label", fx.PAIRING_CODE), 10))
    check("Pair confirms", wait_for(lambda: press_confirm(app, Atspi, "Pair"), 5))
    phone = device_path("Phone")
    check("the device is paired", wait_for(lambda: property_of(bus, fx.BLUEZ, phone, device1, "Paired") is True, 10))
    check("trusts it", wait_for(lambda: property_of(bus, fx.BLUEZ, phone, device1, "Trusted") is True, 5))
    check("and connects it", wait_for(lambda: buttons(app, Atspi, "Phone, connected"), 10))

    # Cancel on the page: the agent rejects, BlueZ fails the pairing, the device stays unpaired.
    press(app, Atspi, "Speaker")
    check("a second pairing shows its digits", wait_for(lambda: labelled(app, Atspi, "label", fx.PAIRING_CODE), 10))
    press(app, Atspi, "Cancel")
    check("Cancel closes the page", wait_for(lambda: not labelled(app, Atspi, "label", fx.PAIRING_CODE), 5))
    check(
        "and the device stays unpaired",
        wait_for(lambda: property_of(bus, fx.BLUEZ, device_path("Speaker"), device1, "Paired") is False, 5),
    )

    # The agent refuses any process that is not bluetoothd.
    try:
        bus.call_sync(
            ctx.bar, BLUEZ_AGENT, "org.bluez.Agent1", "RequestConfirmation",
            GLib.Variant("(ou)", (device_path("Keyboard"), 222222)), None, Gio.DBusCallFlags.NONE, 5000, None,
        )
        refused = False
    except GLib.Error as err:
        refused = Gio.DBusError.get_remote_error(err) == REJECTED
    check("a RequestConfirmation from another process is rejected", refused)
    check("and shows no code", not labelled(app, Atspi, "label", "222222"))

    if buttons(app, Atspi, "Headphones, connected"):
        press(app, Atspi, "Bluetooth")
    check("closing the popover stops discovery", wait_for(lambda: adapter("Discovering") is False, 5))


SECTIONS.append(bluetooth)
```

- [ ] **Step 5: Run it**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e
```

Expected: the network and Bluetooth checks PASS.

If "trusts it" fails: the BlueZ template's `Trusted` is a plain property, so `Properties.Set` succeeds. Look for the warning "the paired device could not be trusted" in `bar-modules-e2e-bar.log`.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/bluetooth.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs \
    forge/test/shell/bar_modules_e2e.py
git commit -m "feat(bar): the Bluetooth module, with BlueZ's pairing agent in the bar"
```

---
### Task 6: The audio module over libpulse, with the media controls

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mpris.rs`
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/audio.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (two `mod` lines, one `build` arm)
- Modify: `forge/test/shell/bar_modules_e2e.py` (the `audio` section)

**Interfaces:**
- Consumes:
  - `athanor_bar::audio::{self, Device, Track}` and `athanor_bar::props` (Task 2);
  - `bus::{call, spawn, Mirror, Source, TIMEOUT_MS}` and `Bar::fit_groups` (Task 4);
  - the crates `libpulse-binding` 2.30 and `libpulse-glib-binding` 2.29 (Task 2, Step 1).
  - The libpulse API used below was checked against the sources of libpulse-binding 2.30.1 and libpulse-glib-binding 2.29.0:
    - `Context::{new_with_proplist, set_state_callback, set_subscribe_callback, connect, disconnect, get_state, subscribe, introspect, set_default_sink, set_default_source}`;
    - `Introspector::{get_sink_info_by_name, get_source_info_by_name, get_sink_info_list, get_source_info_list, set_sink_volume_by_name, set_sink_mute_by_name, set_source_volume_by_name, set_source_mute_by_name}`;
    - `ChannelVolumes::{is_valid, max, scale}`, `Volume(pub u32)` and `ListResult::{Item, End, Error}`;
    - `SourceInfo::monitor_of_sink: Option<u32>`, and `SinkInfo::name` / `SourceInfo::name`: `Option<Cow<str>>`;
    - `libpulse_glib_binding::Mainloop::new(Option<&mut MainContext>) -> Option<Mainloop>`.
  - In the e2e: `open_popover`, `mock_calls` and `property_of` (Task 4); `fx.pactl`, `fx.pids_of`, `fx.pipewire_pulse`, `fx.PLAYER`, `fx.MPRIS_PATH` and `fx.MPRIS_PLAYER` (Task 3).
- Produces:
  - `mpris::Media::new(notify: impl Fn() + 'static) -> Rc<Media>`, with `Media::now() -> Option<NowPlaying>` and `Media::command(&self, method: &'static str)`;
  - `NowPlaying { track: Track, playing: bool, can_next: bool, can_previous: bool }`;
  - `Module::Audio` built, and the `audio` section.

Two libpulse rules, repeated in the code:
1. **A libpulse callback only stages data and schedules an idle callback.** `connect()` fires the state callback synchronously while the bar holds the context, and the introspection callbacks run inside libpulse's dispatch.
2. **Every call on the context goes through `with_ready`.** libpulse-binding asserts on the null operation that a call on a context in any other state returns, and `panic = "abort"` would end the bar. Sink and source names passed back come from the server as C strings, so `CString::new` inside libpulse-binding cannot fail on them.
3. **Only list-style introspection calls.** A single-item callback such as `get_server_info` asserts that the info pointer is not null (`introspect.rs`, `get_server_info_cb_proxy`). libpulse passes null when the reply is an error or times out, and `catch_unwind` catches nothing under `panic = "abort"`, so a hung sound server would end the bar. The defaults are read with `get_sink_info_by_name("@DEFAULT_SINK@")` and `get_source_info_by_name("@DEFAULT_SOURCE@")`, whose callbacks go through the list proxy: an error becomes `ListResult::Error`. Never call `get_server_info`, or any other introspection call whose callback takes a bare `&Info` instead of a `ListResult`.

- [ ] **Step 1: Write `src/ui/mpris.rs`**

```rust
//! The media controls' player (doc_bar.md, BR3): the first MPRIS player on the session bus,
//! by name, mirrored while it owns its name. A player that appears or leaves makes the
//! choice again.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_bar::audio::{self, Track};
use athanor_bar::props;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::bus::{self, Mirror, Source};

const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub track: Track,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

pub struct Media {
    session: RefCell<Option<gio::DBusConnection>>,
    player: RefCell<Option<(String, Rc<Mirror>)>>,
    notify: Rc<dyn Fn()>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

impl Media {
    pub fn new(notify: impl Fn() + 'static) -> Rc<Media> {
        let media = Rc::new(Media {
            session: RefCell::new(None),
            player: RefCell::new(None),
            notify: Rc::new(notify),
            subscription: RefCell::new(None),
        });
        let weak = Rc::downgrade(&media);
        glib::spawn_future_local(async move {
            let session = match gio::bus_get_future(gio::BusType::Session).await {
                Ok(session) => session,
                Err(err) => {
                    tracing::warn!(error = %err, "no session bus; the media controls are hidden");
                    return;
                }
            };
            let Some(media) = weak.upgrade() else { return };
            let watcher = Rc::downgrade(&media);
            let subscription = session.subscribe_to_signal(
                Some(DBUS),
                Some(DBUS),
                Some("NameOwnerChanged"),
                Some(DBUS_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let player = signal
                        .parameters
                        .try_child_value(0)
                        .and_then(|name| name.str().map(audio::is_player))
                        .unwrap_or(false);
                    if let (true, Some(media)) = (player, watcher.upgrade()) {
                        media.choose();
                    }
                },
            );
            media.subscription.replace(Some(subscription));
            media.session.replace(Some(session));
            media.choose();
        });
        media
    }

    /// Follows the first player by name; the choice is stable while players come and go.
    fn choose(self: &Rc<Self>) {
        let Some(session) = self.session.borrow().clone() else { return };
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let reply = bus::call(&session, DBUS, DBUS_PATH, DBUS, "ListNames", None, bus::TIMEOUT_MS).await;
            let Some(media) = weak.upgrade() else { return };
            let names = reply
                .ok()
                .filter(|reply| props::has_type(reply, "(as)"))
                .and_then(|reply| reply.try_child_value(0))
                .and_then(|names| names.get::<Vec<String>>());
            let Some(names) = names else {
                tracing::warn!("ListNames failed; the media controls keep their player");
                return;
            };
            let first = names.into_iter().filter(|name| audio::is_player(name)).min();
            let current = media.player.borrow().as_ref().map(|(name, _)| name.clone());
            if first == current {
                return;
            }
            let player = first.map(|name| {
                let notify = media.notify.clone();
                let mirror = Mirror::new(
                    &session,
                    &name,
                    Source::Fixed(vec![(audio::MPRIS_PATH, audio::MPRIS_PLAYER)]),
                    move || notify(),
                );
                (name, mirror)
            });
            media.player.replace(player);
            (media.notify)();
        });
    }

    pub fn now(&self) -> Option<NowPlaying> {
        let player = self.player.borrow();
        let (_, mirror) = player.as_ref()?;
        let objects = mirror.objects();
        let props = props::lookup(&objects, audio::MPRIS_PATH, audio::MPRIS_PLAYER)?;
        Some(NowPlaying {
            track: audio::track(props)?,
            playing: audio::playing(props),
            can_next: props::value::<bool>(props, "CanGoNext").unwrap_or(false),
            can_previous: props::value::<bool>(props, "CanGoPrevious").unwrap_or(false),
        })
    }

    /// `PlayPause`, `Next` or `Previous` to the followed player.
    pub fn command(&self, method: &'static str) {
        let session = self.session.borrow().clone();
        let player = self.player.borrow();
        let (Some(session), Some((name, _))) = (session, player.as_ref()) else { return };
        bus::spawn(
            method,
            bus::call(&session, name, audio::MPRIS_PATH, audio::MPRIS_PLAYER, method, None, bus::TIMEOUT_MS),
        );
    }
}
```

- [ ] **Step 2: Write `src/ui/audio.rs`**

```rust
//! The audio module (doc_bar.md, BR3): output and input volume, mute, the device in use,
//! and the media controls. The sound server is reached through libpulse on the bar's GLib
//! main loop; `pipewire-pulse` serves it. A lost server hides the module, and the bar
//! reconnects with a backoff from 500 ms to 8 s.
//!
//! Two rules keep libpulse from aborting the bar:
//! - a libpulse callback only stages data and schedules an idle callback: `connect()` calls
//!   the state callback while the context is borrowed, and the other callbacks run inside
//!   libpulse's dispatch;
//! - every call on the context goes through `with_ready`: libpulse-binding asserts on the
//!   null operation a context in any other state returns.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use athanor_bar::audio::{self, Device};
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::proplist::{properties, Proplist};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use libpulse_glib_binding::Mainloop;

use super::mpris::{Media, NowPlaying};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const FIRST_RETRY_MS: u64 = 500;
const LAST_RETRY_MS: u64 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Output,
    Input,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
    outputs: Vec<Device>,
    inputs: Vec<Device>,
    default_output: Option<String>,
    default_input: Option<String>,
}

/// What the four introspection calls of one refresh gather.
#[derive(Default)]
struct Gathering {
    snapshot: Snapshot,
    volumes: HashMap<(Kind, String), ChannelVolumes>,
    remaining: u8,
}

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

struct Service {
    bar: Weak<Bar>,
    mainloop: Option<Mainloop>,
    context: RefCell<Option<Context>>,
    retry_ms: Cell<u64>,
    gathering: RefCell<Option<Gathering>>,
    /// An event arrived during a refresh: refresh again once it ends.
    stale: Cell<bool>,
    snapshot: RefCell<Option<Snapshot>>,
    volumes: RefCell<HashMap<(Kind, String), ChannelVolumes>>,
    media: RefCell<Option<Rc<Media>>>,
    views: RefCell<Vec<Weak<View>>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let mainloop = Mainloop::new(None);
        if mainloop.is_none() {
            tracing::error!("libpulse has no GLib main loop; the audio module is hidden");
        }
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            mainloop,
            context: RefCell::new(None),
            retry_ms: Cell::new(FIRST_RETRY_MS),
            gathering: RefCell::new(None),
            stale: Cell::new(false),
            snapshot: RefCell::new(None),
            volumes: RefCell::new(HashMap::new()),
            media: RefCell::new(None),
            views: RefCell::new(Vec::new()),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        let weak = Rc::downgrade(&service);
        service.media.replace(Some(Media::new(move || {
            if let Some(service) = weak.upgrade() {
                service.show_all();
            }
        })));
        service.connect();
        service
    }

    fn connect(self: &Rc<Self>) {
        let Some(mainloop) = self.mainloop.as_ref() else { return };
        let Some(mut proplist) = Proplist::new() else {
            self.retry();
            return;
        };
        if proplist.set_str(properties::APPLICATION_NAME, "athanor-bar").is_err()
            || proplist.set_str(properties::APPLICATION_ID, "os.athanor.Bar").is_err()
        {
            tracing::error!("libpulse refused the bar's properties");
        }
        let Some(mut context) = Context::new_with_proplist(mainloop, "athanor-bar", &proplist) else {
            self.retry();
            return;
        };
        let weak = Rc::downgrade(self);
        context.set_state_callback(Some(Box::new(move || {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(service) = weak.upgrade() {
                    service.state_changed();
                }
            });
        })));
        let weak = Rc::downgrade(self);
        context.set_subscribe_callback(Some(Box::new(move |facility, _, _| {
            if matches!(facility, Some(Facility::Sink | Facility::Source | Facility::Server)) {
                let weak = weak.clone();
                glib::idle_add_local_once(move || {
                    if let Some(service) = weak.upgrade() {
                        service.refresh();
                    }
                });
            }
        })));
        // No autospawn: the session starts the sound server, never the bar.
        if let Err(err) = context.connect(None, FlagSet::NOAUTOSPAWN, None) {
            tracing::info!(error = %err, "no sound server yet");
            self.retry();
            return;
        }
        self.context.replace(Some(context));
    }

    fn retry(self: &Rc<Self>) {
        let delay = self.retry_ms.get();
        self.retry_ms.set((delay * 2).min(LAST_RETRY_MS));
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(service) = weak.upgrade() {
                service.connect();
            }
        });
    }

    fn state_changed(self: &Rc<Self>) {
        let state = self.context.borrow().as_ref().map(Context::get_state);
        match state {
            Some(State::Ready) => {
                self.retry_ms.set(FIRST_RETRY_MS);
                self.with_ready(|context| {
                    context.subscribe(
                        InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
                        |_| {},
                    );
                });
                self.refresh();
            }
            Some(State::Failed | State::Terminated) => {
                tracing::info!("the sound server went away; the audio module is hidden until it returns");
                if let Some(mut context) = self.context.take() {
                    context.set_state_callback(None);
                    context.set_subscribe_callback(None);
                    context.disconnect();
                }
                self.gathering.replace(None);
                self.stale.set(false);
                self.volumes.borrow_mut().clear();
                self.publish(None);
                self.retry();
            }
            _ => {}
        }
    }

    /// Runs `action` on a ready context, and never on one in any other state.
    fn with_ready(&self, action: impl FnOnce(&mut Context)) {
        let mut context = self.context.borrow_mut();
        if let Some(context) = context.as_mut().filter(|context| context.get_state() == State::Ready) {
            action(context);
        }
    }

    /// Reads the default sink and source, the sinks and the sources; events during a
    /// refresh coalesce into one more. The defaults come through the special names, whose
    /// callbacks turn an error or a timeout into `ListResult::Error` (rule 3).
    fn refresh(self: &Rc<Self>) {
        if self.gathering.borrow().is_some() {
            self.stale.set(true);
            return;
        }
        self.gathering.replace(Some(Gathering {
            remaining: 4,
            ..Gathering::default()
        }));
        let mut started = false;
        self.with_ready(|context| {
            let introspect = context.introspect();
            let weak = Rc::downgrade(self);
            introspect.get_sink_info_by_name("@DEFAULT_SINK@", move |result| match result {
                ListResult::Item(info) => {
                    let output = info.name.as_deref().map(str::to_owned);
                    stage(&weak, |gathering| gathering.snapshot.default_output = output);
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_source_info_by_name("@DEFAULT_SOURCE@", move |result| match result {
                ListResult::Item(info) => {
                    let input = info.name.as_deref().map(str::to_owned);
                    stage(&weak, |gathering| gathering.snapshot.default_input = input);
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_sink_info_list(move |result| match result {
                ListResult::Item(info) => {
                    if let Some(name) = info.name.as_deref() {
                        let device = device(name, info.description.as_deref(), &info.volume, info.mute);
                        let volume = info.volume;
                        stage(&weak, |gathering| {
                            if gathering.snapshot.outputs.len() < audio::MAX_DEVICES {
                                gathering.volumes.insert((Kind::Output, device.name.clone()), volume);
                                gathering.snapshot.outputs.push(device);
                            }
                        });
                    }
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_source_info_list(move |result| match result {
                // A monitor is a sink's loopback, not a microphone.
                ListResult::Item(info) if info.monitor_of_sink.is_none() => {
                    if let Some(name) = info.name.as_deref() {
                        let device = device(name, info.description.as_deref(), &info.volume, info.mute);
                        let volume = info.volume;
                        stage(&weak, |gathering| {
                            if gathering.snapshot.inputs.len() < audio::MAX_DEVICES {
                                gathering.volumes.insert((Kind::Input, device.name.clone()), volume);
                                gathering.snapshot.inputs.push(device);
                            }
                        });
                    }
                }
                ListResult::Item(_) => {}
                ListResult::End | ListResult::Error => finish(&weak),
            });
            started = true;
        });
        if !started {
            self.gathering.replace(None);
        }
    }

    /// One of the four calls ended: the last one publishes, on an idle callback.
    fn finished_one(self: &Rc<Self>) {
        let done = match self.gathering.borrow_mut().as_mut() {
            Some(gathering) => {
                gathering.remaining = gathering.remaining.saturating_sub(1);
                gathering.remaining == 0
            }
            None => false,
        };
        if !done {
            return;
        }
        let Some(gathering) = self.gathering.take() else { return };
        self.volumes.replace(gathering.volumes);
        self.publish(Some(gathering.snapshot));
        if self.stale.replace(false) {
            self.refresh();
        }
    }

    fn publish(&self, snapshot: Option<Snapshot>) {
        let snapshot = snapshot.filter(|snapshot| !snapshot.outputs.is_empty() || !snapshot.inputs.is_empty());
        if *self.snapshot.borrow() == snapshot {
            return;
        }
        self.snapshot.replace(snapshot);
        self.show_all();
    }

    fn show_all(&self) {
        let now = self.media.borrow().as_ref().and_then(|media| media.now());
        for view in self.views() {
            view.show(self.snapshot.borrow().as_ref(), now.as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    fn set_volume(&self, kind: Kind, name: &str, percent: f64) {
        let Some(mut volume) = self.volumes.borrow().get(&(kind, name.to_owned())).copied() else { return };
        // `scale` keeps the balance between channels; an invalid volume would make libpulse
        // return a null operation.
        if !volume.is_valid() || volume.scale(Volume(audio::raw(percent))).is_none() {
            return;
        }
        self.with_ready(|context| {
            let mut introspect = context.introspect();
            match kind {
                Kind::Output => introspect.set_sink_volume_by_name(name, &volume, None),
                Kind::Input => introspect.set_source_volume_by_name(name, &volume, None),
            };
        });
    }

    fn set_mute(&self, kind: Kind, name: &str, muted: bool) {
        self.with_ready(|context| {
            let mut introspect = context.introspect();
            match kind {
                Kind::Output => introspect.set_sink_mute_by_name(name, muted, None),
                Kind::Input => introspect.set_source_mute_by_name(name, muted, None),
            };
        });
    }

    fn set_default(&self, kind: Kind, name: &str) {
        self.with_ready(|context| {
            match kind {
                Kind::Output => context.set_default_sink(name, |_| {}),
                Kind::Input => context.set_default_source(name, |_| {}),
            };
        });
    }

    fn media(&self, method: &'static str) {
        if let Some(media) = self.media.borrow().as_ref() {
            media.command(method);
        }
    }
}

fn device(name: &str, description: Option<&str>, volume: &ChannelVolumes, muted: bool) -> Device {
    Device {
        name: name.to_owned(),
        label: audio::device_label(description, name),
        percent: audio::percent(volume.max().0),
        muted,
    }
}

/// Inside a libpulse callback: writes into the gathering, which no other code borrows then.
fn stage(weak: &Weak<Service>, write: impl FnOnce(&mut Gathering)) {
    if let Some(service) = weak.upgrade() {
        if let Some(gathering) = service.gathering.borrow_mut().as_mut() {
            write(gathering);
        }
    }
}

/// Inside a libpulse callback: the rest runs on the main loop.
fn finish(weak: &Weak<Service>) {
    let weak = weak.clone();
    glib::idle_add_local_once(move || {
        if let Some(service) = weak.upgrade() {
            service.finished_one();
        }
    });
}

/// One direction: a volume slider, a mute switch, and the devices when there are several.
struct Channel {
    kind: Kind,
    section: gtk4::Box,
    scale: gtk4::Scale,
    mute: gtk4::Switch,
    devices: gtk4::Box,
    chosen: RefCell<Option<String>>,
}

impl Channel {
    fn new(kind: Kind, heading: &str, slider: &str, mute: &str) -> Channel {
        let title = gtk4::Label::new(Some(heading));
        title.add_css_class("bar-popover-title");
        title.set_xalign(0.0);
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.update_property(&[Property::Label(slider)]);
        let (mute_row, mute) = switch_row(mute);
        let devices = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let section = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        section.append(&title);
        section.append(&scale);
        section.append(&mute_row);
        section.append(&devices);
        Channel {
            kind,
            section,
            scale,
            mute,
            devices,
            chosen: RefCell::new(None),
        }
    }

    /// Shows the device in use; returns it.
    fn show(&self, devices: &[Device], default: Option<&str>) -> Option<Device> {
        let chosen = audio::chosen(devices, default).and_then(|index| devices.get(index)).cloned();
        self.section.set_visible(chosen.is_some());
        self.chosen.replace(chosen.as_ref().map(|device| device.name.clone()));
        if let Some(device) = &chosen {
            self.scale.set_value(device.percent);
            self.mute.set_active(device.muted);
        }
        while let Some(child) = self.devices.first_child() {
            self.devices.remove(&child);
        }
        if devices.len() > 1 {
            for device in devices {
                let in_use = chosen.as_ref().is_some_and(|chosen| chosen.name == device.name);
                let name = if in_use {
                    tr_with("{device}, in use", "device", &device.label)
                } else {
                    device.label.clone()
                };
                let button = gtk4::Button::with_label(&name);
                button.add_css_class("bar-row");
                button.update_property(&[Property::Label(&name)]);
                let (kind, target) = (self.kind, device.name.clone());
                button.connect_clicked(move |_| {
                    if let Some(service) = service() {
                        service.set_default(kind, &target);
                    }
                });
                self.devices.append(&button);
            }
        }
        chosen
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    output: Channel,
    input: Channel,
    media: gtk4::Box,
    title: gtk4::Label,
    artist: gtk4::Label,
    previous: gtk4::Button,
    play: gtk4::Button,
    next: gtk4::Button,
    /// Widgets are being set from the sound server, not by the person.
    updating: Cell<bool>,
    pending_open: Cell<bool>,
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("audio-volume-medium-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Sound"));
        popup.button.set_visible(false);
        let output = Channel::new(Kind::Output, &tr("Output"), &tr("Output volume"), &tr("Mute output"));
        let input = Channel::new(Kind::Input, &tr("Input"), &tr("Input volume"), &tr("Mute microphone"));

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let artist = gtk4::Label::new(None);
        artist.add_css_class("bar-popover-note");
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let previous = media_button("media-skip-backward-symbolic", &tr("Previous track"));
        let play = media_button("media-playback-pause-symbolic", &tr("Pause"));
        let next = media_button("media-skip-forward-symbolic", &tr("Next track"));
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        controls.set_halign(gtk4::Align::Center);
        controls.append(&previous);
        controls.append(&play);
        controls.append(&next);
        let media = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        media.append(&title);
        media.append(&artist);
        media.append(&controls);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        content.append(&output.section);
        content.append(&input.section);
        content.append(&media);
        popup.popover.set_child(Some(&content));

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            output,
            input,
            media,
            title,
            artist,
            previous,
            play,
            next,
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers();
        view
    }

    fn channel(&self, kind: Kind) -> &Channel {
        match kind {
            Kind::Output => &self.output,
            Kind::Input => &self.input,
        }
    }

    fn connect_handlers(self: &Rc<Self>) {
        for kind in [Kind::Output, Kind::Input] {
            let weak = Rc::downgrade(self);
            self.channel(kind).scale.connect_value_changed(move |scale| {
                let Some(view) = weak.upgrade() else { return };
                let chosen = view.channel(kind).chosen.borrow().clone();
                if let (false, Some(name), Some(service)) = (view.updating.get(), chosen, service()) {
                    service.set_volume(kind, &name, scale.value());
                }
            });
            let weak = Rc::downgrade(self);
            self.channel(kind).mute.connect_state_set(move |_, muted| {
                if let Some(view) = weak.upgrade() {
                    let chosen = view.channel(kind).chosen.borrow().clone();
                    if let (false, Some(name), Some(service)) = (view.updating.get(), chosen, service()) {
                        service.set_mute(kind, &name, muted);
                    }
                }
                glib::Propagation::Proceed
            });
        }
        for (button, method) in [(&self.previous, "Previous"), (&self.play, "PlayPause"), (&self.next, "Next")] {
            button.connect_clicked(move |_| {
                if let Some(service) = service() {
                    service.media(method);
                }
            });
        }
    }

    fn show(self: &Rc<Self>, snapshot: Option<&Snapshot>, now: Option<&NowPlaying>) {
        let Some(snapshot) = snapshot else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.updating.set(true);
        let output = self.output.show(&snapshot.outputs, snapshot.default_output.as_deref());
        let input = self.input.show(&snapshot.inputs, snapshot.default_input.as_deref());
        self.updating.set(false);
        let icon = match (&output, &input) {
            (Some(output), _) => audio::output_icon(output.percent, output.muted),
            (None, Some(input)) => audio::input_icon(input.muted),
            (None, None) => "audio-volume-muted-symbolic",
        };
        self.icon.set_icon_name(Some(icon));
        self.media.set_visible(now.is_some());
        if let Some(now) = now {
            self.title.set_text(&now.track.title);
            self.artist.set_text(now.track.artist.as_deref().unwrap_or(""));
            self.artist.set_visible(now.track.artist.is_some());
            let (icon, name) = if now.playing {
                ("media-playback-pause-symbolic", tr("Pause"))
            } else {
                ("media-playback-start-symbolic", tr("Play"))
            };
            self.play.set_icon_name(icon);
            self.play.set_tooltip_text(Some(&name));
            self.play.update_property(&[Property::Label(&name)]);
            self.previous.set_sensitive(now.can_previous);
            self.next.set_sensitive(now.can_next);
        }
        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }
}

fn media_button(icon: &str, name: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-row");
    button.set_tooltip_text(Some(name));
    button.update_property(&[Property::Label(name)]);
    button
}

struct AudioUi {
    view: Rc<View>,
}

impl ModuleUi for AudioUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    let now = service.media.borrow().as_ref().and_then(|media| media.now());
    view.show(service.snapshot.borrow().as_ref(), now.as_ref());
    Some(Box::new(AudioUi { view }))
}
```

Notes for the implementer:
- The `match` arms in `set_volume`, `set_mute` and `set_default` return an `Operation` that is dropped at once. The operation goes on in libpulse; dropping it only releases the bar's reference. `Operation` carries no `#[must_use]` in libpulse-binding 2.30.1, so there is no warning to silence.
- If clippy flags `let_underscore_future` or `drop_non_drop` on these lines, bind each operation to `_operation` inside its arm. Do not add `allow`.
- `ChannelVolumes::max` is the loudest channel, so the slider follows it. `scale` keeps the balance between channels.

- [ ] **Step 3: Declare the modules and build them**

In `src/ui/mod.rs`, add `mod audio;` after `mod accessibility;`, and `mod mpris;` before `mod network;`. Then add the arm:

```rust
        Module::Audio => audio::new(bar),
```

Run: `ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar`

Expected:
- PASS;
- `check_shim_link_order.py` still passes: `libpulse-mainloop-glib.so.0` and `libpulse.so.0` join the DT_NEEDED list and do not move the layer-shell shim.

- [ ] **Step 4: Add the `audio` section to `bar_modules_e2e.py`**

```python
def audio(ctx):
    app, Atspi = ctx.app, ctx.Atspi
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def slider(name):
        found = labelled(app, Atspi, "slider", name)
        return found[0] if found else None

    def near(name, percent):
        found = slider(name)
        return found is not None and abs(found.get_current_value() - percent) < 1.0

    def switch(name):
        found = labelled(app, Atspi, "check box", name)
        return found[0] if found else None

    def default_sink():
        return fx.pactl("get-default-sink").strip()

    check("the audio module shows", wait_for(lambda: buttons(app, Atspi, "Sound"), 15))
    check("the popover shows the output volume", open_popover(app, Atspi, "Sound", lambda: slider("Output volume")))
    check("the output slider shows the default sink's 40 %", wait_for(lambda: near("Output volume", 40), 5))
    check("the input slider shows the microphone's 55 %", near("Input volume", 55))
    check("the device in use is marked", bool(buttons(app, Atspi, "Speakers, in use")))

    press(app, Atspi, "Headphones")
    check("choosing Headphones makes it the default sink", wait_for(lambda: default_sink() == "headphones", 5))
    check("and the slider follows it to 70 %", wait_for(lambda: near("Output volume", 70), 5))

    switch("Mute output").do_action(0)
    check("the mute switch mutes the sink", wait_for(lambda: "yes" in fx.pactl("get-sink-mute", "headphones"), 5))
    wait_for(lambda: switch("Mute output").get_state_set().contains(Atspi.StateType.CHECKED), 5)
    switch("Mute output").do_action(0)
    check("and unmutes it", wait_for(lambda: "no" in fx.pactl("get-sink-mute", "headphones"), 5))

    slider("Output volume").set_current_value(25.0)
    check("the slider sets the volume", wait_for(lambda: "25%" in fx.pactl("get-sink-volume", "headphones"), 5))
    fx.pactl("set-sink-volume", "headphones", "60%")
    check("a change made elsewhere reaches the slider", wait_for(lambda: near("Output volume", 60), 5))

    # The media controls follow the MPRIS player.
    check("the playing track is shown", wait_for(lambda: labelled(app, Atspi, "label", "Night Drive"), 10))
    check("with its artist", bool(labelled(app, Atspi, "label", "Calmo")))
    press(app, Atspi, "Pause")
    check(
        "Pause reaches the player",
        wait_for(lambda: len(mock_calls(session, fx.PLAYER, fx.MPRIS_PATH, "PlayPause")) == 1, 5),
    )
    check("and the button becomes Play", wait_for(lambda: buttons(app, Atspi, "Play"), 5))

    # The sound server restarts: the module hides, then comes back on its own.
    for pid in fx.pids_of("pipewire-pulse"):
        os.kill(pid, signal.SIGTERM)
    check("without a sound server the module hides", wait_for(lambda: not buttons(app, Atspi, "Sound"), 10))
    ctx.spawned.append(fx.pipewire_pulse(subprocess.DEVNULL))
    check("the sound server back: the module shows within 15 s", wait_for(lambda: buttons(app, Atspi, "Sound"), 15))
    check("the bar survived the restart", alive(ctx.pid))


SECTIONS.append(audio)
```

`mock_calls` takes any connection, so the session bus works as well as the system bus. The player's `PlayPause` is logged in the mock's memory like every other method.

- [ ] **Step 5: Run it**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e
```

Expected: the network, Bluetooth and audio checks PASS.

Two failures to recognise:
- "the slider sets the volume" fails while every other check passes. AT-SPI's `CurrentValue` did not reach the `GtkScale`. Check with `busctl --user` that the property is writable on the slider. If GTK refuses the write, replace this one check with a keyboard step on the focused slider (`Page_Down` through the rig's `wtype`) and record it in the handback. Do not remove the check.
- "the sound server back" fails. Read `bar-modules-e2e-bar.log` for "no sound server yet" lines: there should be one per retry, at most 8 s apart.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/audio.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mpris.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/test/shell/bar_modules_e2e.py
git commit -m "feat(bar): the audio module over libpulse, with the media controls"
```

---
### Task 7: The battery module, with the power profile and the screen brightness

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/battery.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (one `mod` line, one `build` arm)
- Modify: `forge/test/shell/bar_modules_e2e.py` (the `battery` section)
- Modify: `forge/test/shell/rig.sh` (an `atspi bar-modules` case)
- Modify: `.github/workflows/shell-surfaces.yml` (one step in job `bar`)

**Interfaces:**
- Consumes:
  - `athanor_bar::battery::{self, Backlight, Battery, Charge}` and `athanor_bar::props` (Task 2);
  - `bus::{call, set_property, spawn, Mirror, Source, TIMEOUT_MS}` and `Bar::fit_groups` (Task 4);
  - the fake logind's `Session.SetBrightness` and `ATHANOR_BAR_BACKLIGHT_DIR` from `bar_session.py --fixtures` (Task 3).
  - In the e2e: `labelled`, `buttons`, `property_of` and `open_popover` (Tasks 3 and 4).
- Produces:
  - `Module::Battery` built;
  - the `battery` section;
  - `rig.sh atspi bar-modules`, which expects at least 11 named interactive widgets.

The module talks to three services and to none of COSMIC's (BR3):
- **UPower.** It reads `DisplayDevice` through a `Fixed` mirror. Without a present battery, the module hides (SH1).
- **The power-profiles interface.** `org.freedesktop.UPower.PowerProfiles` is served by tuned-ppd on Fedora 43. The module reads it through a second `Fixed` mirror and sets `ActiveProfile`. The daemon asks polkit (`power-profiles-daemon.switch-profile` or tuned-ppd's equivalent), and an active local session is allowed by default.
- **logind's `Session.SetBrightness`.** It needs no polkit for the session's own seat. The bar only reads `/sys/class/backlight` (Landlock restricts writes, not reads), and logind writes the level.

- [ ] **Step 1: Write `src/ui/battery.rs`**

```rust
//! The battery module (doc_bar.md, BR3): the charge and the time left from UPower's display
//! device, the power profile from the power-profiles interface (tuned-ppd on Fedora), and
//! the screen brightness through logind's `SetBrightness`. Nothing goes through COSMIC's
//! settings daemon. Without a present battery the module hides (SH1).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use athanor_bar::battery::{self, Backlight, Battery, Charge};
use athanor_bar::props;
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::bus::{self, Mirror, Source};
use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const LOGIN1: &str = "org.freedesktop.login1";
/// logind resolves `auto` to the caller's session.
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION: &str = "org.freedesktop.login1.Session";

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

/// What one view shows.
struct Status {
    battery: Option<Battery>,
    profiles: Option<(Vec<&'static str>, String)>,
    backlight: Option<Backlight>,
}

struct Service {
    bar: Weak<Bar>,
    connection: RefCell<Option<gio::DBusConnection>>,
    upower: RefCell<Option<Rc<Mirror>>>,
    profiles: RefCell<Option<Rc<Mirror>>>,
    backlight_root: PathBuf,
    views: RefCell<Vec<Weak<View>>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        // The rig points the bar at a fake sysfs directory; the level still goes through
        // logind, which checks the device itself.
        let backlight_root = std::env::var_os("ATHANOR_BAR_BACKLIGHT_DIR")
            .map_or_else(|| PathBuf::from(battery::BACKLIGHT_ROOT), PathBuf::from);
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            connection: RefCell::new(None),
            upower: RefCell::new(None),
            profiles: RefCell::new(None),
            backlight_root,
            views: RefCell::new(Vec::new()),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        let weak = Rc::downgrade(&service);
        glib::spawn_future_local(async move {
            let connection = match gio::bus_get_future(gio::BusType::System).await {
                Ok(connection) => connection,
                Err(err) => {
                    tracing::warn!(error = %err, "no system bus; the battery module is hidden");
                    return;
                }
            };
            let Some(service) = weak.upgrade() else { return };
            // The mirrors hold no strong reference to the service: they find it in the
            // thread-local, as the other modules do.
            let notify = || {
                || {
                    if let Some(service) = service() {
                        service.show_all();
                    }
                }
            };
            service.upower.replace(Some(Mirror::new(
                &connection,
                battery::UPOWER,
                Source::Fixed(vec![(battery::DISPLAY_DEVICE, battery::DEVICE_IFACE)]),
                notify(),
            )));
            service.profiles.replace(Some(Mirror::new(
                &connection,
                battery::PROFILES,
                Source::Fixed(vec![(battery::PROFILES_PATH, battery::PROFILES)]),
                notify(),
            )));
            service.connection.replace(Some(connection));
        });
        service
    }

    fn status(&self) -> Status {
        let battery = self.upower.borrow().as_ref().and_then(|mirror| {
            let objects = mirror.objects();
            props::lookup(&objects, battery::DISPLAY_DEVICE, battery::DEVICE_IFACE).and_then(battery::battery)
        });
        let profiles = self.profiles.borrow().as_ref().and_then(|mirror| {
            let objects = mirror.objects();
            props::lookup(&objects, battery::PROFILES_PATH, battery::PROFILES).and_then(battery::profiles)
        });
        Status {
            battery,
            profiles,
            backlight: battery::read_backlight(&self.backlight_root),
        }
    }

    fn show_all(&self) {
        let status = self.status();
        for view in self.views() {
            view.show(&status);
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    fn set_profile(&self, profile: &'static str) {
        let Some(connection) = self.connection.borrow().clone() else { return };
        bus::spawn(
            "set the power profile",
            bus::set_property(
                &connection,
                battery::PROFILES,
                battery::PROFILES_PATH,
                battery::PROFILES,
                "ActiveProfile",
                profile.to_variant(),
            ),
        );
    }

    fn set_brightness(&self, percent: f64) {
        let Some(connection) = self.connection.borrow().clone() else { return };
        // Read again: the level the slider scales is the device's, which may have changed
        // through the brightness keys since the popover opened.
        let Some(backlight) = battery::read_backlight(&self.backlight_root) else { return };
        let args = ("backlight", backlight.name.as_str(), backlight.raw(percent)).to_variant();
        bus::spawn(
            "set the screen brightness",
            bus::call(&connection, LOGIN1, SESSION_PATH, SESSION, "SetBrightness", Some(&args), bus::TIMEOUT_MS),
        );
    }
}

fn percent_text(percent: f64) -> String {
    // At most 100 after `battery::battery`, so the cast cannot truncate.
    tr_with("{percent} %", "percent", &(percent.round() as u32).to_string())
}

/// A translated `text` with `{hours}` and `{minutes}` filled in. The callers pass `tr(...)`
/// with the literal message id, so xgettext finds it.
fn duration(text: &str, seconds: u64) -> String {
    let (hours, minutes) = battery::hours_minutes(seconds);
    text.replace("{hours}", &hours.to_string()).replace("{minutes}", &minutes.to_string())
}

/// The line under the charge: the time left, the time until full, or the state.
fn note(battery: &Battery) -> Option<String> {
    match (battery.charge, battery.seconds) {
        (Charge::Discharging, Some(seconds)) => Some(duration(&tr("{hours} h {minutes} min left"), seconds)),
        (Charge::Charging, Some(seconds)) => Some(duration(&tr("{hours} h {minutes} min until full"), seconds)),
        (Charge::Charging, None) => Some(tr("Charging")),
        (Charge::Full, _) => Some(tr("Fully charged")),
        (Charge::Discharging | Charge::Unknown, _) => None,
    }
}

fn profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => tr("Power saver"),
        "performance" => tr("Performance"),
        _ => tr("Balanced"),
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    level: gtk4::Label,
    charge: gtk4::Label,
    note: gtk4::Label,
    profiles_section: gtk4::Box,
    profiles: Vec<(&'static str, gtk4::CheckButton)>,
    brightness_section: gtk4::Box,
    brightness: gtk4::Scale,
    /// Widgets are being set from the services, not by the person.
    updating: Cell<bool>,
    pending_open: Cell<bool>,
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("battery-good-symbolic");
        let level = gtk4::Label::new(None);
        let face = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        face.append(&icon);
        face.append(&level);
        let popup = Popup::new(bar, &face, &tr("Battery"));
        popup.button.set_visible(false);

        let charge = gtk4::Label::new(None);
        charge.add_css_class("bar-popover-title");
        charge.set_xalign(0.0);
        let note = gtk4::Label::new(None);
        note.add_css_class("bar-popover-note");
        note.set_xalign(0.0);

        let profiles_title = gtk4::Label::new(Some(&tr("Power mode")));
        profiles_title.add_css_class("bar-popover-title");
        profiles_title.set_xalign(0.0);
        let profiles_section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        profiles_section.append(&profiles_title);
        let mut profiles: Vec<(&'static str, gtk4::CheckButton)> = Vec::new();
        for name in battery::PROFILE_NAMES {
            let label = profile_label(name);
            let button = gtk4::CheckButton::with_label(&label);
            button.update_property(&[Property::Label(&label)]);
            if let Some((_, first)) = profiles.first() {
                button.set_group(Some(first));
            }
            profiles_section.append(&button);
            profiles.push((name, button));
        }

        let brightness_title = gtk4::Label::new(Some(&tr("Screen brightness")));
        brightness_title.add_css_class("bar-popover-title");
        brightness_title.set_xalign(0.0);
        let brightness = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 1.0, 100.0, 1.0);
        brightness.set_draw_value(false);
        brightness.set_hexpand(true);
        brightness.update_relation(&[Relation::LabelledBy(&[brightness_title.upcast_ref()])]);
        let brightness_section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        brightness_section.append(&brightness_title);
        brightness_section.append(&brightness);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        let summary = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        summary.append(&charge);
        summary.append(&note);
        content.append(&summary);
        content.append(&profiles_section);
        content.append(&brightness_section);
        popup.popover.set_child(Some(&content));

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            level,
            charge,
            note,
            profiles_section,
            profiles,
            brightness_section,
            brightness,
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers();
        view
    }

    fn connect_handlers(self: &Rc<Self>) {
        for (name, button) in &self.profiles {
            let (weak, name) = (Rc::downgrade(self), *name);
            button.connect_toggled(move |button| {
                let Some(view) = weak.upgrade() else { return };
                if button.is_active() && !view.updating.get() {
                    if let Some(service) = service() {
                        service.set_profile(name);
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.brightness.connect_value_changed(move |scale| {
            let Some(view) = weak.upgrade() else { return };
            if let (false, Some(service)) = (view.updating.get(), service()) {
                service.set_brightness(scale.value());
            }
        });
        // The backlight is read again on every opening: the keys change it too.
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let (Some(view), Some(service)) = (weak.upgrade(), service()) {
                view.show(&service.status());
            }
        });
    }

    fn show(self: &Rc<Self>, status: &Status) {
        let Some(battery) = status.battery else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.updating.set(true);
        let charge = percent_text(battery.percent);
        let note = note(&battery);
        self.icon.set_icon_name(Some(battery::icon(&battery)));
        self.level.set_text(&charge);
        self.charge.set_text(&charge);
        self.note.set_text(note.as_deref().unwrap_or(""));
        self.note.set_visible(note.is_some());
        let description = match &note {
            Some(note) => format!("{charge}, {note}"),
            None => charge.clone(),
        };
        self.popup.button.set_tooltip_text(Some(&description));
        self.popup.button.update_property(&[Property::Description(&description)]);

        self.profiles_section.set_visible(status.profiles.is_some());
        if let Some((offered, active)) = &status.profiles {
            for (name, button) in &self.profiles {
                button.set_visible(offered.contains(name));
                if *name == active.as_str() {
                    button.set_active(true);
                }
            }
        }
        self.brightness_section.set_visible(status.backlight.is_some());
        if let Some(backlight) = &status.backlight {
            self.brightness.set_value(backlight.percent());
        }
        self.updating.set(false);

        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }
}

struct BatteryUi {
    view: Rc<View>,
}

impl ModuleUi for BatteryUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    view.show(&service.status());
    Some(Box::new(BatteryUi { view }))
}
```

Note for the implementer: the slider starts at 1, not 0, because `Backlight::raw` never returns 0: a slider at its left end must not turn the panel black.

- [ ] **Step 2: Declare the module and build it**

In `src/ui/mod.rs`, add `mod battery;` after `mod audio;`, and the arm:

```rust
        Module::Battery => battery::new(bar),
```

Run: `ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar`

Expected: PASS.

- [ ] **Step 3: Add the `battery` section to `bar_modules_e2e.py`**

```python
def battery(ctx):
    app, Atspi, bus = ctx.app, ctx.Atspi, ctx.bus
    logind_log = Path("/out") / f"{os.environ['RIG_TAG']}-logind.log"
    profiles_path = "/org/freedesktop/UPower/PowerProfiles"

    def radio(name):
        found = labelled(app, Atspi, "radio button", name)
        return found[0] if found else None

    def checked(name):
        found = radio(name)
        return found is not None and found.get_state_set().contains(Atspi.StateType.CHECKED)

    def brightness():
        found = labelled(app, Atspi, "slider", "Screen brightness")
        return found[0] if found else None

    def display_device(state, percent, to_empty, to_full, present):
        fx.call(
            bus, fx.UPOWER, "/org/freedesktop/UPower", fx.MOCK, "SetupDisplayDevice", "(uuddddxxbsu)",
            (2, state, percent, percent / 2, 50.0, 10.0, to_empty, to_full, present, "", 1),
        )

    check("the battery module shows", wait_for(lambda: buttons(app, Atspi, "Battery"), 10))
    module = buttons(app, Atspi, "Battery")
    check(
        "the module describes the charge and the time left",
        bool(module) and module[0].get_description() == "72 %, 3 h 25 min left",
        module[0].get_description() if module else "",
    )
    check(
        "the popover shows the charge",
        open_popover(app, Atspi, "Battery", lambda: labelled(app, Atspi, "label", "3 h 25 min left")),
    )
    check("the active profile is Balanced", checked("Balanced"))
    radio("Performance").do_action(0)
    check(
        "choosing Performance reaches the profiles daemon",
        wait_for(lambda: property_of(bus, fx.PROFILES, profiles_path, fx.PROFILES, "ActiveProfile") == "performance", 5),
    )
    fx.call(
        bus, fx.PROFILES, profiles_path, "org.freedesktop.DBus.Properties", "Set", "(ssv)",
        (fx.PROFILES, "ActiveProfile", GLib.Variant("s", "power-saver")),
    )
    check("a profile chosen elsewhere is shown", wait_for(lambda: checked("Power saver"), 5))

    check(
        "the brightness slider shows the backlight's 60 %",
        brightness() is not None and abs(brightness().get_current_value() - 60.0) < 1.0,
    )
    brightness().set_current_value(30.0)
    check(
        "the slider sets the brightness through logind",
        wait_for(lambda: "SetBrightness backlight intel_backlight 300" in logind_log.read_text(encoding="utf-8"), 5),
    )

    display_device(1, 15.0, 0, 5400, True)
    check("charging shows the time until full", wait_for(lambda: labelled(app, Atspi, "label", "1 h 30 min until full"), 5))
    check("and the new charge", bool(labelled(app, Atspi, "label", "15 %")))
    display_device(2, 72.0, 12300, 0, False)
    check("without a present battery the module hides", wait_for(lambda: not buttons(app, Atspi, "Battery"), 5))
    display_device(2, 72.0, 12300, 0, True)
    check("the battery back, the module shows again", wait_for(lambda: buttons(app, Atspi, "Battery"), 5))


SECTIONS.append(battery)
```

- [ ] **Step 4: Add `rig.sh atspi bar-modules` and its CI step**

In `forge/test/shell/rig.sh`, in the `atspi)` case, after the `bar)` branch (its `;;`), add:

```bash
    bar-modules)
        # 11 interactive widgets under float with every fixture: the 7 of `bar`, and audio,
        # Bluetooth, network and battery. The fixtures start first, so the settle is longer.
        seed_bar "$out/seed-atspi-bar-modules" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=12 RIG_CONFIG_SEED=/out/seed-atspi-bar-modules \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-bar 11" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-bar-modules -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py --fixtures"
        ;;
```

Replace `<greeter|chooser|bar>` with `<greeter|chooser|bar|bar-modules>` in the header line of `rig.sh atspi`.

In `.github/workflows/shell-surfaces.yml`, job `bar`, after the step `Accessibility tree of the bar`, add:

```yaml
      - name: Accessibility tree of the bar with every system module
        run: bash forge/test/shell/rig.sh atspi bar-modules
```

Run: `python3 scripts/verify.py workflows`

Expected: PASS.

- [ ] **Step 5: Run everything the bar has**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh atspi bar-modules
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh atspi bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-e2e
```

Expected:
- every check PASSes;
- `bar-modules-e2e` ends with the PSS line within 64 MB (item 17) and "no typed password in any file the bar can write or the rig keeps";
- `atspi bar-modules` lists 11 or more named interactive widgets;
- `atspi bar` still passes with 7: without the fixtures, the four modules hide.

If the PSS check fails, stop and report the figure in the handback. Do not raise `PSS_LIMIT_KB`.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/battery.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs \
    forge/test/shell/bar_modules_e2e.py forge/test/shell/rig.sh .github/workflows/shell-surfaces.yml
git commit -m "feat(bar): the battery module, with the power profile and the brightness through logind"
```

---
### Task 8: Four surface scenes, 48 cases, and the translations

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in`
- Modify (regenerated by `po/update.sh`): `po/athanor-bar.pot`, `po/en.po`, `po/it.po`
- Modify: `forge/test/shell/locale/bar-de.po`
- Modify: `forge/test/shell/rig.sh` (`capture_bar`, the `surface` dispatch, one header line)
- Modify: `forge/test/shell/cases.py` (four scenes)
- Modify: `forge/test/shell/tests/test_cases.py` (four scenes, a count that follows the list)
- Modify: `.github/workflows/shell-surfaces.yml` (four matrix entries)
- Create: `forge/test/shell/golden/bar-{network,bluetooth,audio,battery}/*.png` (48 files)

**Interfaces:**
- Consumes:
  - every `tr`/`tr_with` string of Tasks 4 to 7;
  - `bar_session.py --fixtures` (Task 3);
  - `ATHANOR_BAR_OPEN=<module id>`, which reaches `ModuleUi::open`. Each module opens as soon as its data arrives, through `pending_open`.
- Produces:
  - the scenes `bar-network`, `bar-bluetooth`, `bar-audio` and `bar-battery`, of 12 cases each (light and dark, scales 1.0 and 1.5, en, de and pseudo-RTL);
  - 156 bar cases in all with the six scenes of 2b.2 and the three of 2b.3 (item 18 counts 180 once 2b.5 adds its two).

- [ ] **Step 1: List the new sources for xgettext**

```bash
python3 - <<'PY'
from pathlib import Path

path = Path("forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in")
lines = path.read_text(encoding="utf-8").splitlines()
# The bar's own sources, sorted, then athanor-apps' sources as Unit A lists them.
own = [line for line in lines if line.startswith("src/")]
shared = [line for line in lines if not line.startswith("src/")]
new = ["src/ui/audio.rs", "src/ui/battery.rs", "src/ui/bluetooth.rs", "src/ui/network.rs"]
path.write_text("\n".join(sorted(set(own) | set(new)) + shared) + "\n", encoding="utf-8")
PY
git diff --stat forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in
tail -n 2 forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in
```

Expected:
- `1 file changed, 4 insertions(+)`, and no deletions;
- the last two lines are `../../../../system/athanor-apps/src/openers.rs` and `../../../../system/athanor-apps/src/row.rs`.

The bar's own lines were sorted already. If the diff shows deletions, `git checkout` the file and insert the four lines by hand in their sorted places.

- [ ] **Step 2: Regenerate the template and merge it into the catalogs**

```bash
podman run --rm --security-opt label=disable -v "$(git rev-parse --show-toplevel):/repo" -w /repo \
    localhost/athanor-shell-rig:build bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh
grep -c '^msgid' forge/specs/athanor-bar/athanor-bar-1.0.0/po/athanor-bar.pot
```

Expected: the count grows by 43. "Cancel" is already in the template, so the new message ids are 43:
- 20 from `network.rs` and `bluetooth.rs`;
- 12 from `audio.rs`;
- 11 from `battery.rs`.

If the number differs, compare with the table in Step 3: that table is the list of new message ids.

- [ ] **Step 3: Fill `it.po` and `en.po`**

```bash
python3 - <<'PY'
import re
from pathlib import Path

ITALIAN = {
    "Network": "Rete",
    "Wi-Fi": "Wi-Fi",
    "Airplane mode": "Modalità aereo",
    "Wired: connected": "Cablata: connessa",
    "Wired: not connected": "Cablata: non connessa",
    "{network}, connected": "{network}, connessa",
    "{network}, connecting": "{network}, connessione in corso",
    "{network}, needs Settings": "{network}, richiede Impostazioni",
    "{network}, secured": "{network}, protetta",
    "Password for {network}": "Password per {network}",
    "The password was not accepted. Try again.": "La password non è stata accettata. Riprova.",
    "Connect": "Connetti",
    "Bluetooth": "Bluetooth",
    "Nearby devices": "Dispositivi vicini",
    "{device}, connected": "{device}, connesso",
    "Pair with {device}?": "Associare {device}?",
    "Pair only if {device} shows the same number.": "Associa solo se {device} mostra lo stesso numero.",
    "Pairing with {device}": "Associazione con {device}",
    "Type this code on {device}, then press Enter there.": "Digita questo codice su {device}, poi premi Invio.",
    "Pair": "Associa",
    "Sound": "Audio",
    "Output": "Uscita",
    "Input": "Ingresso",
    "Output volume": "Volume di uscita",
    "Input volume": "Volume di ingresso",
    "Mute output": "Silenzia l'uscita",
    "Mute microphone": "Silenzia il microfono",
    "{device}, in use": "{device}, in uso",
    "Previous track": "Brano precedente",
    "Next track": "Brano successivo",
    "Play": "Riproduci",
    "Pause": "Pausa",
    "Battery": "Batteria",
    "{percent} %": "{percent}%",
    "{hours} h {minutes} min left": "{hours} h {minutes} min rimanenti",
    "{hours} h {minutes} min until full": "{hours} h {minutes} min alla carica completa",
    "Charging": "In carica",
    "Fully charged": "Carica completa",
    "Power saver": "Risparmio energetico",
    "Balanced": "Bilanciato",
    "Performance": "Prestazioni",
    "Power mode": "Modalità di alimentazione",
    "Screen brightness": "Luminosità dello schermo",
}


def fill(path, translate):
    text = path.read_text(encoding="utf-8")
    missing = []

    def entry(match):
        msgid = match.group(1)
        if msgid not in ITALIAN:
            return match.group(0)
        return f'msgid "{msgid}"\nmsgstr "{translate(msgid)}"'

    text = re.sub(r'^msgid "(.+)"\nmsgstr ""$', entry, text, flags=re.M)
    for msgid in ITALIAN:
        if f'msgid "{msgid}"' not in text:
            missing.append(msgid)
    if missing:
        raise SystemExit(f"{path}: not in the template: {missing}")
    path.write_text(text, encoding="utf-8")


po = Path("forge/specs/athanor-bar/athanor-bar-1.0.0/po")
fill(po / "it.po", ITALIAN.__getitem__)
fill(po / "en.po", lambda msgid: msgid)
PY
msgfmt --check --statistics -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po
msgfmt --check --statistics -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po
```

Expected: both report every message translated, with no fuzzy and no untranslated messages. `en.po` is ASCII: every English message above is ASCII, and the script writes the message id back.

- [ ] **Step 4: German for the scenes' test catalog**

```bash
cat >> forge/test/shell/locale/bar-de.po <<'EOF'

msgid "Network"
msgstr "Netzwerk"

msgid "Wi-Fi"
msgstr "WLAN"

msgid "Airplane mode"
msgstr "Flugmodus"

msgid "Wired: connected"
msgstr "Kabel: verbunden"

msgid "Wired: not connected"
msgstr "Kabel: nicht verbunden"

msgid "{network}, connected"
msgstr "{network}, verbunden"

msgid "{network}, connecting"
msgstr "{network}, wird verbunden"

msgid "{network}, needs Settings"
msgstr "{network}, erfordert Einstellungen"

msgid "{network}, secured"
msgstr "{network}, gesichert"

msgid "Password for {network}"
msgstr "Passwort für {network}"

msgid "The password was not accepted. Try again."
msgstr "Das Passwort wurde nicht akzeptiert. Bitte erneut versuchen."

msgid "Connect"
msgstr "Verbinden"

msgid "Bluetooth"
msgstr "Bluetooth"

msgid "Nearby devices"
msgstr "Geräte in der Nähe"

msgid "{device}, connected"
msgstr "{device}, verbunden"

msgid "Pair with {device}?"
msgstr "Mit {device} koppeln?"

msgid "Pair only if {device} shows the same number."
msgstr "Nur koppeln, wenn {device} dieselbe Zahl zeigt."

msgid "Pairing with {device}"
msgstr "Kopplung mit {device}"

msgid "Type this code on {device}, then press Enter there."
msgstr "Diesen Code auf {device} eingeben und dort die Eingabetaste drücken."

msgid "Pair"
msgstr "Koppeln"

msgid "Sound"
msgstr "Ton"

msgid "Output"
msgstr "Ausgabe"

msgid "Input"
msgstr "Eingabe"

msgid "Output volume"
msgstr "Ausgabelautstärke"

msgid "Input volume"
msgstr "Eingabelautstärke"

msgid "Mute output"
msgstr "Ausgabe stummschalten"

msgid "Mute microphone"
msgstr "Mikrofon stummschalten"

msgid "{device}, in use"
msgstr "{device}, in Verwendung"

msgid "Previous track"
msgstr "Vorheriger Titel"

msgid "Next track"
msgstr "Nächster Titel"

msgid "Play"
msgstr "Wiedergabe"

msgid "Pause"
msgstr "Pause"

msgid "Battery"
msgstr "Akku"

msgid "{percent} %"
msgstr "{percent} %"

msgid "{hours} h {minutes} min left"
msgstr "Noch {hours} h {minutes} min"

msgid "{hours} h {minutes} min until full"
msgstr "{hours} h {minutes} min bis voll"

msgid "Charging"
msgstr "Wird geladen"

msgid "Fully charged"
msgstr "Vollständig geladen"

msgid "Power saver"
msgstr "Energiesparen"

msgid "Balanced"
msgstr "Ausgewogen"

msgid "Performance"
msgstr "Leistung"

msgid "Power mode"
msgstr "Energiemodus"

msgid "Screen brightness"
msgstr "Bildschirmhelligkeit"
EOF
msgfmt --check -o /dev/null forge/test/shell/locale/bar-de.po
```

Expected: `msgfmt` exits 0, with no duplicate message definitions.

- [ ] **Step 5: The four scenes in `rig.sh`, `cases.py`, the unit test and CI**

In `forge/test/shell/rig.sh`:

1. The `surface` header line gains `|bar-network|bar-bluetooth|bar-audio|bar-battery` after `bar-tray`.
2. In `capture_bar`, the first `local` line becomes:

```bash
    local surface=$1 open="" preset=float panel=top dock=visible settle=8
```

3. Its `case` gains, after the `bar-tray)` arm (its `;;`):

```bash
    # The system modules against the fixtures, which start before the bar: a longer settle.
    bar-network) open=network settle=12 session+=(--fixtures) ;;
    bar-bluetooth) open=bluetooth settle=12 session+=(--fixtures) ;;
    bar-audio) open=audio settle=12 session+=(--fixtures) ;;
    bar-battery) open=battery settle=12 session+=(--fixtures) ;;
```

4. In the `in_rig` line of the loop, `RIG_SETTLE=8` becomes `RIG_SETTLE="$settle"`.
5. The comment above `capture_bar` becomes:

```bash
# doc_bar.md, BR9: the bar under its own preset with one running window, the five
# popovers the bar owns in 2b.2, opened by ATHANOR_BAR_OPEN over the float preset,
# 2b.3's notification popups (four waiting notifications: three show), the notification
# list and a tray menu, and the popovers of the four system modules against
# system_fixtures.py.
```

6. The `surface` dispatch line becomes:

```bash
    bar | bar-power | bar-input | bar-calendar | bar-accessibility | bar-tiling | bar-popups | bar-notifications | bar-tray | bar-network | bar-bluetooth | bar-audio | bar-battery) capture_bar "$surface" ;;
```

In `forge/test/shell/cases.py`, the bar tuple gains the four scenes after `"bar-tray",`, and its comment becomes:

```python
    # doc_bar.md, BR9: the bar, and the popovers of power, input source, calendar,
    # accessibility and tiling; the notification popups, the notification list and a
    # tray menu (2b.3); the popovers of network, Bluetooth, audio and battery (2b.4). The
    # other two scenes come with 2b.5.
```

The new entries are:

```python
            "bar-network",
            "bar-bluetooth",
            "bar-audio",
            "bar-battery",
```

In `forge/test/shell/tests/test_cases.py`:
- `BAR_SCENES` gains the same four entries after `"bar-tray",`;
- `test_the_bar_brings_nine_scenes_of_twelve_cases` becomes:

```python
    def test_every_bar_scene_brings_twelve_cases(self):
        found = [
            case for surface in self.BAR_SCENES for case in cases.surface_cases(surface)
        ]
        self.assertEqual(len(self.BAR_SCENES), 13)
        self.assertEqual(len(found), 12 * len(self.BAR_SCENES))
        self.assertEqual(len({c.tag for c in found}), len(found))
```

In `.github/workflows/shell-surfaces.yml`, job `bar-scenes`, the matrix list gains `bar-network`, `bar-bluetooth`, `bar-audio` and `bar-battery` after `bar-tray`.

Run:

```bash
python3 -B -m unittest discover -s forge/test/shell/tests
python3 scripts/verify.py workflows
bash -n forge/test/shell/rig.sh
```

Expected: every command passes.

- [ ] **Step 6: Capture the goldens and look at every one**

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
for scene in bar-network bar-bluetooth bar-audio bar-battery; do
    ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh update-goldens "$scene"
done
ls forge/test/shell/golden/bar-{network,bluetooth,audio,battery}/*.png | wc -l
```

Expected: 48.

Open every PNG and check what it must show:
- **`bar-network`:** the popover under the network button, with "Wired: connected", "Athanor Lab, connected" first, then the other networks by strength, and the switches Wi-Fi and Airplane mode.
- **`bar-bluetooth`:** the power switch on, the paired Headphones and Keyboard, and the nearby Phone and Speaker.
- **`bar-audio`:** the output slider at 40 %, the input slider at 55 %, the two outputs with "Speakers, in use", and "Night Drive" by "Calmo" with the media buttons.
- **`bar-battery`:** "72 %" in the bar and in the popover, "3 h 25 min left", Balanced chosen, and the brightness at 60 %.
- **Every scene:**
  - the de cases show the German strings;
  - the rtl cases are mirrored;
  - no text is clipped at scale 1.5.

A capture that shows an empty or closed popover means the data arrived after the settle. Raise that scene's `settle`, never above 20, and capture again.

- [ ] **Step 7: Run each scene twice against its goldens**

```bash
for scene in bar-network bar-bluetooth bar-audio bar-battery; do
    for run in 1 2; do
        ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface "$scene"
    done
done
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface bar-power
```

Expected: every run passes. A second run that fails where the first passed is a nondeterministic scene. The usual cause is a signal strength or a volume that arrives in a different order. Fix the fixture so that it sets the value before the bar starts; never widen the comparison's tolerance.

- [ ] **Step 8: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/po forge/test/shell/locale/bar-de.po forge/test/shell/rig.sh \
    forge/test/shell/cases.py forge/test/shell/tests/test_cases.py .github/workflows/shell-surfaces.yml \
    forge/test/shell/golden/bar-network forge/test/shell/golden/bar-bluetooth \
    forge/test/shell/golden/bar-audio forge/test/shell/golden/bar-battery
git commit -m "test(bar): the network, Bluetooth, audio and battery popovers as surface scenes, and their translations"
```

---
### Task 9: The dev VM's real session, packaging, and the procedure for item 15

**Files:**
- Create: `scripts/devvm/bar_modules.py`
- Modify: `scripts/devvm/bar-acceptance.sh` (header, `STAGES`, a library check in `stage_deploy`, `stage_modules`)
- Modify: `forge/specs/athanor-bar/athanor-bar.spec` (BuildRequires, description, release, changelog)
- Modify: `forge/config/packages.json` (`pulseaudio-libs-glib2` in `upstream_desktop`, edited through Bash)

**Interfaces:**
- Consumes: the binary from `rig.sh build-bar` (`.scratch/shell-rig/bin/athanor-bar`), and the dev VM of `scripts/devvm` running the Athanor image with a COSMIC session.
- Produces:
  - `bar-acceptance.sh modules`, which prints `PASS modules` or `FAIL modules: <what was read>`;
  - the RPM, which builds against `pulseaudio-libs-devel`.

The dev VM has a wired NetworkManager connection and PipeWire in the session. It has no Wi-Fi radio, no Bluetooth adapter and no battery. The stage therefore proves what the rig cannot:
- the bar reaches the real services under its real unit: Landlock, `ProtectHome=read-only`, `RestrictAddressFamilies`;
- the real D-Bus policy accepts the bar's NetworkManager secret agent;
- the Bluetooth and battery modules hide when their hardware is absent.

- [ ] **Step 1: Write `scripts/devvm/bar_modules.py`**

```python
"""bar_modules.py: run in the guest's session by bar-acceptance.sh (stage modules).

The system modules of athanor-bar against the real services of the dev VM, under the
bar's real unit: the audio module reaches pipewire-pulse through libpulse and follows the
default sink's volume; the network module shows NetworkManager's wired connection; the
Bluetooth and battery modules show exactly when BlueZ has an adapter and UPower a present
battery. Prints one line per step and exits non-zero on the first that fails.
"""

import subprocess
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402


def wait_for(check, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = check()
        if found:
            return found
        time.sleep(0.2)
    return check()


def application():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-bar":
            return app
    return None


def showing(accessible, role, label):
    """Showing accessibles of `role` named `label`, directly or through LabelledBy (the
    volume slider takes its name from its row's label)."""
    found = []

    def name_of(node):
        if node.get_name():
            return node.get_name()
        for relation in node.get_relation_set():
            if relation.get_relation_type() == Atspi.RelationType.LABELLED_BY:
                for index in range(relation.get_n_targets()):
                    target = relation.get_target(index)
                    if target is not None and target.get_name():
                        return target.get_name()
        return ""

    def visit(node):
        if (
            node.get_role_name() == role
            and node.get_state_set().contains(Atspi.StateType.SHOWING)
            and name_of(node) == label
        ):
            found.append(node)
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child is not None:
                visit(child)

    visit(accessible)
    return found


def step(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}{f': {detail}' if detail and not ok else ''}")
    if not ok:
        sys.exit(1)


def system_call(name, path, interface, method, args=None):
    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    try:
        return bus.call_sync(name, path, interface, method, args, None, Gio.DBusCallFlags.NONE, 5000, None).unpack()
    except GLib.Error:
        return None


def has_adapter():
    reply = system_call("org.bluez", "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects")
    return reply is not None and any("org.bluez.Adapter1" in interfaces for interfaces in reply[0].values())


def has_battery():
    reply = system_call(
        "org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice",
        "org.freedesktop.DBus.Properties", "GetAll", GLib.Variant("(s)", ("org.freedesktop.UPower.Device",)),
    )
    return reply is not None and reply[0].get("Type") == 2 and reply[0].get("IsPresent") is True


def default_sink_percent():
    """The default sink's volume in percent, from wireplumber's wpctl: "Volume: 0.40"."""
    text = subprocess.run(
        ["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], check=True, capture_output=True, text=True
    ).stdout
    return round(float(text.split()[1]) * 100)


def main():
    app = wait_for(application, 20)
    step("athanor-bar is on the accessibility bus", app is not None)

    sound = wait_for(lambda: showing(app, "button", "Sound"), 15)
    step("the audio module reached pipewire-pulse under the unit", bool(sound))
    sound[0].do_action(0)
    slider = wait_for(lambda: showing(app, "slider", "Output volume"), 5)
    step("the audio popover shows the output volume", bool(slider))
    expected = default_sink_percent()
    step(
        "the slider shows the default sink's volume",
        abs(slider[0].get_current_value() - min(expected, 100)) < 1.5,
        f"slider {slider[0].get_current_value()}, wpctl {expected}",
    )
    sound[0].do_action(0)

    network = wait_for(lambda: showing(app, "button", "Network"), 10)
    step("the network module shows NetworkManager's state", bool(network))
    network[0].do_action(0)
    step("the wired connection is shown", bool(wait_for(lambda: showing(app, "label", "Wired: connected"), 5)))
    network[0].do_action(0)

    adapter = has_adapter()
    step(
        f"the Bluetooth module {'shows' if adapter else 'hides'}: the VM {'has' if adapter else 'has no'} adapter",
        bool(showing(app, "button", "Bluetooth")) == adapter,
    )
    battery = has_battery()
    step(
        f"the battery module {'shows' if battery else 'hides'}: the VM {'has' if battery else 'has no'} battery",
        bool(showing(app, "button", "Battery")) == battery,
    )


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Add the library check and the `modules` stage to `bar-acceptance.sh`**

1. In the header comment, after "...high contrast reaches COSMIC's theme from inside it (BR3),", insert:

```bash
# the system modules reach NetworkManager, pipewire-pulse, BlueZ and UPower from inside
# it and hide what the VM lacks (package 2b.4),
```

2. `STAGES` becomes:

```bash
STAGES=(deploy unit memory modules high-contrast hotplug crash-loop cleanup)
```

3. At the end of `stage_deploy`, add:

```bash
    # libpulse and its GLib main loop come with the RPM's automatic dependencies; a binary
    # deployed by hand needs them in the image already.
    local libraries
    libraries=$(in_session ldd /usr/bin/athanor-bar) || fail "ldd /usr/bin/athanor-bar failed in the guest"
    if grep -q 'not found' <<< "$libraries"; then
        fail "libraries missing from the guest image: $(grep 'not found' <<< "$libraries")"
    fi
```

4. After `stage_memory`, add:

```bash
# The system modules against the VM's real services, under the real unit: libpulse reaches
# pipewire-pulse, NetworkManager accepts the secret agent, and the modules without hardware
# hide (bar_modules.py). The journal must show no refusal from a service.
stage_modules() {
    local since
    since=$(in_session date +%s)
    fresh_start
    in_session python3 - < "$HERE/bar_modules.py" || fail "a module did not reach its service (steps above)"
    local journal pattern='a system service refused or did not answer|refused the pairing agent|cannot export|does not parse|no GLib main loop'
    journal=$(in_session "journalctl --user -u athanor-bar --since @$since --no-pager -o cat")
    if grep -qE "$pattern" <<< "$journal"; then
        fail "refusals in the journal: $(grep -E "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after the modules: $(unit show -p Result --value)"
    # Recorded, not judged: libpulse warns on every connect that it cannot create its cookie
    # under ProtectHome and Landlock, which pipewire-pulse does not need; a refused connection
    # is a different line. The polkit default decides whether joining a new network prompts.
    local notes="$SHOTS/modules-notes.txt"
    {
        echo "libpulse cookie warnings: $(grep -ci 'cookie' <<< "$journal")"
        echo "libpulse refused connections: $(grep -ciE 'connection refused|access denied|connection terminated' <<< "$journal")"
        echo "polkit, org.freedesktop.NetworkManager.settings.modify.system:"
        in_session pkaction --verbose --action-id org.freedesktop.NetworkManager.settings.modify.system |
            grep -E '^\s*implicit (any|inactive|active):'
    } > "$notes" || fail "could not record the modules' notes"
    cat "$notes"
    "$HERE/screenshot.sh" "$SHOTS/modules.png" > /dev/null
}
```

Run:

```bash
bash -n scripts/devvm/bar-acceptance.sh
shellcheck scripts/devvm/bar-acceptance.sh
python3 -m py_compile scripts/devvm/bar_modules.py
```

Expected: all three exit 0.

With the dev VM running (`scripts/devvm/start.sh`, `GPU_OUTPUTS=2` in `devvm.env` for the hotplug stage):

```bash
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh build-bar
bash scripts/devvm/bar-acceptance.sh
```

Expected:
- `PASS` for every stage, including `modules` and `memory`;
- the memory line prints the PSS with libpulse loaded, within 65536 kB;
- `.scratch/bar-acceptance/modules.png` shows the Sound and Network buttons in the bar, and no Bluetooth or Battery button;
- `modules` prints, and keeps in `.scratch/bar-acceptance/modules-notes.txt`:
  - the count of libpulse cookie warnings, expected to be non-zero and harmless;
  - the count of refused connections, expected 0;
  - the three `implicit` lines of the polkit action for system-wide connections.

  Copy the notes into the handback and the pull request. A non-zero refused count with the audio module shown means libpulse retried until it connected: say so.

`FAIL deploy: libraries missing from the guest image` names the library. The published Athanor image has `pulseaudio-libs` but not `pulseaudio-libs-glib2` (checked with `rpm -q` on an Athanor host on 2026-09-29). Step 3 adds it to the image's package list, so the dev VM has it once the system image is rebuilt from a branch that carries Step 3 and the VM moves to it with `scripts/devvm/upgrade.sh`. Until then this stage is expected to fail at deploy with that message: record it in the handback as the known state, not as a defect. Do not install the library by hand in the VM.

- [ ] **Step 3: The RPM builds against libpulse**

In `forge/specs/athanor-bar/athanor-bar.spec`:
- the `BuildRequires` line gains `pulseaudio-libs-devel` after `gtk4-layer-shell-devel`;
- `Release: 1%{?dist}` becomes `Release: 2%{?dist}`;
- in `%description`, "the input source, accessibility, tiling, the clock and the power menu." becomes "the input source, accessibility, tiling, audio with media controls, Bluetooth, network, battery, the clock and the power menu.";
- `%changelog` gains, above the first entry:

```text
* Tue Sep 29 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- The network, Bluetooth, audio and battery modules (doc_bar.md, BR3): NetworkManager's
  secret agent and BlueZ's pairing agent registered by the bar, with calls from any other
  sender refused; audio over libpulse with MPRIS media controls; the power profile over
  the power-profiles interface and the brightness through logind.
```

`libpulse.so.0` and `libpulse-mainloop-glib.so.0` become automatic `Requires`, which pull in `pulseaudio-libs` and `pulseaudio-libs-glib2`. No explicit `Requires` is added.

The image does not install the bar's RPM with its dependencies when the binary is deployed by hand, and `forge/config/packages.json` lists no `pulseaudio-libs-glib2` today. Add it to `upstream_desktop`, after `gtk4-layer-shell`. The file is reformatted whole by the editor hook, so edit it through Bash and check the diff:

```bash
python3 - <<'EOF'
from pathlib import Path
path = Path("forge/config/packages.json")
text = path.read_text(encoding="utf-8")
old = '    "gtk4-layer-shell",\n'
assert text.count(old) == 1
path.write_text(text.replace(old, old + '    "pulseaudio-libs-glib2",\n'), encoding="utf-8")
EOF
git diff --stat forge/config/packages.json
python3 -c 'import json; print("pulseaudio-libs-glib2" in json.load(open("forge/config/packages.json"))["upstream_desktop"])'
```

Expected: `1 file changed, 1 insertion(+)`, then `True`.

Run:

```bash
podman run --rm --security-opt label=disable -v "$(git rev-parse --show-toplevel):/repo:ro" -w /repo \
    localhost/athanor-shell-rig:build rpmspec -P forge/specs/athanor-bar/athanor-bar.spec
```

Expected: the parsed spec prints the new `BuildRequires` and the 1.0.0-2 changelog, with no warning.

- [ ] **Step 4: Commit**

```bash
git add scripts/devvm/bar_modules.py scripts/devvm/bar-acceptance.sh forge/specs/athanor-bar/athanor-bar.spec \
    forge/config/packages.json
git commit -m "test(devvm): the bar's system modules against the VM's real services; build(bar): libpulse"
```

- [ ] **Step 5: Write the deviations and the procedure for item 15 into the pull request**

Under "Deviations from doc_bar.md" in the pull request description, write:

```markdown
- **GIO, not zbus, for the network row (BR3).** The bar already runs GDBus on GLib's main loop
  for logind. zbus would add a second D-Bus stack and an async executor to a process with a
  64 MB budget (section 5, item 17), for the same calls on the bus. Every system module uses
  GIO.
- **Bluetooth pairing is started from the bar only (decision D8, 2026-09-29).** The agent
  answers only the pairing of the device the person pressed; an incoming request from another
  device is rejected without a prompt, and `AuthorizeService` follows the same rule.
```

Item 15 needs a Wi-Fi radio and a Bluetooth radio. The rig mocks both services, and the dev VM has neither radio. The maintainer runs this procedure on the desktop, with the image that carries this package. Paste it into the pull request description under "Verification on hardware (doc_bar.md, section 5, item 15)", with one box per line:

1. Note the time: `date +%s`, and keep the number as `SINCE`.
2. **Wi-Fi with a password.**
   1. Forget the test network if it is saved: `nmcli connection delete "<SSID>"`.
   2. Open the bar's Network popover, press the network's row, type the password, and press Connect. Note whether an authentication prompt appeared before the connection came up, and which program showed it.
   3. Check the connection is active: `nmcli -f NAME,TYPE,DEVICE connection show --active` lists `<SSID>` of type `802-11-wireless`.
   4. Check the password is in NetworkManager's profile only, readable by root: `sudo nmcli -s -g 802-11-wireless-security.psk connection show "<SSID>"` prints it.
3. **The secret agent.**
   1. Run `sudo nmcli connection modify "<SSID>" 802-11-wireless-security.psk-flags 2`. Flag 2 is `NOT_SAVED`: NetworkManager forgets the password and asks an agent for it on every connection, which is the case the bar's agent serves. Flag 1 (`AGENT_OWNED`) would make NetworkManager ask the agent to store it, which the bar refuses by design.
   2. Run `nmcli connection down "<SSID>"`, then press the row in the bar again.
   3. The bar asks for the password; type it; the connection comes up.
   4. Restore the flag: `sudo nmcli connection modify "<SSID>" 802-11-wireless-security.psk-flags 0`. NetworkManager asks for the password once more at the next connection and keeps it again.
4. **Bluetooth pairing with a confirmation.**
   1. Put a phone in pairing mode.
   2. Open the Bluetooth popover, press the phone's row, and check that the six digits in the bar equal the phone's.
   3. Press Pair in the bar and confirm on the phone.
   4. Check with `bluetoothctl info <MAC>`: the output shows `Paired: yes`, `Trusted: yes` and `Connected: yes`.
5. **Nothing was written by the bar.** Read the password without echoing it, so it never reaches the shell history, then search. Each search must print nothing:

   ```bash
   read -rs -p 'Wi-Fi password: ' PW; echo
   journalctl --user -u athanor-bar --since "@$SINCE" -o cat | grep -F -- "$PW"
   grep -rlsF -- "$PW" ~/.config ~/.local ~/.cache ~/.bash_history /run/user/$(id -u) /tmp
   unset PW
   ```

   `grep -r` skips sockets and FIFOs, and `-s` silences unreadable files; the check is the empty output.

---

## Self-Review

**Spec coverage.**

| Requirement | Where |
| --- | --- |
| BR3, Network row: state, Wi-Fi list, join with a password through NetworkManager's secret agent, airplane mode | Task 2 (logic), Task 4 (module, agent, e2e) |
| BR3, Bluetooth row: power, paired and nearby devices, pairing with a confirmation through BlueZ's agent | Task 2, Task 5 |
| BR3, Audio row: output and input volume and mute, device choice, media controls | Task 2, Task 6 |
| BR3, Battery row: level, time left, power profile, brightness; hidden without a battery | Task 2, Task 7 |
| BR9: the scenes run on python3-dbusmock fixtures (NetworkManager, BlueZ, UPower, power profiles, logind) | Task 1 (packages), Task 3 (fixtures), Task 8 (scenes) |
| Open doubt 4 | Ruled in the header, "Rulings" |
| Section 5, item 14 | Not in this package: it covers the modules of 2b.2, and the dev VM stages of `bar-acceptance.sh` already run them. Task 9 adds the `modules` stage beside them |
| Section 5, item 15 | Task 9, Step 5: the procedure on the maintainer's desktop, copied into the pull request |
| Section 5, item 17 | Task 7, Step 5 (PSS in the rig with every module) and Task 9 (the `memory` stage with libpulse loaded) |
| Section 5, item 18 | Task 8: 48 cases, each scene run twice |
| doc_shell.md SH13 | Task 7 (`rig.sh atspi bar-modules`, 11 widgets), Task 8 (12 cases per scene, popovers open) |

**Placeholder scan.** No step says "TBD", "later" or "similar to". The steps in Tasks 8 and 9 that depend on a machine (goldens, dev VM, hardware) state the expected output and what to report when it differs.

**Type consistency.**
- Tasks 4 to 7 use `bus::call`, `bus::set_property`, `bus::spawn`, `Mirror::new` and `Source` with the signatures that Task 4 defines.
- `ModuleUi`, `Changed`, `Popup::new`, `switch_row`, `tr` and `tr_with` are used as the bar declares them on Unit A (`b648f633`): `Popup::new(bar, child, name)` attaches through `popup::attach`, which calls `athanor_apps::menu::attach(button, popup::towards_inside(bar))`.
- The e2e helpers (`open_popover`, `property_of`, `mock_calls`, `labelled`, `wait_for`) are defined in Tasks 3 and 4 before any later task uses them.
- The accessible names that `bar_modules.py` and the e2e look up match the msgids in the modules and in Task 8's translations:
  - "Sound", "Network", "Bluetooth", "Battery";
  - "Output volume";
  - "Wired: connected".

**Review Focus.** Every line of the section has its test in the owning task:
- item 1, the sound server restarts: Task 6;
- item 2, NetworkManager or BlueZ restarts under an open prompt: Task 4;
- item 3, hostile SSIDs and device names: Task 2's unit tests;
- item 4, a second request, a foreign sender or an unsolicited Bluetooth request: Tasks 4 and 5;
- item 5, two outputs and memory: `prompt_view` in Task 4, and the PSS checks of Tasks 3 and 9.

---

## Acceptance

Run from the repository root. `RIG_CARGO` is the command defined in Global Constraints. Every command must exit 0, except the last group, which needs the dev VM or the maintainer and is marked as such.

```bash
# 0. The pinned rig image carries the fixtures and the libpulse GLib library (B1, decision D9)
podman run --rm "ghcr.io/hr-mes/athanor-shell-rig@$(cat forge/test/shell/rig-image.digest)" \
    rpm -q pulseaudio-libs-glib2 python3-dbusmock pipewire-pulseaudio

# 1. The library and unit tests of the four modules
podman run --rm --memory 6g --security-opt label=disable \
    -v "$(git rev-parse --show-toplevel):/repo:ro" -v "$(git rev-parse --show-toplevel)/.scratch/shell-rig:/out" \
    -v athanor-cargo-registry:/root/.cargo/registry -e CARGO_TARGET_DIR=/out/target -w /repo \
    localhost/athanor-shell-rig:build cargo test --locked -p athanor-bar

# 2. The binary, the end-to-end tests of the existing modules and of the four new ones.
#    bar-modules-e2e also fails if a typed Wi-Fi password appears in any file under /out
#    (recursively), the scene's XDG directories, /run/user/1000 or /tmp.
bash forge/test/shell/rig.sh build-bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-e2e
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh bar-modules-e2e

# 3. SH13: the accessibility tree without and with the fixtures
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh atspi bar
ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh atspi bar-modules

# 4. BR9 and item 18: the four new scenes and the existing bar scenes against the goldens
for scene in bar bar-power bar-input bar-calendar bar-accessibility bar-tiling \
             bar-popups bar-notifications bar-tray \
             bar-network bar-bluetooth bar-audio bar-battery; do
  ATHANOR_RIG_IMAGE=localhost/athanor-shell-rig:rig bash forge/test/shell/rig.sh surface "$scene" || exit 1
done

# 5. The case catalogue, the workflow and the scripts
python3 -B -m unittest discover -s forge/test/shell/tests
python3 scripts/verify.py workflows
bash -n forge/test/shell/rig.sh scripts/devvm/bar-acceptance.sh
shellcheck scripts/devvm/bar-acceptance.sh
python3 -m py_compile scripts/devvm/bar_modules.py

# 6. The translations
for po in forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po \
          forge/test/shell/locale/bar-de.po; do
  msgfmt --check --output-file=/dev/null "$po" || exit 1
done

# 7. The package builds against libpulse
grep -q '^BuildRequires:.*pulseaudio-libs-devel' forge/specs/athanor-bar/athanor-bar.spec
python3 -c 'import json, sys; sys.exit("pulseaudio-libs-glib2" not in json.load(open("forge/config/packages.json"))["upstream_desktop"])'
podman run --rm --security-opt label=disable -v "$(git rev-parse --show-toplevel):/repo:ro" -w /repo \
    localhost/athanor-shell-rig:build rpmspec -P forge/specs/athanor-bar/athanor-bar.spec > /dev/null

# 8. Out of scope files are untouched: no stylesheet, polkit, Gatekeeper or attestation change
# Only this branch's own commits: the merges of Unit A, 2b.3 and iso-v0 bring changes of their own.
test -z "$(git log --first-parent --no-merges --format= --name-only 6e1d357f..HEAD -- \
    system/athanor-style system/athanor-bus-api/src/polkit.rs \
    forge/specs/athanor-gatekeeper-rs system/confidential_computing/athanor-attestation)"
```

Needs the dev VM (`scripts/devvm/start.sh`), run before the pull request leaves draft:

```bash
bash scripts/devvm/bar-acceptance.sh
```

Needs the maintainer, on the desktop with Wi-Fi and Bluetooth: the procedure in Task 9, Step 5, ticked in the pull request description.
