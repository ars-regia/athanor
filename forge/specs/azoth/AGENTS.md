# Azoth kernel: area rules

These rules add to the root and `forge/` `AGENTS.md` files. The build is specified in
`docs/architecture/doc_kernel_build.md` and the profile in
`docs/architecture/doc_kernel_profile.md`; `KERNEL.md` describes every file here and the
local commands.

- **Stop and ask before touching `keys/`**: the Secure Boot and module signing
  certificates, and the revoked ones compiled into the kernel's blacklist. The private keys
  live only in the `signing-kernel` environment (and `signing` during the image key rotation,
  `docs/operations/secrets.md`).
- **Pins move through `bump.py`.** `python3 bump.py check --group kernel` shows a bump;
  `apply` rewrites `pins.env`, the `FROM` lines of the kernel Containerfiles and the pin
  table in `KERNEL.md`. The `system` group (base image, NVIDIA locks) goes in a separate
  pull request. Fedora and CachyOS sources move together, at the same patch level: the
  CachyOS base diff does not apply to a Fedora tree at a newer one.
- **`cmdline` is generated** from `forge/specs/athanor-kernel-profile/profile.toml` by
  `kernel_profile.py generate`: edit the manifest, never the file.
- **Patches apply without fuzz.** When `build.sh --stage prep` stops with
  `refresh needed`, run `--stage refresh`, review the hunks it applied, and commit the copies
  under `patches/refreshed/`. Athanor's own patches are git-format files in `patches/`;
  those in `patches/redhat/` touch the Fedora tree only, never Kconfig.
- **Configuration decisions go in `kernel-local`.** Normalise `=n` to
  `# CONFIG_X is not set` before comparing configs: both spellings mean the same.
- **Locks are regenerated, never edited**: `lock.sh generate`, checked by `lock.sh check`.
- **Every build path exports `KBUILD_BUILD_TIMESTAMP`**; without it the build is not
  reproducible.
- **`cfi=warn` is for one diagnostic boot only**, never a shipped argument: it turns kCFI
  enforcement off. To hunt kCFI faults, build with
  `-Wincompatible-function-pointer-types-strict`: enum-versus-int pointer mismatches compile
  cleanly and trap at runtime.

## Traps already seen

- Koji deletes `data/signed/` of non-latest builds but keeps `src/` and `data/sigcache/`:
  rebuild the signed SRPM with `koji.splice_rpm_sighdr`, which is byte-identical.
- In Fedora 43 `rt-tests` is Request Tracker: hackbench and cyclictest are in
  `realtime-tests`.
- keys.openpgp.org serves kernel.org signing keys without user IDs: fetch them from the
  git.kernel.org `pgpkeys` repository.
- `nm ... | grep -q` under `pipefail` dies of SIGPIPE when grep exits early
  (`forge/specs/azoth/nvidia.sh`): capture the output first, then test it.

## Checks

`python3 -B -m unittest discover -s forge/specs/azoth/tests`. The kernel build needs the
builder container and about an hour on 16 cores; the `Kernel gate` check of
`kernel-build.yml`, required on `iso-v0` and `main`, runs it in CI.
