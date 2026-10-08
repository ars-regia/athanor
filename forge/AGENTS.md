# forge: area rules

These rules add to the root `AGENTS.md`. `forge/README.md` maps this directory; the build
system is specified in `docs/architecture/doc_build_system.md` and package development in
`docs/architecture/doc_forge_development_guide.md`. The kernel has its own rules in
`forge/specs/azoth/AGENTS.md`.

- **The four golden rules** (development guide, section 2): no dynamic downloads in a spec,
  no `%pre` or `%post` that writes under `/usr` or `/etc`, no disabled security
  (`gpgcheck=0`, weakened hardening flags), no network during the build.
- **A payload change bumps the spec.** Any change to what a package installs raises
  `Release` and adds a `%changelog` entry in the spec's existing style.
- **Declare every runtime need** as `Requires:` or in `config/packages.json`. The image
  installs with `install_weak_deps=False`, so a `Recommends:` is not installed.
- **Enable units with a preset** under `/usr/lib/systemd/system-preset/` and the
  `%systemd_post`, `%systemd_preun` and `%systemd_postun` macros; never start a unit from a
  scriptlet.
- **`config/packages.json` decides what is built**: `custom_packages`, the tiers
  `custom_tier0` to `custom_tier3`, and the `upstream_*` lists that `system/Containerfile`
  installs from Fedora. It is not formatter-clean: change the lines you mean, never
  reformat the file.
- **Licences.** Own crates and specs declare `GPL-3.0-or-later`; upstream specs a valid SPDX
  expression.
- **Rust crates under `specs/`** take dependency versions from `[workspace.dependencies]` in
  the root `Cargo.toml` (`name = { workspace = true }`).
- **Registry retention** (`forge/scripts/clean_ghcr.sh`, `forge/specs/azoth/retention.sh`)
  goes by reachability from tagged manifests, never by "untagged": cosign v3 stores
  signatures as untagged manifests.

## Checks

```bash
python3 scripts/verify.py shipped specs forge-rules licence
python3 -B -m unittest discover -s forge/scripts/tests
bash forge/scripts/run_spec_build.sh <builder image> specs/<package>
```

A package with Python tests keeps them in `forge/specs/<package>/tests/`. Shell crates that
link GTK build and test in the rig: `forge/test/shell/rig.sh cargo test -p <crate>`, after `forge/test/shell/rig.sh build-image`.
