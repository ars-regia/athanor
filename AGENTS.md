# Athanor: guide for agents and contributors

The entry point for people and coding agents (Claude Code, Codex and others) working in this
repository. `forge/`, `forge/specs/azoth/`, `system/` and `.github/workflows/` each carry a
nested `AGENTS.md` with the rules of that area: read it before you change files there.
`docs/operations/repository-layout.md` maps the top-level directories.

## What Athanor is

Athanor is an immutable, zero-trust Linux operating system shipped as a bootc image built on
Fedora `base-atomic`. Its kernel, Azoth, is Fedora's kernel rebuilt with clang on the CachyOS
base, with added hardening. The desktop runs on the cosmic-comp Wayland compositor with
Athanor's own GTK4 shell (greeter, bar, dock, launcher). The forge builds Athanor's packages
as RPMs; the Rust crates of the shell and of the platform services form one Cargo workspace.

## Checks

`just check` is the gate every pull request passes (`Justfile`). It requires `actionlint`,
`shellcheck` and `cargo-deny`; install `ksvalidator` (pykickstart) too, or `verify.py kickstart`
passes with only a note. It runs `just check-syntax`, every `verify.py` check, the kernel profile,
the Calmo palette, every `test_*.py` directory, the Rust workspace and `just check-deny`
(`deny.toml`). Before a push, run what you changed:

| Check | Command |
| --- | --- |
| Project checks | `python3 scripts/verify.py <check>...`; `--list` names them |
| A crate | `cargo test -p <crate>`; one that links GTK: `forge/test/shell/rig.sh cargo test -p <crate>`, after `rig.sh build-image` |
| Python tests | `python3 -B -m unittest discover -s <dir>/tests`, e.g. `scripts/tests`, `system/tests` |
| Linters | `just lint`; `just format` rewrites, the `Justfile` through `just --unstable --fmt` |
| A kernel panic read from a QR code | `python3 scripts/decode-drm-panic.py '<url or payload>'` |

A check red on known debt is tolerated only while `scripts/ci/known-red.txt` lists its finding
with an issue and an expiry; the list only shrinks. `just all` runs the whole pipeline for
hours: never use it as a check.

## Inviolable rules

- **Least privilege for services.** Every shipped service sets `NoNewPrivileges=yes` or a
  `CapabilityBoundingSet=` allow-list without `CAP_SYS_ADMIN`, `CAP_SYS_MODULE`,
  `CAP_DAC_OVERRIDE` or `CAP_SYS_PTRACE` (`doc_threat_model.md` TM8, checked by
  `verify.py services`). A confined application never writes the persistence paths of TM3.
  Never `chmod 777`, never run as root directly, never bypass the IPE policy or Landlock
  confinement.
- **No fake security.** Cryptography, token validation, hashes and attestation are real or
  absent. A placeholder or an unconditional `Ok(true)` in a security path is a defect: if
  you cannot build the real thing, stop and say so.
- **`panic = "abort"` in dev and release.** A panic ends the process; in a daemon that is a
  loss of service. No new `unwrap()`, `expect()` or `panic!` outside tests: `verify.py panics`
  holds a budget that only goes down.
- **No hidden failures.** No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides
  an error. Prefer the idiomatic fix to the minimal patch.
- **English for everything committed**: code, comments, commit messages (Conventional
  Commits, as `git log` shows), pull requests, issues, workflow output and documentation,
  in an enterprise tone. The conversation follows the contributor's language. Published
  history is never rewritten to translate it.
- **Portable pipeline, GitHub as glue.** Logic lives in scripts in the repository; workflow
  YAML only checks out, calls them and uploads their output (`.github/workflows/AGENTS.md`).
  No hard-coded registry owner; prefer a standard mechanism (OCI, cosign with a key, a file on
  disk) over one that exists only on GitHub.
- **No attribution.** No model names, "Generated with" lines, co-author trailers or session
  links in code, commits, pull requests or documents.
- **One logical change per commit.** Report unrelated problems instead of fixing them in
  passing.
- **At most five open pull requests.** With five open, finish before starting: a new pull
  request waits until one is merged or closed, and reviewers' findings on open ones are
  closed before new work begins. A branch without an open pull request is merged, archived
  as a tag `archive/<branch>`, or deleted.
- **Every class of defect an audit finds becomes an automatic check** in the pull request that
  fixes it, or an issue that names the check to add. Audit reports stay outside the public
  repository. When audits run and what they re-examine: `docs/operations/audits.md`.

## Stop and ask before editing

- Signing, keys, Secure Boot and MOK: `forge/specs/azoth/keys/`, `system/keys/`,
  `system/sign-images.sh`, `system/promote.sh`, `forge/scripts/sign_attest.sh`, and workflow
  jobs that use the `signing-kernel`, `signing-images` or `signing` environments.
- Polkit: `system/athanor-bus-api/src/polkit.rs`. Attestation: `system/confidential_computing/`.
- Authentication: PAM, the greeter's login path, token validation.
- Migrations of data or on-disk state, and anything destructive: force push, history
  rewrite, `git reset --hard`, deleting branches, tags, releases or registry images.
- Never push to `iso-v0` or `main` directly: every change lands through a pull request.

Never read secrets: dotenv files (`.env`, `.env.*`), private keys, credential stores
(`gh`, `containers/auth.json`, `/etc/credstore*`). When a value is needed, ask for the
variable's name, not its content.

## Read before editing

- The specification of the area, `docs/architecture/doc_<area>.md`. Only an approved
  revision may be implemented (ADR-0074).
- The decision records that touch it, from `docs/decisions/README.md`.
- The component's entry in `docs/architecture/components.toml`.
- `docs/operations/contributing.md`: the unit of work, review, red CI.

## Names and generated files

Crates and packages are named `athanor-*`, with the exceptions of `repository-layout.md`; rename
nothing on your own. Never open `docs/architecture/graph-vaults/`, `docs/architecture/graph-pages/`
or `.graphify/`: generated, git-ignored, thousands of files; ask `graphify query "<question>"`.
Temporary scripts and logs go in `.scratch/` at the root, which is git-ignored.
