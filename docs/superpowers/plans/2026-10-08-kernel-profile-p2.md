# Kernel Profile Block P2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the section 5 kernel profile into Azoth, align the base command line with section 6, and extend the boot matrix to section 12 item 2 without its P4b and P5 parts, until the Kernel Build gate is green.

**Architecture:** One delimited block in `forge/specs/azoth/kernel-local` carries every P2 option; the same options become locked `[base.kconfig]` entries of `profile.toml`, so `check_delta` enforces them at build time and `athanor-profile-check` on every machine, and a unit test keeps the two equal. The four command-line parameters the build now enforces leave `[base.cmdline]`. `boot.sh` gains a testable `case_args` function, Penryn replaces Nehalem, two IOMMU cases join the matrix, and `boot/init` gains assertions, each a shell function tested on the host with files or stub commands the test writes.

**Tech Stack:** Kconfig fragment, bash (`boot.sh`, `signer/run.sh`), busybox sh (`boot/init`), Python 3 `unittest`, QEMU, OVMF with `virt-fw-vars`, keyutils, GitHub Actions (YAML calling scripts only).

**Spec:** `docs/architecture/doc_kernel_profile.md` sections 5, 6, 12 item 2 and 15 (block P2); `docs/architecture/doc_kernel_build.md` section 7 (the gate). Order and context: `docs/superpowers/plans/2026-10-08-update-chain-order.md`.

## Global Constraints

- English for code comments, commit messages and documentation; conventional commit style as in `git log` (`feat(kernel): ...`, `test(kernel): ...`, `docs(kernel): ...`); no AI attribution anywhere.
- Build and test jobs use at most 4 CPU threads (`-j 4`, podman `--cpus 4`). Nothing in this plan builds the kernel locally: the pull request's Kernel Build run is the build.
- One VM at a time on the maintainer's machines; the boot matrix runs on the hosted KVM runner.
- Signing and keys stay with the maintainer: this plan generates only ephemeral test keys inside CI jobs, as `signer/run.sh prepare` already does, and never touches `keys/modules`, `keys/secureboot` or any secret.
- No `|| true`, no `continue-on-error`.
- Workflow YAML only checks out, calls scripts under the repo and moves artifacts.
- Changes to PAM, `system/athanor-bus-api/src/polkit.rs`, the Gatekeeper, attestation or crypto/LUKS require maintainer approval before editing. No task here touches them.
- Section 5 rule: "`kernel-local` sets every disabled option explicitly (`# CONFIG_X is not set`)".
- Section 5 rule: `UBSAN_TRAP`, `PROC_KCORE` off, `BLK_DEV_WRITE_MOUNTED` off and `MSEAL_SYSTEM_MAPPINGS` are "Applied in P2 unless the boot matrix fails, and withdrawn through a recorded decision": if one fails, stop and ask the maintainer for the decision record; never drop it silently.
- Edits to existing `.md` files: the formatter rewrites whole files on Edit/Write, so change them with a small script and check `git diff --stat` shows only the intended lines.
- Never `cd`; use repository-relative paths, `git -C` and `cargo -p`.

## Review Focus

1. **An image that pairs the new command line with an old kernel** would run without lockdown: removing a parameter from `[base.cmdline]` must always come with its locked kconfig replacement. Test in Task 4 (`test_every_removed_parameter_has_a_built_in_replacement`).
2. **An IOMMU case where the emulated IOMMU never initialises** (the AMD device on an Intel host, or the reverse on the hosted runner) must fail, not pass with zero groups. Test in Task 3 (`test_no_iommu_group_fails`).
3. **A kernel log without the `Dynamic Preempt:` line**, or with another mode, must fail the preemption check. Test in Task 3 (`test_preempt_line_missing_fails`, `test_preempt_full_fails`).
4. **A missing `.ima` keyring** would make "the CA-vouched key is refused" pass vacuously. Test in Task 6 (`test_ima_refusal_needs_the_keyring`).
5. **A malformed `--insmod` expectation** (empty errno, trailing colon, unknown name) must stop `boot.sh` before any VM boots. Test in Task 6 (`test_malformed_insmod_specs_are_refused`).

---

### Task 1: The P2 options in `kernel-local` and in `profile.toml`

**Files:**
- Modify: `forge/specs/azoth/kernel-local` (append a block after the last line, `CONFIG_MEMCG=y`)
- Modify: `forge/specs/athanor-kernel-profile/profile.toml` (`[base.kconfig]`)
- Generate: `forge/specs/athanor-kernel-profile/SOURCES/usr/share/athanor/kernel-profile/*.json`
- Test: `forge/specs/athanor-kernel-profile/tests/test_kernel_profile.py`

**Interfaces:**
- Consumes: `kernel_profile.py generate|check` (existing).
- Produces: the markers `# BEGIN kernel profile P2` and `# END kernel profile P2` in `kernel-local`; every option between them is a locked `[base.kconfig]` entry with the same value (`"n"` for `# CONFIG_X is not set`).

- [ ] **Step 1: Write the failing test**

Append to `forge/specs/athanor-kernel-profile/tests/test_kernel_profile.py` (add `import re` to the imports):

```python
KERNEL_LOCAL = PACKAGE.parent / "azoth" / "kernel-local"


def p2_block() -> dict:
    """{CONFIG_X: value} between the P2 markers of kernel-local; `is not set` reads as n."""
    lines = KERNEL_LOCAL.read_text().splitlines()
    start = lines.index("# BEGIN kernel profile P2")
    end = lines.index("# END kernel profile P2")
    options = {}
    for line in lines[start + 1 : end]:
        if match := re.fullmatch(r"(CONFIG_\w+)=(.*)", line):
            options[match[1]] = match[2]
        elif match := re.fullmatch(r"# (CONFIG_\w+) is not set", line):
            options[match[1]] = "n"
    return options


class KernelLocalAgreement(unittest.TestCase):
    def test_every_p2_option_is_a_locked_base_setting_with_the_same_value(self) -> None:
        with open(PACKAGE / "profile.toml", "rb") as handle:
            kconfig = tomllib.load(handle)["base"]["kconfig"]
        block = p2_block()
        self.assertGreaterEqual(len(block), 60)
        for name, wanted in block.items():
            with self.subTest(name):
                self.assertIn(name, kconfig)
                self.assertEqual(kconfig[name]["value"], wanted)
                self.assertTrue(kconfig[name].get("locked"))

    def test_the_block_lists_each_option_once(self) -> None:
        names = re.findall(r"^(?:# )?(CONFIG_\w+)(?:=| is not set$)", KERNEL_LOCAL.read_text(), re.M)
        duplicates = sorted({n for n in names if names.count(n) > 1})
        self.assertEqual(duplicates, [], "kernel-local sets these options twice")
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -k KernelLocalAgreement -v`
Expected: FAIL with `ValueError: '# BEGIN kernel profile P2' is not in list`.

- [ ] **Step 3: Append the block to `kernel-local`**

```
# Kernel profile, block P2 (docs/architecture/doc_kernel_profile.md, section 5). Every option
# between the markers is also a locked [base.kconfig] entry of
# forge/specs/athanor-kernel-profile/profile.toml, so athanor-profile-check verifies it on the
# machine; tests/test_kernel_profile.py keeps the two equal. Items marked P4b, P5 or P6 in
# section 5 are not here.
# BEGIN kernel profile P2
# From the command line into the build (section 6 drops lockdown, init_on_free, vsyscall
# and debugfs from [base.cmdline]). Forced lockdown is registered early and cannot be
# lowered at boot.
CONFIG_SECURITY_LOCKDOWN_LSM=y
CONFIG_SECURITY_LOCKDOWN_LSM_EARLY=y
CONFIG_LOCK_DOWN_KERNEL_FORCE_INTEGRITY=y
# CONFIG_LOCK_DOWN_KERNEL_FORCE_NONE is not set
# CONFIG_LOCK_DOWN_KERNEL_FORCE_CONFIDENTIALITY is not set
CONFIG_INIT_ON_FREE_DEFAULT_ON=y
CONFIG_LEGACY_VSYSCALL_NONE=y
# CONFIG_LEGACY_VSYSCALL_XONLY is not set
CONFIG_DEBUG_FS_ALLOW_NONE=y
# CONFIG_DEBUG_FS_ALLOW_ALL is not set
# CONFIG_DEBUG_FS_DISALLOW_MOUNT is not set
# Reliability (D19, section 11): crash evidence through EFI pstore.
CONFIG_PANIC_TIMEOUT=10
CONFIG_EFI_VARS_PSTORE=y
# CONFIG_EFI_VARS_PSTORE_DEFAULT_DISABLE is not set
# New hardening.
CONFIG_KSTACK_ERASE=y
CONFIG_PAGE_TABLE_CHECK=y
CONFIG_PAGE_TABLE_CHECK_ENFORCED=y
CONFIG_DEBUG_VIRTUAL=y
CONFIG_DEBUG_SG=y
CONFIG_DEBUG_NOTIFIERS=y
CONFIG_ARCH_MMAP_RND_BITS=32
CONFIG_ARCH_MMAP_RND_COMPAT_BITS=16
CONFIG_PROC_MEM_FORCE_PTRACE=y
# CONFIG_PROC_MEM_ALWAYS_FORCE is not set
# CONFIG_PROC_MEM_NO_FORCE is not set
CONFIG_SECONDARY_TRUSTED_KEYRING=y
CONFIG_SECONDARY_TRUSTED_KEYRING_SIGNED_BY_BUILTIN=y
# Applied unless the boot matrix fails; withdrawn only through a recorded decision. MSEAL
# needs CHECKPOINT_RESTORE off (no CRIU, gVisor, rr, UML); Mesa and systemd use kcmp().
CONFIG_UBSAN_TRAP=y
# CONFIG_PROC_KCORE is not set
# CONFIG_BLK_DEV_WRITE_MOUNTED is not set
# CONFIG_CHECKPOINT_RESTORE is not set
CONFIG_KCMP=y
CONFIG_MSEAL_SYSTEM_MAPPINGS=y
# Keyrings (D40, D46): root hashes and IPE policies verify against the builtin keyring only;
# the user path of D40 goes through the machine keyring.
CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG=y
# CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG_SECONDARY_KEYRING is not set
# CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG_PLATFORM_KEYRING is not set
CONFIG_SECURITY_IPE=y
# CONFIG_IPE_POLICY_SIG_SECONDARY_KEYRING is not set
# CONFIG_IPE_POLICY_SIG_PLATFORM_KEYRING is not set
# CONFIG_IMA_KEYRINGS_PERMIT_SIGNED_BY_BUILTIN_OR_SECONDARY is not set
CONFIG_INTEGRITY_MACHINE_KEYRING=y
CONFIG_INTEGRITY_CA_MACHINE_KEYRING=y
CONFIG_INTEGRITY_CA_MACHINE_KEYRING_MAX=y
# Attack surface removed (unused by every role).
# CONFIG_KEXEC is not set
# CONFIG_KEXEC_FILE is not set
# CONFIG_KEXEC_HANDOVER is not set
# CONFIG_CRASH_DUMP is not set
# CONFIG_LIVEPATCH is not set
# CONFIG_HIBERNATION is not set
# CONFIG_SECURITY_TOMOYO is not set
# CONFIG_X86_IOPL_IOPERM is not set
CONFIG_LSM="lockdown,yama,selinux,bpf,landlock,ipe"
# Codegen (D13): preemption lazy, dynamic; AutoFDO profiles arrive in P7.
CONFIG_AUTOFDO_CLANG=y
CONFIG_PREEMPT_DYNAMIC=y
CONFIG_PREEMPT_LAZY=y
# CONFIG_PREEMPT is not set
# Built-in drivers of D17, kept on purpose.
CONFIG_SATA_AHCI=y
CONFIG_VIRTIO_BLK=y
CONFIG_USB_HID=y
CONFIG_SERIO_I8042=y
CONFIG_PINCTRL_AMD=y
CONFIG_I2C_DESIGNWARE_CORE=y
CONFIG_I2C_DESIGNWARE_PLATFORM=y
CONFIG_BTRFS_FS=y
# END kernel profile P2
```

- [ ] **Step 4: Add the matching `[base.kconfig]` entries**

`profile.toml` already holds `CONFIG_SECURITY_IPE`, `CONFIG_INTEGRITY_CA_MACHINE_KEYRING_MAX`, `CONFIG_PREEMPT_DYNAMIC`, `CONFIG_PREEMPT_LAZY` and `CONFIG_BTRFS_FS` with the same values: do not repeat them (TOML refuses duplicate keys). Append after `CONFIG_CPU_MITIGATIONS`:

```toml
# Block P2 (section 5): the build profile, mirrored from the P2 block of kernel-local.
CONFIG_SECURITY_LOCKDOWN_LSM = { value = "y", decision = "section 5", locked = true }
CONFIG_SECURITY_LOCKDOWN_LSM_EARLY = { value = "y", decision = "section 5", locked = true }
CONFIG_LOCK_DOWN_KERNEL_FORCE_INTEGRITY = { value = "y", decision = "section 5", locked = true }
CONFIG_LOCK_DOWN_KERNEL_FORCE_NONE = { value = "n", decision = "section 5", locked = true }
CONFIG_LOCK_DOWN_KERNEL_FORCE_CONFIDENTIALITY = { value = "n", decision = "section 5", locked = true }
CONFIG_INIT_ON_FREE_DEFAULT_ON = { value = "y", decision = "section 5", locked = true }
CONFIG_LEGACY_VSYSCALL_NONE = { value = "y", decision = "section 5", locked = true }
CONFIG_LEGACY_VSYSCALL_XONLY = { value = "n", decision = "section 5", locked = true }
CONFIG_DEBUG_FS_ALLOW_NONE = { value = "y", decision = "section 5", locked = true }
CONFIG_DEBUG_FS_ALLOW_ALL = { value = "n", decision = "section 5", locked = true }
CONFIG_DEBUG_FS_DISALLOW_MOUNT = { value = "n", decision = "section 5", locked = true }
CONFIG_PANIC_TIMEOUT = { value = "10", decision = "D19", locked = true }
CONFIG_EFI_VARS_PSTORE = { value = "y", decision = "D19", locked = true }
CONFIG_EFI_VARS_PSTORE_DEFAULT_DISABLE = { value = "n", decision = "D19", locked = true }
CONFIG_KSTACK_ERASE = { value = "y", decision = "section 5", locked = true }
CONFIG_PAGE_TABLE_CHECK = { value = "y", decision = "section 5", locked = true }
CONFIG_PAGE_TABLE_CHECK_ENFORCED = { value = "y", decision = "section 5", locked = true }
CONFIG_DEBUG_VIRTUAL = { value = "y", decision = "section 5", locked = true }
CONFIG_DEBUG_SG = { value = "y", decision = "section 5", locked = true }
CONFIG_DEBUG_NOTIFIERS = { value = "y", decision = "section 5", locked = true }
CONFIG_ARCH_MMAP_RND_BITS = { value = "32", decision = "section 5", locked = true }
CONFIG_ARCH_MMAP_RND_COMPAT_BITS = { value = "16", decision = "section 5", locked = true }
CONFIG_PROC_MEM_FORCE_PTRACE = { value = "y", decision = "section 5", locked = true }
CONFIG_PROC_MEM_ALWAYS_FORCE = { value = "n", decision = "section 5", locked = true }
CONFIG_PROC_MEM_NO_FORCE = { value = "n", decision = "section 5", locked = true }
CONFIG_SECONDARY_TRUSTED_KEYRING = { value = "y", decision = "D40", locked = true }
CONFIG_SECONDARY_TRUSTED_KEYRING_SIGNED_BY_BUILTIN = { value = "y", decision = "D40", locked = true }
CONFIG_UBSAN_TRAP = { value = "y", decision = "section 5", locked = true }
CONFIG_PROC_KCORE = { value = "n", decision = "section 5", locked = true }
CONFIG_BLK_DEV_WRITE_MOUNTED = { value = "n", decision = "section 5", locked = true }
CONFIG_CHECKPOINT_RESTORE = { value = "n", decision = "section 5", locked = true }
CONFIG_KCMP = { value = "y", decision = "section 5", locked = true }
CONFIG_MSEAL_SYSTEM_MAPPINGS = { value = "y", decision = "section 5", locked = true }
CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG = { value = "y", decision = "D40", locked = true }
CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG_SECONDARY_KEYRING = { value = "n", decision = "D40", locked = true }
CONFIG_DM_VERITY_VERIFY_ROOTHASH_SIG_PLATFORM_KEYRING = { value = "n", decision = "D40", locked = true }
CONFIG_IPE_POLICY_SIG_SECONDARY_KEYRING = { value = "n", decision = "D40", locked = true }
CONFIG_IPE_POLICY_SIG_PLATFORM_KEYRING = { value = "n", decision = "D40", locked = true }
CONFIG_IMA_KEYRINGS_PERMIT_SIGNED_BY_BUILTIN_OR_SECONDARY = { value = "n", decision = "D46", locked = true }
CONFIG_INTEGRITY_MACHINE_KEYRING = { value = "y", decision = "D40", locked = true }
CONFIG_INTEGRITY_CA_MACHINE_KEYRING = { value = "y", decision = "D40", locked = true }
CONFIG_KEXEC = { value = "n", decision = "section 5", locked = true }
CONFIG_KEXEC_FILE = { value = "n", decision = "section 5", locked = true }
CONFIG_KEXEC_HANDOVER = { value = "n", decision = "section 5", locked = true }
CONFIG_CRASH_DUMP = { value = "n", decision = "section 5", locked = true }
CONFIG_LIVEPATCH = { value = "n", decision = "section 5", locked = true }
CONFIG_HIBERNATION = { value = "n", decision = "section 5", locked = true }
CONFIG_SECURITY_TOMOYO = { value = "n", decision = "section 5", locked = true }
CONFIG_X86_IOPL_IOPERM = { value = "n", decision = "section 5", locked = true }
CONFIG_LSM = { value = '"lockdown,yama,selinux,bpf,landlock,ipe"', decision = "section 5", locked = true }
CONFIG_AUTOFDO_CLANG = { value = "y", decision = "section 5", locked = true }
CONFIG_PREEMPT = { value = "n", decision = "D13", locked = true }
CONFIG_SATA_AHCI = { value = "y", decision = "D17", locked = true }
CONFIG_VIRTIO_BLK = { value = "y", decision = "D17", locked = true }
CONFIG_USB_HID = { value = "y", decision = "D17", locked = true }
CONFIG_SERIO_I8042 = { value = "y", decision = "D17", locked = true }
CONFIG_PINCTRL_AMD = { value = "y", decision = "D17", locked = true }
CONFIG_I2C_DESIGNWARE_CORE = { value = "y", decision = "D17", locked = true }
CONFIG_I2C_DESIGNWARE_PLATFORM = { value = "y", decision = "D17", locked = true }
```

Then regenerate: `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py generate`

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -v`
Expected: PASS, including `test_repository_manifest_is_valid_and_generated` and both `KernelLocalAgreement` tests.
Run: `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check`
Expected: exit 0, "generated files, command line and sysctls up to date".

- [ ] **Step 6: Commit**

```bash
git add forge/specs/azoth/kernel-local forge/specs/athanor-kernel-profile/profile.toml forge/specs/athanor-kernel-profile/SOURCES/usr/share/athanor/kernel-profile forge/specs/athanor-kernel-profile/tests/test_kernel_profile.py
git commit -m "feat(kernel): build the section 5 profile into Azoth and declare it in profile.toml"
```

---

### Task 2: Testable matrix cases, Penryn and the two IOMMU cases

**Files:**
- Modify: `forge/specs/azoth/boot.sh` (usage text, `CASES` default, `TEST_CMDLINE` split, `run_case`)
- Create: `forge/specs/azoth/tests/test_boot_cases.py`

**Interfaces:**
- Consumes: globals `ACCEL`, `OUT`, `WORK`, `VMLINUZ`, `OVMF_CODE`, `BIOS_CMDLINE`, `die`.
- Produces: `case_args NAME` sets the global array `CASE_ARGS` (QEMU arguments); case names `bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd`; the IOMMU cases add `k3.iommu=intel|amd` to the BIOS command line. `BIOS_CMDLINE` and `UEFI_CMDLINE` replace the single `TEST_CMDLINE` use in the QEMU and `ukify` calls.

- [ ] **Step 1: Write the failing test**

```python
"""The cases of the boot matrix (forge/specs/azoth/boot.sh, case_args), run on the host:
python3 -B -m unittest discover -s forge/specs/azoth/tests"""

import pathlib
import re
import subprocess
import unittest

BOOT = pathlib.Path(__file__).resolve().parents[1] / "boot.sh"


def function(name):
    match = re.search(rf"^{name}\(\) \{{.*?^\}}$", BOOT.read_text(), re.M | re.S)
    assert match, f"boot.sh has no function {name}"
    return match.group(0)


def case_args(name, accel="kvm"):
    script = (
        f'ACCEL={accel} OUT=/o WORK=/w VMLINUZ=/w/vmlinuz OVMF_CODE=/c BIOS_CMDLINE="base k3.x=1"\n'
        'die() { echo "error: $*" >&2; exit 1; }\n'
        f"{function('case_args')}\n"
        f'case_args {name}\nprintf "%s\\n" "${{CASE_ARGS[@]}}"\n'
    )
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    return result.returncode, result.stdout.splitlines(), result.stderr


class Cases(unittest.TestCase):
    def test_the_default_matrix_has_six_cases_and_no_nehalem(self):
        text = BOOT.read_text()
        self.assertIn("CASES=(bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd)", text)
        self.assertNotIn("Nehalem", text)
        self.assertNotIn("nehalem", text)

    def test_penryn_is_the_baseline_cpu(self):
        code, args, _ = case_args("bios-penryn")
        self.assertEqual(code, 0)
        self.assertEqual(args[args.index("-cpu") + 1], "Penryn")

    def test_host_becomes_max_under_tcg(self):
        _, args, _ = case_args("uefi-host", accel="tcg")
        self.assertEqual(args[args.index("-cpu") + 1], "max")
        self.assertIn("if=pflash,format=raw,file=/w/vars-uefi-host.fd", args)

    def test_intel_iommu_case(self):
        code, args, _ = case_args("iommu-intel")
        self.assertEqual(code, 0)
        self.assertIn("kernel-irqchip=split", args[args.index("-machine") + 1])
        self.assertLess(args.index("intel-iommu,intremap=on"), args.index("virtio-rng-pci"))
        self.assertEqual(args[args.index("-append") + 1], "base k3.x=1 k3.iommu=intel")

    def test_amd_iommu_case(self):
        code, args, _ = case_args("iommu-amd")
        self.assertEqual(code, 0)
        self.assertIn("amd-iommu,intremap=on", args)
        self.assertEqual(args[args.index("-append") + 1], "base k3.x=1 k3.iommu=amd")

    def test_bios_cases_boot_the_kernel_directly(self):
        _, args, _ = case_args("bios-host")
        self.assertEqual(args[args.index("-kernel") + 1], "/w/vmlinuz")
        self.assertNotIn("k3.iommu", " ".join(args))

    def test_an_unknown_case_fails(self):
        code, _, err = case_args("bios-nehalem")
        self.assertNotEqual(code, 0)
        self.assertIn("unknown case: bios-nehalem", err)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -p 'test_boot_cases.py' -v`
Expected: FAIL with `AssertionError: boot.sh has no function case_args`.

- [ ] **Step 3: Implement in `boot.sh`**

Replace the `--case` line of the usage comment with `#   --case   restricts the matrix (repeatable): bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd`, and the header sentence about Nehalem with: "firmware {SeaBIOS, OVMF+Secure Boot via shim} x CPU {Penryn, host}, plus an Intel and an AMD IOMMU case. Penryn (x86-64-v1, no POPCNT or SSE4.2; D14) proves that no instruction beyond the baseline made it into the kernel."

Default cases:

```bash
[[ ${#CASES[@]} -gt 0 ]] || CASES=(bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd)
```

After the `K3_INSMOD` loop, replace `TEST_CMDLINE+="${K3_INSMOD:+ k3.insmod=$K3_INSMOD}"` with:

```bash
BIOS_CMDLINE="$TEST_CMDLINE${K3_INSMOD:+ k3.insmod=$K3_INSMOD}"
UEFI_CMDLINE="$TEST_CMDLINE${K3_INSMOD:+ k3.insmod=$K3_INSMOD} k3.sb=1"
```

and in the `ukify build` call use `--cmdline "$UEFI_CMDLINE"`. Replace `run_case` with:

```bash
case_args() { # case_args NAME: the QEMU arguments of one case, in the global array CASE_ARGS
  local name=$1 cpu machine=q35,smm=on iommu='' fw
  case $name in
    bios-penryn|uefi-penryn) cpu=Penryn ;;
    bios-host|uefi-host) cpu=host ;;
    # Section 12 item 2: the interrupt remapping of both IOMMUs needs the split irqchip.
    iommu-intel) cpu=host iommu=intel-iommu,intremap=on machine+=,kernel-irqchip=split ;;
    iommu-amd) cpu=host iommu=amd-iommu,intremap=on machine+=,kernel-irqchip=split ;;
    *) die "unknown case: $name" ;;
  esac
  [[ $cpu == host && $ACCEL == tcg ]] && cpu=max
  fw=${name%%-*}; [[ $fw == iommu ]] && fw=bios
  CASE_ARGS=(-machine "$machine" -accel "$ACCEL" -cpu "$cpu" -smp 2 -m 2048
             -display none -monitor none -serial "file:$OUT/$name.log" -no-reboot)
  # The IOMMU comes before every other PCI device, as QEMU requires.
  [[ $iommu ]] && CASE_ARGS+=(-device "$iommu")
  CASE_ARGS+=(-device virtio-rng-pci)
  case $fw in
    bios) CASE_ARGS+=(-kernel "$VMLINUZ" -initrd "$WORK/initramfs.img"
                      -append "$BIOS_CMDLINE${iommu:+ k3.iommu=${name#iommu-}}") ;;
    uefi) CASE_ARGS+=(-global "driver=cfi.pflash01,property=secure,value=on"
                      -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
                      -drive "if=pflash,format=raw,file=$WORK/vars-$name.fd"
                      -drive "if=virtio,format=raw,readonly=on,file=fat:ro:$WORK/esp") ;;
  esac
}

run_case() { # run_case NAME
  local name=$1 log="$OUT/$1.log"
  case_args "$name"
  [[ $name == uefi-* ]] && cp "$WORK/vars.fd" "$WORK/vars-$name.fd"
  step "$name (accel $ACCEL)"
  timeout 900 qemu-system-x86_64 "${CASE_ARGS[@]}" || echo "qemu: exit $?"
  if grep -q '^K3 RESULT ok' "$log"; then
    RESULTS+=("| $name | ok |"); echo "$name: ok"
  else
    RESULTS+=("| $name | FAIL |"); FAILED+=("$name")
    echo "$name: FAIL"; grep -E '^K3 (FAIL|RESULT)' "$log" || tail -n 20 "$log"
  fi
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -v`
Expected: PASS (all azoth tests, the new ones included).
Run: `shellcheck forge/specs/azoth/boot.sh`
Expected: no findings.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/azoth/boot.sh forge/specs/azoth/tests/test_boot_cases.py
git commit -m "test(kernel): boot the matrix on Penryn and add the Intel and AMD IOMMU cases"
```

---

### Task 3: Assertions for preemption, ASLR, IOMMU domains and the mesh platform

**Files:**
- Modify: `forge/specs/azoth/boot/init`
- Test: `forge/specs/azoth/tests/test_boot_init.py`

**Interfaces:**
- Consumes: `k3.iommu=intel|amd` from Task 2.
- Produces: shell functions `preempt_lazy`, `aslr_bits`, `iommu_domains`, `kconfig_enabled NAME...` in `boot/init`, with file variables `K3_VM_DIR` (default `/proc/sys/vm`), `K3_IOMMU_GROUPS` (default `/sys/kernel/iommu_groups`), `K3_CONFIG` (default `/proc/config.gz`).

- [ ] **Step 1: Write the failing tests**

Add to `forge/specs/azoth/tests/test_boot_init.py` (add `import gzip` to the imports):

```python
def run_function(name, prelude="", args=""):
    script = f"{prelude}\n{function(name)}\n{name} {args}\n"
    return subprocess.run(["sh", "-c", script], capture_output=True, text=True)


class PlatformAssertions(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def log(self, text):
        (self.dir / "dmesg").write_text(text)
        return f'dmesg() {{ cat "{self.dir}/dmesg"; }}'

    def test_preempt_lazy_passes(self):
        self.assertEqual(run_function("preempt_lazy", self.log("[0.1] Dynamic Preempt: lazy\n")).returncode, 0)

    def test_preempt_full_fails(self):
        self.assertNotEqual(run_function("preempt_lazy", self.log("[0.1] Dynamic Preempt: full\n")).returncode, 0)

    def test_preempt_line_missing_fails(self):
        self.assertNotEqual(run_function("preempt_lazy", self.log("[0.1] nothing\n")).returncode, 0)

    def vm(self, bits, compat):
        (self.dir / "mmap_rnd_bits").write_text(f"{bits}\n")
        (self.dir / "mmap_rnd_compat_bits").write_text(f"{compat}\n")
        return f'vm_dir="{self.dir}"'

    def test_aslr_bits_pass(self):
        self.assertEqual(run_function("aslr_bits", self.vm(32, 16)).returncode, 0)

    def test_aslr_bits_too_low_fail(self):
        self.assertNotEqual(run_function("aslr_bits", self.vm(28, 8)).returncode, 0)

    def groups(self, *types):
        for i, kind in enumerate(types):
            (self.dir / "groups" / str(i)).mkdir(parents=True)
            (self.dir / "groups" / str(i) / "type").write_text(f"{kind}\n")
        (self.dir / "groups").mkdir(exist_ok=True)
        return f'iommu_groups="{self.dir}/groups"'

    def test_lazy_domains_pass(self):
        self.assertEqual(run_function("iommu_domains", self.groups("DMA-FQ", "DMA-FQ")).returncode, 0)

    def test_a_strict_domain_fails(self):
        self.assertNotEqual(run_function("iommu_domains", self.groups("DMA-FQ", "DMA")).returncode, 0)

    def test_no_iommu_group_fails(self):
        self.assertNotEqual(run_function("iommu_domains", self.groups()).returncode, 0)

    def config(self, text):
        with gzip.open(self.dir / "config.gz", "wt") as handle:
            handle.write(text)
        return f'kernel_config="{self.dir}/config.gz"'

    def test_module_or_builtin_passes(self):
        prelude = self.config("CONFIG_WIREGUARD=m\nCONFIG_KVM_AMD=y\n")
        self.assertEqual(run_function("kconfig_enabled", prelude, "WIREGUARD KVM_AMD").returncode, 0)

    def test_a_missing_option_fails_and_is_named(self):
        result = run_function("kconfig_enabled", self.config("CONFIG_WIREGUARD=m\n# CONFIG_UDMABUF is not set\n"), "WIREGUARD UDMABUF")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("UDMABUF", result.stdout)

    def test_init_runs_the_platform_checks(self):
        text = INIT.read_text()
        for needle in ("check preempt", "check aslr", "check iommu", "check mesh-platform"):
            self.assertIn(needle, text)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -p 'test_boot_init.py' -v`
Expected: FAIL with `AssertionError: init has no function preempt_lazy`.

- [ ] **Step 3: Implement in `boot/init`**

Parse the new parameter in the `for w in $(cat /proc/cmdline)` loop: `k3.iommu=*) iommu=${w#*=} ;;`, and add `iommu=''` to the initialisation line. After `cgroup_controllers=...`:

```sh
vm_dir=${K3_VM_DIR:-/proc/sys/vm}
iommu_groups=${K3_IOMMU_GROUPS:-/sys/kernel/iommu_groups}
kernel_config=${K3_CONFIG:-/proc/config.gz}
preempt_lazy() { # D13: debugfs is off, so the mode is read from the boot log
  line=$(dmesg | grep -o 'Dynamic Preempt: [a-z]*')
  echo "${line:-no Dynamic Preempt line}"; [ "$line" = "Dynamic Preempt: lazy" ]
}
aslr_bits() { # section 5: ARCH_MMAP_RND_BITS=32, ARCH_MMAP_RND_COMPAT_BITS=16
  b=$(cat "$vm_dir/mmap_rnd_bits") c=$(cat "$vm_dir/mmap_rnd_compat_bits")
  echo "mmap_rnd_bits=$b mmap_rnd_compat_bits=$c"; [ "$b" = 32 ] && [ "$c" = 16 ]
}
iommu_domains() { # D16: lazy domains (DMA-FQ); an IOMMU case with no group is a failure
  n=0 bad=0
  for t in "$iommu_groups"/*/type; do
    [ -f "$t" ] || continue
    n=$((n + 1)); v=$(cat "$t"); echo "${t%/type}: $v"; [ "$v" = DMA-FQ ] || bad=1
  done
  [ "$n" -gt 0 ] && [ "$bad" = 0 ]
}
kconfig_enabled() { # kconfig_enabled NAME...: each CONFIG_NAME is y or m
  missing=''
  for o in "$@"; do zcat "$kernel_config" | grep -qE "^CONFIG_$o=(y|m)$" || missing="$missing $o"; done
  echo "missing:${missing:- none}"; [ -z "$missing" ]
}
```

After `check memcg       memcg`:

```sh
check preempt     preempt_lazy
check aslr        aslr_bits
# The mesh platform requirements of section 7, enabled in the kernel today.
check mesh-platform kconfig_enabled WIREGUARD KVM_INTEL KVM_AMD VHOST_VSOCK VIRTIO_FS DRM_VIRTIO_GPU UDMABUF VFIO_PCI TCG_TPM
[ "$iommu" ] && check iommu iommu_domains
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -v`
Expected: PASS.
Run: `shellcheck -s sh forge/specs/azoth/boot/init`
Expected: no new findings.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/azoth/boot/init forge/specs/azoth/tests/test_boot_init.py
git commit -m "test(kernel): assert preemption, ASLR bits, IOMMU domains and the mesh platform at boot"
```

---

### Task 4: The base command line aligned with section 6

**Files:**
- Modify: `forge/specs/athanor-kernel-profile/profile.toml` (`[base.cmdline]` and its comment)
- Generate: `forge/specs/athanor-kernel-profile/SOURCES/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml`, `forge/specs/azoth/cmdline`, the JSON profiles
- Modify: `forge/specs/azoth/boot/init`
- Test: `forge/specs/athanor-kernel-profile/tests/test_kernel_profile.py`, `forge/specs/azoth/tests/test_boot_init.py`

**Interfaces:**
- Consumes: the Task 1 kconfig entries.
- Produces: init functions `lockdown_forced`, `init_on_free_built_in`, `vsyscall_none`, `debugfs_off`, and `not_on_cmdline NAME` with `K3_CMDLINE` (default `/proc/cmdline`), `K3_LOCKDOWN` (default `/sys/kernel/security/lockdown`), `K3_MAPS` (default `/proc/self/maps`).

- [ ] **Step 1: Write the failing tests**

In `test_kernel_profile.py`:

```python
BUILT_IN = {
    "lockdown": ("CONFIG_LOCK_DOWN_KERNEL_FORCE_INTEGRITY", "y"),
    "init_on_free": ("CONFIG_INIT_ON_FREE_DEFAULT_ON", "y"),
    "vsyscall": ("CONFIG_LEGACY_VSYSCALL_NONE", "y"),
    "debugfs": ("CONFIG_DEBUG_FS_ALLOW_NONE", "y"),
}


class CommandLineAlignment(unittest.TestCase):
    def test_every_removed_parameter_has_a_built_in_replacement(self) -> None:
        with open(PACKAGE / "profile.toml", "rb") as handle:
            base = tomllib.load(handle)["base"]
        for parameter, (option, wanted) in BUILT_IN.items():
            with self.subTest(parameter):
                self.assertNotIn(parameter, base["cmdline"])
                self.assertEqual(base["kconfig"][option]["value"], wanted)
                self.assertTrue(base["kconfig"][option]["locked"])

    def test_the_boot_matrix_command_line_carries_none_of_them(self) -> None:
        words = (PACKAGE.parent / "azoth" / "cmdline").read_text().split()
        self.assertFalse([w for w in words if w.split("=")[0] in BUILT_IN])
```

In `test_boot_init.py`, inside `PlatformAssertions`:

```python
    def cmdline(self, text):
        (self.dir / "cmdline").write_text(text + "\n")
        return f'cmdline_file="{self.dir}/cmdline"'

    def test_forced_lockdown_passes_without_the_parameter(self):
        (self.dir / "lockdown").write_text("none [integrity] confidentiality\n")
        prelude = self.cmdline("intel_iommu=on") + f'\nlockdown_file="{self.dir}/lockdown"\n' + function("not_on_cmdline")
        self.assertEqual(run_function("lockdown_forced", prelude).returncode, 0)

    def test_lockdown_from_the_command_line_does_not_count(self):
        (self.dir / "lockdown").write_text("none [integrity] confidentiality\n")
        prelude = self.cmdline("lockdown=integrity") + f'\nlockdown_file="{self.dir}/lockdown"\n' + function("not_on_cmdline")
        self.assertNotEqual(run_function("lockdown_forced", prelude).returncode, 0)

    def test_init_on_free_from_the_build(self):
        prelude = self.cmdline("x=1") + "\n" + self.log("mem auto-init: stack:all(zero), heap alloc:on, heap free:on\n") + "\n" + function("not_on_cmdline")
        self.assertEqual(run_function("init_on_free_built_in", prelude).returncode, 0)

    def test_init_on_free_off_fails(self):
        prelude = self.cmdline("x=1") + "\n" + self.log("mem auto-init: stack:all(zero), heap alloc:on, heap free:off\n") + "\n" + function("not_on_cmdline")
        self.assertNotEqual(run_function("init_on_free_built_in", prelude).returncode, 0)

    def test_a_vsyscall_page_fails(self):
        (self.dir / "maps").write_text("ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0 [vsyscall]\n")
        prelude = self.cmdline("x=1") + f'\nmaps_file="{self.dir}/maps"\n' + function("not_on_cmdline")
        self.assertNotEqual(run_function("vsyscall_none", prelude).returncode, 0)

    def test_debugfs_that_mounts_fails(self):
        prelude = self.cmdline("x=1") + "\nmount() { return 0; }\n" + function("not_on_cmdline")
        self.assertNotEqual(run_function("debugfs_off", prelude).returncode, 0)

    def test_debugfs_refused_passes(self):
        prelude = self.cmdline("x=1") + "\nmount() { return 19; }\n" + function("not_on_cmdline")
        self.assertEqual(run_function("debugfs_off", prelude).returncode, 0)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -k CommandLineAlignment -v`
Expected: FAIL with `AssertionError: 'lockdown' unexpectedly found`.
Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -p 'test_boot_init.py' -v`
Expected: FAIL with `init has no function not_on_cmdline`.

- [ ] **Step 3: Implement**

In `profile.toml`, delete the four lines `lockdown = ...`, `init_on_free = ...`, `vsyscall = ...`, `debugfs = ...` of `[base.cmdline]`, and replace the comment sentence "lockdown, init_on_free, vsyscall and debugfs stay until block P2 builds them in;" with "block P2 built lockdown, init_on_free, vsyscall and debugfs into the kernel (kernel-local);". Regenerate: `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py generate`.

In `boot/init`, after `kernel_config=...`:

```sh
cmdline_file=${K3_CMDLINE:-/proc/cmdline}
lockdown_file=${K3_LOCKDOWN:-/sys/kernel/security/lockdown}
maps_file=${K3_MAPS:-/proc/self/maps}
not_on_cmdline() { # not_on_cmdline NAME: the build, not the command line, sets NAME
  if tr ' ' '\n' < "$cmdline_file" | grep -q "^$1="; then echo "$1= is on the command line"; return 1; fi
}
lockdown_forced() { # LOCK_DOWN_KERNEL_FORCE_INTEGRITY, with no lockdown= to lean on
  echo "lockdown: $(cat "$lockdown_file")"
  not_on_cmdline lockdown && grep -q '\[integrity\]' "$lockdown_file"
}
init_on_free_built_in() { # INIT_ON_FREE_DEFAULT_ON
  dmesg | grep 'mem auto-init:'; not_on_cmdline init_on_free && dmesg | grep -q 'heap free:on'
}
vsyscall_none() { # LEGACY_VSYSCALL_NONE: no vsyscall page in any process
  not_on_cmdline vsyscall && ! grep -q '\[vsyscall\]' "$maps_file"
}
debugfs_off() { # DEBUG_FS_ALLOW_NONE: debugfs never registers, so it cannot be mounted
  mkdir -p /tmp/debugfs
  not_on_cmdline debugfs || return 1
  if mount -t debugfs debugfs /tmp/debugfs; then echo "debugfs mounted"; return 1; fi
}
```

Replace `check lockdown    grep -q '\[integrity\]' /sys/kernel/security/lockdown` with:

```sh
check lockdown    lockdown_forced
check init-on-free init_on_free_built_in
check vsyscall    vsyscall_none
check debugfs     debugfs_off
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -v && python3 -B -m unittest discover -s forge/specs/azoth/tests -v && python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check`
Expected: PASS and exit 0; `cat forge/specs/azoth/cmdline` shows no `lockdown`, `init_on_free`, `vsyscall` or `debugfs`.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-kernel-profile forge/specs/azoth/cmdline forge/specs/azoth/boot/init forge/specs/azoth/tests/test_boot_init.py
git commit -m "feat(kernel-profile): drop the parameters the kernel now builds in from the base command line"
```

---

### Task 5: The exact set of builtin certificates

**Files:**
- Modify: `forge/specs/azoth/boot/toolchain.packages` (add `keyutils`), `forge/specs/azoth/boot/toolchain.lock` (regenerated by `lock.sh`)
- Modify: `forge/specs/azoth/boot.sh` (install `keyctl` in the initramfs, pass `k3.builtin`)
- Modify: `forge/specs/azoth/boot/init`
- Test: `forge/specs/azoth/tests/test_boot_init.py`

**Interfaces:**
- Consumes: `install_binary PATH` (new in this task, used for `bpftool` and `keyctl`).
- Produces: `k3.builtin=N` on the test command line; init function `builtin_exact` reading `builtin_expected`.

- [ ] **Step 1: Measure the builtin keyring of today's kernel**

The builtin keyring holds the certificates of `keys/modules` plus the key the kernel build generates for its own modules. Confirm the count from the last green run (run `gh` outside the sandbox):

```bash
id=$(gh run list --workflow "Kernel Build" --branch iso-v0 --status success --limit 1 --json databaseId --jq '.[0].databaseId')
gh run download "$id" -n kernel-boot-logs -D /var/tmp/p2-bootlogs
grep -c "Loaded X.509 cert" /var/tmp/p2-bootlogs/bios-host.log
ls forge/specs/azoth/keys/revoked/*.pem | wc -l
```

Expected: the first number minus the revoked count equals `1 + $(ls forge/specs/azoth/keys/modules/*.pem | wc -l)`. If it does not, stop and report the measured descriptions instead of continuing: the "+1" below is then wrong.

- [ ] **Step 2: Write the failing tests**

```python
    def keyring(self, count):
        ids = " ".join(str(100 + i) for i in range(count))
        return f'keyctl() {{ case $1 in rlist) echo "{ids}" ;; list) echo "{count} keys" ;; esac; }}'

    def test_the_expected_number_of_builtin_keys_passes(self):
        self.assertEqual(run_function("builtin_exact", self.keyring(2) + "\nbuiltin_expected=2").returncode, 0)

    def test_an_extra_builtin_key_fails(self):
        self.assertNotEqual(run_function("builtin_exact", self.keyring(3) + "\nbuiltin_expected=2").returncode, 0)

    def test_a_missing_expectation_fails(self):
        self.assertNotEqual(run_function("builtin_exact", self.keyring(2) + "\nbuiltin_expected=").returncode, 0)
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -p 'test_boot_init.py' -v`
Expected: FAIL with `init has no function builtin_exact`.

- [ ] **Step 4: Implement**

`boot/toolchain.packages`: append `keyutils` to the line `busybox bpftool cpio zstd openssl e2fsprogs realtime-tests fio netperf`, and add "keyutils (keyctl) for the keyring assertions" to the header comment. Regenerate the lock (network and podman, `--cpus 4` is the lock tool's own setting): `bash forge/specs/azoth/lock.sh generate boot/toolchain`. Check `git diff --stat forge/specs/azoth/boot/toolchain.lock` shows only the added keyutils entries.

`boot.sh`: replace the bpftool library loop with a function used twice:

```bash
install_binary() { # install_binary PATH: the binary and its libraries, all in /lib64
  install -D -m 755 "$1" "$R$1"
  ldd "$1" | awk '/=> \//{print $3} /^\s*\/lib64\/ld-linux/{print $1}' \
    | while read -r lib; do install -D "$lib" "$R/lib64/${lib##*/}"; done
}
install_binary /usr/sbin/bpftool
install_binary /usr/bin/keyctl
```

After `K3_CERTS` is computed:

```bash
# The builtin keyring holds exactly the certificates of keys/modules and the key the kernel
# build generates for its own modules (measured on the 7.2 series, plan 2026-10-08 P2 task 5).
K3_BUILTIN=$(( $(find "$HERE/keys/modules" -name '*.pem' | wc -l) + 1 ))
TEST_CMDLINE="$CMDLINE console=ttyS0,115200 panic=-1 k3.uname=$KVER k3.certs=$K3_CERTS k3.builtin=$K3_BUILTIN"
```

`boot/init`: parse `k3.builtin=*) builtin_expected=${w#*=} ;;` (add `builtin_expected=''` to the initialisation), then:

```sh
builtin_exact() { # D40: the builtin keyring holds exactly the expected keys
  keyctl list %:.builtin_trusted_keys
  n=$(keyctl rlist %:.builtin_trusted_keys | wc -w)
  echo "builtin keys: $n, expected ${builtin_expected:-nothing}"
  [ -n "$builtin_expected" ] && [ "$n" = "$builtin_expected" ]
}
```

and after the `cert-$skid` loop: `check builtin-set builtin_exact`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -v && shellcheck forge/specs/azoth/boot.sh`
Expected: PASS, no findings.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/azoth/boot.sh forge/specs/azoth/boot/init forge/specs/azoth/boot/toolchain.packages forge/specs/azoth/boot/toolchain.lock forge/specs/azoth/tests/test_boot_init.py
git commit -m "test(kernel): assert the exact builtin certificate set in every boot case"
```

---

### Task 6: The D40 module chain and the IMA key refusal

**Files:**
- Create: `forge/specs/azoth/keys/profiles/test-user-ca.cnf` (certificate profile only, no key)
- Modify: `forge/specs/azoth/signer/run.sh` (`prepare`)
- Modify: `forge/specs/azoth/boot.sh` (`--mok-ca`, `--ima-key`, per-firmware `--insmod` errno)
- Modify: `forge/specs/azoth/boot/init` (`k3.imakey`)
- Modify: `.github/workflows/call-nvidia-kmod-prepare.yml` (upload `mokca/`, `mokleaf/`), `.github/workflows/nvidia-kmod.yml` (download them, pass the new arguments)
- Test: `forge/specs/azoth/tests/test_signer_run.py`, `forge/specs/azoth/tests/test_boot_cases.py`, `forge/specs/azoth/tests/test_boot_init.py`

**Interfaces:**
- Consumes: `case_args`, `BIOS_CMDLINE`, `UEFI_CMDLINE` (Task 2), `install_binary`, `keyctl` (Task 5).
- Produces: `boot.sh --mok-ca CERT` (enrols CERT in MokList and sets `MokListTrusted`, as `mokutil --trust-mok` does); `boot.sh --ima-key CERT` (DER copy in the initramfs at `/ima/key.der`, `k3.imakey=1`); `--insmod FILE.ko:ERRNO[:BIOS_ERRNO]` where `BIOS_ERRNO` applies to the `bios-*` and `iommu-*` cases; function `insmod_spec SPEC` that prints `file uefi bios` or fails; init function `ima_key_refused`. Signer `prepare` produces `mokca/open/...` signed directly by a test CA and `mokleaf/open/...` signed by a leaf that CA issued, with `mokca/test-ca.pem` and `mokleaf/test-leaf.pem`.

- [ ] **Step 1: Check the varstore tool in the boot image**

Run: `podman build --cpus 4 -t localhost/azoth-boot -f forge/specs/azoth/boot/Containerfile forge/specs/azoth && podman run --rm localhost/azoth-boot virt-fw-vars --help | grep -E -- '--set-json|--output-json'`
Expected: both options listed. Then print the JSON shape the tool writes: `podman run --rm localhost/azoth-boot sh -c 'virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd --output-json /dev/stdout' | head -20`. The `MokListTrusted` entry in Step 4 must follow that shape; if the tool lacks `--set-json`, stop and report.

- [ ] **Step 2: Write the failing tests**

`test_boot_cases.py`:

```python
def insmod_spec(spec):
    script = 'die() { echo "error: $*" >&2; exit 1; }\n' + function("insmod_spec") + f'\ninsmod_spec "{spec}"\n'
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    return result.returncode, result.stdout.split()


class InsmodSpecs(unittest.TestCase):
    def test_one_errno_applies_to_both_firmwares(self):
        self.assertEqual(insmod_spec("a.ko:ENODEV"), (0, ["a.ko", "ENODEV", "ENODEV"]))

    def test_a_bios_errno_overrides(self):
        self.assertEqual(insmod_spec("a.ko:ENODEV:EKEYREJECTED"), (0, ["a.ko", "ENODEV", "EKEYREJECTED"]))

    def test_malformed_insmod_specs_are_refused(self):
        for spec in ("a.ko", "a.ko:", "a.ko:ENODEV:", "a.ko:EPERM", "a.ko:ENODEV:EPERM", ":ENODEV"):
            with self.subTest(spec):
                self.assertNotEqual(insmod_spec(spec)[0], 0)
```

`test_boot_init.py`, inside `PlatformAssertions`:

```python
    def ima(self, describe_rc, padd_rc, listed=""):
        return (f'keyctl() {{ case $1 in describe) return {describe_rc} ;; padd) return {padd_rc} ;; '
                f'list) echo "{listed}" ;; esac; }}\nimakey_file=/dev/null')

    def test_a_refused_key_passes(self):
        self.assertEqual(run_function("ima_key_refused", self.ima(0, 1)).returncode, 0)

    def test_an_accepted_key_fails(self):
        self.assertNotEqual(run_function("ima_key_refused", self.ima(0, 0)).returncode, 0)

    def test_ima_refusal_needs_the_keyring(self):
        self.assertNotEqual(run_function("ima_key_refused", self.ima(1, 1)).returncode, 0)
```

`test_signer_run.py`: replace `test_prepare_signs_a_copy_the_allow_list_admits_and_ships_its_certificate` with a version that also copies `forge/specs/azoth/keys/profiles/test-user-ca.cnf` into the fixture and asserts:

```python
        self.assertEqual(len(runs), 5)
        self.assertIn(f"-v {self.root}/mokca:/modules ", runs[3])
        self.assertIn(f"-v {self.root}/mokleaf:/modules ", runs[4])
        self.assertEqual(sum("nothing else" in c for c in self.calls()), 4, "out/, mok/, mokca/, mokleaf/ admitted")
        self.assertIn("BEGIN CERTIFICATE", (self.root / "mokca/test-ca.pem").read_text())
        self.assertIn("BEGIN CERTIFICATE", (self.root / "mokleaf/test-leaf.pem").read_text())
        ca = subprocess.run(["openssl", "x509", "-in", str(self.root / "mokca/test-ca.pem"), "-noout", "-ext", "basicConstraints,keyUsage"], capture_output=True, text=True).stdout
        self.assertIn("CA:TRUE", ca)
        self.assertIn("Certificate Sign", ca)
        self.assertNotIn("Digital Signature", ca)
        verify = subprocess.run(["openssl", "verify", "-CAfile", str(self.root / "mokca/test-ca.pem"), str(self.root / "mokleaf/test-leaf.pem")], capture_output=True, text=True)
        self.assertEqual(verify.returncode, 0, verify.stdout + verify.stderr)
```

(add `import subprocess` if the file lacks it).

- [ ] **Step 3: Run tests to verify they fail**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -v`
Expected: FAIL: `boot.sh has no function insmod_spec`, `init has no function ima_key_refused`, and `3 != 5` in the signer test.

- [ ] **Step 4: Implement**

`keys/profiles/test-user-ca.cnf`:

```
# Test-only certificate profiles of the D40 user path (docs/architecture/doc_kernel_profile.md,
# section 12 item 2): a user CA enrolled in MokList and trusted for the machine keyring, and a
# leaf it issues. signer/run.sh prepare generates both keys in its own work directory; no key
# of this profile ever leaves the CI job.
[ req ]
default_bits = 2048
default_md = sha256
distinguished_name = req_distinguished_name
prompt = no
string_mask = utf8only
x509_extensions = ca

[ req_distinguished_name ]
CN = Athanor OS K3 test user CA

[ ca ]
basicConstraints = critical,CA:TRUE
keyUsage = critical,keyCertSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid

[ leaf ]
basicConstraints = critical,CA:FALSE
keyUsage = digitalSignature
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
```

`signer/run.sh`: add `USER_CA_PROFILE=forge/specs/azoth/keys/profiles/test-user-ca.cnf` beside `SECUREBOOT_PROFILE`, and at the end of the `prepare)` branch, before `;;`:

```bash
    # D40: a user CA (CA:TRUE, keyCertSign only) signs one copy directly, and a leaf it issued
    # signs another. boot.sh enrols and trusts the CA; the kernel must accept the first copy
    # under UEFI and refuse the second everywhere.
    mkdir mokca mokleaf
    cp -a out/open mokca/open
    cp -a out/open mokleaf/open
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$ROOT/$USER_CA_PROFILE" \
        -keyout "$WORK/test-ca" -out "$WORK/test-ca.pem" 2> /dev/null
    openssl req -new -newkey rsa:2048 -nodes -subj '/CN=Athanor OS K3 test user leaf/' \
        -keyout "$WORK/test-leaf" -out "$WORK/test-leaf.csr" 2> /dev/null
    openssl x509 -req -days 2 -in "$WORK/test-leaf.csr" -CA "$WORK/test-ca.pem" -CAkey "$WORK/test-ca" \
        -extfile "$ROOT/$USER_CA_PROFILE" -extensions leaf -out "$WORK/test-leaf.pem" 2> /dev/null
    for pair in ca:mokca leaf:mokleaf; do
        name=${pair%%:*} tree=${pair#*:}
        signer -v "$ROOT/$tree:/modules" -v "$ROOT/kernel-unsigned:/in:ro" \
            -v "$WORK/test-$name:/run/keys/test-$name:ro" -v "$WORK/test-$name.pem:/run/certs/test-$name.pem:ro" -- \
            modules --key "/run/keys/test-$name" --cert "/run/certs/test-$name.pem" --hash /in/module-sig-hash \
            --kver "$(< kernel-unsigned/kver)" --dir /modules
        cp "$WORK/test-$name.pem" "$tree/test-$name.pem"
    done
```

`boot.sh`: add `MOK_CAS=() IMA_KEY=''` to the defaults, the options `--mok-ca) MOK_CAS+=("$2"); shift 2 ;;` and `--ima-key) IMA_KEY=$2; shift 2 ;;`, their usage lines, and:

```bash
insmod_spec() { # insmod_spec FILE.ko:ERRNO[:BIOS_ERRNO]: print "file uefi-errno bios-errno"
  local ko=${1%%:*} rest=${1#*:} uefi bios
  [[ $1 == *:* && $ko ]] || die "--insmod expects FILE.ko:ERRNO[:BIOS_ERRNO], got: $1"
  uefi=${rest%%:*}; bios=$uefi; [[ $rest == *:* ]] && bios=${rest#*:}
  for e in "$uefi" "$bios"; do
    [[ $e =~ ^(0|ENODEV|EKEYREJECTED)$ ]] || die "--insmod: unknown errno '$e' in $1"
  done
  echo "$ko $uefi $bios"
}
```

Replace the `K3_INSMOD` loop with:

```bash
K3_INSMOD_UEFI='' K3_INSMOD_BIOS=''
for i in "${!INSMOD[@]}"; do
  read -r ko uefi bios < <(insmod_spec "${INSMOD[$i]}")
  [[ -f $ko ]] || die "--insmod: no such file: $ko"
  install -D -m 644 "$ko" "$R/modules/$i-${ko##*/}"
  K3_INSMOD_UEFI+="${K3_INSMOD_UEFI:+,}$i-${ko##*/}:$uefi"
  K3_INSMOD_BIOS+="${K3_INSMOD_BIOS:+,}$i-${ko##*/}:$bios"
done
if [[ $IMA_KEY ]]; then
  install -d "$R/ima" && openssl x509 -in "$IMA_KEY" -outform DER -out "$R/ima/key.der"
  TEST_CMDLINE+=" k3.imakey=1"
fi
BIOS_CMDLINE="$TEST_CMDLINE${K3_INSMOD_BIOS:+ k3.insmod=$K3_INSMOD_BIOS}"
UEFI_CMDLINE="$TEST_CMDLINE${K3_INSMOD_UEFI:+ k3.insmod=$K3_INSMOD_UEFI} k3.sb=1"
```

(`insmod_spec` runs in a process substitution, so also validate every spec once in the main shell before the loop: `for s in "${INSMOD[@]}"; do insmod_spec "$s" > /dev/null; done`.) After the `ADD_MOK` loop:

```bash
# D40: a user CA enrolled in MokList and trusted for the machine keyring (mokutil --trust-mok
# sets MokListTrusted, which shim mirrors to MokListTrustedRT).
for cert in "${MOK_CAS[@]}"; do ADD_MOK+=(--add-mok "$(< /proc/sys/kernel/random/uuid)" "$cert"); done
SET_JSON=()
if [[ ${#MOK_CAS[@]} -gt 0 ]]; then
  printf '%s\n' '{"version": 2, "variables": [{"name": "MokListTrusted", "guid": "605dab50-e046-4300-abb6-3dd810dd8b23", "attr": 3, "data": "01"}]}' > "$WORK/moktrust.json"
  SET_JSON=(--set-json "$WORK/moktrust.json")
fi
virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd -o "$WORK/vars.fd" "${ADD_MOK[@]}" "${SET_JSON[@]}" > "$OUT/varstore.log"
```

(remove the earlier `virt-fw-vars` line; adjust the JSON keys to the shape printed in Step 1).

`boot/init`: parse `k3.imakey=1) imakey=1 ;;` (initialise `imakey=''`), set `imakey_file=${K3_IMAKEY:-/ima/key.der}`, and:

```sh
ima_key_refused() { # D40, D46: a key vouched only by a machine-keyring CA never enters .ima
  keyctl describe %:.ima || { echo "no .ima keyring"; return 1; }
  if keyctl padd asymmetric '' %:.ima < "$imakey_file"; then echo "the key was accepted"; return 1; fi
  keyctl list %:.ima
}
```

before the insmod loop: `[ "$imakey" ] && check ima-key ima_key_refused`.

`call-nvidia-kmod-prepare.yml`, after the `nvidia-mok-signed` upload:

```yaml
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-mokca-signed
          path: mokca/
          if-no-files-found: error

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-mokleaf-signed
          path: mokleaf/
          if-no-files-found: error
```

`nvidia-kmod.yml` boot job: two `actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` steps with `name: nvidia-mokca-signed`, `path: mokca` and `name: nvidia-mokleaf-signed`, `path: mokleaf`; the `boot.sh` call becomes:

```yaml
            bash forge/specs/azoth/boot.sh --rpms /forge/out --out /forge/boot-out \
              --mok mok/test-mok.pem \
              --mok-ca mokca/test-ca.pem \
              --ima-key mokleaf/test-leaf.pem \
              --insmod "signed/open/${ko}:ENODEV" \
              --insmod "signed/legacy/${ko}:ENODEV" \
              --insmod "unsigned/open/${ko}:EKEYREJECTED" \
              --insmod "mok/open/${ko}:EKEYREJECTED" \
              --insmod "mokca/open/${ko}:ENODEV:EKEYREJECTED" \
              --insmod "mokleaf/open/${ko}:EKEYREJECTED"
```

and the comment above it gains: "a module signed directly by a trusted user CA loads under UEFI and is rejected without firmware; a module signed by a leaf of that CA and an IMA key vouched by it are refused everywhere (D40, D46)."

- [ ] **Step 5: Run tests to verify they pass**

Run: `python3 -B -m unittest discover -s forge/specs/azoth/tests -v && shellcheck forge/specs/azoth/boot.sh forge/specs/azoth/signer/run.sh && python3 scripts/verify.py workflows`
Expected: PASS, no findings, workflows check green.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/azoth/keys/profiles/test-user-ca.cnf forge/specs/azoth/signer/run.sh forge/specs/azoth/boot.sh forge/specs/azoth/boot/init forge/specs/azoth/tests .github/workflows/call-nvidia-kmod-prepare.yml .github/workflows/nvidia-kmod.yml
git commit -m "test(kernel): prove the D40 user CA path and the IMA keyring restriction in the module chain"
```

---

### Task 7: Documentation of the matrix and the profile

**Files:**
- Modify: `docs/architecture/doc_kernel_build.md` (section 7 item 3), `forge/specs/azoth/KERNEL.md` (Nehalem mentions)

**Interfaces:**
- Consumes: the case names and assertions of Tasks 2 to 6.
- Produces: documentation only.

- [ ] **Step 1: Write the failing check**

Run: `grep -n -i nehalem docs/architecture/doc_kernel_build.md forge/specs/azoth/KERNEL.md`
Expected now: matches (the check fails while any remain).

- [ ] **Step 2: Rewrite the affected sentences with a script, not Edit**

Write `/.scratch/p2_docs.py` that reads each file, replaces exactly the sentences naming Nehalem and the four-case matrix with English text naming the six cases (`bios-penryn`, `bios-host`, `uefi-penryn`, `uefi-host`, `iommu-intel`, `iommu-amd`), Penryn's purpose (D14: x86-64-v1, no POPCNT or SSE4.2), and the new assertions (forced lockdown without `lockdown=`, `init_on_free`, `vsyscall`, `debugfs` from the build, `Dynamic Preempt: lazy`, ASLR bits 32/16, lazy IOMMU domains, the mesh platform options, the exact builtin set, the D40 CA and leaf modules, the IMA key refusal), and writes the file back unchanged elsewhere. Run it with `python3 -B /.scratch/p2_docs.py`.

- [ ] **Step 3: Verify**

Run: `grep -c -i nehalem docs/architecture/doc_kernel_build.md forge/specs/azoth/KERNEL.md; git diff --stat`
Expected: `0` for both files; the diff touches only the replaced lines (no whole-file reformat). Then `python3 scripts/verify.py docs` exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/architecture/doc_kernel_build.md forge/specs/azoth/KERNEL.md
git commit -m "docs(kernel): describe the P2 boot matrix"
```

---

### Task 8: The P2 gate

**Files:** none new; CI evidence only.

**Interfaces:**
- Consumes: every commit above on one branch based on `origin/iso-v0`.
- Produces: a green Kernel Build run (build, config, boot matrix, kmod with the module chain) and the evidence for the maintainer.

- [ ] **Step 1: Local checks**

Run: `just lint && python3 scripts/verify.py && python3 -B -m unittest discover -s forge/specs/azoth/tests && python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests`
Expected: all exit 0.

- [ ] **Step 2: Open the pull request and watch Kernel Build**

```bash
git push -u origin HEAD
gh pr create --base iso-v0 --title "feat(kernel): block P2 of the kernel profile" --body-file /.scratch/p2-pr.md
gh run watch "$(gh run list --workflow 'Kernel Build' --branch "$(git branch --show-current)" --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```

`/.scratch/p2-pr.md` says what changes (section 5 options, command line, matrix), why (block P2, critical path to Fedora 45, ADR-0078), and how it was verified (the commands of Step 1 and the run links). Expected: exit 0. A `check_delta` failure names the option and the generated value: if it is one of `UBSAN_TRAP`, `PROC_KCORE`, `BLK_DEV_WRITE_MOUNTED`, `MSEAL_SYSTEM_MAPPINGS` (or `KSTACK_ERASE` because the compiler lacks support), stop and ask the maintainer for a decision record before removing it.

- [ ] **Step 3: The module chain**

The `kmod` job of Kernel Build, and on push the Orchestrator's NVIDIA kmod run, must show in `nvidia-boot-logs` for every case: `insmod-4` (mokca) `ENODEV` under `uefi-*` and `EKEYREJECTED` under `bios-*`/`iommu-*`, `insmod-5` (mokleaf) `EKEYREJECTED`, and `K3 ok   ima-key`.
Run: `gh run download <run-id> -n nvidia-boot-logs -D /var/tmp/p2-kmod && grep -h -E '^K3 (FAIL|RESULT)' /var/tmp/p2-kmod/*.log`
Expected: six `K3 RESULT ok` lines and no `K3 FAIL`.

- [ ] **Step 4: Acceptance after the merge**

After the squash merge, the image built from the merge commit must pass ISO Acceptance with `profile-ok` (the new kconfig entries are checked on the booted machine).
Run: `gh run list --workflow 'ISO Acceptance' --branch iso-v0 --limit 1 --json conclusion,headSha`
Expected: `"conclusion":"success"` on the merge commit. Then report to the maintainer: the run links, and that section 15's implementation status line for P2 can be updated.
