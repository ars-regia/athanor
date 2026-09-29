# Development VM

Shell and application work does not need an image build and a desktop reboot for every
change. Pick the fastest tier that can show the change:

| Tier | Where | Turnaround | For |
|------|-------|------------|-----|
| A | `nested.sh`: cosmic-comp in a window on the host | seconds | a Wayland client (shell, settings, applets) against the real compositor |
| B | this VM: the published image under KVM, `deploy.sh` into it | minutes | anything that needs the real system: greetd and the session, units, sandboxing, polkit, `/usr` layout |
| C | CI image build, `bootc upgrade` on the desktop | hours | the image itself, the kernel, drivers, what only real hardware shows |

## Tier A

```sh
scripts/devvm/nested.sh ./target/debug/athanor-settings-rs
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
  target/release/athanor-shell-rs:/usr/bin/athanor-shell-rs
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
  kickstart pins the guest to the run-id tag it was installed from, and upgrade only
  re-pulls that same tag.
- `layout-acceptance.sh [stage...]` deploys the stage 1c layout crates from
  `.scratch/shell-rig/bin` (build them with `forge/test/shell/rig.sh build-layout`) and runs
  acceptance item 10 of `doc_shell.md` in the VM's session: the first-session default,
  rotation, live presets, a rejected document, a mandatory key added mid-session, memory,
  and the crash loop. Screenshots land in `.scratch/layout-acceptance/`; the script checks
  the panel configuration, so look at them: they are the only check of what the panel draws.
- Settings are in `devvm.env` and are overridden from the environment: `CPUS=4`,
  `MEMORY=8G`, `DISK_GIB=40`, `SSH_PORT`, `ISO_TAG`, `REGISTRY`. State (ISO, disks, logs)
  is in `${XDG_DATA_HOME:-~/.local/share}/athanor-devvm`: about 6 GB of ISO and up to
  `DISK_GIB` of disk.
- Needs `qemu-system-x86_64` with the virtio-gpu-gl device, `qemu-img`, OVMF, `skopeo`,
  `jq`, `mkfs.ext4`.

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
