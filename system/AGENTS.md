# system: area rules

These rules add to the root `AGENTS.md`. `system/README.md` describes the image, its
pipeline and installation; the image is specified in
`docs/architecture/doc_system_image.md`.

- **Stop and ask** before editing `athanor-bus-api/src/polkit.rs`, `confidential_computing/`,
  `keys/`, `cosign.pub`, `sign-images.sh` or `promote.sh`.
- **Enable units with a preset file, never `systemctl enable` in the `Containerfile`.** The
  `Containerfile` runs `systemctl preset-all` after it and again in the GPU stages, and
  Fedora's `99-default-disable.preset` (`disable *`) then disables every unit no preset
  names (`docs/architecture/doc_tetragon.md`). `Containerfile:180` still enables three units
  that way: known debt, tracked in `doc_tetragon.md`, not a pattern to copy.
- **A workspace crate ships in a package** or is listed in `experimental/EXEMPT`
  (`verify.py shipped`).
- **No runtime path into the build tree or `/tmp`.** Load artefacts from installed paths,
  never `target/`; privileged state goes under `/run/athanor`, not `/tmp` (`verify.py paths`).
- **Only `athanor-compositor-client` depends on COSMIC** or reads its configuration
  (`verify.py boundary`).
- **The polkit subject identifies the caller**, not the bus connection, and every action the
  code checks is declared in a `.policy` file (`verify.py polkit polkit-subject`).
- **Dependency versions** come from `[workspace.dependencies]` in the root `Cargo.toml`.
- **`system/.gitignore` hides `output`, `test_*/`, `*.log`, `logs_*`, `mnt_*/` and `ctr_id` at
  any depth.** After adding files, run `git status --porcelain --ignored -- <dir>` and add a
  narrow negation for anything it hides.
- **`build-image.sh` without `SECUREBOOT_SIGNING_KEY`** signs the UKI with a throwaway key
  and refuses to push. That is the local path; do not work around it.

## Checks

```bash
python3 scripts/verify.py paths shipped polkit polkit-subject boundary panics
python3 -B -m unittest discover -s system/tests
cargo test -p <crate>
```

Crates that link GTK build and test in the shell rig:
`forge/test/shell/rig.sh cargo test -p <crate>`.
