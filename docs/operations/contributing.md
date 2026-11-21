# Contributing to Athanor

| Field | Value |
| --- | --- |
| Purpose | From a clean machine to a merged pull request, for people and for agents |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 2, 2026-10-08. CT7 is decided ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)); CT6 enforcement is decided ([ADR-0107](../decisions/0107-code-owner-review-with-unowned-paths.md)) |
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
| Project checks | `python3 scripts/verify.py`, or the checks named after it (`--list` names them all) |
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
- **A small amendment to an approved specification may be approved in the pull request of the code that needs it** ([ADR-0145](../decisions/0145-small-spec-amendment-in-the-code-pull-request.md), amending ADR-0074 decision 5). "Small" is a corrected requirement, a renamed identifier or a narrowed rule, that is, one that allows less; a new component, a new trust boundary, a loosened requirement and any amendment in the stop-and-ask areas or the inviolable rules of `AGENTS.md` are not small and keep the separate revision. The pull request shows the specification hunk next to the code, and the review of the code owner is required as for any pull request.
- **A new or revised specification holds contracts only** (maintainer decision, 2026-11-18, issue #1287): interfaces, behaviour, limits and tests, with no crate internals and no line numbers into source files. Existing `path:N` citations stay until the migration to symbol citations that issue #1278 proposes, and `verify.py line-citations` checks them meanwhile; the symbol form is the next rule.
- **In an exploratory area the specification follows the spike** (maintainer decision, 2026-11-18, issue #1290). A spike under `docs/spikes/` runs first, and the specification revision that binds the design is written from its results. Until then the specification only states the goal and the open questions. Exploratory areas are declared case by case: no area is exploratory today, an area becomes exploratory only when the maintainer declares it at its start with a dated line in [the decision log](../decisions/log.md), and it stops being exploratory when the specification revision written from its spike is approved. An area with an approved specification revision stays specification-first.
- **A spec cites code by symbol, not by line:** `` `path#symbol` ``, or `` `#symbol` `` for the file named last on its line, for example `` `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/fixture.rs#a_second_lock_takes_over_a_lock_whose_client_died` ``. The symbol is, by the kind of the file: a Rust item (`fn`, `struct`, `enum`, `union`, `trait`, `type`, `const`, `static`, `mod`, `macro_rules!`), qualified by its `impl`, `trait` or `mod` when its name repeats (`Lock::new`), or an impl block by its header (`impl Drop for Lock`); a Python function, class or module-level name; a shell function; a TOML table or key path (`workspace.members`); a YAML key path (`jobs.<id>`, `jobs.<id>.steps[name=...]`); a markdown section id (`SH3`) or heading anchor. `verify.py symbol-citations` checks that the symbol names exactly one definition of the file, and that the definition holds the code its sentence names. A file with no symbol to name, such as a data file or a generated list, is cited as `path:N` and is listed with its reason in `CITATION_LINES_KEPT`. Until the documents move to the new form ([#1278](https://github.com/ars-regia/athanor/issues/1278)), `verify.py line-citations` checks every `path:N`.
- **A decision is a record:** `docs/decisions/NNNN-slug.md`, with the front matter described in `docs/decisions/README.md`. A spec states what; the record states why and which alternative was rejected.
- A runbook (run, release, rotate, rebuild) goes to `docs/operations/<topic>.md`.

## CT6. Review

- **Area owners** are listed in `.github/CODEOWNERS`; the area map is [ownership.md](ownership.md).
- **Two-person review** for signing, attestation, polkit, cryptography and authentication: two approvals, at least one from an owner of the area, never the author. Today the maintainer is the only owner, so every such change waits for the maintainer.
- **After a CLEAN audit, audit only the delta** (maintainer decision, 2026-11-18, issue #1283). When a head was audited CLEAN, the audit of a later head covers the delta between the two heads, plus whatever a base merge brought into the pull request's own diff. A full audit is repeated only when the delta touches an area of the merge card's list: process launch, files, `unsafe`, D-Bus, confinement, the trust seal, power, signing and keys, an approved document's text, CI workflows, repository settings, `CODEOWNERS`, dependencies, polkit, authentication, the merge rule and its tooling, and systemd units. The list is the card's, and its source is `scripts/merge_card_categories.json`, so that the two cannot diverge. The audit report names the audited range as `<CLEAN head sha>..<audited head sha>`, with full shas. [audits.md](audits.md) holds the rule.
- Enforcement: the `product-review` ruleset (`.github/settings/rulesets.json`) requires the review of a code owner and no other approval, so a pull request touching only a path with no owner merges without a review ([ADR-0107](../decisions/0107-code-owner-review-with-unowned-paths.md)); `iso-v0` also requires the status check `gate` (`.github/settings/branch-protection.json`).

## CT7. Red CI (decided 2026-10-08, [ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md))

- A workflow of the pipeline target that is kept (`doc_pipeline.md` section 3.5, `doc_ci.md` CP1) is, on the product branch, green or disabled. There is no third state. A workflow the target deletes or merges is not disabled: it goes with its block.
- A red workflow gets, within one working day, a fix or `gh workflow disable <file>` plus an open issue that names the failing run id. The fix pull request enables it again.
- No `continue-on-error`, no `|| true`, no retry loop that hides the failure.
- Today, on `iso-v0`: `forge-ghcr-cleanup.yml` failed (run 37173567085) on 2026-10-06. The same listing then showed `test-mok.yml` registered as active with no file in the tree (`gh api repos/ars-regia/athanor/actions/workflows`); `ls .github/workflows` still has no such file on 2026-10-09, and the registration was not queried again.

## CT8. Claude Code

- **Project configuration lives in `.claude/`:** rules with `paths:` front matter under `.claude/rules/`, skills under `.claude/skills/`, plus the shared `settings.json`, which holds permissions only (ADR-0074). Personal settings stay in `.claude/settings.local.json`, which is not committed.
- **Scope a session to one area and one issue.** Start a new session for the next issue instead of carrying a large context.
- **Load little.** Read a document's header first, then only the sections you need. Never open `docs/architecture/graph-vaults/` or `docs/architecture/graph-pages/`: they are generated.
- **Project knowledge goes to the repository, not to personal memory:** a trap to `.claude/rules/<area>.md`, a behaviour to its spec, a decision to `docs/decisions/`, a procedure to `docs/operations/`. Personal memory holds personal preferences only; what it alone knows is lost to the team.
