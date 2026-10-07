# Contributing to Athanor

| Field | Value |
| --- | --- |
| Purpose | From a clean machine to a merged pull request, for people and for agents |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-06. Items marked _(Proposal)_ await the maintainer |
| Depends on | [.github/CONTRIBUTING.md](../../.github/CONTRIBUTING.md) (code rules), [branching](branching.md), [ownership](ownership.md) |
| Defines | CT1 to CT8 |

This file is the operating model. The code rules (panics, placeholders in security paths,
commit language) stay in [.github/CONTRIBUTING.md](../../.github/CONTRIBUTING.md). Read each
linked document's header before its body.

## CT1. First day

| Need | Where |
| --- | --- |
| Tools | `git`, `gh`, `just`, `podman`, a Rust toolchain (`cargo`), `python3`; `actionlint`, `shellcheck` and `ksvalidator` (pykickstart), because `scripts/verify.py` passes a check with a note when one is missing |
| Shell and application work without an image build | [scripts/devvm/README.md](../../scripts/devvm/README.md): tiers A (nested compositor), B (KVM VM), C (CI image) |
| GTK crates, golden captures, end-to-end scenes | the shell rig, [forge/test/shell/rig.sh](../../forge/test/shell/rig.sh) (its header lists every command) |
| The builder image and local package builds | [doc_build_system.md](../architecture/doc_build_system.md) sections 3 and 8, [forge/README-build-local.md](../../forge/README-build-local.md) |
| The self-hosted kernel runner (maintainers only) | [scripts/runner/README.md](../../scripts/runner/README.md) |
| What exists and why | `docs/architecture/doc_<area>.md` (specs), `docs/decisions/` (decision records) |

## CT2. Build

| Target | Command |
| --- | --- |
| One crate | `cargo build -p <crate>`; a crate that links GTK: `forge/test/shell/rig.sh build-image` once, then `forge/test/shell/rig.sh cargo build -p <crate>` |
| One spec | `bash forge/scripts/run_spec_build.sh <builder image> specs/<package>` (RPMs in `forge/RPMS/`) |
| The image | `system/build-image.sh` (no key reaches the build: the vmlinuz comes signed from `azoth-boot`, [system/README.md](../../system/README.md)) |
| An image with unmerged RPMs, for the dev VM | `scripts/devvm/local-image.sh` |

`just all` runs the whole pipeline and takes hours: never run it as a check.

## CT3. Tests and checks

| Check | Command |
| --- | --- |
| Linters | `just lint` |
| Project checks | `python3 scripts/verify.py`, or one check: `workflows`, `kickstart`, `polkit`, `paths`, `shipped`, `docs`, `panics`, `cmdline`, `polkit-subject`, `specs`, `boundary`, `registry`, `forge-rules`, `licence`, `decisions` |
| Tests of `verify.py` | `python3 -B -m unittest discover -s scripts/tests` |
| Python tests of an area | `python3 -B -m unittest discover -s <area>/tests` |
| A crate | `cargo test -p <crate>`; there is no single suite |

A new `verify.py` check is registered with `@check` like the others and has a test under
`scripts/tests/`. Compare the full `verify.py` run with the product branch: no new failure.

## CT4. The unit of work

- **One issue per unit.** Assign it to yourself before starting (`gh issue edit <n> --add-assignee @me`). No issue, no branch.
- **One area per agent at a time.** Areas are defined in [ownership.md](ownership.md). A change that spans areas is split, or reviewed by every owner it touches.
- **Small pull requests against the product branch** (today `iso-v0`, [branching.md](branching.md)). No stacked chains: when B needs A, wait for A to merge or put both in one pull request.
- Commit subjects follow Conventional Commits in English, as `git log` shows. No attribution of any kind.

## CT5. Specs and decisions

- **A behaviour change updates its spec in the same pull request:** `docs/architecture/doc_<area>.md`, citing the item id it changes (for example `BR7`).
- **A decision is a record:** `docs/decisions/NNNN-slug.md`, with the front matter described in `docs/decisions/README.md`. A spec states what; the record states why and which alternative was rejected.
- A runbook (run, release, rotate, rebuild) goes to `docs/operations/<topic>.md`.

## CT6. Review

- **Area owners** are listed in `.github/CODEOWNERS`; the proposed area map is [ownership.md](ownership.md).
- **Two-person review** for signing, attestation, polkit, cryptography and authentication: two approvals, at least one from an owner of the area, never the author. Today the maintainer is the only owner, so every such change waits for the maintainer.
- Enforcement _(Proposal)_: once a second owner exists, turn on "Require review from Code Owners" on the product branch; today `iso-v0` requires only the status checks `Kernel gate`, `Spec gate` and `gate` (`.github/settings/branch-protection.json`), and `gate` alone once the follow-up of `github-settings.md` section 8 removes the legacy `pull_request` triggers.

## CT7. Red CI _(Proposal)_

- A workflow on the product branch is green or disabled. There is no third state.
- A red workflow gets, within one working day, a fix or `gh workflow disable <file>` plus an open issue that names the failing run id. The fix pull request enables it again.
- No `continue-on-error`, no `|| true`, no retry loop that hides the failure.
- Today, on `iso-v0`: `forge-ghcr-cleanup.yml` failed (run 37173567085), `fuzzing.yml` failed (run 37190403649), and `test-mok.yml` is registered as active with no file in the tree (`gh api repos/ars-regia/athanor/actions/workflows`, 2026-10-06).

## CT8. Claude Code

- **Project configuration lives in `.claude/`:** rules with `paths:` front matter under `.claude/rules/`, skills under `.claude/skills/`, plus the shared settings being added there. Personal settings stay in `.claude/settings.local.json`, which is not committed.
- **Scope a session to one area and one issue.** Start a new session for the next issue instead of carrying a large context.
- **Load little.** Read a document's header first, then only the sections you need. Never open `docs/architecture/graph-vaults/` or `docs/architecture/graph-pages/`: they are generated.
- **Project knowledge goes to the repository, not to personal memory:** a trap to `.claude/rules/<area>.md`, a behaviour to its spec, a decision to `docs/decisions/`, a procedure to `docs/operations/`. Personal memory holds personal preferences only; what it alone knows is lost to the team.
