# Athanor OS: Kernel and Platform Profile

Status: **approved by the maintainer (gate P0), revision 15 (2026-09-14).** Revision 2 folded in a
second full audit of the platform; revisions 3 to 15 fold in approval-gate verification
loops that check every statement against kernel v7.2, systemd v258–v262, shim, bootc, the
Fedora targeted SELinux policy and the running system. The maintainer approved the policy
decisions introduced by the verifications on 2026-09-14 and chose the firmware policy of
D48 on the same day. The specification passed gate P0 on 2026-09-14, with the mechanism of
the guided reseal (D42) left open for P4b.

This document is the definitive profile of the Athanor kernel and of the platform layer
that makes its guarantees real: what the kernel is, how it boots, how its integrity is
proven, how machine roles compose, and how every property is verified. It supersedes
sections 4 (config), 6 (signing and boot chain), 11 (outside the kernel) and 13
(maintainer decisions) of [doc_kernel_build.md](doc_kernel_build.md), which remains the
specification of *how the kernel is built, pinned, published and maintained*.

The profile is definitive in one precise sense: every property below is recorded with its
rationale, in section 2 or in its own section, every build and every installed machine is checked against
it, and it changes only through a new written decision. Upstream moves, so the profile is
re-verified at every kernel bump; it is not frozen. Kernel statements refer to Linux 7.2,
the series the next bump moves to (D37).

**What exists today.** Little of sections 5 to 13 is implemented: the running
system still carries the previous configuration (section 14). What is real is the Azoth
kernel pipeline (Fedora and CachyOS merge, clang, kCFI, Rust, BORE, boot matrix), the
split of the Secure Boot and module signing keys with the module certificate compiled
into the kernel, module verification restricted to the kernel's own keyrings
(`forge/specs/azoth/patches/redhat/0001-module-verify-signatures-with-trusted-keyrings-only.patch`,
on the default branch `iso-v0` and not yet deployed), and the
self-hosted KVM runner. Section 15 turns the rest into gated blocks.

## 1. Goals and constraints

- A single kernel binary for every machine, matching or exceeding other Linux
  distributions, macOS and Windows on integrity wherever Linux allows; remaining gaps are
  stated (section 10).
- Full control of the kernel as a platform for purpose-built Athanor applications, built
  on stable kernel interfaces rather than a deeper fork. Athanor patches are small, each
  one explains why, and each one is proposed upstream.
- Maximum technical level with minimum maintenance: upstream mechanisms over custom code,
  every gate fails loudly, the system maintains itself. The scope of each release is
  sized for one maintainer (D36).
- Designed for everyone: features are defined by capability tiers detected at runtime,
  never by one hardware configuration. The userland requires x86-64-v3 and UEFI; the
  kernel itself stays bootable on any x86-64 CPU so that an unsupported machine gets a
  clear message (D14).
- The integrity chain (UEFI Secure Boot, TPM 2.0) is optional per machine, with a
  declared degraded mode (section 4).
- Updates never force a reboot: the user decides when a system update takes effect
  (section 8).
- Roles: desktop, laptop, mesh; a machine may hold several. The mesh is personal: it
  joins the devices of one owner, and it comes after release 1.0 (D26, D38).
- Areas that require explicit maintainer approval before any code change remain so:
  the Gatekeeper (`forge/specs/athanor-gatekeeper-rs`), attestation
  (`system/confidential_computing/athanor-attestation`) and
  `system/athanor-bus-api/src/polkit.rs`. This document fixes requirements and kernel
  primitives for them, not their code.

**Terms.** *Image*: the signed operating system (`/usr` and its UKI) of one version.
*Slot*: A/B storage for an image (dm-verity option of D6 only). *Keyslot*: a LUKS2 key slot, unrelated to image slots. *Role*: a runtime profile
activated by a signed addon; the *interactive roles* are desktop and laptop. *Mesh role*:
the role that makes a machine a mesh-only host or, combined with an interactive role, an
interactive mesh host. *Mesh host*: an attested machine that holds data or runs workloads
for the owner's other devices. *Client*: a device that uses the mesh without being a
host.

## 2. Decision record

States: **final**; **provisional** (a default, closed by the benchmarks of section 13);
**open** (closed by the named spike, block or maintainer decision); **after 1.0** (kept as
the target, outside the release 1.0 scope of D36); **revised** (amended by the named
decision); **superseded** (replaced by the named decision). A final decision may name the
release or block that delivers it, or the S1 outcome it depends on.

| ID | Decision | Rationale | State |
| --- | --- | --- | --- |
| D1 | One kernel binary; roles are runtime profiles | one build, one signature, simple attestation | final |
| D2 | Roles compose; a machine holds zero or more. A machine with no role runs the base profile, with the desktop IPE class under the dm-verity option (D6) | a workstation can also be a mesh host | final; combinations of an interactive role with the mesh role from the mesh delivery (D38) |
| D3 | Userland baseline x86-64-v3 and UEFI, enforced by the installer; integrity chain optional with declared degraded mode | matches the v3 userland; keeps machines without TPM or Secure Boot installable | final; installer checks in P4b |
| D4 | Rust enabled; performance from AutoFDO; ThinLTO re-checked at every bump; Propeller deferred until profiling runs automatically on more than one CPU vendor | `RUST` still depends on `!DEBUG_INFO_BTF \|\| (PAHOLE_HAS_LANG_EXCLUDE && !LTO)`; AutoFDO and Propeller do not require LTO; a Propeller profile is bound to one binary and one test machine's workload | final |
| D5 | BORE is the base scheduler; sched_ext only through `scx_loader` as a role setting; `athanor-ebpf-sched` retired as a scheduler. If the BORE patch does not apply to a bump required by D37, the kernel ships on plain EEVDF rather than waiting | one owner; sched_ext falls back to the fair class; an out-of-tree patch must never delay a security bump | provisional (BORE against plain EEVDF in P7) |
| D6 | The verified image and A/B update mechanism: either `/usr` on dm-verity with a signed root hash, A/B slots with `systemd-sysupdate` and UKI boot with systemd-boot, or bootc with sealed composefs and UKI | the first option leaves `/etc` to be designed, depends on a `systemd-sysupdate` marked experimental again in systemd 262, and makes Athanor maintain its own update system; bootc's sealed composefs backend is close to stable and handles `/etc` per deployment, but has no boot counting today and cannot be covered by IPE | open (spike S1 on Fedora 45, no time box) |
| D7 | Delta updates by content-defined chunking, verified by the image signature | small downloads without weakening integrity | after 1.0 (D36); if S1 selects bootc, OCI layer pulls provide partial downloads first |
| D8 | IPE is the in-kernel enforcer for code integrity on dm-verity images. Class policies carry versions `class.release.0` ordered by class, so a running machine can only move to an equal or stronger class; a new release of a class replaces the previous one through IPE's update operation | on activation IPE refuses only a lower policy version and does not compare policy names, so without the ordering root could activate a weaker signed policy; IPE's documentation positions it for fixed-function devices, so its full default-deny use is the mesh class | final; applies if S1 selects dm-verity (D6) |
| D9 | Role content ships inside the signed image; the active role set is a signed UKI addon per role | role content is verified with the image on every read, instead of living in extension images on the ESP; addons are verified through shim and measured into PCR 12 | final; revisited if S1 selects bootc |
| D10 | Composition follows NixOS's type-based merge with explicit priorities; unlike NixOS, lists merge by union (NixOS concatenates), numeric settings may declare an ordering (`max`, `min`), and base settings may be locked; any other conflict is a build error | explicit, no silent resolution, proven model | final |
| D11 | On a desktop or laptop, the mesh role runs inside a MicroVM | the services that matter (backup, inference, remote sessions) run on the host anyway, and on consumer CPUs without SEV-SNP or TDX the VM adds no trust | superseded by D38 |
| D12 | The mesh role requires attested mode | a host that keeps its key but runs altered code must not stay a host | revised by D38: required for mesh hosts only |
| D13 | Preemption `lazy` as the base (the upstream x86 default, also Fedora's); `full` through a role addon (`preempt=full`, possible with `PREEMPT_DYNAMIC`) only if P7 measures a gain | x86 offers only FULL and LAZY; LAZY delays preemption of normal tasks by at most one tick; a boot parameter belongs to a role, not to the build | provisional |
| D14 | Kernel built for the x86-64 baseline (`X86_64_VERSION=1`, an option of the CachyOS base); x86-64-v3 is enforced by the installer and by `athanor-cpu-check` in the initramfs, which stops the boot with a clear message on an unsupported CPU. `athanor-cpu-check` is built for the x86-64 baseline, overriding the v3 `%_optflags` and `%rustflags` of the Forge build, and runs before any other Athanor-built binary and before the passphrase prompt | the kernel is compiled with `-mno-sse -mno-avx` (`arch/x86/Makefile`), so a v3 build gains only integer extensions, while a v1 kernel and a v1 checker can still explain why the machine is unsupported instead of crashing | final |
| D15 | Swap on zram (zstd), zswap off; writeback of idle or incompressible pages to an encrypted block device evaluated for machines with 8 GiB or less | no double compression; swap on LUKS is encrypted, so disk-backed writeback is excluded only on wear and latency grounds, which P7 measures; zram writeback requires a block device, not a file | provisional (swappiness and writeback in P7) |
| D16 | IOMMU in lazy mode by default; external ports are forced into strict DMA domains with bounce buffering by the kernel when firmware marks them untrusted, and `athanor-profile-check` reports that marking; `iommu.strict=1` is available per role | the kernel already isolates untrusted devices; global strict only costs throughput on internal NVMe and NICs | provisional |
| D17 | Minimal generic initramfs (never host-only): storage (`nvme`, `vmd`, `sdhci_pci`, `sdhci_acpi` and `mmc_block`, `ufshcd_pci`, `usb_storage` and `uas`, `ahci_platform`, `mpt3sas`, `megaraid_sas`, `virtio_scsi`, `hv_storvsc`, `pci_hyperv`, `vmw_pvscsi`, `mptspi`, `mptsas`, `sym53c8xx`, `xen_blkfront`; AHCI and `virtio_blk` are built in), `vfat` with `nls_cp437` and `nls_iso8859_1` (the pcrlock policy file of D42 is read from the ESP or XBOOTLDR), `dm_crypt`, `tpm_tis_spi` and `tpm_tis_i2c` (TPMs on SPI or I2C), input for the LUKS passphrase (`i2c_hid_acpi` with the Intel `pinctrl_*` drivers, `intel_quicki2c`, `intel_quickspi`, `applespi` with `spi_pxa2xx_platform`, `surface_aggregator_registry` and `surface_aggregator_hub` with `surface_hid` and `surface_kbd`, `hyperv_keyboard`; USB HID, i8042, AMD pinctrl and DesignWare I2C are built in), `thunderbolt`, which keeps the tunnels the firmware created for docks (docks that need user authorisation rely on the firmware pre-boot option, recorded in the hardware matrix); no GPU driver, so the passphrase prompt uses the firmware framebuffer (`simpledrm`). ESP of 2 GiB when Athanor owns the disk; otherwise an XBOOTLDR partition (VFAT, 2 GiB) next to the existing ESP, which then holds only shim, systemd-boot and MokManager | today's initramfs is 102 MB; dual boot with a preinstalled Windows ESP of 100–260 MB must work; a missing storage or keyboard driver makes a machine unbootable; the built-in drivers the initramfs relies on (`SATA_AHCI`, `ATA_PIIX`, `BLK_DEV_SD`, `VIRTIO_PCI`, `VIRTIO_BLK`, `USB_XHCI_HCD`, `USB_XHCI_PCI`, `USB_EHCI_HCD`, `USB_EHCI_PCI`, `USB_OHCI_HCD`, `USB_OHCI_HCD_PCI`, `USB_UHCI_HCD`, `USB_HID`, `HID_GENERIC`, `SERIO_I8042`, `KEYBOARD_ATKBD`, `MFD_INTEL_LPSS_PCI` and `MFD_INTEL_LPSS_ACPI`, `PINCTRL_AMD`, `I2C_DESIGNWARE_PLATFORM`, `SERIAL_8250_DW`, `SERIAL_DEV_BUS`, `TCG_TIS`, `TCG_CRB`, `BTRFS_FS`, `DRM_SIMPLEDRM`) are set explicitly in `kernel-local`, so `check_delta` enforces them | final |
| D18 | Laptop is an autonomous role; desktop and laptop share an `interactive` fragment of the manifest | a laptop without the desktop role keeps 32-bit, gaming and interactive settings, without duplicated definitions | final |
| D19 | `PANIC_ON_OOPS` stays off in the build; `kernel.panic_on_oops=1` and `kernel.warn_limit=100` on mesh-only hosts; `kernel.panic_on_oops=0` with `kernel.oops_limit=100` and `kernel.warn_limit=0` on interactive roles. Crash evidence comes from EFI pstore, enabled in the build and emptied by `systemd-pstore` | with `PANIC_ON_OOPS=y` the first oops panics before `oops_limit` is consulted, so a desktop would reboot and lose work at the first driver oops; Fedora disables EFI pstore by default, and without a backend a panic leaves no record | final |
| D20 | SELinux `DEVELOP` and `BOOTPARAM` are switched off only after zero AVC denials in Athanor domains in acceptance; denials from Fedora packages are triaged and documented, and none may affect boot, login or security. Recovery from a denial that breaks the system after boot is the previous image in the boot menu, not `enforcing=0` | a gate that does not depend on upstream policy bugs, and a recovery path that survives the switch | final |
| D21 | Role addons are built and signed in CI, shipped inside the image and published with it in the same signed source, and installed per UKI in `<uki>.efi.extra.d/` (named without the boot-counting suffix) on the partition that holds the UKI, before the UKI itself | the stub reads global addons only from the volume of the loaded UKI, and a global addon without `.uname` would be used by every UKI, so a fallback boot would run the previous image with the new image's parameters; no second distribution channel | final; revisited with D9 if S1 selects bootc |
| D22 | Hosting of the delta chunk store | chosen from measured delta sizes, chunk counts and expected traffic | after 1.0 |
| D23 | Desktop class: no `noexec` on user-writable locations; code in the home runs and is measured by IMA. Protection against downloaded executables is a quarantine prompt in the launcher and file manager, based on the `user.xdg.origin.url` attribute, specified in the desktop specification after 1.0 | `noexec` on `/tmp`, removable media or `~/Downloads` is bypassed by passing the file to an interpreter and breaks `go test`, Java native libraries, PyInstaller and .NET single-file apps, Steam libraries on external drives and AppImages; a prompt outside the kernel is bypassable from a terminal, which is a stated residual risk | final |
| D24 | SELinux denies `execmem`, and `execmod` on files of the image, to system domains and to Athanor services, each of which runs in its own declared domain and never in `unconfined_service_t`; exceptions are declared; user applications stay in `unconfined_t` as in Fedora. For a Fedora policy module that grants `execmem`, the module is patched or disabled, recorded per module | restricting user domains would break Electron, Java, .NET, Python ctypes and emulators, not only browsers; a local CIL `deny` rule could remove Fedora's grants but would tie Athanor to Fedora's internal type names | final |
| D25 | The Fedora 45 rebase starts on the beta as soon as P3 is green; the image that reinstalls the maintainer's desktop is built only on the final release (target 2026-10-20); any block that needs a systemd 262 feature waits for systemd 262 final in Fedora 45 updates | problems surface early, production waits for supported releases | final |
| D26 | The mesh is personal: its members are the devices of one owner. It provides a private network between them, synchronisation and backup, compute sharing and remote applications; it is not an update channel | the purposes the maintainer set; updates keep one signed source and one verification path | final; delivery after 1.0 (D38) |
| D27 | The mesh subsystem has its own specification, a rewrite of `doc_cloud_mesh.md`; this profile holds only its kernel and platform requirements | protocol, identity, discovery and scheduling are not kernel decisions | final |
| D28 | Compute sharing by capability tiers: CPU workloads in MicroVMs; GPU inference through a signed host service; a GPU inside a guest with SR-IOV, a second GPU in its own IOMMU group, or virtio-gpu Venus for Linux guests | works on any machine; extra hardware unlocks more. Today SR-IOV exists on consumer hardware only for some Intel GPUs, and Venus only with QEMU, crosvm or libkrun | after 1.0; the GPU-in-guest tiers need hardware evidence before they are promised |
| D29 | Remote applications from interactive mesh hosts, by tiers: a session on a virtual output with hardware encoding, applications of a Windows VM forwarded one by one, a 3D-accelerated VM where the hardware allows; the local session is never closed | usable across GPU vendors; the owner keeps working locally | after 1.0; blocked until the session compositor provides virtual outputs (COSMIC does not today) |
| D30 | Updates ship as a base image plus signed layers that apply without a reboot | most changes could apply without rebooting | after 1.0; redesign required (section 8) |
| D31 | Update classes: A applications, no interruption; B service layers, applied by restarting only their services; C the desktop layer, at the end of the session or at a soft reboot the user chooses; D the base, at a full reboot the user chooses and never forced; mesh-only hosts reboot in a maintenance window set by the owner | no forced reboots; each change costs only the interruption it needs | final; classes A and D in 1.0, B and C after 1.0 |
| D32 | A layer is trusted only when the kernel verifies its signature: the UKI command line sets `systemd.allow_userspace_verity=0`, and image policies are passed with `--image-policy=` and `ExtensionImagePolicy=` from units shipped in `/usr` | systemd's default extension image policy accepts unprotected images, userspace verity trusts certificates in `/etc/verity.d`, and IPE sees `dmverity_signature=TRUE` only for signatures the kernel verified; extension images placed on the ESP reach the initrd through systemd-stub and pass the strict signed-verity policy, so the layer design pins initrd extensions to the booted base or disables `systemd-sysext` in the initrd | after 1.0 |
| D33 | Channels stable and beta; a release is promoted from beta to stable as the same signed artefacts | stable runs, bit for bit, what beta tested | after 1.0 |
| D34 | Gradual rollout without telemetry: each machine derives its rollout day locally, a signed stop manifest halts a release, security releases skip the window; the download server's log retention is set by the update specification after 1.0 | nothing leaves the machine by design; the server still sees requests | after 1.0 |
| D35 | Rollback returns the base and every layer to the versions that belong together | a failure returns to a known pair, never to a mix; layers are selected by the booted base, not by the newest version | after 1.0 |
| D36 | Release 1.0 scope: verified image with fallback to the previous version (D6), boot counting and automatic fallback, full-image system updates that take effect only after the user confirms (except, with `systemd-sysupdate`, when firmware loses `LoaderEntryPreferred`, a stated residual risk of section 8), applications through Flatpak. With the dm-verity option, IPE enforces the boot policy and the desktop class, whose `EXECUTE` table defaults to allow and whose `KMODULE` and `FIRMWARE` tables default to deny (D48); with bootc, module and firmware integrity rely on module signatures and the measures S1 decides. The mesh IPE class policy is built and tested in P6, but the mesh role is not offered before the mesh delivery. Deltas, layers, classes B and C, channels and gradual rollout follow in releases 1.1 and 1.2, the mesh after them | the smallest release that is robust for everyone; the second platform audit (revision 2) found the layer and rollout design unsafe as written | final |
| D37 | Kernel series: the stable channel, with a mandatory bump when the pinned series reaches end of life on kernel.org; the bump bot moves the kernel off a series that reaches end of life and fails while no Fedora/CachyOS pair can move it, and `KERNEL_CHANNEL=lts` is the maintainer's fallback when no stable pair exists | a series without fixes must never stay pinned in silence, as 7.1 did after 2026-09-02 | final |
| D38 | The mesh has two tiers: attested mesh hosts (backup, compute, remote sessions) and non-attested clients (phones, other operating systems, degraded machines) with limited keys. It composes upstream components (kernel WireGuard with an existing coordination server, Syncthing-class sync, btrfs send and receive) rather than new protocol code. An interactive mesh host enforces the desktop class (with the dm-verity option), so its attestation proves the boot state, not runtime code integrity. On desktops and laptops the mesh services run on the host, never in a MicroVM (D11 superseded); only workloads received from other devices run in MicroVMs (D28). No mesh code with placeholder cryptography or attestation ships | mandatory attestation for every device excludes phones and most ordinary machines; the existing mesh and hypervisor crates contain placeholder PQC keys and file-existence attestation | final; delivery after 1.0 |
| D39 | `/etc` is part of the update design. S1 compares factory defaults in `/usr` with `/etc` as a local overlay (`tmpfiles` from `/usr/share/factory`), a per-slot `/etc`, and bootc's per-deployment `/etc` with a three-way merge, judged on user and group databases, `machine-id`, NetworkManager connections and the SELinux policy store | with A/B `/usr` and a shared `/etc`, a rollback would keep the newer configuration and SELinux policy | open (spike S1) |
| D40 | Keyrings: dm-verity root hashes and IPE policies are trusted only from the builtin keyring, and Athanor policies that trust signed BPF programs require it (the kernel itself accepts unsigned programs and caller keyrings, section 5); modules from the builtin and secondary keyrings; `SECONDARY_TRUSTED_KEYRING_SIGNED_BY_BUILTIN=y`. Attested mode requires that `.machine` and `.secondary_trusted_keys` hold exactly the expected certificates. A user who needs modules outside the image enrols a CA certificate (CA=true, keyCertSign, no digitalSignature) in MokList with physical presence, trusts it for the machine keyring with `mokutil --trust-mok`, signs the modules directly with that CA key, and selects the `user-modules` IPE variant with its signed addon (because addons are measured only into PCR 12, which the TPM policy excludes, a unit in the signed initramfs, with `DefaultDependencies=no`, `Wants=` and `Before=cryptsetup-pre.target`, `ConditionSecurity=measured-os` (a machine without a usable TPM has no TPM keyslot to invalidate), `After=tpm2.target`, `systemd-pcrphase-initrd.service` and `systemd-tpm2-setup-early.service`, and a failure that stops the boot, runs `systemd-pcrextend` with a fixed word when the authenticated command line selects the `user-modules` variant, so the signed PCR 11 policy no longer matches and every TPM keyslot bound to it, TPM-only and TPM plus PIN alike, fails; `athanor-role` replaces the TPM keyslots with the passphrase, D42), which is measured into PCR 12 and takes effect at the next boot; the machine is then reported as not attested, `mokutil --untrust-mok` closes the path, and the boot matrix proves the path (section 12 item 2) | the platform keyring (UEFI db, non-CA MOKs) must not authorise code or policy; with `CA_MACHINE_KEYRING_MAX` only such CAs reach the secondary keyring, leaf certificates they issue cannot be added under `SIGNED_BY_BUILTIN`, and `sign-file` embeds no chain | final; the `user-modules` IPE variant applies with the dm-verity option (D6) |
| D41 | Anti-rollback: every system update is described by a manifest signed with the integrity key (image digest, expiry, minimum version), checked before the update mechanism installs anything. Boot-time downgrade is limited by revoking UKIs older than the fallback version through MokListX hashes, which the owner confirms in MokManager and which require a pcrlock update for PCR 14, or by rotating the Secure Boot key; until then a validly signed older UKI remains bootable, a stated residual risk. A revoked integrity certificate is enforced only by kernels built after the revocation. systemd-boot carries an SBAT section because shim loads it directly; UKIs and addons carry one so that shim checks it where present | with Fedora's shim only shim publishes SBAT levels; MokListX can only be written through MokManager at the console; an older UKI with a valid signature boots its own kernel, which still trusts old images, and a signed PCR policy never expires | final |
| D42 | LUKS: TPM-only unlock only in attested mode, offered and never applied automatically; a degraded machine uses TPM plus PIN, or a passphrase. PCR 7 and PCR 14 are bound through a policy built with `systemd-pcrlock make-policy --pcr=7 --pcr=14 --strict=yes` and enrolled on the machine itself (the pcrlock policy is computed from the machine's own event log, and together with the signed PCR 11 policy it forms a two-part TPM secret that cannot be prepared offline); because `systemd.import_credentials=no` stops the import of the `pcrlock` credential, a unit in the signed initramfs ordered before `cryptsetup-pre.target` copies the policy file from the partition that holds the UKI to `/run/systemd/pcrlock.json`, which `systemd-cryptsetup` reads first and which grants nothing by itself; PCR 14 uses Athanor-generated `lock-raw` components built from the data shim mirrors and logs, each only when present under `/sys/firmware/efi/mok-variables/` (`MokListRT` and `MokListXRT` always, because shim mirrors and logs a placeholder all-zero SHA-256 entry for an empty list and `MokListRT` also carries shim's built-in certificate; `MokSBStateRT` only when MokSBState is set and so never in attested mode, `MokListTrustedRT` only while MokListTrusted is unset, `MokPolicyRT` when set), in shim's measurement order, because pcrlock ships none for shim; the policy is updated before known firmware, db, dbx, shim (SbatLevel, signing authority, built-in vendor certificate and denylist), MokList, MokListX, MokListTrusted (`mokutil --trust-mok` or `--untrust-mok`) and MokPolicy changes; an unforeseen change falls back to the recovery key with a guided reseal. The guarantees of the reseal are final: TPM-only unlock only in attested mode, no rollback of db, dbx or SbatLevel revocations, only the newest PCR policy key, and never an unlock method that cannot unlock; its mechanism is designed in P4b and proven by section 12 item 5, starting from this outline: the reseal runs after switch-root on a boot of UKI profile 0, once `athanor-profile-check` has evaluated attested mode (section 4); it keeps TPM-only unlock only if attested mode holds and the measured db, dbx and SbatLevel revoke at least what those of the previous seal revoked (a rollback is reported and TPM-only is not offered), offers the passphrase alone when the `user-modules` variant is selected (D40), otherwise TPM plus PIN or the passphrase, and reports the reasons; on a compatibility-profile boot it is deferred. The PCR 11 policy is signed with the PCR policy key through `ukify --sign-initrd-pcrs`, and the TPM keyslot is enrolled with `--tpm2-public-key-policyref=initrd` (both systemd 262, as is pcrlock's `--strict=`), so only the initrd-phase signature unlocks it; it covers UKI profile 0 only (D45); rotating that key invalidates every TPM keyslot, and re-enrolment is the guided reseal above, on the first boot of profile 0 of a UKI signed with the new key; the reseal enrols only the PCR policy public key of the newest installed UKI and is deferred on a boot of a UKI that carries an older key. The installer checks that the firmware db accepts a certificate authority that signs the shipped shim | without Secure Boot the command line is measured only into PCR 12, which the policy excludes, so TPM-only unlock would release the key to a modified boot; a dbx update from Windows Update on a dual-boot machine can land before Athanor runs | final; the reseal mechanism open (P4b) |
| D43 | The Secure Boot, PCR policy, module signing, integrity and update keys are used only in sign-only CI jobs that receive built artefacts, never in a job that runs third-party actions or the image build, and share one custody: their own environment with required reviewers | a compromised build step must not be able to sign; any builtin key can sign modules, IPE policies and root hashes, and the Secure Boot and PCR policy keys decide which kernels boot and which boots unlock the disk, so the weakest custody would set the protection of all | final |
| D44 | Root boundary: an Azoth patch, proposed upstream, makes IPE enforcement one-way once enforcing; class-ordered policy versions (D8) prevent activating a weaker signed policy. SELinux is not a boundary against an unconfined root under Fedora's targeted policy, which lets `unconfined_t` enter any domain and load policy: this is a stated residual risk | `CAP_MAC_ADMIN` can write `enforce=0` today; confining root would require `sysadm_u` and break the Fedora administration model | final; with the dm-verity option (D6), the patch is written in P6 |
| D45 | Compatibility boot profile: the UKI carries a second profile whose command line repeats the base command line (including `usrhash=` and the verity options, or the composefs digest with bootc) and adds `efi=no_disable_early_pci_dma intel_iommu=off amd_iommu=off`; it is selectable from the boot menu, never selected automatically, and named in the installer's first-boot troubleshooting guidance. Only profile 0 carries signed PCR 11 measurements (profile 0 sets `ID=athanor` in its `.profile` section, the compatibility profile `ID=compat` with a `TITLE=`, and ukify runs with `--sign-profile=athanor`), so a boot of the compatibility profile is degraded and unlocks the disk with a passphrase or the recovery key | firmware that misbehaves with `EFI_DISABLE_PCI_DMA` or broken DMAR tables hangs before an addon could be installed; a profile's `.cmdline` replaces the base one; users cannot edit a signed command line | final |
| D46 | IMA keys are vouched by the builtin keyring only (`IMA_KEYRINGS_PERMIT_SIGNED_BY_BUILTIN_OR_SECONDARY` off). An Athanor unit writes the absolute path of an IMA policy in `/usr` to securityfs, and `/etc/ima/ima-policy` is masked. P4b either requires a signed policy through the build-time rule (`IMA_APPRAISE_BUILD_POLICY` with `IMA_APPRAISE_REQUIRE_POLICY_SIGS`, active before the first load, with `IMA_WRITE_POLICY` off) or records the remaining risk | with that option on, a CA enrolled by the user (D40) could vouch an IMA key; with it off, the Secure Boot architecture policy no longer requires a signed IMA policy; IPE's `POLICY` table governs only policies loaded by path, while rules written as text bypass it, systemd's `ima-setup` falls back to exactly that for `/etc/ima/ima-policy`, and `IMA_WRITE_POLICY` lets root append rules, and with `IMA_READ_POLICY` on the policy file stays readable by `CAP_SYS_ADMIN` after the first load (further writes fail with `EBUSY`), so P4b also sets `IMA_READ_POLICY` off or records that disclosure | final for the keyring option (P2); the signed IMA policy and `IMA_LOAD_X509` open (P4b) |
| D47 | Base hardening sysctls hold on every role and cannot be overridden by roles: `kernel.yama.ptrace_scope=1`, `kernel.kptr_restrict=2`, `kernel.dmesg_restrict=1`, `dev.tty.ldisc_autoload=0`, `fs.protected_fifos=2`, `fs.protected_regular=2`, `fs.suid_dumpable=0` | maintainer decision of 2026-09-13 for `ptrace_scope=1` everywhere; the rest are hardening recommendations at least as strict as the kernel defaults and stricter than the systemd and Fedora `sysctl.d` defaults, with no role-specific need; `fs.suid_dumpable=0` overrides Fedora's `50-coredump.conf`, so crashes of processes that changed privileges are not collected | final; delivered in P3 |
| D48 | Firmware loaded without a file (EFI-embedded firmware through `firmware_request_platform`, used by the touchscreens of some x86 tablets) or placed outside the image is refused on the desktop and mesh classes; the `user-modules` variant admits every `FIRMWARE` load, and acceptance lists the affected hardware | IPE finds no property for a load without a file and denies it under a default-deny `FIRMWARE` table; enforcement keeps firmware integrity on the desktop, and the affected devices get a visible opt-in that marks the machine not attested. The alternative, allowing `FIRMWARE` on the desktop class, was rejected by the maintainer on 2026-09-14 | final; enforced by IPE with the dm-verity option (D6); if S1 selects bootc, S1 delivers an equivalent refusal or the decision returns to the maintainer |
| D49 | systemd-stub does not append the SMBIOS type 11 string `io.systemd.stub.kernel-cmdline-extra=` to the command line when Secure Boot is enabled; Athanor builds its UKIs with a stub that enforces this, through an upstream contribution or a patch proposed upstream, and the boot matrix proves it | under Secure Boot the stub still appends that string, measured only into PCR 12, so whoever controls the SMBIOS OEM strings (the host of a virtual machine, or someone able to rewrite the firmware's OEM data) could add `rd.systemd.debug_shell`, `systemd.mask=` or `ipe.enforce=0` while TPM-only unlock still succeeds; a unit cannot refuse it, because the injected command line can mask that unit | final; delivered in P4b |

## 3. Architecture overview

The chain below is the dm-verity option of D6; spike S1 compares it with bootc sealed
composefs, which keeps the firmware, shim, UKI and kernel levels and replaces `/usr` on
dm-verity with a composefs image verified by fs-verity.

```text
firmware (UEFI db)
  └─ shim (Fedora, Microsoft-signed)    verifies with db, shim's built-in Fedora certificate, MokList
       └─ systemd-boot as shim's second stage   signed: Secure Boot key   boot counting (+3)
            └─ UKI                      signed: Secure Boot key
                 │   profile 0: .cmdline (usrhash=, verity options, base params)
                 │              .pcrsig (PCR 11 policy, initrd phase, PCR policy key)
                 │   profile 1: compatibility (D45), no signed PCR policy
                 ├─ <uki>.efi.extra.d/athanor-role-<r>.addon.efi   signed: Secure Boot key → PCR 12
                 └─ kernel (Azoth, IPE boot policy, module and integrity certificates compiled in)
                      └─ /usr: dm-verity, root hash signature verified by the kernel
                           ├─ /usr/lib/athanor/roles/<r>/     role content
                           └─ athanor-roles generator → /run/{sysctl.d,modprobe.d,tmpfiles.d,systemd}
/etc: handling per D39
/ , /var, /home: btrfs on LUKS (TPM-only offered in attested mode, TPM+PIN or passphrase otherwise)
```

Four layers, one source of truth each:

1. **Kernel build profile**: `forge/specs/azoth/kernel-local` (section 5).
2. **Runtime base profile**: the package `forge/specs/athanor-kernel-profile`, which
   replaces the kernel-related files scattered today in `athanor-base-config` (`kargs.d`,
   `modprobe.d`, `scx_loader` drop-in, NVIDIA dracut configuration) and
   `athanor-system-tweaks` (`sysctl.d`) (section 6).
3. **Roles**: `athanor-kernel-profile/roles/<role>/`, installed under
   `/usr/lib/athanor/roles/<role>/`, activated by signed addons (section 7).
4. **Manifest**: `athanor-kernel-profile/profile.toml` declares every setting of the base
   and of each role with its type and priority; a repository script generates the files,
   validates every role combination and computes the expected effective profile that the
   drift checker enforces (sections 7 and 12).

## 4. Integrity modes

- **Attested**: Secure Boot on with shim validation enabled (MokSBState unset); MokList
  holding only the project Secure Boot certificate, and the PCR 7 event log showing only
  project-signed authorities after shim; `.machine` and `.secondary_trusted_keys` holding
  exactly the expected certificates (D40); TPM 2.0 present; the signed PCR 11 policy valid
  (UKI profile 0); PCR 12 consistent with the declared role set; with the dm-verity option, the IPE class policy
  of the role set active (section 10), and with bootc the integrity measures S1 decides. Required by mesh hosts (D38) and by TPM-only disk
  unlock (D42).
- **Degraded** (declared "not attested"): anything missing. With the dm-verity option, the image is still verified
  on every read against the root hash on the command line, but without Secure Boot
  systemd-stub accepts a command line from the boot loader and the UKI itself is not
  verified by firmware, so an attacker with root or physical access can replace the UKI,
  its command line (root hash and `ipe.enforce=0` included) and its addons on the ESP.
  IPE enforcement, the root boundary (D44) and image verification are guarantees of
  attested mode only; on a degraded machine a TPM+PIN prompt can come from a replaced boot
  chain. `athanor-profile-check` reports the mode and the reasons.
- In degraded mode the role addons are not verified either: shim verifies them only
  under Secure Boot, so someone with physical access can add a forged addon such as
  `athanor.role=mesh`. Roles are therefore *declared*, not *proven*, on a degraded
  machine: `athanor-profile-check` reports them as declared, `athanor-role` never grants
  anything on the strength of the command line alone, and no trust decision (mesh
  admission, attestation, IPE policy class stronger than the one the machine actually
  enforces) may rest on a role that is not backed by attested mode.
- A machine booting the compatibility profile (D45) or using the `user-modules` variant
  (D40) is reported as not attested even when everything else holds.
- The installer requests the enrolment of the project Secure Boot certificate before the
  first reboot: `mokutil --timeout -1` disables MokManager's countdown, `mokutil --import`
  takes, through `--hash-file`, the SHA-512 crypt hash (`mokutil --generate-hash`) of a
  one-time password shown to the owner, and the DER
  certificate is placed on the ESP for MokManager's "Enroll key from disk"; P4b verifies
  that the installer environment can do this. shim processes the request whether or not
  Secure Boot is on, so the first boot always goes through MokManager, where the owner
  confirms the enrolment, a human action by design. With Secure Boot on, the machine then
  boots with the certificate enrolled, and TPM-only unlock is offered once attested mode
  holds (D42); with Secure Boot off the machine is degraded. If the request is declined or
  not completed, a Secure Boot machine returns to MokManager at every boot until the
  certificate is enrolled from the ESP with "Enroll key from disk" (on those boots
  MokManager waits only its default 10 seconds for a key, which the installer's guidance
  states); a machine without
  Secure Boot boots degraded, and a guided step in the installed system offers the
  enrolment again.

## 5. Kernel build profile (`kernel-local`)

Principle: what holds for every role lives in the build configuration, so no command line
carries it. Most of these options remain overridable by a boot parameter; the command line
is authenticated only in attested mode (section 4), and `athanor-profile-check` reports any
such parameter in `/proc/cmdline`. Forced lockdown (registered early by
`SECURITY_LOCKDOWN_LSM_EARLY=y`, set explicitly) and module signature enforcement
(`MODULE_SIG_FORCE=y`) cannot be lowered at boot; `lsm=` can still omit SELinux, IPE, Yama,
Landlock and the BPF LSM (only lockdown, registered early, and IMA and EVM, ordered last,
cannot be omitted), and `athanor-profile-check` reports it like any other override. Items marked P4b, P5 or P6 depend on a later block and are not
part of P2. Fedora enables several of the options below, so `kernel-local` sets every
disabled option explicitly (`# CONFIG_X is not set`).

**From the command line into the build:**
`LOCK_DOWN_KERNEL_FORCE_INTEGRITY=y`, `INIT_ON_FREE_DEFAULT_ON=y`, `LEGACY_VSYSCALL_NONE=y`,
`DEBUG_FS_ALLOW_NONE=y`; from P4b, together with the compatibility
profile of D45 that disables them at boot, `INTEL_IOMMU_DEFAULT_ON=y` and
`EFI_DISABLE_PCI_DMA=y`.

**Reliability:** `PANIC_TIMEOUT=10` and `EFI_VARS_PSTORE_DEFAULT_DISABLE` off (D19,
section 11).

**New hardening:** `KSTACK_ERASE=y`, `PAGE_TABLE_CHECK=y`, `PAGE_TABLE_CHECK_ENFORCED=y`,
`DEBUG_VIRTUAL=y`, `DEBUG_SG=y`, `DEBUG_NOTIFIERS=y`, `ARCH_MMAP_RND_BITS=32`,
`ARCH_MMAP_RND_COMPAT_BITS=16`, `PROC_MEM_FORCE_PTRACE=y`,
`SECONDARY_TRUSTED_KEYRING_SIGNED_BY_BUILTIN=y` (D40),
`IA32_EMULATION_DEFAULT_DISABLED=y` (P5, together with the addons of the interactive
roles that re-enable it, so 32-bit applications never break between blocks),
`SECURITY_SELINUX_DEVELOP` and `SECURITY_SELINUX_BOOTPARAM` off (P6, after the D20 gate).
Applied in P2 unless the boot matrix fails, and withdrawn through a recorded decision if a later acceptance run fails,
with their cost recorded in P7:
`UBSAN_TRAP`, `PROC_KCORE` off, `BLK_DEV_WRITE_MOUNTED` off, and `MSEAL_SYSTEM_MAPPINGS`,
which requires `CHECKPOINT_RESTORE` off and so breaks CRIU and container checkpointing,
gVisor, rr and User-Mode Linux (with `KCMP=y` set explicitly, because Mesa and systemd use
`kcmp()`).

**Keyrings** (D40, D46): `DM_VERITY_VERIFY_ROOTHASH_SIG=y` and `SECURITY_IPE=y` with
`DM_VERITY_VERIFY_ROOTHASH_SIG_SECONDARY_KEYRING`, `DM_VERITY_VERIFY_ROOTHASH_SIG_PLATFORM_KEYRING`,
`IPE_POLICY_SIG_SECONDARY_KEYRING` and `IPE_POLICY_SIG_PLATFORM_KEYRING` all off (Fedora
enables all four), so root hashes and policies verify against the builtin keyring only.
With the secondary option on, dm-verity verification starts against the builtin and
secondary keyrings; the platform option retries against the platform keyring on `-ENOKEY`
and `-EKEYREJECTED`; in every configuration a final retry uses the `.dm-verity` keyring if
it holds keys and has been restricted, which requires `dm_verity.keyring_unsealed=1`. Module verification
uses the builtin and secondary keyrings; the Red Hat fallback to the platform keyring is
removed by `patches/redhat/0001`. Signed BPF programs may name the builtin, secondary or
platform keyring, or a keyring of the caller; Athanor ships no signed BPF programs today,
and any Athanor policy that trusts one requires the builtin keyring. `IMA_KEYRINGS_PERMIT_SIGNED_BY_BUILTIN_OR_SECONDARY` is off, and `IMA_LOAD_X509` is decided with D46 in P4b.
`SYSTEM_TRUSTED_KEYS` carries the module signing certificate and, from P4b, the integrity
certificate; `SYSTEM_REVOCATION_KEYS` the retired ones. The kernel cannot restrict a
builtin certificate to one purpose, so the keys share one custody (D43).
`INTEGRITY_MACHINE_KEYRING` with `INTEGRITY_CA_MACHINE_KEYRING_MAX` stays on for the user
path of D40. From P4b, with the dm-verity option: `DM_VERITY=y` and `EROFS_FS=y` built in, `BTRFS_FS=y` kept built in, and `IPE_BOOT_POLICY` set to the compiled
boot policy (section 10). `MODULE_DECOMPRESS=y` uses the algorithm selected by
`MODULE_COMPRESS_*`, and external modules (NVIDIA, D40) ship uncompressed or with that
algorithm: IPE sees `init_module(2)` as a `KMODULE` load without a file, which, with the dm-verity option, every class
except `user-modules` denies, and kmod falls back to it when the kernel cannot decompress a
module.

**Attack surface removed** (unused by every role): `KEXEC`, `KEXEC_FILE` (its signature
check falls back to the platform keyring) and `KEXEC_HANDOVER` (which selects
`KEXEC_FILE`), `CRASH_DUMP`, `LIVEPATCH` (updates are images), `HIBERNATION` (blocked by
lockdown), `SECURITY_TOMOYO`, `X86_IOPL_IOPERM`.
`LSM="lockdown,yama,selinux,bpf,landlock,ipe"` (IMA and EVM always initialise last and
take no position in the list).

**Kept on purpose:** `IA32_EMULATION` and `MODIFY_LDT_SYSCALL` (Steam, Wine), `MODULES`
(NVIDIA, and user modules through D40), `BPF_LSM`, `KALLSYMS`, kprobes and ftrace (the
eBPF nerve, observability), `IMA_ARCH_POLICY` (attestation), `ZRAM` with
`ZRAM_WRITEBACK`, `PREEMPT_DYNAMIC`, `SHUFFLE_PAGE_ALLOCATOR`,
`SECURITY_LOCKDOWN_LSM_EARLY`, `MODULE_SIG_FORCE`, and the built-in drivers listed in D17.

**Rejected:** `PANIC_ON_OOPS` (D19), `IOMMU_DEFAULT_DMA_STRICT` (D16),
`STATIC_USERMODEHELPER` (modprobe and the coredump pipe need helpers),
`RESET_ATTACK_MITIGATION` (without userspace support the firmware wipes RAM at every
boot), `TRIM_UNUSED_KSYMS` (breaks the NVIDIA modules), `RANDSTRUCT` (excluded by Rust),
`SECURITY_LOADPIN` (IPE covers `KMODULE` and `FIRMWARE` with the dm-verity option;
reconsidered by S1 if bootc is chosen).

**Codegen:** `RUST=y`, `AUTOFDO_CLANG=y` (profiles arrive in P7), `X86_64_VERSION=1`
(D14; without the CachyOS base the upstream build already targets the x86-64 baseline),
preemption `lazy` (D13).

## 6. Runtime base profile

**Kernel command line.** Nearly empty: what installation knows (LUKS device, root),
`page_alloc.shuffle=1` (page allocator randomisation is off by default even when built
in), `amd_pstate=active`, `rd.shell=0 rd.emergency=halt` and `systemd.import_credentials=no`
(section 10), and from P4b with the dm-verity option `usrhash=`,
`systemd.verity_usr_options=root-hash-signature=base64:<signature>` (written into both UKI
profiles by the image build, because `auto` depends on udev data that exists only on the
disk holding the ESP; the signature is a detached PKCS#7 without signed attributes and
without embedded certificates, because x86 truncates the command line at 2048 bytes and a
signature carrying a 4096-bit certificate exceeds that limit on its own; the image build
fails if a profile command line with the largest allowed role addon set reaches 2048
bytes) and `dm_verity.require_signatures=1`: the kernel must verify the root
hash signature, otherwise IPE's `dmverity_signature` is false and every module and
firmware load from `/usr` is denied.
The interactive roles add `rhgb quiet`. Today's `kargs.d`, the kickstart, the UKI scripts of
section 14 and `forge/specs/azoth/cmdline` change as follows:

- already defaults: `slab_nomerge`, `randomize_kstack_offset`, `ima_hash`,
  `init_on_alloc`, `mitigations=auto`;
- kept: `amd_pstate=active` (the driver does not load by default when the firmware power
  profile is undefined, or is a server profile on CPUs older than family 0x1A; ignored on
  Intel and on CPUs without CPPC);
- moved into the build: `module.sig_enforce`, `lockdown`, `init_on_free`, `vsyscall`,
  `debugfs`; `intel_iommu=on` and `efi=disable_early_pci_dma` stay on the command line until
  P4b builds them in together with the compatibility profile of D45;
- moved into roles, or dropped: `oops`, `preempt=full` (the base becomes lazy, D13),
  `zswap.enabled=1` and `zswap.compressor=zstd` (D15), `splash` and `fastboot` (not used),
  `rootflags=noatime` (mount options live in the file system table);
- wrong for Athanor: `pti=on` (forces page table isolation on CPUs not affected by
  Meltdown), `iommu=pt` (identity mapping, against D16);
- invalid: `amd_iommu=on` (the driver logs an unknown option), `lam=on` and
  `arm64.mte=on` (not x86 parameters);
- capability-specific, never global: `mem_encrypt=on`, `kvm_amd.sev=1`, `kvm_intel.tdx=1`.

NVIDIA parameters and dracut configuration move out of the base: they apply only in the NVIDIA image variants (doc_system_image.md, S4 and S5). `ima_policy=tcb` is removed in P4b, when the policy of D46 replaces it; IMA measurement follows D23 with the
key and policy rules of D46.

**sysctl (base, locked):** the hardening of D47, `kernel.oops_limit=100`,
`net.core.default_qdisc=fq`, and from P6 `kernel.io_uring_disabled=1` with
`kernel.io_uring_group` set to the numeric GID of the `athanor-io-uring` group (fixed in
`sysusers.d`, because sysctl takes no group names; processes with `CAP_SYS_ADMIN` are also
allowed). **sysctl (base, overridable by roles):** `kernel.sysrq=176` (sync, remount
read-only, reboot). The TCP congestion control is not set, so the kernel default BBRv3
applies. `athanor-system-tweaks/.../99-bore.conf` is removed with its CFS tunables that
no longer exist under EEVDF and its override to BBRv1. KSM stays off unless a role
declares it.

**Scheduler:** EEVDF with BORE at the defaults of the pinned patch, `HZ=1000`.

**Memory:** zram swap with zstd (`zram-size = min(ram / 2, 8192)`), MGLRU on, THP
`madvise` with defrag `defer+madvise`; `vm.max_map_count` keeps Fedora's default
(1048576).

**Power:** power-profiles-daemon is the only owner of EPP and platform profile. The CPU
vendor's driver runs in active mode where the CPU supports it (`amd_pstate` with CPPC,
set by `amd_pstate=active`; `intel_pstate` with HWP; Intel CPUs without HWP run `intel_pstate` in passive mode); other machines and virtual machines keep the kernel's default cpufreq
driver, and power-profiles-daemon uses its `platform_profile` driver where the firmware exposes
one, otherwise its `placeholder` driver.

## 7. Roles

**Content.** Each role is a directory in the signed image,
`/usr/lib/athanor/roles/<role>/`, holding `sysctl.d`, `modprobe.d`, `tmpfiles.d`,
`systemd` units and drop-ins, and `scx_loader` configuration. It is protected by the image
verification like the rest of `/usr` and updated atomically with the OS.

**Activation.** A role is active when its signed addon is installed next to the UKI in
`<uki>.efi.extra.d/athanor-role-<role>.addon.efi` (D21; the directory name carries no
boot-counter suffix). The addon carries `athanor.role=<role>` and the role's kernel
parameters, is verified through shim, and is measured into PCR 12 together with the other
command line fragments. Addons are built and signed in CI and shipped inside the image as
`/usr/lib/athanor/roles/<role>/athanor-role-<role>.addon.efi`; `athanor-role` installs the
addons of the chosen roles for the running UKI from the running image and for the pending UKI from the pending
image, because each addon carries the `.uname` of its own UKI, and every update installs
them for the new UKI from the new image before the UKI, so a UKI never boots with
parameters from another image version. The Secure Boot key signs only the UKIs and addons
of a release build. The `profile.toml` validator restricts each addon's command line to
the parameters declared for its role and rejects `ipe.*`, `lsm=`, `lockdown=`, `module.*`,
`rd.*`, `systemd.*`, `init=`, `efi=`, `iommu=`, `iommu.passthrough=`, `iommu.strict=0`, `intremap=`, `nointremap`, `intel_iommu=`, `amd_iommu=`,
`dm_verity.*` and `proc_mem.*` (`iommu.strict=1` is admitted where a role declares it, D16); every addon carries the `.uname` of its UKI; a parameter
whose removal matters for security lives in the UKI command line or the build, never in
an addon alone, except a role's own addon (`athanor.role=` with the parameters that role declares, such as
`iommu.strict=1`), whose removal is visible only to attestation through the PCR 12 event log (section 10, residual risks).
Roles and the `user-modules` variant are selected with `athanor.role=` and
`athanor.variant=`, which the validator declares. Boot matrix and acceptance artefacts that need test parameters are signed
with an ephemeral key enrolled only in the test machine's varstore.

**Application.** The `athanor-roles` systemd generator reads `/proc/cmdline`
(authenticated under Secure Boot, with the SMBIOS exception closed by D49) and links the role files into `/run/sysctl.d`,
`/run/modprobe.d`, `/run/tmpfiles.d` and `/run/systemd/`. `scx_loader` configuration is
read from `/etc` and reached through a tmpfiles symlink; power profiles are set by a role
unit through power-profiles-daemon's D-Bus API. P5 verifies the ordering of the generator
against `systemd-sysctl.service` and module coldplug, and re-checks at every systemd bump
that the generator sandbox still leaves `/run` writable (systemd 258 documents only the
output directories as writable).

**Composition (`profile.toml`).** Settings have types. Lists merge by union, maps merge
recursively, scalars from two roles with different values are a build error unless one
definition carries an explicit higher priority; numeric sysctls may declare an ordering
(`max`, `min`) when the direction is obvious. Base settings marked locked cannot be
overridden. The validator evaluates every role combination and fails on any unresolved
conflict. The effective profile of each combination is the input of the drift checker.
Shared definitions live in manifest fragments that are not roles and cannot be
activated alone: desktop and laptop both include the `interactive` fragment (32-bit
emulation, `rhgb quiet`, autogroup, MGLRU `min_ttl_ms`, `ntsync`,
`split_lock_mitigate=0`, `panic_on_oops=0`, `warn_limit=0`), so the laptop stands alone
without duplicating the desktop (D18).

**Roles:**

| Setting | No role | Desktop | Laptop | Mesh-only host |
| --- | --- | --- | --- | --- |
| addon parameters | none | `rhgb quiet`, `ia32_emulation=1` (interactive) | `rhgb quiet`, `ia32_emulation=1` (interactive) | none defined yet (mesh specification) |
| scheduler | BORE | BORE; `scx_lavd` Gaming only on demand (`scxctl`) | `scx_lavd` PowerSave (provisional, P7) | BORE |
| autogroup | on (kernel default) | on | on | off |
| power profile | `balanced` | `balanced` | `balanced` on AC, `power-saver` on battery, switched by a role unit that follows UPower's `OnBattery` | `performance` where the platform offers it, otherwise `balanced` |
| memory and sleep | base | MGLRU `min_ttl_ms=1000`, `ntsync` (interactive) | interactive settings; suspend mode and ASPM left to the firmware defaults unless P7 measures a gain | network buffers `net.core.rmem_max` and `wmem_max` 16 MiB, `netdev_max_backlog=16384` (provisional, P7) |
| other | base (`panic_on_oops=0` and `warn_limit=0`, kernel defaults) | `split_lock_mitigate=0`, `panic_on_oops=0`, `warn_limit=0` (interactive) | interactive settings, Wi-Fi power saving | `kernel.sysrq=0`, `bpf_jit_harden=2`, `panic_on_oops=1`, `warn_limit=100` (D19), attested mode required |
| IPE policy class (dm-verity option, D6) | desktop | desktop (section 10) | desktop | mesh |

A machine holding both an interactive role and the mesh role is an interactive mesh host
(D38): with the dm-verity option it enforces the desktop class, and the composition of its settings (including
`split_lock_mitigate=0`, which lets a local process stall other cores while guests run)
is defined with the mesh specification (D27). Until then the validator rejects the
combination.

**Purpose of the mesh** (D26, D38; after 1.0). The mesh joins the devices of one owner:
a private network over kernel WireGuard with an existing coordination server;
synchronisation and backup between devices (Syncthing-class sync, btrfs send and
receive); compute sharing by capability tier (D28); remote applications from interactive
mesh hosts (D29). Mesh hosts hold data and run workloads; clients (phones, other systems,
degraded machines) connect with limited keys. The mesh never distributes updates.
Protocol, identity, discovery and scheduling belong to the mesh specification (D27).

**Platform requirements of the mesh**, enabled in the kernel today and asserted by the
boot matrix from P2: WireGuard, KVM (AMD and Intel), vhost-vsock, virtio-fs, virtio-gpu
with `udmabuf`, VFIO for the optional GPU tier, and the TPM and attestation chain of
section 9. The virtual machine monitor is chosen once, in the mesh specification. GPU
inference stacks that compile code into writable caches do not run on the mesh class.

**Assignment.** Roles are chosen at installation (a kickstart variable) and changed with
`athanor-role add|remove`, which installs or removes signed addons, refuses the mesh
role when the machine is not attested or the mesh has not been delivered (D36), refuses role sets the
validator rejects, and
states when a reboot is needed. With the dm-verity option, IPE allows a single active policy, so a signed policy is
generated at build time for every allowed role combination, with a version ordered by
class (D8); a change that lowers the class, including the `user-modules` variant, takes
effect at the next boot through its signed addon.

## 8. Image, boot and updates

**Boot requirements.** UEFI is required; the installer refuses legacy BIOS and CSM boot
and CPUs without x86-64-v3 (D3). shim and systemd-boot (as shim's second stage)
are installed and updated by the update chain (P4b), because bootc does not update the
boot loader after installation.

**Disk layout** (installation creates it): ESP of 2 GiB when Athanor owns the disk, or
XBOOTLDR (VFAT, 2 GiB) next to an existing ESP (D17); storage for two images per D6, each sized
to twice the largest image built in P4b; root and data on btrfs inside LUKS (`/`, `/var`,
`/home` subvolumes); `/etc` per D39.

**Image build.** The existing OCI build (GitHub Actions, dnf5, tier repositories, cosign,
SBOM) stays the source of the root filesystem and of provenance. With the dm-verity
option a new stage produces with `systemd-repart` a `/usr` image (EROFS, reproducible:
fixed timestamps, stable file order, compression per block) with its verity and signed
root hash partitions; for either option a UKI is built with `ukify` (profile 0 with
`usrhash=` or the composefs digest, the verity options and the base command line, PCR 11
policy signed for the initrd phase; profile 1 of D45; SBAT section; the build job runs ukify with `--policy-digest`, and the
sign-only job signs the digests and joins them with `--join-pcrsig` before the Secure
Boot signature) and role addons with the addon stub. Every system update also gets its manifest (D41). Signing happens only in
sign-only jobs (D43).

**Minimal initramfs** (D17): generic, never host-only, with `athanor-cpu-check` (D14).
The `bcachefs` userspace tools leave the image.

**Boot counting.** UKIs are installed with three tries (`+3`; with bootc, subject to the S1 gate of step 4). `boot-complete.target`
requires `athanor-profile-check` and the critical services; `systemd-bless-boot` marks a
good boot. Only drift attributable to the image fails `boot-complete.target`: degraded
mode and hardware findings (a missing TPM, firmware that does not mark external ports) are
reported, never failed, so no machine is locked out of updates by its hardware.

Until the update service below exists, `doc_update_trust.md` is the interim update
service: bootc behind the same interface, key-based image signatures, a download-only
timer and confirmation by the user (`doc_shell.md`, SH11).

**Release 1.0 updates** (D31 classes A and D, D36, D41):

1. Applications update through Flatpak with no interruption (class A).
2. A system update is a full image, downloaded and verified in the background. The
   Athanor update service first checks the update manifest (D41): its signature with the
   integrity key, its expiry, the minimum version and the image digest. S1 then chooses
   the verification path. With `systemd-sysupdate`, it runs as
   `/usr/lib/systemd/systemd-sysupdate --definitions=/usr/lib/sysupdate.d update <version>` with the
   version the manifest names, so definitions in `/etc/sysupdate.d` are ignored and
   reported. Either a `url-file` source uses `Verify=yes`, with its OpenPGP keyring only in
   `/usr` and no `/etc/systemd/import-pubring.*` present (its presence is reported); or the update service
   verifies the image itself and passes it to a `regular-file` source, which performs no
   verification, both steps running in one root-only service over a staging directory so
   the verified image cannot be replaced before it is written. Role addons are published
   with the image in the same signed source and are identical to the copies inside the
   image (D21). With bootc, `bootc upgrade --download-only` stages the image digest the manifest names as a
   deployment that is locked against being applied.
3. The new version is installed but does not become the boot default until the user
   confirms. With `systemd-sysupdate` and systemd-boot: before anything new is
   written, `bootctl set-preferred`
   names profile 0 of the booted UKI (the booted entry identifier without its `@` profile
   suffix) and is read back; the new UKI's addons are written into its
   `.extra.d` first and the UKI transfer comes last, because sysupdate's renames are not
   atomic; `ProtectVersion=%A` keeps the booted version; confirmation refuses a UKI whose
   role addons are missing and then moves `LoaderEntryPreferred` to the new entry.
   `LoaderEntryDefault` is never set by Athanor, because it ignores boot assessment; the
   update service removes it before `bootctl set-preferred`, and `athanor-profile-check`
   reports it if the boot menu set it. With bootc,
   `bootc upgrade --from-downloaded` unlocks the staged deployment on confirmation, which
   is applied at the next clean shutdown; a reboot before confirmation discards the staged
   deployment, and with the ostree backend the pulled image stays cached, so staging it
   again needs no new download; S1 verifies the same for the composefs backend. A reboot, a
   crash or a power loss before confirmation boots the running version. The previous
   version stays in the boot menu, and `LoaderEntryOneShot` selects it for one boot.
   Nothing reboots by itself (class D). With `systemd-sysupdate`, if firmware loses `LoaderEntryPreferred`, or the
   user clears it in the boot menu, the newest installed version becomes the default before
   confirmation: a stated residual risk, reported by `athanor-profile-check`.
4. On a boot with boot counting in effect (`systemd-bless-boot status` reports
   `indeterminate`, or `dirty` on the last try), if `athanor-profile-check` or a critical service required by
   `boot-complete.target` fails, its `OnFailure=` unit runs
   `/usr/lib/systemd/systemd-bless-boot bad`, which sets the entry's tries left to zero,
   and the user is told that the next reboot, which the user chooses, returns to the
   previous version; a hang or a panic uses the remaining tries. On a boot without boot counting in
   effect (`systemd-bless-boot status` reports `good` or `clean`), a failure is reported and the previous version is offered for one boot through
   `LoaderEntryOneShot`. `loader.conf` sets
   no `default`, so selection follows the sorted entry list, where entries without tries
   left come last. With bootc, this step requires boot counting, which the composefs
   backend does not configure today: an S1 gate.

**After 1.0: the target update model** (D7, D30–D35). Kept as the goal, redesigned before
implementation because the second platform audit found the first version unsafe:

- **Deltas** (D7, D22): content-defined chunks seeded from the running image, verified
  by the image signature; or, if S1 selects bootc, OCI layer pulls.
- **Layers** (D30, D32): a global `systemd-sysext refresh` unmerges and remounts every
  layer at once, with a moment where no overlay is mounted, so a service update would
  also apply a staged desktop layer mid-session. Service layers therefore attach to
  their own units (`ExtensionImages=` or portable services) and restart alone; the
  desktop layer merges only at boot or at a soft reboot. Image policies follow D32.
  `systemd-sysupdate-notify-sysext.socket` (systemd 262) stays disabled.
- **Selection and rollback** (D35): layers are stored per base version and selected from
  the booted base (its root hash), not from `SYSEXT_LEVEL` alone, which only skips a
  mismatching layer.
- **Measurement:** the verity NvPCR exists from systemd 260; from 262 its definitions ship
  inside the UKI, which then needs signed initrd-phase PCR policies.
- **Interface:** the update interface is built on the Varlink API of `systemd-sysupdate`,
  which today lists targets and checks for new versions only; the D-Bus API of
  `systemd-sysupdated` is scheduled for removal in systemd 263.
- **Channels and rollout** (D33, D34): signed channel manifests with expiry; rollout day
  from an application-specific identifier derived from `machine-id` and the release
  version; a signed stop manifest; security releases skip the window.

**Prerequisites.** `systemd-sysupdate` was marked experimental again in systemd 262
("more breaking changes are forthcoming"), and `LoaderEntryPreferred` needs systemd 260;
the dm-verity option of D6 depends on both, which S1 weighs on Fedora 45.
`RestrictFileSystemAccess=` (systemd 261) allows execution only from signed dm-verity;
systemd 262 extends this to overlayfs over verity on kernel 7.2 and later. Fedora 45 carries
systemd 262 release candidates today, so blocks that need 262 features wait for its final
release (D25).

## 9. Keys, measurements and attestation

| Key | Signs | Trusted by | Custody (D43) |
| --- | --- | --- | --- |
| Secure Boot (`SECUREBOOT_SIGNING_KEY`) | systemd-boot (shim's second stage), UKIs and their profiles, role addons | shim via MokList | shared, own environment with required reviewers |
| PCR policy (new, P4b) | signed PCR 11 policies of UKI profile 0 (initrd phase) | TPM keyslots enrolled with its public key | shared |
| Module signing (`MODULE_SIGNING_KEY`) | external kernel modules (NVIDIA) | kernel, builtin certificate | shared |
| Integrity (new, P4b) | update manifests; with the dm-verity option, image root hashes and IPE policies; the IMA certificate if D46 enables `IMA_LOAD_X509` | kernel, builtin certificate; the update service | shared |
| Update (new, P4b, OpenPGP; only if S1 selects `url-file` with `Verify=yes`) | `SHA256SUMS` of the image source of `systemd-sysupdate` | the keyring shipped in `/usr` | shared |

The integrity key signs root hashes and policies because both decide which code may run;
the module key could sign them too, because both are builtin, hence the shared custody.
**Revocation and rollback** (D41): `keys/revoked/` covers compiled-in certificates for the
kernels built after the revocation; superseded UKIs older than the fallback version are
revoked through MokListX hashes confirmed by the owner, or by a rotation of the Secure
Boot key; manifests carry an expiry and a minimum version.

**Measurements:** PCR 7 (Secure Boot state and the authorities used, including db, dbx,
MOK, shim's built-in certificate, SbatLevel and MokSBState), PCR 11 (UKI sections of the
booted profile, including `usrhash=`), PCR 12 (command line, addons, credentials, the
number of a non-default profile), PCR 13 (system extensions), PCR 14 (MokList and MokListX, and MokSBState, MokListTrusted
and MokPolicy when shim logs them), PCR 15 (machine identity, root file system, LUKS volume key), the verity NvPCR (after 1.0,
with layers), IMA log (D23).

**LUKS** (D42): in attested mode, TPM 2.0 policy through `systemd-pcrlock` on PCR 7 and
PCR 14 plus the signed PCR 11 policy (initrd phase, UKI profile 0), never PCR 12, so role
changes do not require resealing; in degraded mode TPM plus PIN, or a passphrase. A
recovery key is always enrolled.

**Attestation** (restricted area): admission of a mesh host requires its identity, a
verified TPM quote and with the dm-verity option, the active IPE class derived from PCR 11 and the PCR 12 event log (D38, section 10).
Keylime's example measured-boot policy considers PCRs 0–9 and 14 only, so a dedicated
policy covers PCR 11 and the command line events of the allowed role sets in PCR 12. The
attestation code in the repository today returns fixed results and quotes the wrong PCRs;
it is replaced, with the maintainer's approval, before any mesh admission depends on it.

## 10. Execution integrity and security primitives

The command line settings and SELinux restrictions of this section
(`systemd.import_credentials=no`, `rd.shell=0 rd.emergency=halt`, `execmem`, `execmod`)
apply with either option of D6; the IPE policies apply with the dm-verity option.

**IPE policies** (D8, with the dm-verity option). IPE operations are `EXECUTE` (including
executable mappings of files and of anonymous memory), `FIRMWARE`, `KMODULE`,
`KEXEC_IMAGE`, `KEXEC_INITRAMFS`, `POLICY` (policy files the kernel reads, such as the IMA
policy) and `X509_CERT` (certificates the kernel reads through `CONFIG_IMA_X509_PATH`; a
certificate added from userspace is governed by the `.ima` keyring restriction instead).
Loading an IPE policy is controlled by its signature (D40), not by an operation. IPE's
audit or enforce state is global, not per operation. Properties in one rule are combined
with AND, so alternatives take separate rules.

- **Boot policy** (compiled into the kernel from P4b, version 0.0.0): a global
  `DEFAULT action=DENY`, as in every class policy, so an operation unknown to the policy
  is never allowed; `EXECUTE` defaults to allow; `FIRMWARE`, `KMODULE`, `KEXEC_*`, `POLICY` and `X509_CERT` default to deny,
  with one rule admitting `boot_verified=TRUE` (the initramfs, including files the stub
  adds from the partition that holds the UKI) and one admitting `dmverity_signature=TRUE`. Firmware built into the
  kernel does not pass through IPE; firmware loaded without a file follows D48. Every UKI
  profile's command line carries `systemd.import_credentials=no`: systemd-stub measures
  credentials from `<uki>.efi.extra.d/` only into PCR 12, which the TPM policy excludes,
  and `systemd-debug-generator` would turn a TPM2-sealed `systemd.extra-unit.*` credential
  into an initrd unit after the disk is unlocked; SMBIOS, QEMU `fw_cfg`, `systemd.set_credential=` and credentials handed from the
  initrd to the host are disabled with them, in both stages, so neither acceptance nor any
  Athanor component relies on them; the pcrlock policy reaches `systemd-cryptsetup` as a
  file instead (D42). systemd-stub's SMBIOS command line addition is closed by D49.
- **Activation:** the class policy of the role set is loaded and activated at the end of
  the initrd by a unit in the UKI's initramfs, ordered after `systemd-udev-trigger.service`,
  `initrd-root-fs.target` and `initrd-usr-fs.target`, before `initrd.target` and required by
  it (`RequiredBy=initrd.target`), so a failure fails `initrd.target`, and the UKI command
  line carries `rd.shell=0 rd.emergency=halt`, which dracut's `dracut-systemd` emergency service honours
  (the initramfs excludes `systemd-emergency`), so the initrd halts instead of offering a
  shell after the disk is unlocked; refused modalias loads are repeated by coldplug from `/usr`, but refused firmware
  requests and kernel `request_module()` calls are not, so `DM_VERITY`, `EROFS_FS` and the
  root file system are built in and the boot matrix asserts no IPE denial before
  switch-root; selected from the role set on the authenticated command line;
  a failure stops the boot. `athanor-profile-check` reports the active policy name and
  version and, from P6, fails `boot-complete.target` if the boot policy is still active; mesh host
  admission derives the active class from the quoted PCR 11 (an initrd that halts unless
  activation succeeds) and the role addons in the PCR 12 event log; the report is a
  consistency check only, because IPE activation is audited, not measured, and root
  controls the reporting software.
- **Runtime class policies** use `boot_verified` only where the initrd needs it after
  activation: the mesh class carries `op=EXECUTE boot_verified=TRUE action=ALLOW`, because
  the initrd keeps starting units from the initramfs until switch-root, which empties the
  initramfs; no other operation of a runtime class admits `boot_verified`. Each policy has
  a version `class.release.0`, where the class is `user-modules` 1, desktop 2 or mesh 3,
  and a `policy_name` fixed per class and role combination across releases; a new release
  replaces the previous one through IPE's update operation, a running machine can only
  activate an equal or stronger class (D8), and policies sharing a version have identical
  rules, because root can switch between them. A policy of a higher class, in any
  release, admits nothing that a lower class of any release denies; a signed policy found
  too permissive is withdrawn only by rotating the integrity certificate.
- **Mesh class:** `DEFAULT action=DENY` for every operation, allowing
  `dmverity_signature=TRUE` and, for `EXECUTE` in the initrd, `boot_verified=TRUE`. Anonymous executable memory is denied as well, so JIT engines
  that use anonymous memory do not run on a mesh-only host; IPE still admits a writable
  private executable mapping of a verified file and write access added to an already
  executable mapping, which SELinux `execmem` denies; making a private file mapping executable again after it
  was written is checked by SELinux as `execmod` on the file's type, not `execmem` (D24); workloads of other devices run in virtual
  machines. The BPF JIT is a kernel component and stays on with `bpf_jit_harden=2`. `dracut-shutdown.service`
  is masked on mesh-only hosts, because `/run/initramfs/shutdown` is not `boot_verified`.
  The policy is built and tested in P6 and deployed with the mesh delivery (D36).
- **Desktop class:** `EXECUTE` defaults to allow, because browsers, Mesa and development
  tools need it; code in the home runs and is measured by IMA (D23). `KMODULE`:
  `dmverity_signature=TRUE` allowed, then default deny, so modules outside the image are
  refused even when signed. `FIRMWARE` admits only `dmverity_signature=TRUE`, so firmware loaded without a file or
  from outside the image is refused (D48); `KEXEC_*`, `POLICY` and `X509_CERT` admit only
  `dmverity_signature=TRUE`. This covers an IMA policy the kernel reads from a path; rules
  written as text bypass IPE (D46). With `IMA_LOAD_X509=y`, the IMA certificate at
  `IMA_X509_PATH` is read from the initramfs before `/init`, under the boot policy, and
  must be signed by a builtin key and carry keyUsage digitalSignature without CA or
  keyCertSign (D46). SELinux denies `execmem`, and `execmod` on files of the image, to system domains and to Athanor services, which run in
  their own declared domains and never in `unconfined_service_t` (D24); user applications
  stay in `unconfined_t` as in Fedora.
- **`user-modules` variant** of the desktop class (D40): `KMODULE` and `FIRMWARE` (D48) default to
  allow, the module signature is still enforced by `MODULE_SIG_FORCE` against the builtin and secondary
  keyrings, and the machine is reported as not attested. It is selected at boot by its
  signed addon (D40).
- **Rollout:** release 1.0 enforces the boot policy and the desktop class (D36). Coverage
  of a default-deny `EXECUTE` table is measured in acceptance with `ipe.enforce=0` and the
  full class policy, never on deployed machines.
- **Root boundary** (D44): the Azoth patch makes `enforce` one-way once enforcing, and
  class-ordered versions prevent activating a weaker signed policy.

**Gatekeeper** (restricted area): leaves the blocking path (`FAN_OPEN_EXEC_PERM`), where a
hung daemon blocks the system and a dead one disables protection; it may remain an audit
consumer. Its new specification is written separately and approved before any change.
No custom BPF LSM is written. `RestrictFileSystemAccess=` is a per-service second layer
(section 8, prerequisites).

**Application self-confinement:** Landlock (ABI 10 on Linux 7.2, negotiated best-effort)
is a platform rule; Athanor applications declare filesystem, network and IPC access
through a shared crate `athanor-sandbox` over the `landlock` crate. Services also use
systemd sandboxing.

**eBPF without root:** unprivileged BPF stays off. Services receive BPF tokens with
`PrivateBPF=yes`, `PrivateUsers=` (a token cannot be created in the initial user
namespace) and comma-separated delegation lists, for example
`BPFDelegateCommands=BPFMapCreate,BPFProgLoad,BPFTokenCreate`,
`BPFDelegateMaps=BPFMapTypeXskmap`, `BPFDelegatePrograms=BPFProgTypeXdp`,
`BPFDelegateAttachments=BPFXdp` (syntax verified with systemd 258). A service holding an
XDP token can attach through `BPF_LINK_CREATE` to any interface of its own network
namespace without further capability checks, so a service delegated `BPFProgTypeXdp` runs
with `PrivateNetwork=` or a dedicated network namespace. Athanor policies that trust signed BPF
programs require the builtin keyring; the kernel itself accepts unsigned programs (D40).

**io_uring:** from P6, disabled except for services given the `athanor-io-uring` group
and processes with `CAP_SYS_ADMIN`.

**User namespaces:** available (Flatpak, rootless podman, browser sandboxes). The loaded
SELinux policy exposes `user_namespace create`, so creation is restricted per system
domain through SELinux; services also set `RestrictNamespaces=`.

**Network:** per-service `IPAddressAllow=`/`IPAddressDeny=`, `RestrictNetworkInterfaces=`,
`SocketBindAllow=`, and host nftables. Tetragon leaves the image.

**Residual risks, stated:**

- A script passed to a verified interpreter escapes IPE (no widespread interpreter uses
  `AT_EXECVE_CHECK` yet).
- On the desktop class, JIT and code in the home remain allowed; the quarantine prompt of
  D23 is bypassable from a terminal; nothing comparable to macOS Gatekeeper or Windows
  Smart App Control exists yet.
- A process allowed to ptrace another can write code into its executable mappings, which
  IPE does not see; `ptrace_scope=1` still allows root and a process's ancestors.
- Code running as the user persists through autostart entries, `systemd --user` units and
  shell startup files; a Flatpak application with home access can write those files and
  leave its sandbox.
- Unconfined user code can create user namespaces and reach kernel code gated by
  in-namespace capabilities, such as `nf_tables`.
- An unconfined root is not bounded by SELinux under Fedora's targeted policy (D44), keeps
  persistence in `/etc` and `/var`, and can mount an older image signed with a valid
  integrity key until kernels carrying the revocation are deployed.
- A validly signed older UKI remains bootable until it is revoked through MokListX or the
  Secure Boot key is rotated (D41).
- Any builtin key can sign modules, IPE policies and root hashes (D43).
- On the mesh class, a process with SELinux `execmem`, `unconfined_t` included, can run
  native code outside the image through a writable private mapping of a verified file, and
  a process with `execmod` on that file's type by re-enabling execution after writing;
  IPE checks neither.
- Root can remove the mesh addon of a mesh-only host; with the dm-verity option the next boot enforces
  the desktop class and TPM-only unlock still succeeds, because PCR 12 is not in the TPM policy;
  attestation reports the change.
- An addon of an earlier release with the same `.uname` can be replayed without affecting
  TPM unlock; attestation sees it in the PCR 12 event log.
- `boot_verified` covers every file of the initramfs, including files the stub adds from
  the partition that holds the UKI; module signatures still bound what such files can load. After switch-root,
  every file on the initramfs superblock stays `boot_verified`, including files created
  later through a root, working directory or descriptor held by an initrd process that
  survives it, so on the mesh class root can execute such files, within the ptrace
  residual risk.
- On an attested machine, someone with the ESP can boot any binary shim accepts (db or
  shim's built-in Fedora certificate) with an initramfs of their own; no TPM keyslot
  unlocks, but that chain can show a counterfeit passphrase or recovery-key prompt.
- Degraded machines have no boot-chain guarantees (section 4).
- IPE has almost no production experience in enforcement.
- Linux has no hypervisor-isolated code integrity comparable to Windows HVCI or Apple's
  kernel page protection.
- TPM-only unlock on an attested machine is exposed to TPM bus sniffing and DMA attacks,
  as with BitLocker without a PIN.

## 11. Reliability

A panic reboots after 10 seconds; three failed boots fall back to the previous version;
on mesh-only hosts an oops or repeated warnings become a panic, on interactive roles
repeated oops do (D19). Crash evidence comes from EFI pstore archived by
`systemd-pstore`, the persistent journal and `DRM_PANIC` with its QR code where the GPU
driver supports it; acceptance verifies that a deliberate panic leaves a pstore record.
There is no kdump.

## 12. Verification

1. **CI, no VM:** the `profile.toml` validator over every role combination;
   `check_delta` on the kernel configuration (existing); the bump bot fails while the
   pinned series is end of life and no pair moves it (D37, existing); `scripts/verify.py workflows` fails if a signing
   secret reaches a job that runs third-party actions or a build step, or if a signing
   job does not use the protected signing environment (D43); the `profile.toml` validator
   fails on an addon parameter outside its role's declaration, an addon without `.uname`,
   or a role set without a defined class policy (including `user-modules` with each role
   set it admits; interactive plus mesh, until the mesh specification defines it (section 7), and mesh plus
   `user-modules` are rejected); every class policy of the release is compared with every policy of a different class
   from this release and from every previously signed release archived in the repository,
   and the check fails if a higher-class policy admits what a lower-class policy denies, or
   if policies with an equal version differ.
2. **Kernel boot matrix** (`forge/specs/azoth/boot.sh`, with the external module chain in
   `nvidia-kmod.yml`): CPU models Penryn (x86-64-v1, no POPCNT or SSE4.2, replacing
   today's Nehalem; D14) and host; an Intel IOMMU case (`-device intel-iommu,intremap=on`
   with `kernel-irqchip=split`, and `intel_iommu=on` on the test command line until P4b) and
   an AMD IOMMU case; assertions for forced lockdown, the
   preemption mode from the `Dynamic Preempt:` line of the kernel log (debugfs is not
   available), the IOMMU domain type, ASLR bits, the exact set of builtin certificates,
   the mesh platform requirements of section 7; the module chain: a module signed by the
   module key accepted, one signed by an enrolled non-CA MOK rejected (existing), one
   signed directly by an enrolled CA key accepted and one signed by a leaf certificate of
   that CA rejected, and an IMA key vouched only by that CA refused (D40, D46); from P4b, under Secure Boot, with
   `-smbios type=11,value=io.systemd.stub.kernel-cmdline-extra=rd.systemd.debug_shell`, the
   string is absent from `/proc/cmdline` (D49); from P4b, with the chosen IMA
   configuration, an unsigned `/etc/ima/ima-policy` is not the active policy and writing a
   text rule to `/sys/kernel/security/ima/policy` fails (D46). From P4b, with the dm-verity option (otherwise the checks S1 decides): the
   IPE boot policy is active, with `ipe.success_audit=1` on
   the test command line IPE audit shows `dmverity_signature` true for `/usr`, modules and firmware load from `/usr` with no `KMODULE`
   denial for `ipe_hook=KERNEL_LOAD`, and `keyring_unsealed` reads `N` with an empty `.dm-verity` keyring. From P5, under Secure
   Boot, an unsigned addon is rejected.
3. **ISO acceptance** (`forge/test/iso`): automated in KVM on every run (virtio, NVMe,
   OVMF with Secure Boot): a required `profile-ok` marker emitted by
   `athanor-profile-check`, whose failure report lists the drifting settings; from P4b the
   installer refuses a CPU without x86-64-v3 and a non-UEFI boot and the real initramfs
   booted on a CPU model without x86-64-v2 and on `SandyBridge` (AVX without AVX2) stops
   with the `athanor-cpu-check` message, which tests CPUID leaves 1, 7 and 0x80000001 and
   the YMM state enabled in XGETBV; from P3 a deliberate panic leaves a pstore record (D19); from P4b a
   binary executed from the home appears in the IMA measurement log (D23, D46); from P4b, with
   swtpm, a TPM2-sealed `systemd.extra-unit.*` credential in `<uki>.efi.extra.d/` produces
   no unit, and TPM unlock succeeds with `systemd.import_credentials=no` set; from P4b, TPM
   unlock succeeds on the real generic initramfs with a plain ESP and with XBOOTLDR, and
   profile 0 of a UKI built with `--sign-profile`, `--policy-digest` and `--join-pcrsig`
   finds its PCR signature; from P4b, the generic initramfs boots with the root disk on
   `mptsas1068` and `lsi53c895a` controllers; from P3, with the D47 sysctls, the eBPF
   probes attach and shipped observability tools resolve kernel symbols or state that
   they cannot; from P6
   zero AVC denials in Athanor domains, with
   Fedora denials triaged (D20). A hardware matrix is run by the maintainer before each
   release gate (VMD, AMD and Intel eMMC, a laptop with an I2C or THC keyboard, an Apple
   SPI keyboard, a Thunderbolt dock, Hyper-V, VMware and Xen guests; D17); hardware not
   available to the maintainer is recorded as untested, not as passed. The matrix also
   records the devices that need firmware loaded without a file or from outside the image
   (D48), boots the compatibility profile (D45), and installs next to an existing Windows
   ESP with XBOOTLDR (D17). The base profile
   runs on every acceptance, and from P5 the desktop role on every run and the laptop
   role weekly.
4. **On the machine:** `athanor-profile-check` gates `boot-complete.target` on
   image-attributable drift only and reports the rest: `/proc/config.gz`,
   `/proc/cmdline` (including build options overridden by a boot parameter), sysctls,
   `scx_loader` state, swap on zram active and zswap off (D15), integrity mode, with the dm-verity option the active IPE policy (from
   P6, the boot policy still active fails the gate), the contents of `.machine` and `.secondary_trusted_keys`, external
   PCIe ports marked untrusted (D16), from P4b, with `systemd-sysupdate`, `LoaderEntryDefault` absent and
   `LoaderEntryPreferred` set (with bootc, the equivalents S1 records), the absence of
   `/etc/systemd/import-pubring.*` and of `/etc/sysupdate.d` definitions where applicable
   and `/etc/ima/ima-policy` masked (D46), and from P5, when a TPM is present, that the roles applied under `/run` match the
   role addons in the PCR 12 event log.
5. **Release 1.0 updates** (P4b, in a VM with Secure Boot and swtpm): an update with an
   invalid manifest signature, an expired manifest, a version below the minimum or a
   digest mismatch is refused; a new version does not become the boot default before the
   user confirms; with `systemd-sysupdate`, the preferred entry is set and read back before anything new
   is written (with bootc, the staged deployment stays locked until confirmation); a reboot, a crash and a power loss during installation or before confirmation
   boot the running version; the booted version is never removed; the previous version
   boots once from the menu; a failing health check marks the entry bad (through `OnFailure=` with systemd-boot, or the S1
   equivalent with bootc) and the
   next reboot returns to the previous version, never to the compatibility profile; the new default entry finds the addons of the
   active roles (from P5); TPM-only unlock is refused in degraded mode and when booting
   the compatibility profile, and the TPM keyslot cannot be unsealed on the running system
   after `leave-initrd`; the pcrlock policy contains both PCR 7 and PCR 14 and
   survives an announced shim, MokList, dbx and `mokutil --untrust-mok` change; an unannounced PCR 7 change falls
   back to the recovery key, and the guided reseal keeps the chosen unlock method only while
   attested mode holds, refusing TPM-only after Secure Boot is disabled, after an unannounced MokList enrolment
   or after dbx is reset to an older content; a reseal started from the compatibility profile is
   deferred, and with `user-modules` it offers only the passphrase; a PCR
   policy key rotation re-enrols the TPM keyslot through the guided flow; a UKI revoked through MokListX does not boot; an addon signed with the test key is
   rejected on a machine enrolled with the production key; from P6, with the dm-verity option, selecting the
   `user-modules` variant, through `athanor-role` or by copying its addon directly, makes
   every TPM keyslot fail; a rollback boots with the `/etc` state that the design chosen for D39 assigns to the
   previous version; a first boot reaches MokManager and then
   boots the installed system; after a declined or incomplete enrolment, a Secure Boot
   machine returns to MokManager and enrols from disk, and a machine without Secure Boot
   boots degraded and offers the enrolment again;
   the installer refuses firmware whose db does not accept the shim's certificate
   authority; systemd-boot, UKIs and addons carry SBAT sections (D41); a Flatpak
   application update applies without interrupting the session (D31 class A). Also in P4b: a VM killed while shim or systemd-boot is written still boots;
   the update service refuses a shim whose SbatLevel revokes an installed UKI or addon;
   boot loader files are replaced only from a boot marked good, through a temporary name
   and a rename; after a PCR policy key rotation, the previous version boots once from the
   menu and asks for the recovery key or passphrase, and the guided reseal is deferred on that
   boot (D42).
6. **Execution integrity** (P6; the IPE items with the dm-verity option, otherwise the
   measures S1 decides): a module outside the image and an unsigned module are refused on
   the desktop class; a firmware file outside the image and firmware loaded without a file are refused, and
   admitted with the `user-modules` variant (D48); `enforce=0` is refused once
   enforcing; activating a signed policy of a lower class is refused, including the
   `user-modules` variant on a desktop-class machine, and so is an older release of the
   same class; the boot policy is no longer active at `boot-complete.target`; a system
   domain calling `mprotect(PROT_EXEC)` on anonymous memory, or mapping a verified file `MAP_PRIVATE`
   with `PROT_WRITE|PROT_EXEC`, or re-enabling `PROT_EXEC` on a private mapping of a
   verified file after writing it, is denied (D24); a service outside the `athanor-io-uring`
   group cannot create an io_uring instance; an application built with `athanor-sandbox`
   is refused access outside its declaration; an XDP
   program of a delegated service cannot attach to a host interface; with the mesh class
   policy, execution outside the image and anonymous executable memory are denied, while
   the initrd still reaches switch-root after activation and the host shuts down cleanly; with a role set that has no class policy, and with a
   class policy whose signature is corrupted, the VM halts after unlock with no switch-root
   and no shell on any console; the initramfs contains dracut's `emergency.service` and
   `emergency.target`.
7. **Attestation** of mesh hosts (restricted area, after 1.0).

## 13. Benchmarks and provisional decisions

Last phase before 1.0, on real hardware of more than one CPU vendor where available and
not in a VM: latency (`schbench`, `cyclictest`), throughput (`hackbench`, kernel
compilation, `fio`), network (`netperf`). At least ten runs per configuration, reported as
median and dispersion. The results close D5 (BORE against plain EEVDF), D13 (lazy against
full), D15 (swappiness, zram writeback on small machines), D16 (the cost of
`iommu.strict=1` where a role wants it), the costs of `KSTACK_ERASE`, `INIT_ON_FREE_DEFAULT_ON`,
`PAGE_TABLE_CHECK`, `DEBUG_VIRTUAL`, `DEBUG_SG` and `UBSAN_TRAP`, `scx_lavd` PowerSave
against BORE with `power-saver` on battery, THP, and the mesh network settings. AutoFDO
profiles are collected with representative workloads and committed with their hashes; a
stale profile is tolerated across bumps. A setting that costs too much is recorded as a
decision, never silently removed; hardware not available is recorded as unmeasured.

## 14. Outside the profile, tracked

Found on the running system and in the repository (2026-09-14):

- **NVIDIA** does not load on the deployed image (`Key was rejected by service`,
  `systemd-modules-load.service` failed, nouveau in use): its modules are signed by the
  retired MOK, which the running kernel cannot verify with Secure Boot disabled, and the
  image predates the new module certificate. Images from Orchestrator runs 34842214986 and
  34853705590 are built but not deployed, and NVIDIA kmod run 34854397484 failed in its
  sign job on an artifact download error after the green run 34847702229. The cut-over is
  the redeploy of the newest image; `MOK_PRIVATE_KEY` is deleted once a deployed system loads `nvidia`
  signed by the module signing key (task 9 of
  `docs/superpowers/plans/2026-09-13-signing-key-rotation.md`). Until then `cosmic-comp`
  floods the journal with `VRR_ENABLED` warnings under nouveau.
- **Kernel series:** Azoth is pinned to 7.1.8 while 7.1 is end of life; the bump bot,
  scheduled daily from the default branch `iso-v0` (first scheduled run pending), moves it to 7.2 (D37).
- **CI signing:** the Secure Boot key is used in the same job as the image build and
  third-party actions, and the `signing` environment, which had no required reviewers, requires the
  maintainer's approval since 2026-09-14 (D43; immediate item of section 15).
- **Base configuration** (P3): `ermete-base-config` came with the former `ermete-base-nvidia`
  base and duplicated `10-ermete.conf` (scx_loader), `99-ermete-slim-boot.conf`,
  `99-Ermete-Base.preset`, `10-ermete-hw-groups.conf`, `kargs.d/01-nvidia.toml` and a
  `bootc-fetch-apply-updates` override, with masks for `akmods@` and `dkms`; it disappears with
  the switch to `base-atomic` (doc_system_image.md, S1), which does not ship it. The NVIDIA dracut
  and kargs configuration moved to `athanor-nvidia-config`, installed only by the NVIDIA image
  variants (doc_system_image.md, S4). The `bootc-fetch-apply-updates` override that staged
  updates automatically and the preset line naming the non-existent `bootc-fetch-apply.timer`
  are removed; `athanor-update` disables `bootc-fetch-apply-updates.timer` and ships a
  `--download-only` timer that follows section 8 (`doc_shell.md`, SH11; `doc_update_trust.md`, UT1).
- **Command line sources:** `kargs.d` 02–06, `system/athanor-install.ks`,
  `system/scripts/assemble_uki.sh`, `forge/specs/athanor-secure-boot/SOURCES/usr/libexec/athanor-secure-boot-measure.sh`
  (`iommu=pt`, `oops=panic`,
  `pti=on`, `zswap.enabled=1`, `splash`, `fastboot`, `rootflags=noatime`, ...) and
  `forge/specs/azoth/cmdline`, which the boot matrix uses with `zswap.enabled=1`,
  `lockdown=` and `preempt=full`: all aligned to section 6 (P3; the boot matrix command
  line in P2).
- **Sysctl and memory** (P3): `99-bore.conf` forces BBRv1 and CFS tunables that fail
  under EEVDF; `99-azoth-sysfs.conf` enables KSM; the machine has no swap at all;
  `kernel.yama.ptrace_scope` is 0, and `oops=panic` sets `kernel.panic_on_oops=1` on the
  desktop (D47, D19).
- **Image content** (P3): `kernel-devel` and `kernel-headers` 6.18 from another vendor,
  `kernel-uki-virt` with addons unused by the boot path, `bcachefs-tools`,
  `athanor-tetragon` and a Fedora 41 `bore-sysctl` package are installed.
- **Units** (P3): `athanor-journal-seal.service` fails on every boot because Fedora's
  systemd lacks forward-secure sealing: the unit, `Seal=yes` in `99-immutable.conf` and
  its preset line leave the image. `system/Containerfile` enables `tetragon.service`, `athanor-tpm-luks-seal.service`,
  `athanor-tpm-rollback-check.service` and `athanor-tpm-rollback-update.service`,
  and `preset-all` disables them again. Shipped disabled and reviewed in a dedicated
  session before P6: `athanor-secure-boot`, `athanor-lvfs-rs`, `athanor-backup`,
  `athanor-recovery`, the TPM rollback units and `athanor-tpm-luks-seal.service`.
  `athanor-gatekeeper-rs`, `athanor-daemon` and `athanor-store-rs` were removed from the
  image on 2026-09-17 pending redesign; the Gatekeeper and attestation are restricted areas.
- **Snapshots** (P3): `athanor-timewarp` targets bcachefs, which left mainline in Linux
  6.18, and misdetects `/var/home` as tmpfs; `athanor-backup-hourly` fails because
  `athanor-backup` is disabled. Both are ported to btrfs subvolume snapshots.
- **LUKS script** (P4b): `athanor-tpm-luks-seal.sh` has a syntax error (`|| {` after `fi`)
  and binds LUKS to PCRs 0, 2, 7 and 11: it is replaced by the LUKS policy of D42.
- **Boot ordering** (P3): udev reports unknown groups (`disk`, `kvm`, `render`, `audio`,
  `lp` and others) and tmpfiles cannot apply the journal ACLs early in boot.
- **Desktop** (P3): `cosmic-panel.service`, shipped by `athanor-system-services`, sets
  `MemoryHigh=1G` and `MemoryMax=1536M`, which kill the panel under normal use; the limits
  are removed in that package.
- **Placeholder security code** (before any of these crates re-enters the package DAG, and before the mesh delivery):
  `athanor-mesh-sync`, `athanor-cluster-mesh` and `athanor-mesh-bus` are workspace
  members excluded from the package DAG (`experimental/EXEMPT`); `athanor-mesh-sync`
  returns all-zero Kyber and Dilithium public keys while logging post-quantum key
  exchange, and `athanor-hypervisor-daemon`, also excluded, derives attestation from the
  existence of device files. They are removed or made to fail explicitly (D38).
  `doc_cloud_mesh.md` is replaced by the mesh specification (D27).
- **Retirements** (P3): `athanor-ebpf-sched` (embeds a `candle` AI model) is retired as a scheduler.
  `.github/workflows/live-patching.yml` builds kernel live patches, which section 5 removes
  (`LIVEPATCH`); it leaves the repository.
- **Userland flags** (after 1.0, with the Forge pipeline review): `forge/config/rpmmacros`
  includes `-mlam=u48`, an Intel-only feature, for every CPU vendor.
- **Maintainer machine** (at the reinstallation): Secure Boot is disabled; MokList holds
  the Fedora CA, a uBlue kernel key, three akmods keys and three retired Ermete OS
  certificates, including the retired project MOK, while the new Athanor Secure Boot
  certificate is not enrolled.

## 15. Implementation blocks

A new block group P in `NEXT.md` (Italian heading `BLOCCO P`); no block of the P sequence starts before
the gate of the previous P block is green; the immediate items are independent, and S1 is
outside that sequence: it starts after P0, runs alongside P1–P4a, and P4b waits for it.

**Immediate, independent of P0**, each with its own gate:

| Item | Gate |
| --- | --- |
| Release chain (Kernel Build, NVIDIA kmod and system image workflows) after pull request #25 (merged) and the NVIDIA cut-over | NVIDIA kmod green, `modinfo -F signer nvidia` on the redeployed machine names the module signing key |
| The 7.2 bump after pull request #26 (merged; D37) | the 7.2 bump pull request merged with the Kernel Build gate (doc_kernel_build.md section 7) green |
| Sign-only CI jobs and shared key custody (D43) | the `verify.py workflows` check of section 12 item 1 green, and the protection rules of the signing environment include required reviewers |

| Block | Content | Gate |
| --- | --- | --- |
| P0 | this specification | maintainer approval (passed 2026-09-14) |
| S1 | spike, no time box, on Fedora 45 (beta, systemd 262 release candidate): bootc sealed composefs with UKI against dm-verity with `systemd-sysupdate`, built and exercised in a VM with Secure Boot and swtpm; covers `/etc` (D39), boot counting, the confirmation of section 8, fallback, update and rollback, the manifest and the verification path, IPE coverage, signing and maintenance cost | a written comparison with measurements; D6 and D39 closed by the maintainer. Runs alongside P1–P4a; P4b waits for it |
| P1 | `profile.toml`, validator, `athanor-profile-check` covering settings already in force; later blocks extend it | acceptance with `profile-ok` |
| P2 | kernel build profile (section 5) without the items marked P4b, P5 or P6; the boot matrix of section 12 item 2 without its P4b and P5 parts, with its command line aligned to section 6 | Kernel Build gate green |
| P3 | `athanor-kernel-profile` base package, removal of old `kargs.d`, `99-bore.conf` and the kickstart command line, zram, the base sysctls of D47, NVIDIA configuration by detection, the clean-ups of section 14 marked P3 | acceptance `profile-ok` on the base profile and the pstore check of section 12 item 3 |
| P4a | rebase on Fedora 45, starting on the beta as soon as P3 is green (D25) | full DAG, image and acceptance green; the reinstall image waits for the final release |
| P4b | the release 1.0 update chain on the mechanism chosen by S1: shim and systemd-boot installation and updates, UKIs with both profiles, verified images (with dm-verity, the root hash signature verified by the kernel; with bootc, the sealed composefs digest), boot counting, update manifests, full-image updates with confirmation before the new default, `/etc` per D39, generic minimal initramfs with `athanor-cpu-check`, installer checks (x86-64-v3, UEFI, shim authority), ESP or XBOOTLDR, integrity, PCR policy and update keys (generated offline by the maintainer), and, if dm-verity is chosen, `DM_VERITY=y` and the compiled IPE boot policy, LUKS per D42 with the pcrlock policy file unit and the guided reseal mechanism, IMA per D46, the systemd-stub of D49 | section 12 items 2, 3 and 4 parts marked P4b and item 5 without its P5 and P6 parts, green in a VM with Secure Boot and swtpm; D46's signed IMA policy, `IMA_LOAD_X509` and `IMA_READ_POLICY` decided; waits for systemd 262 final in Fedora 45 updates (D25) |
| P5 | roles and their addons per UKI, generator, composition and precedence, `athanor-role`, `IA32_EMULATION_DEFAULT_DISABLED` with the interactive addons | validator over every combination; section 12 items 2, 3, 4 and 5 parts marked P5; acceptance desktop and laptop |
| P6 | with the dm-verity option: IPE desktop class enforced through the class policy activation unit (section 10), the `user-modules` variant, firmware per D48, class-ordered policy versions, the one-way `enforce` patch (D44), the mesh class policy built and tested; with bootc: the module and firmware measures S1 decides. For both: the D20 gate and SELinux `DEVELOP`/`BOOTPARAM` off; SELinux `execmem` restrictions (D24); BPF token delegation, io_uring group, `athanor-sandbox` crate | section 12 items 3 (AVC) and 6 and the parts of items 4 and 5 marked P6 green, item 6 in the variant of the mechanism chosen by S1 |
| P7 | benchmarks, AutoFDO, closing the provisional decisions | no decision left provisional; report |
| 1.0 | release gate | the immediate items, S1 and P1–P7 green; the maintainer's hardware matrix run, with untested hardware recorded |

After 1.0, each with its own specification and gates: deltas, layers and update classes B
and C, channels and gradual rollout (releases 1.1 and 1.2, section 8); the mesh
specification (D27), then the mesh network, sync and backup with the two tiers of D38;
later compute sharing and remote applications once their prerequisites (D28, D29) hold.

After P5 is green in a VM and the image is built on the Fedora 45 final release (D25): backup of `/var/home` and reinstallation of the maintainer's
desktop on the new image.

## 16. Sources

Verified during the design and its verification loops (2026-09-13/14):

- Kernel v7.2 (and v7.1 where compared): `init/Kconfig` (`RUST` dependencies, `KCMP`),
  `arch/Kconfig` (`AUTOFDO_CLANG`, `PROPELLER_CLANG`), `arch/x86/Makefile`,
  `kernel/Kconfig.preempt`, `kernel/sched/core.c` (`Dynamic Preempt:`), `certs/Kconfig`,
  `certs/system_keyring.c`, `certs/blacklist.c`, `crypto/asymmetric_keys/restrict.c` and
  `pkcs7_trust.c`, `security/Kconfig` (`MSEAL_SYSTEM_MAPPINGS`),
  `security/integrity/Kconfig`, `security/integrity/platform_certs/machine_keyring.c`,
  `security/integrity/iint.c`, `security/integrity/ima/ima_efi.c`,
  `security/integrity/digsig.c`, `security/ipe/*` (`fs.c`, `eval.c`, `policy.c`,
  `policy_parser.c`, `audit.c`, `ipe.c`), `Documentation/admin-guide/LSM/ipe.rst`,
  `drivers/md/dm-verity-verify-sig.c` and `dm-verity-target.c`,
  `drivers/firmware/efi/Kconfig` and `efi-pstore.c`,
  `drivers/base/firmware_loader/fallback_platform.c`, `drivers/block/zram/zram_drv.c`,
  `drivers/iommu/iommu.c`, `dma-iommu.c` and `amd/init.c`, `drivers/mmc/host/sdhci-acpi.c`,
  `arch/x86/kernel/dumpstack.c` and `kernel/exit.c`, `kernel/kexec_file.c`,
  `kernel/liveupdate/Kconfig`, `kernel/ptrace.c`, `kernel/module/signing.c` (upstream and
  Red Hat patch `patch-7.1-redhat.patch` of kernel 7.1.8-100.fc43), `kernel/bpf/token.c`
  and `syscall.c`, `net/core/dev.c`, `io_uring/io_uring.c`, `security/landlock/syscalls.c`,
  `mm/Kconfig`, `scripts/sign-file.c`, `Documentation/admin-guide/kernel-parameters.txt`;
  commits f2c61db29f27 (bcachefs removal), 0c8c88b8eb82 (overlayfs verity fix);
  kernel.org `releases.json` (7.1 end of life on 2026-09-02); the running configuration
  of Azoth 7.1.8 and the NVIDIA kmod runs 34834656982 (the failure that exposed the platform keyring fallback) and
  34847702229 (green after the fix).
- systemd: v258 `man/systemd-stub.xml`, `man/systemd-boot.xml`, `man/sysupdate.d.xml`,
  `man/systemd.exec.xml`, `src/core/exec-invoke.c`, `src/core/ima-setup.c`,
  `src/veritysetup/veritysetup-generator.c` and `veritysetup.c`,
  `docs/TPM2_PCR_MEASUREMENTS.md`; main `src/boot/boot.c`, `src/boot/stub.c` and
  `src/boot/util.c` (entry sorting, `LoaderEntryPreferred`, addon locations, profiles);
  v262-rc2 `man/systemd-sysext.xml`, `man/kernel-command-line.xml`,
  `man/systemd-cryptenroll.xml`, `man/systemd-pcrlock.xml`, `man/ukify.xml`,
  `man/systemd-bless-boot.service.xml`, `src/pcrlock/pcrlock.d`; `NEWS` for 260–262;
  Fedora systemd versions (F43 258.10, F44 259.8, F45 262~rc1).
- bootc: `docs/src/experimental-composefs.md`, `bootloaders.md`, `filesystem.md`,
  `upgrades.md`, `boot-failure-detection.md`, `crates/lib/src/cli.rs`,
  `crates/lib/src/bootc_composefs/update.rs`, issues #7, #1976, #2079, #2174; composefs
  issue #360.
- shim `README.tpm`, `mok.c`, `verify.c`, `pe.c`, `loader-proto.c`,
  `SbatLevel_Variable.txt`, Fedora `shim-unsigned-x64.spec`; Red Hat article on the
  Microsoft UEFI CA 2011 expiry; Keylime measured boot documentation and `example.py`;
  Azure Linux OS Guard documentation; power-profiles-daemon README.
- Fedora `selinux-policy-targeted` 43.8 (queried with setools on the running system);
  SELinux CIL documentation (`deny` rules).
- desync README and releases (v1.1.3); RAUC advanced documentation; CachyOS
  `linux-cachyos/PKGBUILD` and `kernel-patches` at the pinned commits (BORE 6.6.3 on 7.1,
  6.8.0-rc1 on 7.2); Mesa Venus documentation; `drivers/gpu/drm/xe/xe_pci.c` (SR-IOV
  platforms); cosmic-comp and xdg-desktop-portal-cosmic issues on remote desktop and
  virtual outputs; QEMU `target/i386/cpu.c` (CPU model features).
- kernel-hardening-checker runs on the installed system.
