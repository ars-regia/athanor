# Update chain for 1.0: real order and what is unblocked (2026-10-08)

Scope: the kernel and platform blocks of `docs/architecture/doc_kernel_profile.md` section 15
(P2 to P7 and S1, issues #122, #123, #124, #125, #126, #127, #128, #129) and the update
control of `docs/architecture/doc_update_trust.md` UT13 (pipeline block PB10). Decision
records in `docs/decisions/` override specification text; where an issue body disagrees with
a decision record, the record wins. Checked against `origin/iso-v0` at 42dc49db.

## 1. The order

```
P1 (done 2026-10-05)
 └─ P2 kernel build profile + boot matrix ──────────── gate: Kernel Build green
     └─ P3 base package, sysctls, zram, cleanup ────── gate: acceptance profile-ok + pstore
         └─ P4a rebase on Fedora 45 (beta first) ───── gate: DAG, image, acceptance green
             └─ P4b 1.0 update chain (bootc ostree) ── needs P4a and S1
                 └─ P5 roles ── P6 execution control ── P7 benchmarks ── 1.0 gate
S1 (bootc on Fedora 45, verification spike) ──────── runs beside P2..P4a, P4b waits for it
PB4 provenance ── PB5 evidence and policy gate ──┬── PB6 signed install path
                                                 └── PB10 update control (UT13, ADR-0082)
```

- The P sequence is strict: no P block starts before the previous gate is green (section 15).
- ADR-0078 puts a date on it: the move to Fedora 45 starts on the beta once P3 is green; if
  the Fedora 45 image is not green by mid-November 2026 the fallback is Fedora 44. Fedora 43
  reaches end of life on 2026-12-02. P2 and P3 are therefore on the critical path of the base
  release, not only of the kernel profile.
- S1 has no time box (A2-31, ADR-0068) and P4b waits for it, so S1 should start as soon as
  the machine budget allows, not after P3.
- PB10 (UT13) is a pipeline block, outside the P sequence. It depends on PB5, and in fact on a
  producer of the key-signed release attestation (section 4, point 1), which no block owns.

## 2. Unblocked today on iso-v0

| Block | State | Evidence |
| --- | --- | --- |
| P2 | **unblocked, recommended first** | P1 is in tree (`forge/specs/athanor-kernel-profile/profile.toml`, `kernel_profile.py`, `athanor-profile-check`). `forge/specs/azoth/kernel-local` carries none of the section 5 P2 options (0 of the options checked by grep). `boot.sh` still runs Nehalem in 4 cases with no IOMMU case. `[base.cmdline]` still carries `lockdown`, `init_on_free`, `vsyscall`, `debugfs`, marked "stay until block P2". No external dependency: Kernel Build runs on the self-hosted runner and the hosted KVM runner. |
| S1 | startable, not plannable as code | Needs Fedora 45 beta, a VM with Secure Boot and swtpm, and a written report; its gate is a maintainer closure (D39). Under "one VM at a time" it competes with the runner VM on the desktop, so it serialises with P2 builds. |
| P3 | blocked by the P2 gate | content is independent code (package, sysctls, kargs cleanup), but section 15 forbids starting it before P2 is green |
| P4a | blocked by P3 | `system/Containerfile` stays on `base-atomic:43` until the move is green (ADR-0078). Whether `base-atomic:45` beta is published was not checked (no network in this session). |
| P4b | blocked by P4a and S1 | much of the 1.0 chain already exists on iso-v0: bootc in `athanor-update`, `greenboot-rs` with the `GREENBOOT_AUTO_REBOOT` patch, the MOK-signed kernel through `azoth-boot` |
| P5, P6, P7 | blocked in sequence | |
| PB10 / UT13 | blocked on definitions | see `2026-10-08-update-trust-ut13.md`: seven open points, four of them maintainer decisions |

## 3. External dependencies

- **Fedora 45:** beta available, final due 2026-10-20 (ADR-0078). P4a needs the beta, the
  reinstall image needs the final release.
- **systemd 262 final in Fedora 45 updates (D25):** needed by sealed composefs and UKI, which
  A2-8 moved to 1.1. For the 1.0 chain on ostree with GRUB it is no longer a prerequisite of
  P4b; section 15 still lists it (section 4, point 3).
- **Hardware:** the maintainer's hardware matrix before each release gate (D17, section 12
  item 3); hardware not available is recorded as untested.
- **Maintainer signing (#223, ADR-0084):** Secure Boot and module keys, the image key 2
  rotation (PB5b), the key of the `sign-images` job that signs the release attestation and
  the security class (PL31, PQ4). Plans never use keys; they stop at the maintainer step.

## 4. Contradictions and how the decisions resolve them

| Where | Text | Resolution |
| --- | --- | --- |
| #126 (P4b) body; doc_kernel_profile P4b row, first sentence | shim and systemd-boot, UKIs with both profiles, dm-verity root hash, PCR 11 policy keys, LUKS per D42, D49 stub, IMA per D46 | A2-8 (ADR-0043, amended by ADR-0076): 1.0 is bootc on the ostree backend with GRUB, greenboot and a MOK-signed kernel; UKI, systemd-boot and sealed composefs are 1.1. The amendment at the end of the P4b row lists what has no 1.0 carrier. The issue body was not updated. |
| #124 (S1) body; S1 row, first sentence | bake-off of sealed composefs against dm-verity, IPE coverage | A2-8: no bake-off; S1 verifies the two-step plan on Fedora 45. IPE coverage is lost under bootc (ADR-0076). |
| P4b row, D25 | P4b waits for systemd 262 final | moot for the 1.0 chain on ostree and GRUB; holds for 1.1. Needs the maintainer's confirmation (point 3 below). |
| P6 row | "with bootc: the module and firmware measures S1 decides" | ADR-0076: composefs with fs-verity for `/usr` where supported, plus `noexec` on system-writable temporary mounts. |
| Section 5, keyrings | `DM_VERITY_VERIFY_ROOTHASH_SIG`, `SECURITY_IPE` and their keyring options, not marked P4b | they serve the dm-verity option only, which 1.0 does not ship. The P2 plan keeps them as written (cheap, harmless, `SECURITY_IPE` is already a P1 setting). |
| Section 12 item 2 | the module chain "in `nvidia-kmod.yml`" | the CA cases need a signed out-of-tree module, so the P2 plan puts them in that workflow's boot job, beside the existing MOK case. |
| doc_update_delivery UD6 and decision 3 | security class promoted through `promote.sh` with a signing approval | PQ4 (ADR-0088): the class and its advisory ids are set and signed with the release in `sign-images`; promotion holds no key. doc_update_delivery is still headed "draft, rev 2". |
| doc_update_trust UT13, "no Later" | no deferral for the security class | ADR-0082 and PQ6: one postpone of up to seven days, and an opt-out with admin authorisation. UT13 text not amended yet. |
| ADR-0082 point 3 vs doc_settings.md | "Settings offers a switch" vs "Settings has no updates page" (SW16, F-settings-28) | unresolved: see the UT13 document, point 1. |

## 5. Points that need the maintainer

1. P4b dependency on systemd 262 final (D25): confirm it applies to 1.1 only.
2. D39 (`/etc`), D48 (firmware refusal under bootc), and the candidates D17 (2 GiB ESP or
   XBOOTLDR) and D41 (SBAT) remain open with the maintainer; P4b cannot close without them.
3. Issue bodies #124 and #126: update them to the A2-8 text, or accept that their comments
   carry the amendment.
4. The producer of the key-signed release attestation (PL31, PQ4) has no owning PB block;
   PB10 cannot pass its gate without it.
5. S1 scheduling against the runner VM under "one VM at a time".

## 6. What is planned now

- `2026-10-08-kernel-profile-p2.md`: block P2, the first unblocked block on the critical path.
- `2026-10-08-update-trust-ut13.md`: UT13 and ADR-0082 are not defined enough to plan as code;
  that document lists what is undefined and the proposed defaults.
