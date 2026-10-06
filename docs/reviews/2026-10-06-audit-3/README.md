# Audit 3: program

Snapshot: `91aefb9f` (`iso-v0`, 2026-10-06). Mandate: hold Athanor to the standard of a major
vendor operating system or a mature Linux distribution, and make it maintainable from this
repository by a team of 5-10 people working with coding agents. Every earlier choice was open
to challenge, the maintainer's included.

The audit ran as eight independent read-only reviews against one charter (`charter.md`). Each
review wrote its findings in a fixed format: severity, category, location, evidence, the
standard broken, a recommendation, and whether a decision is needed. This document
deduplicates them into workstreams, records the decisions they required, and orders the
remediation. Progress is tracked in the GitHub issue linked from the decision records.

| Report                    | Scope                                              | Findings | Critical | High |
| ------------------------- | -------------------------------------------------- | -------: | -------: | ---: |
| `platform-specs.md` (SPA) | platform, boot, update and delivery specifications |       25 |        0 |   10 |
| `desktop-specs.md` (SPD)  | desktop and session specifications                 |       25 |        0 |   12 |
| `system-components.md`    | every component under `system/`, both eras (CSY)   |       50 |        6 |    4 |
| `forge-packages.md` (CFG) | every package under `forge/specs/`, both eras      |       26 |        3 |    7 |
| `engineering.md` (ENG)    | workspace, CI, supply chain, repository hygiene    |       18 |        0 |    5 |
| `team-maintainability.md` | agent entry points, process, governance (TEAM)     |       16 |        0 |    6 |
| `open-prs.md` (PRS)       | triage of the ten open pull requests               |       10 |        0 |    1 |
| security (SEC)            | threat surface, signing, CI governance             |       22 |        3 |    7 |

The security report is not published. Ten of its findings duplicate findings in the reports
above and are cited there. The others concern signing, key custody, CI governance and the
build runner. They are handled with the maintainer under the private reporting process and
are listed below only by workstream.

## What the audit found

The shipped product core is real and of good quality: the shell (bar, dock, launcher,
shelld, greeter-ui), the portal, `athanor-update` and its trust state, the kernel package
`azoth`, `nix-support`, `kernel-profile`, recovery and backup. `azoth` is the model the rest
of `forge` should follow: pinned sources, locked by digest, reproducibility-checked.

Around that core, five problems recur across every report:

1. **Fake security code is still in the tree.** It includes post-quantum key exchange with
   all-zero keys, attestation that fabricates verification, a greeter whose "TPM unseal" is a
   hash, and a GRUB password stanza with a placeholder hash. One of these (the GRUB stanza)
   ships; the rest are one packaging step away.
2. **Controls exist but do not bind.** The Rust security workflow never runs on the default
   branch and suppresses its own failures. The cargo-vet audit is fabricated and `deny.toml`
   is the template. No pull request runs the verifier or the tests. The verifier itself is red
   on five checks.
3. **Accepted decisions were not applied.** Records 0036-0072 were accepted on 2026-10-05/06,
   but most specifications still say the opposite. Several were appended as amendments that
   contradict the text they amend.
4. **The specifications no longer describe one system.** The boot chain is described three
   ways, data at rest four ways, and the update channel two ways. Specifications are missing
   for the installer, the threat model, privacy indicators, idle policy and key lifecycle.
5. **The repository is not yet a team's repository.** The only agent entry point is
   Claude-specific, in Italian and partly false. There is no local gate equal to CI, no
   newcomer path, and the spec process is not written down.

For every component that exists in both eras, the Athanor-era (`iso-v0`) version is the same
or better. No component that exists only on the Ermete-era `main` is worth bringing back.

## Decisions

The audit required these decisions. They are recorded as decision records under
`docs/decisions/`:

- **ADR-0073, component verdicts.** Retire every component without a product role, starting
  with those that contain fake security. Replace with Fedora packages where Fedora ships the
  same software. Make real the few components a product feature depends on.
- **ADR-0074, agent and contributor model.** `AGENTS.md` is canonical. Agent instructions are
  in English. Only approved specification revisions may be implemented. New records use
  `ADR-NNNN` ids. The target layout of TEAM-16 is adopted.
- **ADR-0075, engineering gates.** `just check` is the pull request gate and equals CI.
  cargo-deny becomes strict and cargo-vet is dropped. Lint and profile policy lives in one
  place. Binaries that ship nothing are deleted.
- **ADR-0076, platform scope for 1.0.** The record covers:
  - execution control (it replaces the IPE choice);
  - the DNF channel on GitHub Pages;
  - factory reset;
  - local AI;
  - the release package;
  - the builder;
  - the specification template.
- **ADR-0077, desktop decisions.** The record covers:
  - where privacy controls live;
  - the session bus of our own apps;
  - merging the control-center and notification-center specifications;
  - use of the mark.

### Reserved for the maintainer

Signing and keys stay with the maintainer. The following findings need the maintainer's
decision or action, and no workstream below changes them:

- **Boot chain for 1.0.** GRUB plus a MOK-signed kernel versus a UKI; what the image build
  signs, and where (SPA-01, SPA-14, and the related security findings).
- **Key exposure in the image build.** Isolating the Secure Boot key from the networked image
  build; signature verification of the intermediate images.
- **Key lifecycle.** Rotation, revocation and recovery of the cosign key (SPA-03). RPM signing
  that fails open, and the retired MOK key still stored (SPA-09).
- **Update freshness and promotion.** The key roles if TUF is adopted (SPA-04), and the signer
  of the promotion attestation (SPA-05).
- **Data at rest.** Whether 1.0 encrypts by default, and with which key binding (SPA-02).
- **Console access.** Whether an attacker with console access ("evil maid") is in scope for
  1.0; this depends on the boot chain.
- **Governance of signing.** The retired `main` is still a trusted signing ref. The branch
  rename (TEAM-13) changes the signing refs. Also: admin bypass of branch protection, and a
  second reviewer for the `signing` environment.
- **The build runner.** The host side of the runner findings.
- **Signing pull requests.** #115, #185 and #186.

## Workstreams

Each workstream lists the findings it closes. A finding closes when its fix is merged and the
check that would catch its return exists.

### W1. Retire what has no product role

Closes CSY-01..06, CSY-08, CSY-09, CSY-12..15, CSY-19, CSY-20, CFG-04..12, ENG-08, and the
duplicated security findings on fake fleet cryptography and fake attestation. CSY-43..50 and
CFG-20 need no action: the Ermete-era components they judge stay deleted.

- **Wave 1.** Delete the crates and package directories that the image manifest does not
  list:
  - the fleet crates (`mesh-sync`, `cloud-rs`, `cluster-mesh`, `mesh-bus`,
    `hypervisor-daemon`);
  - `confidential_computing`, the Ermete-era greeter, `agentic-kernel`, `telemetry`,
    `init-oracle`, `net-unikernel`;
  - `ebpf`, `ebpf-sched`, `oobe`, `store`, `store-rs`, `lvfs-rs`, `mdm-rs`, `rosenpass`,
    `sysmon-ebpf`, `ai-daemon`, `ananicy`;
  - the frozen `shell-rs` with its only consumers (`niri-ipc`, `cliphist`, `matugen`,
    `dart-sass`).

  The workspace, `components.toml`, the verifier and every document that names them change
  in the same pull request.

- **Wave 2.** After #169 is reworked, retire the stubs that ship through the tier
  repositories: `stage0-bootstrap`, `antigravity`, `astro-toolchain`, `cargo-tools`,
  `ide-bootstrap`, `qa`, `bpf-linker`, `syft`, `cosign` and `doctor`. Replace `bat`,
  `buildah` and `osbuild` with the Fedora packages.
- **Check.** `verify.py specs` fails on a manifest entry without a package directory. The
  DAG fails at load on the same condition.

### W2. Image inputs and manifest integrity

Closes CFG-01, CFG-21, CFG-22, SPD-04, SPD-17, PRS-169, ENG-12, SPA-22, and the part of
the intermediate-image finding that needs no key:

- Remove `secure-boot` from the manifest.
- Consume the tier repositories by digest, passed between steps through files, under one
  registry variable.
- Apply decision 0045 through the reworked #169.

### W3. Shipped packages with fake or broken mechanisms

Closes CFG-02, CFG-03, CFG-13..18, SPA-19, and the GRUB placeholder in the security report:

- Delete the GRUB password stanza.
- Give `base-config` an own release package, so the `--nodeps` install goes away.
- Make `tetragon` real (TG1), and fix `keylime`, `system-config` (its `%post` writes under
  `/usr` and `/etc`) and `selinux`.
- Merge `system-tweaks` into `system-config` and `kernel-profile`.

### W4. CI and the local gate

Closes ENG-01..05, ENG-09, ENG-11, SPA-16 and TEAM-04:

- `just clean` keeps the tracked lockfiles.
- The security workflow runs on the default branch and on pull requests, without
  suppressions.
- Remove the fabricated audit; make `deny.toml` strict; delete the unused `.trivyignore`.
- Fix the root cause of the workflows that cannot start node24 inside the Nix container.
- `just check` runs lint, the verifier and the tests of every crate that has them. A
  `pr.yml` workflow runs it as the required check. Current verifier failures are
  allow-listed, one issue each, and leave the list as they are fixed.
- The repository side of the runner findings.

### W5. Agent and contributor entry points

Closes TEAM-01..03, TEAM-05, TEAM-07, TEAM-08, ENG-13, ENG-15, PRS-170, PRS-192 and
PRS-213:

- `AGENTS.md` in English is the single source; `CLAUDE.md` imports it.
- Nested `AGENTS.md` files for `forge/`, `forge/specs/azoth/`, `system/` and
  `.github/workflows/`.
- `.claude/rules` is translated and trimmed. #192 is fixed and absorbs #213.
- CODEOWNERS is generated from the area list and checked (#170). `SECURITY.md` follows the
  private reporting process.

### W6. Specification coherence

Closes SPD-01..03, SPD-10..16, SPD-18, SPD-22, SPD-23, SPA-06, SPA-10..13, SPA-17, SPA-18,
SPA-21, SPA-24, SPA-25, TEAM-10 and TEAM-11:

- Apply every accepted record in the text it changes, instead of appending an amendment.
- `verify.py docs` resolves every `doc_*.md` link and every `path:line` anchor.
- Generate the feature register from data.
- Adopt one specification template with front matter, applied as each specification is next
  revised.
- Translate `doc_kernel_build.md`.

### W7. Missing specifications

Closes SPA-08, SPA-15, SPA-23, SPD-07, SPD-08, SPD-24, CSY-11, TEAM-09 and the missing threat
model of the security report, with these documents:

- threat model;
- installer;
- privacy indicators and per-app permissions;
- idle, lock and suspend policy;
- recovery and reset;
- support lifecycle;
- a system overview (arc42 sections 1-5 with C4 context and container views);
- the newcomer tutorial "build and boot".

The key lifecycle specification is the maintainer's.

### W8. Product defects the audit found

Closes SPD-05, SPD-06, SPD-09, SPD-19..21, SPA-07 and CSY-07:

- Volume keys and the headset dialog: the session daemon has no writable state.
- The wallpaper daemon's sandbox cannot run glycin.
- Settings and Software can start transient units.
- x86-64-v3 is not enforced at install or boot.
- `athanor-bus-api`: give it a specification, close its PID-reuse window, and remove the
  mesh, telemetry and shared-memory code that only retired crates use.

### W9. Engineering hygiene

Closes ENG-06, ENG-07, ENG-10, ENG-14, ENG-16..18, TEAM-15, CSY-10 and CSY-16..18:

- Lint and profile policy, written once.
- MSRV and edition.
- Duplicate dependencies.
- The dead Justfile recipes; merge `system/Justfile` into the root `Justfile`.
- The forbidden-names check of decision 0045.
- `.gitignore` traps.
- Cross-crate conventions.
- SPDX identifiers.

### W10. Repository model

Closes TEAM-06, TEAM-12..14, TEAM-16 and SPA-20:

- A docs index by Diátaxis quadrant.
- Approved operations documents.
- Roadmap and planning in the repository.
- `0.y.z` versions until 1.0.
- The target layout.

## Order

1. **Now, in parallel:**
   - W1 wave 1, W2 (manifest entry), the first items of W4, the GRUB stanza of W3, and the
     `AGENTS.md` of W5;
   - the open pull requests in the order `open-prs.md` gives.
2. **When the verifier is green, apart from the allow-list:**
   - `pr.yml` becomes a required check;
   - W1 wave 2, the rest of W3, W6.
3. **Then:** W7, W8, W9 and W10.

The maintainer's items run alongside and gate nothing above, except W2's digest pinning of
intermediates. That item lands without signature verification. Verification follows the
maintainer's decision on intermediate signing.
