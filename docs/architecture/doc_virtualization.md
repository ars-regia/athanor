# Athanor Virtual Machines: Windows applications, whole systems and a dedicated GPU

Status: **revision 1 draft, 2026-10-08: the design the maintainer approved section by section in chat on 2026-10-08; text not yet reviewed.** It turns the maintainer's request of 2026-10-08 into a specification: an ordinary person runs Windows easily, its applications appear as windows of the Athanor desktop, other systems install the same way, a second GPU can be given to a virtual machine, and a Windows installed on a disk of its own boots both natively and as a virtual machine. Section 6 records the maintainer's decisions with the options that were weighed. Section 4 lists what must be proven before the first plan is written.

## 1. Context

### 1.1 What binds this document

- `doc_threat_model.md`: unconfined user code is the user (TM1); confined applications are untrusted (TM2); every shipped service sets `NoNewPrivileges=yes` or a capability allow-list (TM8); every polkit action Athanor declares or overrides has a row in TM9. This document adds a guest tier (VZ2) and a TM9 row (VZ6).
- `doc_shell.md`: one crate per program on plain `gtk4-rs`, logic in modules with no GTK type (SH4); every string read from a system file or a device is plain text, truncated, stripped of control and bidirectional characters (SH12); Italian and English from the first commit (SH13).
- `doc_bar.md` and `doc_session_daemons.md`: applications start behind a `wp_security_context_v1` socket in a transient unit of the user manager (BR2, SD8). A window that shows what a guest draws is shown through such a socket (VZ9).
- `doc_kernel_profile.md`: the IOMMU runs in lazy mode by default and `iommu=pt` is rejected (D16, section 6); GPU-in-guest tiers are after 1.0 and "need hardware evidence before they are promised" (D28); dual boot beside a preinstalled Windows ESP must work (D17); 1.0 boots through GRUB on the ostree backend (D6).
- `doc_software.md`: the scope budget (section 9.5, A2-14 (#159)): every new component of our own that does what an upstream project already does carries a written "why not upstream" and an owner before its first plan. Section 2 of this document is that text.
- ADR-0091 (2026-10-08): QEMU (`qemu-system-x86-core`, `qemu-img`, `edk2-ovmf`, `passt`, the display and user-interface modules) ships in the image; `qemu-kvm` and `virt-manager` stay out; `qemu-bridge-helper` loses its setuid bit. This document builds on it.

### 1.2 What ships today

Read on the maintainer's desktop and in the repository on 2026-10-08.

- **Kernel.** The running Azoth configuration has `KVM=m`, `VFIO=m`, `VFIO_PCI=m`, `VFIO_IOMMU_TYPE1=m`, `IOMMUFD=m`, `INTEL_IOMMU=y`, `AMD_IOMMU=y`, `VHOST_NET=m` and `UDMABUF=y`; `forge/specs/azoth/kernel-local` holds no virtualization line, so these come from Fedora's configuration. The command line carries `intel_iommu=on` (`forge/specs/azoth/cmdline`). Nothing in the kernel blocks this document.
- **QEMU** as ADR-0091 lists. No libvirt, no `swtpm`, no FreeRDP, no `virtiofsd` in the image.
- **Firmware.** `edk2-ovmf` ships `OVMF_CODE_4M.secboot.qcow2` with `OVMF_VARS_4M.secboot.qcow2` and the firmware descriptor `30-edk2-ovmf-4m-qcow2-x64-sb-enrolled.json`: Secure Boot with Microsoft's keys enrolled, which libvirt selects by itself.
- **Fedora 43 packages** (repository metadata, installed sizes): `libvirt-daemon-driver-qemu` 11.6.0 (3.0 MB), `libvirt-daemon-common` (0.4 MB), `libvirt-daemon-driver-storage-core` (0.9 MB), `libvirt-daemon-driver-nodedev` (0.8 MB), `libvirt-daemon-log` and `libvirt-daemon-lock` (0.1 MB each), `libvirt-client` (1.0 MB), `swtpm` with `swtpm-tools` (0.3 MB), `osinfo-db` (4.4 MB), `freerdp` 3.31 with `freerdp-libs` (5.5 MB). The dependency closure is measured in S5.
- **The maintainer's tooling** runs QEMU directly: the self-hosted runner (`scripts/runner`) and the development VM (`scripts/devvm`). This document does not change them.

### 1.3 What the maintainer asked for

On 2026-10-08: Windows virtualization made easy for the user, with GPU passthrough; WinBoat evaluated as a candidate; a downloaded or a user-supplied installation image, and other systems too; a Windows that also boots natively, for dual boot. The answers to each question are in section 6.

## 2. Why not upstream

Read in the sources on 2026-10-08.

- **WinBoat** (MIT, v0.9.2 of 2026-08-25, commit `b9a2df2`): the user experience this document wants, Windows applications as windows through FreeRDP RemoteApp, built on a privileged container. Its Podman compose sets `privileged: true` (`src/renderer/data/podman.ts:37`), mounts the whole home into the guest (`:40`), passes `/dev/kvm` and `/dev/bus/usb` (`:43`) and writes a default account `MyWindowsUser` / `MyWindowsPassword` in clear (`:25-26`). By the evaluation of 2026-10-08 (`/var/tmp/athanor-vm-research/winboat.md`, not re-read line by line): QEMU's monitor is reachable unauthenticated on a loopback TCP port, the guest agent runs as SYSTEM, the container image is pinned by tag, Electron runs with `contextIsolation` off, there is no GPU support and no Flatpak. It violates TM8 and VZ2 by construction; patching it means replacing its engine. **Taken: its user experience. Not taken: its code.**
- **GNOME Boxes** uses libvirt in session mode, which gives no per-machine SELinux label and no PCI passthrough; Windows applications as windows are not in its scope. Two tools for the same Windows would split the experience.
- **virt-manager** is an administrator's tool, out of the image by A2-10 and ADR-0091.
- **Quickemu** downloads and runs systems well, with no confinement beyond the user's and no desktop integration.

**Owner:** the maintainer, until the team model of A2-34 assigns one.

## 3. Decisions

**VZ1. libvirt in system mode is the engine.** The image ships libvirt's modular daemons (`virtqemud`, `virtstoraged`, `virtnodedevd`, `virtlogd`, `virtlockd`), socket-activated, each in its upstream SELinux domain. Machines are `qemu:///system` domains, because only system mode gives each machine an SELinux category pair of its own (sVirt), manages `swtpm` per machine, and detaches and reattaches PCI devices. QEMU runs as the `qemu` user, never as root and never as the person.

**VZ2. A guest is hostile.** Every guest, Windows above all, is treated as an attacker that already controls the virtual machine. The properties that hold against it:

- it cannot read or write the person's home except the folders VZ5 shares;
- it cannot reach another machine's disk, memory or TPM state (sVirt categories, VZ1);
- it cannot reach the host's services or the local network unless VZ4 grants it for that machine;
- it cannot reach QEMU's monitor, the person's keyring or the RDP credentials;
- what it draws, names and titles reaches the desktop as untrusted content (VZ9).

`doc_threat_model.md` gains this as a tier of its own (section 7).

**VZ3. Who controls a machine.** libvirt checks every API call against polkit (`access_drivers = [ "polkit" ]` in `virtqemud.conf`). An Athanor rule lets a process of the active local session manage a domain only when the domain carries that person's owner mark (the uid in the domain name prefix and in its metadata); every other domain is `no`. No `libvirt` group is shipped or used: membership of it equals root. Giving a machine a physical device (a GPU, VZ11; a disk, VZ13) asks for an administrator's password once, when the device is assigned.

**VZ4. Network.** A machine has Internet through libvirt's `passt` backend: no bridge, no `dnsmasq`, no firewall rules on the host. The host's own services and the local network are not reachable from the guest by default. "Local network access" is a per-machine setting. `qemu-bridge-helper` keeps no setuid bit (ADR-0091).

**VZ5. Files.** Each machine has one shared folder in the person's home, `~/Machines/<name>/Shared` (localised name), shown in the file manager, mounted in the guest through `virtiofs`; `virtiofsd` runs sandboxed per machine. Further folders are added one at a time in the application. The home is never shared whole.

**VZ6. Secrets.** The guest account password and the agent's secret are generated per machine (32 random bytes, base64) and stored in the person's keyring through the Secret Service; never in a file in clear, never on a command line (FreeRDP reads them from a file descriptor or standard input). The TPM state is encrypted by libvirt with a secret of its own per machine. TM9 gains the row of VZ3's rule.

**VZ7. The catalogue and verified downloads.** A catalogue file in the repository, shipped in the image under `/usr/share/athanor/machines/catalog.toml`, lists each system: where to download it, how to verify it (a checksum the vendor publishes, read over HTTPS from the vendor, or an OpenPGP signature checked against a key pinned in the image), how to install it unattended, and the machine's defaults. Requirements come from `osinfo-db`. A download that fails verification stops the installation and is deleted. A person's own image is accepted: the application shows its SHA-256 and says that it is not verified. The catalogue changes only with an image update, so a changed URL or key is a reviewed commit. Windows comes from Microsoft's official download service; Fedora, Ubuntu, Debian and others from their own mirrors and signatures. The virtio drivers for Windows come from the `virtio-win` RPM of Fedora's `virtio-win` repository, verified with its key pinned in the image, and the driver ISO is taken from it.

**VZ8. The application, working title Machines.** One crate, `gtk4-rs` and libadwaita, the logic in modules with no GTK type (SH4), shipped as an RPM of the forge (it needs the system libvirt socket, which a Flatpak would get only as a hole in its sandbox). It talks to libvirt through `libvirt-client` and does:

- **New machine.** Pick a system from the catalogue or "I have an image"; the hardware check (virtualization enabled in firmware, memory, disk space) explained in plain words; proposed resources, editable under Advanced: memory half the host's, at most 16 GiB; half the cores; a growable disk, 64 GiB for Windows; download with resume, verification, unattended installation, a notification when done.
- **Windows unattended.** A local account (no Microsoft account), the virtio drivers and the agent of VZ10 installed, hibernation and Fast Startup off (VZ13 needs them off), the account password of VZ6.
- **The machine list.** Start, stop, resources, snapshots, delete. Delete removes the disk, the TPM state, the secrets and the launcher entries, after the person types the machine's name.
- **Whole desktop.** Windows through RDP in a window or fullscreen, with dynamic resolution, clipboard and audio; other systems through SPICE (`spice-gtk`).
- **Lifecycle.** When the last window of a machine closes, the machine is saved to disk after 10 minutes (adjustable); a host shutdown saves running machines in order, it does not kill them. The application offers a snapshot before a Windows feature update.

**VZ9. Windows applications as windows.** The agent of VZ10 lists the applications installed in Windows with their names and icons. The person picks which ones appear in the launcher; each gets a `.desktop` entry in the person's home with its real name, its icon and a "Windows" badge. Opening one starts the machine if needed ("Starting Windows…") and opens the application through FreeRDP 3 in RemoteApp mode. FreeRDP is started as a `confined` application of `doc_session_daemons.md` SD8: behind a security-context socket, so its windows are tier 2 and carry the trusted-path border of `doc_lock_and_prompts.md` LP13 when it ships. Names, titles and icons from the guest are untrusted strings (SH12): plain text, truncated, stripped of control and bidirectional characters; icons are decoded by a size-bounded loader and re-encoded.

**VZ10. The agent and the host channel.** A small Windows service of our own, written in Rust and cross-compiled in the pipeline, shipped in the image and installed by the unattended setup. It lists applications, reports state and nothing else: it starts no program on the host's request beyond RemoteApp's own launch. It speaks only over `virtio-vsock`, never over the network, and every request carries the per-machine secret of VZ6. RDP itself does not listen on a network the host or the LAN can reach: the host reaches it through a channel only the machine's owner can open (a vsock forward or a Unix socket owned by the person; S3 fixes which).

**VZ11. A dedicated GPU: two GPUs.** The setting "Dedicated GPU" appears for a machine only when the hardware holds it: the IOMMU is on; a second GPU exists that does not drive the Athanor session; its IOMMU group holds only that GPU and its audio function (PCI bridges aside). Otherwise the application says why ("one GPU only", "the GPU shares its group with the USB controller"). The ACS override patch is never applied. No `iommu=pt`: it removes DMA protection from the host's devices (D16). When the machine starts, libvirt (`managed='yes'`) detaches the GPU from its host driver to `vfio-pci` and gives it back when the machine stops; this needs the GPU free of the compositor and of the NVIDIA driver (S2). If S2 shows dynamic detaching does not hold, the fallback is a GPU reserved for machines from boot, bound to `vfio-pci` early, with no privileged helper of our own; the plan fixes the mechanism from S2's evidence. The GPU drives its own monitor or monitor input; keyboard and mouse move to the guest and back with both Ctrl keys, or a whole USB controller goes to the guest. The application warns that some games with anti-cheat refuse to run in a virtual machine, and it keeps a compatibility list of GPUs that do not reset cleanly.

**VZ12. Secure Boot and TPM are real.** Every Windows machine boots the Secure Boot firmware with Microsoft's keys enrolled and a `swtpm` TPM 2.0, so Windows 11's requirements are met, never bypassed.

**VZ13. Windows on its own disk, natively and as a machine.** A Windows installed on a physical disk of its own boots from the firmware's boot menu as a normal dual boot, and Athanor can start the same installation as a machine by giving QEMU that whole disk. The application can also install Windows straight onto such a disk.

- **Whole disks only.** The application refuses a disk that holds any Athanor partition, the ESP or XBOOTLDR that Athanor boots from, or any mounted filesystem. Repartitioning a shared disk is out of revision 1.
- **Assignment** asks for an administrator's password once (VZ3); sVirt labels the block device for that machine only while it runs.
- **Both storage drivers.** The Windows installation carries the virtio drivers and its native NVMe or AHCI driver, so it boots in both places.
- **Never two at once, never half-asleep.** Hibernation and Fast Startup stay off (VZ8): a Windows hibernated natively and then started as a machine corrupts its filesystem. Before it starts such a machine, Athanor reads the Windows volume read-only, without mounting it, and refuses to start when the volume is hibernated or marked dirty (S6 fixes how); the agent cannot do this, since it runs only once the machine has started.
- **Stated to the person:** Windows activation is bound to the hardware and may ask to be renewed when it moves between the machine and the PC; BitLocker sealed to a TPM asks for its recovery key at each move (the virtual TPM is not the physical one), so the application recommends BitLocker off or a password protector; a Windows booted natively controls the whole PC, cannot read Athanor's encrypted disk, and can write the ESP, which Secure Boot and Athanor's measured boot detect (`doc_kernel_profile.md` section 9).
- **The boot menu.** Revision 1 uses the firmware's boot menu. An entry in Athanor's own menu waits for the boot chain of D6.

## 4. Spikes

Each spike answers one question before the plan that depends on it. Evidence goes in the plan.

- **S1. RemoteApp under cosmic-comp.** FreeRDP 3's SDL client in RemoteApp mode on cosmic-comp, native Wayland: window placement, resize, focus, popups, the taskbar entry. Fallback: XWayland for the RDP client only.
- **S2. Dynamic GPU detaching.** On the maintainer's desktop or laptop with a second GPU: does `managed='yes'` detach and reattach while cosmic-comp runs, on the default and the NVIDIA images; how cosmic-comp is told to leave the second GPU alone.
- **S3. The RDP channel.** Which owner-only channel carries RDP (vsock forward or Unix socket) and how FreeRDP connects to it.
- **S4. `passt` in system mode.** That libvirt 11.6's `passt` backend works for `qemu:///system` domains with sVirt, and that it blocks the host and the LAN by default.
- **S5. Size. Answered 2026-10-08:** `dnf install --assumeno` without weak dependencies, over the 2026-10-08 system image, for the libvirt pieces of VZ1 with `libvirt-daemon-proxy`, `swtpm`, `swtpm-tools`, `osinfo-db`, `libosinfo`, `freerdp`, `virtiofsd` and `spice-gtk3`: 29 packages, 8 MiB to download, 24 MiB installed. The application, the catalogue and the agent come on top.
- **S6. A clean Windows volume.** How the host reads, without mounting and with no write, that an NTFS volume on an assigned disk is neither hibernated nor marked dirty, and who reads it (the application through a privileged read, or libvirt's hook).

## 5. Placement and packages

- **In the image** (signed, read-only under `/usr`): the libvirt modular daemons and client of VZ1, `swtpm` and `swtpm-tools`, `osinfo-db`, `freerdp`, `virtiofsd`, `spice-gtk`, the Machines application, the catalogue, the Windows agent, the polkit rule. 24 MiB of packages over the QEMU of ADR-0091 (S5), plus the application, the catalogue and the agent.
- **Downloaded on first use, always verified (VZ7):** installation images and the `virtio-win` RPM.
- **Data.** Machine disks live under `/var/lib/libvirt/images/<uid>/`, labelled per machine; the shared folders live in the home. Whether machine disks are excluded from Athanor's backup by default is open (section 9).

## 6. Decisions of the maintainer (2026-10-08)

1. **Purpose.** Options: applications and games, applications only, a whole Windows in a window, games first. **Chosen: applications and games.** A base tier for everyone (Windows applications as windows, or the whole desktop) and an advanced tier with a dedicated GPU on suitable hardware.
2. **Where systems come from.** Options: a guided download, the person's image only, other systems too. **Chosen: a guided download or the person's image, and other systems too.**
3. **GPUs.** Options: two GPUs; a single GPU too; two GPUs plus shared acceleration. **Chosen: two GPUs now.** Later, in a new revision: a single GPU (the session gives the GPU up while the machine runs), and laptops with two GPUs and one display output, where the second GPU accelerates the guest and the picture reaches the screen through the first (Looking Glass or an equivalent).
4. **Files.** Options: one dedicated folder, the person's folders, none by default. **Chosen: one dedicated folder,** more added one at a time.
5. **Approach.** Options: A, libvirt with our application; B, QEMU directly with our application; C, GNOME Boxes with an applications layer. **Chosen: A.** B would rewrite sVirt, the TPM lifecycle and PCI detaching by hand; C has no passthrough and splits the experience.
6. **Native boot.** Options: Windows on its own disk, also as a machine; the machine's VHDX booted natively (needs NTFS and some editions); two separate installations. **Chosen: its own disk, also as a machine** (VZ13).
7. **Sections 1 to 5 of the chat design**, approved one by one, with `iommu=pt` left out on purpose (VZ11).

## 7. Changes to other documents

- `doc_threat_model.md`: a guest tier with the properties of VZ2; a TM9 row for VZ3's rule (active session: `yes` for the person's own domains, `no` otherwise; `auth_admin` for assigning a physical device).
- `doc_kernel_profile.md` D28: the "second GPU in its own IOMMU group" tier is specified here, gated on S2's hardware evidence.
- `doc_software.md`: Machines joins the own applications, with section 2 of this document as its "why not upstream".
- ADR-0091: this document builds on it; `qemu-kvm` and `virt-manager` stay out.
- `scripts/verify.py`: `qemu-bridge-helper` carries no setuid bit; no `libvirt` group is created; the polkit rule is checked with the others.

## 8. Acceptance and stages

Each stage has its gate; the next starts when the gate is green.

- **V0. Spikes** S1 to S4 and S6 (S5 is answered) answered, with evidence.
- **V1. Engine and base application.** Catalogue, verified download, unattended installation, a whole Windows or Linux desktop in a window, the shared folder. Gate: on the self-hosted runner with nested KVM, a Fedora machine is created and installed unattended, boots, writes a file into the shared folder that the host reads; unit tests prove that a wrong checksum stops the installation, that the polkit rule denies another person's domain, and that a new machine reaches neither the host nor the LAN. On the maintainer's desktop, Windows 11 installs unattended with Secure Boot and TPM on.
- **V2. Windows applications as windows.** Agent over vsock, RemoteApp, launcher entries, the save after the last window. Gate: on the maintainer's desktop, a Windows application picked in Machines opens from the launcher as a window, with clipboard and audio, and the machine is saved 10 minutes after its last window closes.
- **V3. Dedicated GPU.** Gate: on hardware with two GPUs, a Windows machine runs a 3D application on the dedicated GPU, and the GPU returns to the host when the machine stops; on hardware without, the application says why the setting is absent.
- **V4. Windows on its own disk.** Gate: on a test machine with a second disk, Windows installed by Machines boots natively from the firmware menu and as a machine, and the application refuses a disk that holds an Athanor partition or was not shut down cleanly.
- **Later, a new revision:** a single GPU, Looking Glass, laptops with one display output, shared 3D acceleration, an entry in Athanor's boot menu, repartitioning a shared disk.

## 9. Open for the maintainer's review

1. **The application's name.** Working title Machines (Italian: Macchine); GNOME Boxes uses "Boxes".
2. **Backup of machine disks.** Recommendation: excluded by default, because a Windows disk is tens of GiB and changes on every boot; the person can include a machine in the backup from its settings.
3. **The first catalogue.** Recommendation: Windows 11, Fedora Workstation, Ubuntu LTS, Debian; others when each has a verified download and an unattended installation tested in the pipeline.
