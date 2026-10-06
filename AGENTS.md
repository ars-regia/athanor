# Athanor: guide for agents and contributors

The entry point for people and coding agents (Claude Code, Codex and others) working in this
repository. `forge/`, `forge/specs/azoth/`, `system/` and `.github/workflows/` each carry a
nested `AGENTS.md` with the rules of that area: read it before you change files there.

## What Athanor is

Athanor is an immutable, zero-trust Linux operating system shipped as a bootc image built on
Fedora `base-atomic`. Its kernel, Azoth, is Fedora's kernel rebuilt with clang on the CachyOS
base, with added hardening. The desktop runs on the cosmic-comp Wayland compositor with
Athanor's own GTK4 shell (greeter, bar, dock, launcher). The forge builds Athanor's packages
as RPMs; the Rust crates of the shell and of the platform services form one Cargo workspace.

## Map

| Path | What it holds |
| --- | --- |
| `system/` | the image (`Containerfile`, `build-image.sh`), its signing and promotion, platform crates |
| `forge/` | the RPM build: `config/packages.json`, `specs/<package>/`, pipeline `scripts/`, `test/` rigs |
| `forge/specs/azoth/` | the Azoth kernel: pins, patches, build and boot matrix |
| `docs/architecture/` | specifications (`doc_<area>.md`) and the component inventory `components.toml` |
| `docs/decisions/` | decision records, indexed in `docs/decisions/README.md` |
| `docs/operations/` | runbooks and the team model: contributing, ownership, branching, secrets |
| `scripts/` | `verify.py` (project checks), dev VM and runner tooling, their tests |
| `.github/workflows/` | CI, every workflow described in `docs/architecture/doc_ci.md` |
| `experimental/EXEMPT` | workspace crates that no package ships |

`docs/operations/repository-layout.md` describes every top-level entry.

## Checks

Run them from the repository root. There is no single suite: target what you changed.

| Check | Command |
| --- | --- |
| Linters (shell scripts, Justfiles) | `just lint`; `just format` rewrites, `just check-syntax` only checks |
| Project checks that CI runs | `python3 scripts/verify.py workflows kickstart boundary cmdline registry licence ci coverage` |
| All project checks | `python3 scripts/verify.py` (`--list` names the checks; pass names to run some) |
| A crate | `cargo test -p <crate>`, for every crate you touch |
| Python tests | `python3 -B -m unittest discover -s <dir>/tests`, e.g. `scripts/tests`, `system/tests` |

Some `verify.py` checks fail at HEAD on known debt: compare with `origin/iso-v0` and add no
new failure. `workflows` and `kickstart` pass with only a note when `actionlint` or
`ksvalidator` (pykickstart) is missing, so install both. `just all` runs the whole pipeline
for hours: never use it as a check.

## Inviolable rules

- **Zero-trust.** No daemon or application outside a compartment or a MicroVM. Never
  `chmod 777`, never run as root directly, never bypass the IPE policy, Landlock
  confinement or the compartments.
- **No fake security.** Cryptography, token validation, hashes and attestation are real or
  absent. A placeholder or an unconditional `Ok(true)` in a security path is a defect: if
  you cannot build the real thing, stop and say so.
- **`panic = "abort"` in dev and release.** A panic ends the process; in a daemon that is a
  loss of service. No `unwrap()` or `expect()` outside tests (`verify.py panics`).
- **No hidden failures.** No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides
  an error. Prefer the idiomatic fix to the minimal patch.
- **English for everything committed**: code, comments, commit messages (Conventional
  Commits, as `git log` shows), pull requests, issues, workflow output and documentation,
  in an enterprise tone. The conversation follows the contributor's language. Published
  history is never rewritten to translate it.
- **Portable pipeline.** Logic lives in scripts in the repository; workflow YAML only checks
  out, calls them and uploads their output.
- **No attribution.** No model names, "Generated with" lines, co-author trailers or session
  links in code, commits, pull requests or documents.
- **One logical change per commit.** Report unrelated problems instead of fixing them in
  passing.

## Stop and ask before editing

- Signing, keys, Secure Boot and MOK: `forge/specs/azoth/keys/`, `system/keys/`,
  `system/cosign.pub`, `system/sign-images.sh`, `system/promote.sh`,
  `forge/scripts/sign_attest.sh`, workflow jobs that use the `signing` environment.
- Polkit: `system/athanor-bus-api/src/polkit.rs`. Attestation: `system/confidential_computing/`.
- Authentication: PAM, the greeter's login path, token validation.
- Migrations of data or on-disk state, and anything destructive: force push, history
  rewrite, `git reset --hard`, deleting branches, tags, releases or registry images.

Never read secrets: `.env` files, private keys, `scripts/runner/*.env`. When a value is
needed, ask for the variable's name, not its content.

## Read before editing

- The specification of the area, `docs/architecture/doc_<area>.md`. Only an approved
  revision may be implemented (ADR-0074).
- The decision records that touch it, from `docs/decisions/README.md`.
- The component's entry in `docs/architecture/components.toml`.
- `docs/operations/contributing.md`: the unit of work, review, red CI.

## Naming and generated files

Crates and packages are named `athanor-*`; no `ermete-*` crate remains (the project was
Ermete OS until 2026-09-05, commit `02bf9c05`). Exceptions: the crates `ebpf-core`,
`ebpf-loader` and `xdg-desktop-portal-athanor`, and the upstream specs `azoth`,
`cosmic-comp`, `buildah`, `osbuild` and `stage0-bootstrap`. Rename nothing on your own.

Never open `docs/architecture/graph-vaults/`, `docs/architecture/graph-pages/` or
`.graphify/`: they are generated locally, git-ignored, and thousands of files. Temporary
scripts and logs go in `.scratch/` at the root, which is git-ignored.
