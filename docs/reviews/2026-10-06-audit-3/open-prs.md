# Open pull requests: triage

Snapshot: 91aefb9f. Scope: ten open PRs (#213, #192, #177, #198, #170, #169, #165, #180, #187, #114). Each PR body and diff was checked against the snapshot. Conflicts were found with a read-only `git merge-tree` of each head against `origin/iso-v0`. There were no builds and no network access, so the CI logs themselves were not read: each check failure is explained from the files, and causes not proven there are marked Suspected.

## Summary

- One PR can merge as it is: #187, the nixpkgs bump.
- Six need fixes, then a merge: #165, #114, #192, #177, #170 and #180. #198 also lands as fixes plus a two-step merge, because its image guard cannot pass until its own RPM is published.
- One needs rework: #169. It silently deletes `docs/architecture/doc_forge_development_guide.md`, which iso-v0 rewrote and still cites (`scripts/verify.py:1071`, `doc_build_system.md:12`, `components.toml:77`). Its new `FORBIDDEN_DOCS` would make `verify.py docs` fail on iso-v0. Its rebase touches 8 files with textual conflicts and 6 modify/delete cases.
- One should close: #213. Its distinct items should move into #192, in English.
- Two failure causes repeat across PRs:
  - System Image Check installs the published tier overlays (`:latest`), not the PR's specs. Any Containerfile guard that depends on a changed or new spec therefore fails on its own PR. This explains the image `build` failures on #169 and #198.
  - The failing checks on #180 are the "Rust Security & FFI Audit" workflow. Its jobs cannot start node24 inside the Nix container, and this has failed on every run since 2026-09-03; it is unrelated to #180.
- Release numbers collide:
  - iso-v0 already took `athanor-system-services` 1.0.1-25 and `athanor-nix-support` 1.0.0-9 (#215). #177 and #198 claim the same numbers, so each needs the next free Release when it rebases.
  - #187 changes the nix-support payload without any Release bump, so two different builds share one NEVR.
- Two false claims repeat across PR bodies (#213, #177): "CLAUDE.md still names `forge/specs/athanor-gatekeeper-rs`". The baseline CLAUDE.md does not name the Gatekeeper.
- The ISO is built from `system/disk_config/iso.toml` (`forge/scripts/build_iso.sh:20`). Nothing that builds an image or ISO reads `system/athanor-install.ks`; only `verify.py` and `components.toml` mention it. The kickstart edits in #169 (`services --enabled=sshd`) and #180 therefore do not reach the shipped ISO.

## Verdicts and merge order

| Order | PR   | Title                                                      | Verdict                       | Severity | Depends on                                                  |
| ----- | ---- | ---------------------------------------------------------- | ----------------------------- | -------- | ----------------------------------------------------------- |
| 1     | #187 | chore(nix): bump nixpkgs                                   | MERGE-AS-IS                   | low      | none                                                        |
| 2     | #114 | fix(style): focus ring on focused widget only              | FIX-THEN-MERGE                | low      | none                                                        |
| 3     | #165 | fix(system): reject empty passwords in the PAM stack       | FIX-THEN-MERGE                | medium   | before #169 (same `RUN authselect` line)                    |
| 4     | #192 | chore(claude): version shared Claude Code config           | FIX-THEN-MERGE (absorbs #213) | medium   | #213 closed into it                                         |
| —     | #213 | docs(claude): align project rules and skill                | CLOSE (fold into #192)        | low      | —                                                           |
| 5     | #170 | docs: governance                                           | FIX-THEN-MERGE                | medium   | before #180 (both touch `doc_update_trust.md`)              |
| 6     | #177 | docs(threat-model): three tiers and a checked service rule | FIX-THEN-MERGE                | medium   | after #192 (both edit CLAUDE.md); after #187 (nix-support)  |
| 7a    | #198 | feat(polkit): backport polkit 127, spec half               | FIX-THEN-MERGE                | medium   | after #177 (system-services Release)                        |
| 7b    | #198 | Containerfile guard half                                   | after 7a is published         | —        | 7a published                                                |
| 8a    | #169 | chore(system): ship only what a spec asks for, spec half   | REWORK                        | high     | after #165 and #198                                         |
| 8b    | #169 | image, kickstart and manifest half                         | REWORK                        | —        | 8a published                                                |
| 9     | #180 | feat(delivery): promote :stable on signed evidence         | FIX-THEN-MERGE                | medium   | after #170; the `stable-override` environment created first |

Under CLAUDE.md and decision 0071 (A2-34), the maintainer must merge #165, #169 and #198 (they touch authselect or polkit), #177 and #192 (they edit CLAUDE.md), and #170 and #180 (governance, signing and promotion).

## Findings

### PRS-187 #187 chore(nix): bump nixpkgs: MERGE-AS-IS

- Severity: low
- Category: packaging hygiene
- Where: `forge/specs/athanor-nix-support/SOURCES/usr/share/athanor/nix/registry.json`, produced by `forge/specs/athanor-nix-support/bump.py` (workflow `nix-registry-bump.yml`)
- Evidence:
  - Only the `rev` and `narHash` change (0d9e9b83 to b25309931cfd); the baseline still pins 0d9e9b83.
  - The PR merges cleanly and every check is green.
  - The spec stays at Release 1.0.0-9 with no changelog entry, so the published RPM and the new build share one NEVR with different content.
  - The forge rebuilds by content hash (`dag_orchestrator.py`), so the build itself is not blocked.
  - The `narHash` cannot be checked offline (Suspected-free only as far as the bot computed it).
- Standard: Fedora Packaging Guidelines (Versioning: every content change bumps Release, with a changelog entry); decision 0050 (A2-13), which calls for a pin plus a bump bot.
- Recommendation: merge. As a follow-up, make `bump.py` raise Release and add a `%changelog` entry in the same commit.
- Needs a decision: no.

### PRS-114 #114 fix(style): focus ring on focused widget only: FIX-THEN-MERGE

- Severity: low
- Category: accessibility, documentation accuracy
- Where:
  - `system/athanor-style/calmo/templates/surfaces.css.in:11` and the four generated CSS files;
  - `docs/architecture/doc_accessibility.md:29` and `:118` (AX8).
- Evidence:
  - The selector changes from `*:focus-visible` to `*:focus:focus-visible`, and a new test (`test_only_the_focused_widget_draws_the_focus_ring`) covers it.
  - The PR merges cleanly and every check is green.
  - `doc_accessibility.md:29` cites "branch `focus-ring` (commit 85fed168)". No such branch exists; the PR's head is `fix-focus-ring` (0ac47104).
  - The on-screen check on a real session has not been recorded.
- Standard: doc_accessibility.md AX8; project rule that docs state what ships.
- Recommendation: in this PR, or in one that lands right after it, update `doc_accessibility.md:29` and `:118` to describe the shipped rule and drop the dead branch reference. Record the on-screen check.
- Needs a decision: no.

### PRS-165 #165 fix(system): reject empty passwords in the PAM stack: FIX-THEN-MERGE

- Severity: medium
- Category: security (authentication), documentation accuracy
- Where:
  - `system/Containerfile`, after the `RUN authselect enable-feature with-systemd-homed ... preset-all` line (about line 180);
  - `docs/architecture/doc_lock_and_prompts.md:28`, `:121` and `:279` (item 11).
- Evidence:
  - The PR adds `RUN authselect enable-feature without-nullok && ! grep -n nullok /etc/pam.d/system-auth /etc/pam.d/password-auth`.
  - It merges cleanly (98 commits behind) and every check is green.
  - No shipped kickstart creates an account with an empty password: `collaudo.ks` sets one, `devvm.ks` uses `password=@USER@`, and root is `rootpw --lock`.
  - The three `doc_lock_and_prompts.md` lines say nullok is present and undecided. They become false on merge.
  - Decision 0060 (A2-23) asks for a "verify.py check". Only a build-time guard exists, though that guard is arguably stronger.
- Standard: decision 0060 (A2-23); CLAUDE.md "Modifiche a ... autenticazione: fermati e chiedi".
- Recommendation:
  - Update the three doc lines to cite A2-23.
  - Either add a small `verify.py` check that the Containerfile keeps `without-nullok`, or amend 0060 to accept the build guard.
  - Merge before #169, which rewrites the same `RUN authselect` line.
- Needs a decision: yes. The maintainer chooses between a build guard alone and a build guard plus a verify check.

### PRS-213 #213 docs(claude): align project rules and skill: CLOSE

- Severity: low
- Category: language rule, false PR statements, duplicate work
- Where: `.claude/rules/ci.md`, `.claude/rules/rust.md`, `.claude/skills/convert-documents-to-markdown/SKILL.md`
- Evidence:
  - The PR adds new Italian prose to `ci.md` and `rust.md`. The standing rule asks for new documentation in English.
  - Two statements in the body are false:
    - It says security.md changed, but the net diff leaves it untouched: 14ccd0be reverted b8309149.
    - It says CLAUDE.md "still names `forge/specs/athanor-gatekeeper-rs`", but the baseline CLAUDE.md does not mention it.
  - It conflicts with #192, which rewrites the same `ci.md` and `rust.md` in English.
  - Three items are worth keeping:
    - the `system/*.sh` and `system/tests/**` path globs;
    - the note that `verify.py workflows` passes silently without actionlint;
    - the known `|| true` debt, verified at `call-build-builder.yml:81,88,90` and `call-dag-compile.yml:173,217,224,226`.
  - The anydoc pin (`@0.2.4`) and the warning before `--ocr hosted` uploads are also correct and wanted.
- Standard: CLAUDE.md standing rule "English on GitHub" (2026-09-06).
- Recommendation: close. Carry the items listed above into #192, in English. The alternative is to merge #213 first and rebase #192 onto it, which costs more.
- Needs a decision: no.

### PRS-192 #192 chore(claude): version shared Claude Code config: FIX-THEN-MERGE

- Severity: medium
- Category: agent configuration, secret handling, rule accuracy
- Where:
  - `.claude/settings.json`, `.claude/README.md`, `.claude/rules/*.md` (new: acceptance, image, kernel, shell, system, systemd, workflow);
  - translated `ci.md` and `rust.md`;
  - `CLAUDE.md`.
- Evidence:
  - The PR merges cleanly. Several statements check out:
    - "Kernel gate" is required (`.github/settings/branch-protection.json:17`);
    - `docs/architecture/graph-vaults` is absent at the baseline, so "generated locally" is true;
    - the stale CLAUDE.md lines it replaces really are stale;
    - #188, the PR it waits for, is merged (e0b95cd5).
  - Defects:
    - `shell.md` globs `system/athanor-shell-rs/**`, a path that does not exist (the crate lives in `forge/specs/athanor-shell-rs`). This is the dead-glob failure that security.md's own comment warns against.
    - `shell.md` also omits `system/athanor-style`.
    - `settings.json` denies `.env` and `.env.*` only. It misses `*.env` files such as `scripts/runner/runner.env` and `scripts/devvm/devvm.env`.
    - Suspected: `systemd.md` says build-time `systemctl enable` "does not survive into the deployment". That contradicts `system/Containerfile:180` (`systemctl enable tetragon ... && systemctl preset-all`) and bootc practice; the real trap is more likely that a later `preset-all` resets the enablement.
    - The README sends readers to the private, unlicensed `hr-mes/cc-setup` for hooks and the gate. Decision 0071 (A2-34) wants those versioned in `.claude/`.
    - The maintainer's checkout has an untracked `.claude/settings.json`, so a pull of this PR collides with it.
    - Ask rules such as `rpm-ostree *` also catch read-only commands (minor).
- Standard: decision 0071 (A2-34); CLAUDE.md "Segreti ... li bloccano il gate e permissions.deny".
- Recommendation:
  - Fold #213 in, in English.
  - Fix the `shell.md` glob and add `system/athanor-style/**`.
  - Add `*.env` deny rules.
  - Prove the systemd trap with a test, or strike it.
  - Add a merge note telling the maintainer to move the local `settings.json` to `settings.local.json` first.
  - The maintainer merges.
- Needs a decision: yes. Should hooks and the gate live in `.claude/` now (A2-34 in full), or stay in `cc-setup` for now?

### PRS-170 #170 docs: governance (security policy, code owners, support window, key succession): FIX-THEN-MERGE

- Severity: medium
- Category: governance documentation, decision record
- Where: `.github/CODEOWNERS`, `.github/SECURITY.md`, `.github/CONTRIBUTING.md`, `README.md`, `docs/architecture/doc_kernel_profile.md` (D3), `docs/architecture/doc_update_trust.md`
- Evidence:
  - The work is wanted: decisions 0054 (A2-17), 0055 (A2-18) and 0062 cover the content.
  - Every CODEOWNERS path exists at the baseline, and the owner `@hr-mes` replaces the nonexistent `@hr-mes-architect`, which the baseline still carries.
  - Five textual conflicts: CONTRIBUTING.md (2 hunks), SECURITY.md (1), README.md (1), doc_kernel_profile.md (1). iso-v0 had already rewritten SECURITY.md and CONTRIBUTING.md in English (#163). This PR rewrites them again from the older text, and the two versions disagree on supported versions: the baseline says "fixes land on iso-v0", #170 says "the current `:stable` image".
  - Two decisions appear only in the PR text and README, not in `docs/decisions/`:
    - the Fedora 45 target with a Fedora 44 fallback by mid-November 2026;
    - the signing-environment bot as an additional required check.
  - The Fedora end-of-life dates (F43 2026-12-09, F45 2027-11-24) could not be checked offline.
  - "x86-64-v3: AVX2, BMI1, BMI2, FMA, MOVBE, F16C and LZCNT" reads as a complete list but omits AVX and XSAVE (minor).
  - No checks ran on the PR (docs-only paths).
- Standard: decisions 0054, 0055 and 0062; the decision-record practice of `docs/decisions/` (A2-34 "everything described in the repo").
- Recommendation:
  - Rebase onto iso-v0's SECURITY.md and CONTRIBUTING.md and add the response targets, scope and upstream-path section to them. Do not reinstate the older text.
  - Settle the supported-versions statement.
  - Record the F45/F44 decision and the signing-bot decision as ADRs, or amend 0062.
  - The maintainer merges.
- Needs a decision: yes. Is the supported version iso-v0 or `:stable`?

### PRS-177 #177 docs(threat-model): one threat model in three tiers, and a checked service hardening rule: FIX-THEN-MERGE

- Severity: medium
- Category: security model, CI check, unit hardening
- Where:
  - `docs/architecture/doc_threat_model.md` (new);
  - `scripts/verify.py` (`services` check) and `scripts/tests/test_verify_services.py`;
  - `.github/workflows/call-lint.yml:41`;
  - `athanor-nix-gc.service` and `athanor-nix-relabel.service`;
  - `athanor-desktop.service` and `scripts/runner/athanor-runner.service`;
  - CLAUDE.md, and four spec documents.
- Evidence:
  - The work is wanted: decision 0044 (A2-9) and 0066 (A2-29); the cited A2-6, A2-7 and A2-31 all exist.
  - I ran the PR's `service_problems` against the baseline tree. It flags exactly the four units the PR fixes (nix-gc, nix-relabel, athanor-desktop, athanor-runner), and rosenpass stays exempt. The 7 commits iso-v0 has gained since the merge base add no new failing unit.
  - Four textual conflicts:
    - `call-lint.yml`: iso-v0 added `coverage` to the same `run:` line.
    - `scripts/verify.py`.
    - `athanor-nix-support.spec`: iso-v0 already took 1.0.0-9.
    - `athanor-system-services.spec`: iso-v0 already took 1.0.1-25.
  - The Releases must become 1.0.0-10 and 1.0.1-26 (or later, if #198 lands first).
  - Suspected: `athanor-nix-gc.service` gets `CapabilityBoundingSet=CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH CAP_FOWNER`. Without `CAP_SYS_PTRACE`, root cannot read `/proc/<pid>/maps`, `exe` or `fd` of other users' processes. Nix's runtime-root scan ignores `EACCES`, so store paths in use by running user processes could be collected. Neither unit has been run with the bound (author's own comment).
  - The PR body contains three false statements:
    - "Line 92 of CLAUDE.md still names gatekeeper";
    - "Exemptions need a reason and an expiry date", while `SERVICE_EXEMPT` holds only a reason;
    - "open: add (allow-list)", while the diff already contains it.
  - The `DynamicUser=` implies `NoNewPrivileges=` statement is correct per systemd.exec(5).
- Standard: decision 0044 (A2-9; it also requires showing the CLAUDE.md diff to the maintainer); `panic`-free and zero-trust rules of CLAUDE.md; Fedora Packaging Guidelines (Release).
- Recommendation:
  - Rebase:
    - `call-lint.yml` runs `... services registry licence ci coverage`;
    - bump both Releases past the baseline;
    - rebase CLAUDE.md onto #192's version.
  - Add `CAP_SYS_PTRACE` to the nix-gc bound, or prove on a VM that gc keeps a user process's runtime roots. Run both nix units on the dev VM.
  - Correct the body.
  - The maintainer merges, because the PR edits CLAUDE.md.
- Needs a decision: no. The CLAUDE.md wording is already approved.

### PRS-198 #198 feat(polkit): backport polkit 127: FIX-THEN-MERGE (two steps)

- Severity: medium
- Category: security packaging, CI chicken-and-egg
- Where:
  - `forge/specs/polkit/polkit.spec` (new);
  - `forge/config/packages.json` (adds `polkit` to `custom_packages` and `custom_tier1`);
  - `cosmic-settings-daemon.service` (`ReadWritePaths=%t`);
  - `athanor-system-services.spec`;
  - the `system/Containerfile` guard.
- Evidence:
  - The work is wanted: decision 0036 (shipped-image health block: polkit 127, the helper socket, and `ReadWritePaths=%t` for cosmic-settings-daemon) and 0003 D9.
  - One conflict, in the `athanor-system-services.spec` Release/changelog (iso-v0 is at 1.0.1-25). The rebase is easy.
  - Image `build` fails. The new Containerfile guard requires `polkit` 127-_.athanor_, a non-setuid helper and an enabled `polkit-agent-helper.socket`, but System Image Check installs the published tier1 overlay, which has no Athanor polkit yet. The guard cannot pass on this PR by construction.
  - `build (specs/polkit)` fails for a cause the files do not settle (Suspected). The builder provides meson, elfutils, llvm, duktape, linux-pam, expat, dbus, gobject-introspection and the systemd macros (`flake.nix:27-50,150-191`), and the spec calls `%set_build_flags`. The likeliest candidates are its own `%check` assertions:
    - `! grep -aq '/nix/store'` on the binaries (a pkg-config variable from the Nix builder baked into polkitd or the helper);
    - the FORTIFY or stack-protector import checks under the builder's clang flags.
    - A third candidate is the `<<<` bash-isms if rpm's `%check` shell is not bash.
  - The helper is installed 0755 instead of setuid, pkexec stays 4755, and the unit edit goes into our own unit rather than a drop-in. All of this is consistent with 0036.
  - `verify.py polkit` fails on `os.athanor.ebpfsched.update`, as it already does on iso-v0.
- Standard: decision 0036; CLAUDE.md stop-and-ask on polkit.
- Recommendation:
  - Read the spec-build log and fix the cause.
  - Split the PR: the spec, `packages.json` and system-services first, published; then the Containerfile guard in a follow-up that System Image Check can pass.
  - Rebase the Release to the next free number.
  - The maintainer reviews and merges.
- Needs a decision: no.

### PRS-169 #169 chore(system): ship only what a current specification asks for: REWORK

- Severity: high
- Category: deletion of live documentation, broken verifier, CI failure, hard rebase
- Where:
  - `forge/config/packages.json`;
  - `athanor-desktop-ui.spec`, `athanor-system-config.spec` (the homed preset);
  - `system/Containerfile`, `system/athanor-install.ks`;
  - `scripts/verify.py` (`removed_name_problems`, `keylime_problems`, `forbidden_doc_problems`);
  - `launcher-acceptance.sh`, `provision_flatpak.sh`;
  - deleted: five specs and two docs.
- Evidence:
  - The cleanup is still wanted (decisions 0045 A2-10 and 0049). The baseline still lists ide-bootstrap, qa, antigravity, astro-toolchain, cargo-tools, qemu, virt-manager, the cosmic apps, swaybg, swaylock and Thunar, still installs compiler-rt and still enables homed.
  - Commit 1f6d83cb deletes `docs/architecture/doc_forge_development_guide.md`. At this PR's base that was the old Italian file, but iso-v0 rewrote it in English (#164), and it is live: `scripts/verify.py:1071` ("Golden rules 2 and 3"), `doc_build_system.md:12` and `components.toml:77` all cite it.
  - The PR's `FORBIDDEN_DOCS` lists that file, so `verify.py docs` would fail on iso-v0. Decision 0045 lists it as dead, so the record now disagrees with the tree.
  - Conflicts:
    - Textual, 8 files: README.md, forge/README.md, athanor-desktop-ui.spec (3 hunks), athanor-system-config.spec (2), verify.py, system/Containerfile, system/README.md, athanor-install.ks.
    - Modify/delete, 6 cases: the deleted guide, the antigravity, astro-toolchain, cargo-tools, ide-bootstrap and qa specs, and the moved `test-nvidia-modules.sh`. iso-v0 edited all of these (licence, URL).
  - The image `build` most likely fails on the new `! systemctl is-enabled systemd-homed.service` guard. The published `athanor-system-config` lacks the new `disable systemd-homed.service` preset, so Fedora's 90-systemd.preset enables homed through `preset-all`.
  - The PR removes the `! -name "*astro-toolchain*"` and `"*cargo-tools*"` excludes from the Containerfile `find`. While the published tier repos still hold those RPMs, the image could install them, and nothing at image level guards against it.
  - The new checks hook into `shipped` and `docs`, which CI does not run (`call-lint.yml:41`).
  - Docs the PR does not update: `doc_first_run.md:23,26`, `components.toml:808` ("enables sshd and systemd-homed") and `doc_lock_and_prompts.md:28`.
  - The kickstart change does not reach the shipped ISO, which is built from `iso.toml`.
  - Suspected: `launcher-acceptance.sh` expects the row "acceptance-window — Terminal". Ptyxis's desktop Name and its single-instance process may not match `pgrep -x ptyxis`.
- Standard: decisions 0045 and 0049; CLAUDE.md "Formal, idiomatic solutions"; the project's documentation-truth rule.
- Recommendation:
  - Split the PR:
    - (a) the specs: the system-config homed preset, the desktop-ui Requires (nautilus, ptyxis, gnome-text-editor, gnome-disk-utility, foot dropped) and the bar favourites. Merge them and wait for publication.
    - (b) the rest, rebased on iso-v0 and on #165 and #198: the Containerfile, `packages.json`, spec deletions and guards.
  - Keep the rewritten forge guide, drop it from `FORBIDDEN_DOCS`, and amend 0045.
  - Add an image-level `! rpm -q athanor-antigravity athanor-astro-toolchain athanor-cargo-tools ...` guard, like the cosmic-panel one.
  - Give the new checks their own verify id and run it in `call-lint.yml`.
  - Update the three docs and verify the Ptyxis expectation on the rig.
  - The maintainer merges, because authselect is touched.
- Needs a decision: yes. Amend 0045, because `doc_forge_development_guide.md` is no longer dead.

### PRS-180 #180 feat(delivery): promote :stable automatically on signed acceptance evidence: FIX-THEN-MERGE

- Severity: medium
- Category: release and signing pipeline
- Where:
  - `.github/workflows/promote-stable.yml`, `iso-acceptance.yml`, `call-system-image.yml`, `athanor-forge-orchestrator.yml`;
  - `system/promote.sh`, `promote-auto.sh`, `require-review.sh`, `publish-iso.sh`, `rechunk-image.sh`;
  - `forge/test/iso/attest.sh`, `evidence.py`, `verdict.py`;
  - `system/athanor-install.ks`, `doc_update_trust.md`, `doc_system_image.md`.
- Evidence:
  - The work is wanted: decisions 0039 (acceptance evidence, `:stable`, automatic promotion after dwell, `promote.sh` as the override) and 0063 (security class carried in the promotion attestation).
  - Two textual conflicts, both easy:
    - `doc_update_trust.md`: iso-v0 added 28 lines; #170 also edits this file.
    - `athanor-install.ks`: iso-v0 reworded the `:latest` comment that #180 removes.
  - Four more files changed on both sides but auto-merge: the orchestrator, `call-system-image.yml`, `rust-security-audit.yml` and `doc_system_image.md`.
  - The five failing checks are "Rust Security & FFI Audit" jobs. They fail at checkout because node24 cannot load `libstdc++.so.6` in the Nix container, as on every run since 2026-09-03 (maintainer's comment on the PR). This is unrelated to #180.
  - `require-review.sh` reviewed:
    - It fails closed under `set -euo pipefail` and `inherit_errexit`.
    - It validates the environment name and run id.
    - It does not count self-approval or non-User approvers, and requires write or admin permission.
    - The promote job uses `!cancelled()`, and `packages: write` is scoped to that job.
    - The registry comes from `REGISTRY_HOST`.
    - I found no `|| true`, `continue-on-error`, attribution line or hard-coded registry in code. The `ghcr.io/hr-mes/...:stable` strings sit in user-facing `bootc switch` commands in `doc_system_image.md`, which already use the literal (allowed in docs by 0028, read narrowly).
  - Open points:
    - The `stable-override` environment must exist before any override; until then every override fails, by design.
    - With a sole maintainer, an override needs a second person with write access.
    - The certificate SHA extension, the approvals endpoint and `ksvalidator` are unverified until a real release-branch run.
    - The body's residuals list `promote-auto.sh:107` (an empty list exits 0) and `attest.sh:35` (unescaped dots in `RELEASE_BRANCH`). The first is a fail-open path in the promoter and should be fixed, not carried.
    - The kickstart edit does not reach the ISO (`iso.toml`).
    - The PR body uses the Italian heading "Residuals (BASSO)".
- Standard: decisions 0039 and 0063; CLAUDE.md "Formal, idiomatic solutions" (no silent pass); "English on GitHub".
- Recommendation:
  - Rebase after #170.
  - Fix `promote-auto.sh:107` so that an empty list for a renamed workflow fails.
  - Escape `RELEASE_BRANCH` in `attest.sh`.
  - Create the `stable-override` environment.
  - Plan one supervised release-branch run.
  - The maintainer merges.
- Needs a decision: yes. Who is the second human with write access for overrides? (This ties to 0062: no second signing reviewer is named.)
