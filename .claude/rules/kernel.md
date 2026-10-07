---
paths:
  - "forge/specs/azoth/**"
---

# Azoth kernel

<!-- Patch layout and the no-fuzz rule are owned by forge/specs/azoth/KERNEL.md. -->

- Bump the Fedora and CachyOS sources together, at the same patch level. Why: the CachyOS base diff does not apply to a Fedora tree at a newer patch level.
- Normalise `=n` lines from `make listnewconfig` to `# CONFIG_X is not set` before comparing configs. Why: the two spellings mean the same and diff as changes.
- Export `KBUILD_BUILD_TIMESTAMP` in every build path. Why: otherwise `UTS_VERSION` carries the build time and the reproducibility check fails.
- Rebuild a pruned signed Fedora SRPM with `koji.splice_rpm_sighdr` from `src/` and `data/sigcache/`. Why: Koji deletes `data/signed/` for non-latest builds but keeps both inputs, and the splice is byte-identical.
- Install `realtime-tests` for hackbench and cyclictest. Why: in Fedora 43 `rt-tests` is Request Tracker.
- Fetch kernel.org signing keys from git.kernel.org `pgpkeys`. Why: keys.openpgp.org serves them without user IDs.
- Build with `-Wincompatible-function-pointer-types-strict` when hunting kCFI faults. Why: enum-versus-int pointer mismatches compile cleanly and trap at runtime.
- Use `cfi=warn` for one diagnostic boot only, never as a shipped argument. Why: it turns kCFI enforcement off.
