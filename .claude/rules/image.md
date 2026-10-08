---
paths:
  - "system/Containerfile"
  - "system/*.sh"
  - "system/disk_config/**"
  - "forge/build/**"
  - "forge/builder/**"
  - "forge/config/**"
  - "**/*.spec"
  - "**/*.ks"
  - "flake.nix"
---

# Image, packages and registry

- Handle both update paths when changing kernel arguments. Why: `rpm-ostree upgrade` of a layered deployment ignores `kargs.d`, and `bootc upgrade` applies `kargs.d` but refuses a layered deployment.
- Treat `incompatible: true` and `image: null` in `bootc status --json` as a valid state. Why: a deployment with rpm-ostree layered packages reports exactly that.
- Pass `--registries-conf` or a throwaway `HOME` to skopeo. Why: skopeo ignores `CONTAINERS_REGISTRIES_CONF` and `XDG_CONFIG_HOME`.
- Set `use-sigstore-attachments: true` in `registries.d` before `--sign-by-sigstore-private-key`. Why: without it the signature is not attached.
- Probe a registry anonymously first and treat ghcr's 403 as absent or private. Why: ghcr answers 403, not 404, and skopeo `--creds` with a rejected token fails with 403 even on a public image.
- Test an unpublished RPM through a `registries.conf` prefix remap to a local registry, after `system/kernel-artifacts.sh resolve`. Why: `build-image.sh --pull=newer` replaces a locally retagged tier overlay with the published one.
- Load SELinux modules at build time with `semodule -i`. Why: a `.cil` file under `/usr/share/selinux/packages` is not active.
- List both `nix` and `nix-daemon` in `upstream_core`. Why: `nix` does not pull `nix-daemon`, and non-root users then get Permission denied on the store lock.
- Declare every runtime need as `Requires:` or in `packages.json`. Why: the image installs with `install_weak_deps=False`, which drops `Recommends:` such as busybox.
- Point Nix-built binaries at the system interpreter (`patchelf`) and label them `bin_t` before a system service runs them. Why: they load the `/nix/store` interpreter, labelled `var_t`, and the service fails with 203/EXEC.
- Audit images by digest. Why: a local `:latest` can be days old.
- Keep read-only btrfs snapshots out of the checkout. Why: they break podman `:Z` relabelling of the build context.
