# Spike S1: the two-step bootc chain on Fedora 45

Probe plan of spike S1 (issue #124, milestone 0.3), written on 2026-10-09 against
`origin/iso-v0` at `52352f15`. Nothing below has run yet: every item carries its probe and the
observation that passes or fails it, and its **Answer** is filled in when the spike runs.

The question format follows the shell-standard spikes (`docs/shell-bench/spikes.md`):
question, run, answer, verdict. S1 is a platform spike, not a shell one, so it has a directory
of its own here.

## Scope

Under A2-8 ([ADR-0043](../decisions/0043-bootc-in-two-steps.md), amended for execution control
by [ADR-0076](../decisions/0076-platform-scope-for-1-0.md)), release 1.0 is bootc on the ostree
backend, booted by Fedora's shim and GRUB, with greenboot as the automatic fallback and the
Azoth kernel signed with the project Secure Boot key enrolled as a MOK. Release 1.1 moves to
sealed composefs with UKIs once bootc's composefs backend has boot counting.

S1 verifies that plan on Fedora 45 (`doc_kernel_profile.md`, section 15, row S1; D6; D39). It is
not a bake-off: the `systemd-sysupdate` and dm-verity branch is not built, and IPE is not
measured, because A2-8 gave it up. The acceptance of S1 is the four `/etc` checks of D39
(decision B2-1, 2026-10-08) and a written report with the measurements below. S1 has no time
box (A2-31) and P4b waits for it.

Sources: [doc_kernel_profile.md](../architecture/doc_kernel_profile.md) sections 8, 12 and 15;
[doc_recovery.md](../architecture/doc_recovery.md) R5;
[doc_update_trust.md](../architecture/doc_update_trust.md) section 6;
[the update chain order](../superpowers/plans/2026-10-08-update-chain-order.md) sections 1 to 5.

## 1. Inputs

Checked on 2026-10-09 from this machine.

| Input | Reference | Digest |
| --- | --- | --- |
| Base of the Athanor image (`system/Containerfile`, stage `system`) | `quay.io/fedora-ostree-desktops/base-atomic:45`, version `45.20261009.0`, kernel `7.2.9-300.fc45` | index `sha256:2c4fec150532fe1f3c30645f532e63c1ff3280791828c31364aedbd616c8c842`; amd64 `sha256:aa550e38df19a245b31ec40252c41670bcb78e350b0f47f6ea7c7e33211ed065` |
| Fedora's bootc reference image (composefs probe only, item 7) | `quay.io/fedora/fedora-bootc:45`, version `45.20261009.0` | index `sha256:c4002d9d14baf04b6bf076b6da3cb17cfa2ab661cb11e560de22ac0572fe16b9`; amd64 `sha256:5feded3391842ad44aaf7b45dc15eb3fa6711383ce2c57e007d9772f4b9dfc33` |
| Current base, for comparison | `quay.io/fedora-ostree-desktops/base-atomic:43` | `sha256:aa38745b34ff9ebc65976303af139ddd5528c8c67aaba683b04fcfcf87976efa` (the digest pinned in `system/Containerfile`) |
| Fedora 45 release state | `dl.fedoraproject.org/pub/fedora/linux/releases/test/45_Beta/` exists; `releases/45/` answers 404 | Beta published, final not (due 2026-10-20, ADR-0078) |
| Kernel | the Azoth vmlinuz signed for Secure Boot, from `azoth-boot` by the digest `system/kernel-artifacts.sh resolve` records | an fc43 build: S1 measures the MOK chain, the fc45 kernel rebuild is P4a |
| Secure Boot certificate enrolled as MOK | `forge/specs/azoth/keys/secureboot/athanor-secureboot.pem` (public) | no private key is used anywhere in S1 |

The images are pinned by these digests for the whole run. The package versions that matter
(`bootc`, `ostree`, `rpm-ostree`, `systemd`, `grub2-efi-x64`, `shim-x64`, `bootupd`,
`containers-common`, `selinux-policy`, `composefs`) are recorded by item 0 from the built
image, because the Fedora package metadata service does not index `f45` yet.

## 2. Probes

Test images. Each is built `FROM` the Fedora 45 acceptance image `v2` (section 3), signed with
the acceptance key `acc-1`, and published to the acceptance registry under its own tag:

- `bad-greeter`: a drop-in that makes `greetd.service` run `/usr/bin/false` (the image of
  `doc_recovery.md` acceptance 7);
- `panic`: `/usr/lib/bootc/kargs.d/90-s1-panic.toml` with `kargs = ["panic=10"]`, and a
  unit `s1-panic.service` enabled in `sysinit.target` (`DefaultDependencies=no`,
  `Before=sysinit.target`, `ExecStart=/bin/sh -c 'echo c > /proc/sysrq-trigger'`). It runs in
  the real root after the switch from the initrd, so nothing in the initrd's handling of the
  command line can skip it, and before greenboot, which never runs. A write of `c` to
  `/proc/sysrq-trigger` by root crashes the kernel whatever `kernel.sysrq` says (that sysctl
  gates the keyboard only), and `panic=10` reboots ten seconds later, so every try is spent
  the same way;
- `unsigned-kernel`: the deployment's `vmlinuz` with its Authenticode signature removed
  (`sbattach --remove`, in a build stage that has `sbsigntools`);
- `etc-v2`: `v2` plus a `sysusers.d` user `s1img`, a new file `/etc/s1-new.conf`, and a
  changed `/etc/issue`.

### Item 0: what Fedora 45 ships

**Question.** Which versions of the chain's components the Fedora 45 image carries, and which
Athanor pieces are present and enabled.

**Run.** In the booted F45 `v1` guest: `rpm -q bootc ostree rpm-ostree systemd grub2-efi-x64
shim-x64 bootupd containers-common selinux-policy-targeted composefs greenboot-rs
athanor-update`; `bootc --version`; `bootupctl status --json`; `systemctl is-enabled
greenboot-healthcheck.service greenboot-set-rollback-trigger.service grub-boot-success.timer
bootloader-update.service`; `cat /etc/greenboot/greenboot.conf`.

**Pass.** `greenboot-rs` (not Fedora's `greenboot`) and `athanor-update` installed;
both greenboot units enabled by `80-athanor-recovery.preset`; `GREENBOOT_AUTO_REBOOT=false`.
**Fail.** Any of them missing or disabled: P4a input, the remaining items wait.

**Answer.** Not run.

### Item 1: `/etc` per deployment (D39, the acceptance of S1)

**Question.** Whether bootc's per-deployment `/etc` with its three-way merge gives the four
D39 subjects the state `doc_kernel_profile.md` section 12 item 5 requires: "a rollback boots
with the `/etc` state that the design chosen for D39 assigns to the previous version".

**Run.** Three phases on the F45 guest, the same record taken at each point (`R`), kept
whole: the AVC part of the record taken on `v1` in phase 1 is the baseline later records are
compared with.

```
R = { getent passwd s1a s1b s1img; sudo getent shadow s1a | cut -d: -f1,3;
      cat /etc/machine-id;
      nmcli -g NAME connection show;
      sudo semanage boolean -l -C; sudo semodule -l | grep s1;
      getenforce; sudo ausearch -m AVC -ts boot --format text;
      cat /etc/issue; ls /etc/s1-new.conf; sudo ostree admin config-diff | wc -l }
```

1. On `v1`: `useradd s1a`; `passwd s1a` (password P1); `nmcli connection add type dummy
   ifname s1d0 con-name s1-before`; `semanage boolean -m --on virt_use_nfs`; a one-rule CIL
   module `s1before.cil` loaded with `semodule -i`; a local edit of `/etc/issue`.
   Take `R`.
2. Update to `etc-v2` through the update service (download, `Apply()`). Take `R`.
3. On `etc-v2`: `useradd s1b`; `passwd s1a` (password P2); `nmcli connection add type dummy
   ifname s1d1 con-name s1-after`; `semanage boolean -m --on virt_sandbox_use_all_caps`;
   `groupadd s1g`; `gpasswd -a s1a s1g`. Take `R`, with `getent group s1g` and
   `sudo getent gshadow s1g`. Then `GoBack()` (bootc rollback) and reboot. Take `R` on `v1`,
   read the rollback notice (`screenshot.sh`), and log in as `s1a` with P2. Then roll forward
   with `bootc rollback` and take `R` once more.
4. The same three phases across the major upgrade: from the Fedora 43 guest that `create.sh`
   installs, switch to the F45 `v1` image, then roll back to Fedora 43.

**Pass**, per subject:

- *User and group databases.* After phase 2, `s1a` and `s1img` both resolve; the merge keeps
  the local `/etc/issue` and adds `/etc/s1-new.conf` (the three-way merge). After the rollback,
  see the rule of phase 3 below.
- *`machine-id`.* The same value in every record, across both Fedora releases.
- *NetworkManager connections.* `s1-before` survives the update; after the rollback the list
  matches phase 1.
- *SELinux policy store.* After the update, `getenforce` reads `Enforcing`, the local boolean
  and module of phase 1 are present, and no AVC denial appears that the `v1` baseline does
  not hold; after the rollback the policy store loads with the phase 1 customisations and no
  relabel is required.

**Phase 3 after the rollback** (rule decided by the maintainer on 2026-10-09, ADR pending; not
yet specification text). On rollback, `passwd`, `shadow`, `group` and `gshadow` are carried
forward from the current state; the rest of `/etc` reverts to the previous deployment's, and
the rollback notice says so.

- *Pass:* on `v1`, `s1b` and `s1g` resolve, `s1a` is a member of `s1g`, the `shadow` change
  date of `s1a` is the phase 3 one and P2 logs in while P1 does not; `s1-after` and the second
  boolean are absent and `/etc/issue` is `v1`'s; the notice states that the configuration
  returned to the previous version except accounts and passwords. After the roll forward, the
  four files again equal the state just before it.
- *Fail:* any of those four files taken from the previous deployment (P1 working again,
  `s1b` or `s1g` gone), any other `/etc` file carried forward, or a notice that does not say
  so. With ostree's model, where the previous deployment keeps the `/etc` it had, today's
  bootc is expected to fail this; that failure is the finding the report records.

**Fail.** A subject that loses phase 1 state through the update, a `machine-id` that changes, an
AVC denial absent from the `v1` baseline, a non-enforcing boot, or an `/etc` that no longer
matches either deployment.

**Answer.** Not run.

### Item 2: boot counting and greenboot's fallback

**Question.** Whether greenboot-rs on Fedora 45 marks a failing deployment and returns to the
previous one without rebooting by itself (`doc_recovery.md` R5, as decided by A2-26), and what
happens to a deployment that never reaches user space, where greenboot cannot run.

**Run.**

- G0, static: `grep -n boot_counter /boot/grub2/grub.cfg /boot/grub2/*.cfg`, `sudo
  grub2-editenv list`, `ls /boot/loader/entries`. Records whether bootupd's static GRUB
  configuration on Fedora 45 carries the boot counter at all.
- G1, `bad-greeter`: `doc_recovery.md` acceptance 7 as written: stage and apply it; in that
  boot, ten minutes without a reboot (`journalctl --list-boots` count unchanged); `bootc status
  --format json | jq .status.rollbackQueued` reads `true`; the recovery console on `tty1`
  (screenshot through `console.sh` or the SPICE display); then `sudo systemctl reboot` and
  `bootc status` shows `v2` booted; three `athanor-update-check` runs download nothing
  (`.update` reads `held`).
- G2, `panic`: stage and apply it; watch `console.log` for 15 minutes and count `Kernel panic`
  lines and GRUB menus; record which deployment is running at the end, if any.
- G3: on a good deployment, log in and wait three minutes, then `sudo grub2-editenv list`:
  records whether `grub-boot-success.timer` set `boot_success=1`, the reason R5 gives for not
  using the counter.

**Pass.** G1 as written in `doc_recovery.md` acceptance 7, 8 and 10. G0 and G3 are records.
G2 as section 5 point 3 states it.
**Fail.** G1: any reboot nobody asked for, no mark, a return to the bad digest, or a download
of it.

**Answer.** Not run.

### Item 3: the confirmation of section 8

**Question.** Whether a feature update stays staged and locked until the user confirms
(`doc_kernel_profile.md` section 8, step 3, with bootc), whether nothing short of a confirmed
clean shutdown applies it, and whether staging again after a discard needs no new download.

**Run.**

- C1: after the check timer, `bootc status --format json | jq '.status.staged |
  {downloadOnly, digest: .image.imageDigest}'`: `downloadOnly` true, the digest the registry
  names.
- C2: before confirmation, once each: `sudo poweroff` and start; a hard reset
  (`system_reset` on `monitor.sock`); a crash (`echo c | sudo tee /proc/sysrq-trigger`). After
  each, the marker reads `v1`; record whether `.status.staged` is still there.
- C3: after C2, run a check (`check_now` of `acceptance/lib.sh`) and read `journalctl -u
  athanor-update-check` for bootc's `layers already present` and `layers needed` lines, and
  the guest's received bytes (`ip -s link show`) before and after.
- C4: confirm (`Apply()` with an inhibitor first, as `run.sh apply` does, then without), and
  before the shutdown completes, crash with sysrq: the marker reads `v1`; a clean reboot after
  a second confirmation reads `v2`.
- C5: confirm and reboot, and send `system_reset` the moment `console.log` shows
  `ostree-finalize-staged` starting
  (`tail -F console.log | grep -m1 -i finaliz && echo system_reset | socat - unix:monitor.sock`).
- C6: on `v2`, `bootc status` lists `v1` as rollback; GRUB shows both entries; the booted
  deployment is never pruned by a later `--download-only`.

**Pass.** C1 true and matching; every C2 case boots `v1`; C3 shows zero layers needed and less
than 1 MiB received; C4 boots `v1` after the crash and `v2` after the clean reboot; C5 boots
either `v1` or `v2` and `ostree admin status` is consistent; C6 holds.
**Fail.** Any boot into `v2` without a confirmed clean shutdown, a re-download in C3, or an
unbootable or inconsistent state after C5.

**Not probed.** The security-class path of UT13 (the check service unlocks the deployment
itself): PB10 is not built.

**Answer.** Not run.

### Item 4: update and rollback

**Question.** Whether the acceptance of `doc_update_trust.md` section 6, items 1 to 15, passes
unchanged on a Fedora 45 base, and whether the major upgrade from Fedora 43 and back works.

**Run.** `scripts/devvm/acceptance/run.sh` with `ACC_BASE` naming the Fedora 45 image
(section 3), every stage from `install` to `report`, among them `rotate`, `recover` and
`goback`. Then the major upgrade: the Fedora 43 guest from `create.sh` switched to the F45
`v1`, booted, greenboot green, `bootc rollback` and reboot back to Fedora 43.

**Pass.** Every stage prints its `PASS` lines and `acceptance complete`; the major upgrade
boots, passes greenboot and rolls back to a working Fedora 43.
**Fail.** Any `FAIL` line; record the stage and the guest journal.

**Answer.** Not run.

### Item 5: the manifest and the verification path

**Question.** Whether Fedora 45's `containers-common` and bootc verify the key-based image
signature with the shipped policy as Fedora 43 does, and what on bootc fills the role of the
D41 manifest (image digest, minimum version, expiry, signed with the integrity key).

**Run.** The `migrate`, `refuse` and `older` stages of `run.sh` (item 4) cover the signed
reference, a missing, foreign and bundle-only signature, an older signed digest and a
signature copied from another repository. In addition: `bootc status --format json | jq
.status.booted.image.image.signature` reads `containerPolicy`, and `skopeo --version` and
`rpm -q containers-common` are recorded next to the Fedora 43 values.

**Pass.** The three stages pass and the booted image enforces the policy.
**Fail.** Any refusal that does not happen, or a policy that F45's `containers-common` reads
differently.

**Recorded.** The release 1.0 chain verifies a key signature and refuses a build time older
than the booted one (UT5, PL31). D41 lists an expiry; `doc_pipeline.md` section 4.8, under
PL31, leaves it out on purpose, because an expiry needs a key in a scheduled job. Decided by
the maintainer on 2026-10-09, ADR pending: no expiry at 1.0; rollback protection comes from
the signed build time (UT5), which the `older` stage checks.

**Answer.** Not run.

### Item 6: signing and the MOK chain

**Question.** Whether Fedora 45's shim and GRUB boot the MOK-signed Azoth kernel with Secure
Boot on, refuse an unsigned one, and who updates shim and GRUB after installation.

**Run.**

- S1-a: `mokutil --sb-state` reads `SecureBoot enabled`; `mokutil --list-enrolled` lists the
  Athanor certificate; `cat /sys/kernel/security/lockdown` shows `[integrity]`;
  `sudo keyctl show %:.platform` holds the Athanor Secure Boot certificate and
  `%:.builtin_trusted_keys` the module certificate.
- S1-b: stage and apply `unsigned-kernel`; `console.log` shows shim's or GRUB's refusal; the
  previous entry, chosen at the GRUB menu through `console.sh`, boots.
- S1-c: an unsigned test module is refused with `EKEYREJECTED`, as the Kernel boot matrix
  checks, now under a Fedora 45 userspace.
- S1-d: `bootupctl status` and whether any unit runs `bootupctl update`: record the shim and
  GRUB versions on the ESP before and after an image update that carries newer ones.
- S1-e (fresh install only, section 3 path B): with no MOK pre-enrolled, the first boot
  reaches MokManager and, after enrolment, boots the installed system.

**Pass.** S1-a all four readings; S1-b refused and the previous entry boots; S1-c refused;
S1-e as described.
**Fail.** Any of those not holding. S1-d is a record: an ESP that is never updated is a
maintenance cost and an SBAT risk the report states.

**Answer.** Not run.

### Item 7: maintenance cost, and the 1.1 gate

**Question.** What the chain costs to run and to keep, and whether bootc's composefs backend on
Fedora 45 has boot counting yet, the condition A2-8 sets for release 1.1.

**Run.**

- Measurements on the F45 guest, for the `v1` to `v2` update: bytes received
  (`ip -s link`), staging time (`athanor-update-check` journal), finalisation time
  (`ostree-finalize-staged` journal), boot time (`systemd-analyze`), `df -h /sysroot` with two
  deployments and the cached image.
- The list of Athanor-owned parts of the chain and what each follows upstream: `greenboot-rs`
  with its `GREENBOOT_AUTO_REBOOT` patch, `athanor-update` with `render-policy`, the kargs.d
  files, the release package, and the signing steps per release (`sign-images`, the
  `azoth-boot` signature per kernel).
- The 1.1 gate, outside the 1.0 acceptance: `bootc install to-disk --help` in
  `fedora-bootc:45` for its composefs backend option; if present, one install with it and
  `ls /boot/loader/entries` (or the ESP) for a `+N` tries suffix.

**Pass.** The measurements are recorded. The 1.1 gate is a yes or no.

**Answer.** Not run.

## 3. How to run it on this machine

### What exists

- `scripts/devvm/`: the development VM (`create.sh` from the published ISO, `start.sh`,
  `reset.sh`, `console.sh`, `monitor.sock`), 4 vCPU and 8 GB by default. It boots **plain
  OVMF without Secure Boot and without a TPM**: `ovmf_code` picks `OVMF_CODE.fd`, and nothing
  in the tooling starts `swtpm`.
- `scripts/devvm/acceptance/`: `images.sh` (registry on `127.0.0.1:5000`, throwaway keys, ten
  images `FROM $ACC_BASE`) and `run.sh` with the stages `install migrate download apply refuse
  older goback rotate recover podman report`. `ACC_BASE` is overridable, so the harness runs on
  a Fedora 45 base unchanged. `images.sh` labels every image version `43.<date>.0`; ordering
  uses `created`, so the label is cosmetic.
- `forge/specs/azoth/boot.sh`: the Secure Boot pattern to copy: `OVMF_CODE.secboot.fd`,
  `virt-fw-vars --add-mok` to enrol a certificate in MokList, `-machine q35,smm=on` and the
  secure pflash. Its container (`forge/specs/azoth/boot/Containerfile`) carries
  `qemu-system-x86-core`, `edk2-ovmf` and `python3-virt-firmware`.
- The host, checked on 2026-10-09: 16 CPUs, 31 GB, 421 GB free on `/var/tmp`; `swtpm` 0.11.0
  from Nix. **No `qemu-system-x86_64`, `edk2-ovmf` or `virt-fw-vars`**: the booted image
  `43.20261008.253` predates #321 (ADR-0091), which ships QEMU and OVMF again. The runner
  service restarts every few seconds and exits with `qemu-system-x86_64: command not found`.
  The maintainer's dev VM state, `~/.local/share/athanor-devvm`, holds `base.qcow2` of
  2026-09-18 and the development overlay `dev.qcow2`; S1 never touches it.
- State directory. `create.sh`, `reset.sh` and `start.sh` take no state-directory option; the
  location comes from `devvm.env`, documented in `scripts/devvm/README.md` as
  `${XDG_DATA_HOME:-~/.local/share}/athanor-devvm`, and `create.sh` replaces `base.qcow2`
  and `dev.qcow2` there. S1 therefore runs every dev VM script with
  `XDG_DATA_HOME=/var/tmp/athanor-s1/xdg`, so its state is
  `/var/tmp/athanor-s1/xdg/athanor-devvm`. `devvm.env` itself was not read in this session
  (the local command gate blocks shell reads of `*.env` names), so section 5 lists the
  confirmation as a prerequisite. `XDG_DATA_HOME` also moves rootless podman's storage, so the
  image builds run without it. `forge/specs/azoth/boot.sh` writes only under its `--out`
  directory and its own temporary work directory.

### Commands

Preconditions, before every start:

```
uptime                                        # load average under 4
pgrep -a qemu-system                          # nothing: no runner guest, no other VM
systemctl --user is-active athanor-devvm      # inactive
gh api 'repos/ars-regia/athanor/actions/runs?status=in_progress' --jq .total_count   # 0
command -v qemu-system-x86_64 swtpm virt-fw-vars
```

Build the Fedora 45 variant in a throwaway worktree (no commit, no push), in containers
only: `scripts/devvm/local-image.sh` builds the `athanor-update` RPM in the builder image
(`athanor-builder`, as `call-dag-compile.yml` does), serves it through a tier 3 overlay on the
acceptance registry, and builds the system image with `system/build-image.sh`, which resolves
the kernel and tier digests itself. Nothing is installed on the host. Four jobs: the builder's
`cargo` reads `CARGO_BUILD_JOBS` and `rpmbuild` reads `RPM_BUILD_NCPUS`, which a
containers.conf override puts into every container (checked on 2026-10-09: a container started
with the override prints `4 4` for the two variables; `taskset` on the host does not reach a
rootless container, which still saw 16 CPUs).

```
git -C /var/home/hr-mes/athanor worktree add --detach /var/tmp/athanor-wt/s1-build origin/iso-v0
W=/var/tmp/athanor-wt/s1-build
sed -i -e 's|^FROM quay.io/fedora-ostree-desktops/base-atomic:43@sha256:[0-9a-f]* AS system$|FROM quay.io/fedora-ostree-desktops/base-atomic:45@sha256:2c4fec150532fe1f3c30645f532e63c1ff3280791828c31364aedbd616c8c842 AS system|' \
       -e 's|^ARG FEDORA_VERSION=43$|ARG FEDORA_VERSION=45|' "$W/system/Containerfile"
git -C "$W" diff --numstat                    # 2 2 system/Containerfile
mkdir -p /var/tmp/athanor-s1
printf '[containers]\nenv = ["CARGO_BUILD_JOBS=4", "RPM_BUILD_NCPUS=4"]\n' > /var/tmp/athanor-s1/jobs.conf
export CONTAINERS_CONF_OVERRIDE=/var/tmp/athanor-s1/jobs.conf
bash "$W/scripts/devvm/local-image.sh" athanor-update
# -> RPMs in $W/.scratch/local-image/rpms, the image localhost:5000/acc/athanor-system:<tag>,
#    with <tag> in $W/.scratch/local-image/tag
```

If the tier 3 transaction fails on Fedora 45 (the tier repositories hold fc43 builds), the
failing packages are P4a input; S1 then builds its base from `base-atomic:45` with only the
chain's packages (`athanor-update`, `greenboot-rs`, `athanor-system-config`,
`athanor-recovery`, `athanor-kernel-profile`) and the `azoth-boot` vmlinuz, and says so in
the report.

Acceptance images and test images:

`images.sh` takes the `athanor-update` RPM from `ACC_RPM_DIR` (default `RPMS_OUT/` at the
root) and the base from `ACC_BASE`; both come from the step above. Its keys and registry
configuration go to `ACC_STATE`, kept out of the maintainer's state:

```
ACC_BASE=localhost:5000/acc/athanor-system:$(cat "$W/.scratch/local-image/tag") \
ACC_RPM_DIR=$W/.scratch/local-image/rpms ACC_STATE=/var/tmp/athanor-s1/acceptance \
  bash "$W/scripts/devvm/acceptance/images.sh"
# then bad-greeter, panic, unsigned-kernel and etc-v2 FROM localhost:5000/acc/athanor-system:v2,
# pushed and signed with acc-1 as images.sh's build and sign functions do
```

Install, path A (primary), in S1's own state:

```
export XDG_DATA_HOME=/var/tmp/athanor-s1/xdg ACC_STATE=/var/tmp/athanor-s1/acceptance CPUS=4 MEMORY=8G
bash "$W/scripts/devvm/create.sh"                  # newest published ISO, into S1's state only
bash "$W/scripts/devvm/acceptance/run.sh" install  # switches the guest to the F45 v1
```

`run.sh install` switches the guest to the F45 `v1`. This also exercises the major upgrade of
item 1 phase 4 and item 4. Path B (once, for item 6 S1-e): a fresh disk from the F45 image with
`bootc install to-disk --via-loopback --filesystem btrfs` in a privileged root `podman run` of
the image.

Boot with Secure Boot and a TPM. `start.sh` has neither, so S1 boots the same disk with its
QEMU command plus these differences (a `start.sh` option is left for the run, if the probes
show it is worth keeping):

```
S=/var/tmp/athanor-s1/xdg/athanor-devvm; mkdir -p "$S/s1/tpm"
virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd -o "$S/s1/vars.fd" \
  --add-mok "$(cat /proc/sys/kernel/random/uuid)" forge/specs/azoth/keys/secureboot/athanor-secureboot.pem
swtpm socket --tpm2 --tpmstate "dir=$S/s1/tpm" --ctrl "type=unixio,path=$S/s1/swtpm.sock" --daemon
#   -machine q35,smm=on -smp 4 -m 8G
#   -global driver=cfi.pflash01,property=secure,value=on
#   -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd
#   -drive if=pflash,format=raw,file=$S/s1/vars.fd
#   -chardev socket,id=tpm,path=$S/s1/swtpm.sock -tpmdev emulator,id=tpm0,chardev=tpm -device tpm-crb,tpmdev=tpm0
```

Path B omits `--add-mok`, so the first boot reaches MokManager (item 6, S1-e).

### Estimated time

An estimate, not a measurement: about **2 hours of builds** (the F45 system image at 4 jobs
through `CONTAINERS_CONF_OVERRIDE` above,
`images.sh` about 20 minutes, the four test images) and about **4 hours of VM time** over two
or three sittings: the `run.sh` acceptance about 90 minutes, item 1 about 40 minutes, item 2
about 45 minutes (G2 alone 15), item 3 about 40 minutes, item 6 about 30 minutes, item 7's
composefs install about 20 minutes.

## 4. Constraints for the run

- One VM at a time. The runner guest and the S1 guest never run together: S1 starts only when
  `pgrep -a qemu-system` prints nothing, and `run.sh` refuses to start while a workflow run is
  in progress. How S1 and the runner share the host is the maintainer's call (section 5).
- At most 4 vCPU for the guest (`CPUS=4`, `-smp 4`), 8 GB of memory, and builds at 4 jobs
  (`CONTAINERS_CONF_OVERRIDE`, section 3).
- Never the runner VM, and never the maintainer's dev VM state: S1 uses only
  `/var/tmp/athanor-s1`.
- `uptime` before every start: a load average above 4 waits.
- Fixed inputs: the digests of section 1 for the whole run; a newer Fedora 45 compose is a new
  run, recorded as such.
- No project key: only the public Secure Boot certificate and the throwaway acceptance keys.

## 5. Points that need the maintainer

1. **QEMU on the desktop.** The booted image predates #321, so the host has no
   `qemu-system-x86_64` or OVMF and the runner service fails at every start. An upgrade and
   reboot of the desktop into an image with #321 is the first step; `virt-fw-vars` then comes
   from Nix or the boot-matrix container.
2. **S1 against the runner VM** (`2026-10-08-update-chain-order.md`, section 5, point 5).
   Under "one VM at a time" S1 and the runner guest compete for the host; whether the runner is
   stopped for a sitting, and when, is the maintainer's decision. Open.
3. **Boot counting on 1.0.** D6, section 8 and `doc_recovery.md` R5 all rely on GRUB's boot
   counter with greenboot; A2-26 only turns off greenboot's own reboot. A kernel panic or hang
   in a new deployment never reaches greenboot, so the counter alone returns the machine: item 2
   G2 passes when, after the boots the user starts, GRUB selects the previous deployment once
   the tries are spent, with no step at the console. It fails if the panicking deployment stays
   the default.
4. **S1's own dev VM state.** The dev VM scripts have no state-directory option; S1 relies on
   `devvm.env` deriving its state from `XDG_DATA_HOME`, as `scripts/devvm/README.md` documents.
   Prerequisite before the first `create.sh`: confirm that line of `devvm.env` (this session's
   gate kept it unread), or give the scripts a state-directory variable, so that `create.sh`
   cannot replace the maintainer's `base.qcow2` and `dev.qcow2`.
5. **Issue #124.** Its body still describes the bake-off and asks for D6 and D39 to be closed;
   both are closed. Update the body or let the A2-8 comment stand.
