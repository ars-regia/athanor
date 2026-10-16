# Development VM

Shell and application work does not need an image build and a desktop reboot for every
change. Pick the fastest tier that can show the change:

| Tier | Where                                                       | Turnaround | For                                                                                                   |
| ---- | ----------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------------- |
| A    | `nested.sh`: cosmic-comp in a window on the host            | seconds    | a Wayland client (shell, settings, applets) against the real compositor                               |
| B    | this VM: the published image under KVM, `deploy.sh` into it | minutes    | anything that needs the real system: greetd and the session, units, sandboxing, polkit, `/usr` layout |
| C    | CI image build, `bootc upgrade` on the desktop              | hours      | the image itself, the kernel, drivers, what only real hardware shows                                  |

## Tier A

```sh
scripts/devvm/nested.sh ./target/debug/athanor-bar
```

cosmic-comp detects the parent Wayland session, opens its own socket (`wayland-2`) and
starts the command with that `WAYLAND_DISPLAY`. Close the window to end it. The client
talks to the host's session bus and user services, not to an Athanor session.

## Tier B

```sh
scripts/devvm/create.sh            # once: fetch the newest ISO (5 GB), install it unattended (~5 min)
scripts/devvm/start.sh             # boot (~20 s to SSH), report the guest GL renderer
scripts/devvm/ssh.sh               # a shell; ssh.sh CMD runs CMD
scripts/devvm/deploy.sh --restart-session \
  target/release/athanor-bar:/usr/bin/athanor-bar
scripts/devvm/ssh.sh sudo poweroff # stop (or: systemctl --user stop athanor-devvm)
scripts/devvm/reset.sh             # back to the freshly installed system
```

- `create.sh [TAG]` installs `athanor-iso:TAG` (default `newest`, the highest run id in
  `$REGISTRY`) the way `forge/test/iso` does: the ISO unchanged, `devvm.ks` on a disk
  labelled `ATHANORKS`, selected with `inst.ks=` typed at GRUB's command line. The
  installed disk is `base.qcow2`, read-only from then on; the VM boots `dev.qcow2`, a
  qcow2 overlay on it that `reset.sh` recreates.
- The account is the host user's name, password equal to the name (for the greeter),
  passwordless sudo, and SSH with `~/.ssh/id_ed25519` (or a key generated in the state
  directory). This is a development VM only: SSH listens on `127.0.0.1:2222` alone.
  `devvm.ks` binds the host, `10.0.2.2` inside the guest, to the firewall's `home` zone,
  because `public` has no `ssh` (ADR-0089, ADR-0116). A base created before that binding
  loses SSH at the first boot of an image that carries ADR-0116's migration: once after
  every `reset.sh`, run `ssh.sh sudo firewall-cmd --permanent --zone=home --add-source=10.0.2.2`
  before booting such an image, or recreate the base with `create.sh`.
- `deploy.sh SRC:DEST...` installs local files at guest paths. `/usr` is read-only, so the
  first deploy of a boot runs `bootc usr-overlay`: **files under `/usr` vanish when the
  guest reboots**. `--restart-session` restarts greetd (you log in again),
  `--restart-unit U` restarts a system unit.
- QEMU runs in the user unit `athanor-devvm` (`journalctl --user -u athanor-devvm`), outside
  the sandbox greetd puts on the graphical session. The serial console goes to
  `console.log`, and live to `console.sock` (`scripts/devvm/console.sh`, a real
  bidirectional terminal for when SSH is unreachable -- a hung boot, a dead network,
  typing at a boot menu). QEMU's own monitor is at `monitor.sock`
  (`socat - unix:$STATE/monitor.sock`, or any client that speaks the QEMU HMP), for
  things the guest OS cannot do for itself, such as typing at the greeter with no
  keyboard focus or adding a one-off `hostfwd_add` if the network ever needs it. Both
  sockets are created user-only (`start.sh` sets `umask 077` on the QEMU process itself).
- Graphics: `virtio-vga-gl`, a virgl GPU rendered by the host GPU, shown in a GTK window.
  `DISPLAY_BACKEND=egl-headless` renders without a window and serves the screen with SPICE
  on `127.0.0.1:5930`; that is what an agent verifying the image uses, because it has no
  graphical session, and the screen is then watched with a SPICE client
  (`nix profile add nixpkgs#virt-viewer`, `remote-viewer spice://127.0.0.1:5930`). `start.sh` prints the guest's OpenGL renderer (`gl_renderer.py`,
  surfaceless EGL, since the greeter runs without Xwayland): `virgl (zink ...)` is the
  host GPU, `llvmpipe` would be software.
- `screenshot.sh [DEST]` saves a PNG of the logged-in session (default `./screenshot.png`).
  QEMU's `screendump` monitor command does not work with `virtio-vga-gl`
  (`Error: no surface`); the working way is `grim`, already in the image, run inside the
  guest as the session user against its own compositor socket and streamed back over SSH
  -- `screenshot.sh` is exactly that. To shoot the greeter instead (before login, a
  different user and compositor), run `grim` as `greetd`, e.g.
  `ssh.sh sudo -u greetd env XDG_RUNTIME_DIR=/run/user/967 WAYLAND_DISPLAY=wayland-1 grim /tmp/greeter.png`
  then copy it off with `ssh.sh cat /tmp/greeter.png > greeter.png` (the greeter's uid can
  differ; `ssh.sh id -u greetd` confirms it).
- `upgrade.sh` switches the guest to `$SYSTEM_IMAGE:latest` (`$REGISTRY/athanor-system` by
  default) and reboots into it. `bootc upgrade` cannot do this on its own: the ISO's
  kickstart (`osbuild-base.ks`, generated by bootc-image-builder and included by `devvm.ks`) pins the guest to the run-id tag it was installed from, and upgrade only
  re-pulls that same tag.
- `layout-acceptance.sh [stage...]` deploys the stage 1c layout crates from
  `.scratch/shell-rig/bin` (build them with `forge/test/shell/rig.sh build-layout`) and runs
  acceptance item 10 of `doc_shell.md` in the VM's session: the first-session default,
  rotation, live presets, a rejected document, a mandatory key added mid-session, memory,
  and the crash loop. Screenshots land in `.scratch/layout-acceptance/`; the script checks
  the panel configuration, so look at them: they are the only check of what the panel draws.
- `bar-acceptance.sh [stage...]`: athanor-bar under its real unit (package 2b.2):
  Type=notify, the confinement against glycin's sandbox, PSS within 64 MB, an output that
  comes and goes (needs `GPU_OUTPUTS=2`, the default), and the crash-loop fallback to the
  vendor layout. Build the binary first with `forge/test/shell/rig.sh build-bar`.
- `dock-acceptance.sh [stage...]`: athanor-dock under its real unit (package 2c):
  Type=notify, the confinement against glycin's sandbox and the favourites file, PSS within
  48 MB, a pinned entry launched from the dock behind a security context (item 8, pressed
  through `dock_press.py`; the session's favourites are restored afterwards), an output
  that comes and goes (needs `GPU_OUTPUTS=2`, the default), and the crash-loop fallback to
  the vendor layout. Build the binary first with
  `forge/test/shell/rig.sh build-dock`. COSMIC's own dock keeps running beside it.
- `notifications-acceptance.sh [stage...]`: athanor-bar and athanor-shelld under their real
  units on a private bus (package 2b.3): the daemon admits the bar's List, a notification
  sent while the bar is down shows after its restart (item 9), popups across an output that
  comes and goes (needs `GPU_OUTPUTS=2`), and PSS within 64 MB with notifications held.
  Build both binaries first with `forge/test/shell/rig.sh build-bar` and `build-shelld`.
  Screenshots land in `.scratch/notifications-acceptance/`; look at them.
- `shield-acceptance.sh [stage...]`: the trust shield of athanor-bar under its real unit
  (package 2b.5), on fixed trust states from `forge/test/shell/trust_state.py` with the
  update units masked for the run: the sheet opens and closes (`sheet`), the bar trusts the
  state athanor-update answers with (`state`), the sheet offers "Restart to update" and
  "Go back to the previous version" only when the state allows them (`actions`), and a
  machine with no check for 15 days reads "Not verified yet" (`stale`). `deploy` installs a
  locally built athanor-bar and athanor-update (`forge/test/shell/rig.sh build-bar`, and
  `rig.sh cargo build --locked --release -p athanor-update`); without it the stages read the
  image's. `cleanup` unmasks the units and republishes the real state.
  Screenshots land in `.scratch/shield-acceptance/`.
- `compositor-acceptance.sh [stage...]` deploys `cc-probe` from `.scratch/shell-rig/bin`
  (build it with `forge/test/shell/rig.sh build-compositor-client`) and checks, in the VM's
  session, the launch of package 2a (`doc_bar.md`, BR2 and BR3): an application on its own
  restricted socket in a transient unit, a `Terminal=true` entry through `xdg-terminal-exec`,
  and the three COSMIC openers, cold and warm. Screenshots land in
  `.scratch/compositor-acceptance/`; look at the openers' ones.
- `lock-acceptance.sh [stage...]` checks the session lock and the polkit agent of release 0.9
  on the image as installed (ADR-0115, `docs/spikes/lock-and-polkit-agent-0-9.md`): the sleep
  inhibitor, `loginctl lock-session`, a wrong password (seen in PAM's journal line) and a
  right one, a locker killed while locked that leaves the session locked and comes back, the
  idle lock, `pkexec true` answered in cosmic-osd's dialog with the user's pam_faillock tally
  present, and six cancelled prompts that must record no failure (the stage refuses to start
  within two failures of `deny=`, so it never locks the account). Before each cancel it
  checks only that a helper is alive, 2 s after the prompt opened: no state or log line shows
  that the helper is asking for the password. "Locked" is cosmic-greeter's
  marker file, the locker's intent: cosmic-comp exposes no lock state, so the screenshots in
  `.scratch/lock-acceptance/` are the only check of what the compositor draws; look at them.
  The login creates the tally, so `polkit` fails on images built before #445 merged
  (#144), and `cancel` fails until #447 is fixed. `lock-acceptance.sh polkit` is the polkit
  criterion of #144. Log in at the greeter first.
- `session-acceptance.sh [stage...]` checks the other two criteria of #144 and the
  libadwaita criterion of #157 in the real session. `volume`: cosmic-settings-daemon is active with its varlink socket bound, the
  image binds the volume keys to the system action, the session's cosmic-comp keeps its key
  bindings (`--keep-shortcuts`, #510), and on a null sink the script makes with `pactl`
  (the VM has no sound device) three volume-up presses raise and three volume-down presses
  lower the volume `wpctl` reads, with no "panicked" in the daemon's journal. `portal`:
  Flatpak org.gnome.TextEditor (installed from Flathub on the first run), its host access
  dropped by a user override that the cleanup resets (the stage refuses to start over an
  existing one), opens a file of the home directory through the file chooser, which the
  document portal must export under `$XDG_RUNTIME_DIR/doc/by-app/<app id>/`, and a typed
  character saved with Ctrl+S must change the file. Keys come from `keyboard_type.py`, which knows
  `@volup`, `@voldown` and `@mute`. The chooser has no log line, so the stage waits a fixed
  time for it; the screenshots in `.scratch/session-acceptance/` are the only check of what
  is drawn. `scheme`: with `color-scheme` set to `default`, `prefer-dark` and `default`
  again, the Settings portal answers 0, 1 and 0, and a libadwaita window (`adw_scheme.py`,
  copied into `$XDG_RUNTIME_DIR`) turns light, dark and light again. The window runs with
  `GSETTINGS_BACKEND=memory`, so only the portal can turn it dark; the user's value is
  restored. Log in at the greeter first.
- Settings are in `devvm.env` and are overridden from the environment: `CPUS=4`,
  `MEMORY=8G`, `DISK_GIB=40`, `GPU_OUTPUTS=2` (the second head starts disconnected, so
  every other stage sees one output; `bar-acceptance.sh hotplug` is the one that needs
  it), `SSH_PORT`, `ISO_TAG`, `REGISTRY`. State (ISO, disks, logs)
  is in `${XDG_DATA_HOME:-~/.local/share}/athanor-devvm`: about 6 GB of ISO and up to
  `DISK_GIB` of disk.
- Needs `qemu-system-x86_64` with the virtio-gpu-gl device, `qemu-img`, OVMF, `skopeo`,
  `jq`, `mkfs.ext4`. OVMF is looked up in `OVMF_DIR` first, then under `/usr/share`: on an
  image-based host, such as an Athanor laptop, QEMU and the firmware can come from Nix:
  `nix profile add nixpkgs#qemu_kvm`, then
  `export OVMF_DIR=$(nix build --no-link --print-out-paths nixpkgs#OVMF.fd)/FV` in the shell
  that runs `create.sh` and `start.sh`, since both look the firmware up. Nix's QEMU loads
  Nix's Mesa, which looks for its drivers where NixOS keeps them. Outside NixOS, `start.sh`
  is given that Mesa through the user manager, and the manager is cleared once QEMU runs, so
  no other user service inherits it:

  ```sh
  m=$(nix build --no-link --print-out-paths nixpkgs#mesa)
  vars=(GBM_BACKENDS_PATH=$m/lib/gbm LIBGL_DRIVERS_PATH=$m/lib/dri
    __EGL_VENDOR_LIBRARY_DIRS=$m/share/glvnd/egl_vendor.d)
  systemctl --user set-environment "${vars[@]}"
  scripts/devvm/start.sh
  systemctl --user unset-environment "${vars[@]%%=*}"
  ```

### Resources

The self-hosted runner guest takes 12 vCPUs and 16 GB (`scripts/runner/runner.env`), this
VM 4 vCPUs and 8 GB: both together leave the desktop about 7 GB on a 31 GB host.

### What the VM can and cannot show

Verifiable in the VM: boot, greetd and the greeter, the session and its units, the
applications, D-Bus and polkit, systemd sandboxing, SELinux and Landlock denials, updates
of `/usr` content, Mesa on a virtual GPU (virgl).

Hardware only (tier C): NVIDIA and nouveau drivers and GPU-specific rendering,
Bluetooth, Wi-Fi, audio devices, suspend and power management, firmware and Secure Boot
with the project keys, TPM-bound secrets, multi-monitor and HiDPI panels, the kernel's
behaviour on the real CPU and disks.
