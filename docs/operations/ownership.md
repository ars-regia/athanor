# Ownership by area

| Field | Value |
| --- | --- |
| Purpose | The areas of the repository, their paths and their owners |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-06, _(Proposal for the maintainer)_. `.github/CODEOWNERS` is not changed here: PR #170 owns it |
| Depends on | [contributing.md](contributing.md) CT4 and CT6, [branching.md](branching.md) |
| Defines | OWN1 to OWN4, and the areas `kernel`, `build`, `signing`, `security`, `shell`, `apps`, `docs` |

## 1. Today (facts)

- `.github/CODEOWNERS` on `iso-v0` names `@hr-mes-architect` and the path `/forge/specs/kernel/`, which no longer exists (the kernel is `forge/specs/azoth/`).
- PR #170 replaces it: owner `@hr-mes`, the protected paths last (polkit, attestation, `system/Containerfile`, signing and promotion, `CLAUDE.md`), and "Require review from Code Owners" off while there is a single owner.
- The repository belongs to a user account (`gh api users/hr-mes --jq .type` is `User`). GitHub teams, and with them per-area owner groups, need an organisation.
- The Gatekeeper left the tree in #121 (`ca00f23b`); its area stays reserved under `security` for its return.

## 2. Rules _(Proposal)_

- **OWN1.** Every path belongs to one area; an owner of the area reviews its changes. Paths outside every area fall to the default owner.
- **OWN2.** A spec belongs to the area it specifies, not to `docs`.
- **OWN3.** `security` and `signing` changes need two approvals (contributing.md CT6). On a path shared by two areas, both owners are listed on one line.
- **OWN4.** Today every area has one owner, the maintainer. When the repository moves to an organisation, each `@hr-mes` below becomes a team such as `@<org>/kernel`, with the maintainer kept on `security` and `signing`.

## 3. Area map _(Proposal)_

CODEOWNERS syntax; the last matching line wins, so broad areas come first and the
protected paths last, as in PR #170.

```
# Default owner: paths outside every area
*                                              @hr-mes

# docs: documents without an area, project instructions for people and agents
/docs/                                         @hr-mes
/README.md                                     @hr-mes
/.claude/                                      @hr-mes

# build: CI, forge, image definition, project checks
/.github/                                      @hr-mes
/forge/                                        @hr-mes
/flake.nix                                     @hr-mes
/flake.lock                                    @hr-mes
/Justfile                                      @hr-mes
/scripts/verify.py                             @hr-mes
/scripts/tests/                                @hr-mes
/scripts/runner/                               @hr-mes
/system/scripts/                               @hr-mes
/system/image-digests.sh                       @hr-mes
/system/package-delta.sh                       @hr-mes
/docs/architecture/doc_build_*.md              @hr-mes
/docs/architecture/doc_system_image.md         @hr-mes
/docs/architecture/doc_forge_development_guide.md @hr-mes

# kernel: Azoth, its profile, NVIDIA modules
/forge/specs/azoth/                            @hr-mes
/forge/specs/athanor-kernel-profile/           @hr-mes
/system/nvidia/                                @hr-mes
/system/sysctl.d/                              @hr-mes
/system/kernel-artifacts.sh                    @hr-mes
/.github/workflows/kernel-*.yml                @hr-mes
/.github/workflows/nvidia-*.yml                @hr-mes
/docs/architecture/doc_kernel_*.md             @hr-mes

# shell: compositor, greeter, bar, dock, launcher, rig, dev VM
/system/athanor-layout/                        @hr-mes
/system/athanor-compositor-client/             @hr-mes
/system/athanor-style/                         @hr-mes
/system/athanor-unit/                          @hr-mes
/system/athanor-i18n/                          @hr-mes
/system/athanor-preview/                       @hr-mes
/system/athanor-preview-render/                @hr-mes
/forge/specs/cosmic-comp/                      @hr-mes
/forge/specs/athanor-bar/                      @hr-mes
/forge/specs/athanor-dock/                     @hr-mes
/forge/specs/athanor-launcher/                 @hr-mes
/forge/specs/athanor-shelld/                   @hr-mes
/forge/specs/athanor-greeter-ui/               @hr-mes
/forge/specs/athanor-layout-chooser/           @hr-mes
/forge/specs/athanor-calmo/                    @hr-mes
/forge/test/shell/                             @hr-mes
/scripts/devvm/                                @hr-mes
/scripts/cosmic-comp-rebase/                   @hr-mes
/scripts/session-memory/                       @hr-mes
/.github/workflows/shell-*.yml                 @hr-mes
/.github/workflows/cosmic-comp-*.yml           @hr-mes
/docs/architecture/doc_shell.md                @hr-mes
/docs/architecture/doc_bar.md                  @hr-mes
/docs/architecture/doc_launcher.md             @hr-mes

# apps: portal, software, backup, recovery, first run
/system/athanor-apps/                          @hr-mes
/system/athanor-portal/                        @hr-mes
/system/athanor-search/                        @hr-mes
/forge/specs/athanor-xdg-desktop-portal-athanor/ @hr-mes
/forge/specs/athanor-backup/                   @hr-mes
/forge/specs/athanor-recovery/                 @hr-mes
/docs/architecture/doc_software.md             @hr-mes
/docs/architecture/doc_recovery.md             @hr-mes

# signing: keys' public halves, signing, promotion, updates and the trust state
/system/keys/                                  @hr-mes
/system/sign-images.sh                         @hr-mes
/system/promote.sh                             @hr-mes
/system/build-image.sh                         @hr-mes
/forge/scripts/sign_attest.sh                  @hr-mes
/forge/specs/athanor-update/                   @hr-mes
/forge/specs/athanor-cosign/                   @hr-mes
/system/athanor-trust-state/                   @hr-mes
/.github/workflows/call-system-image.yml       @hr-mes
/.github/workflows/athanor-forge-orchestrator.yml @hr-mes
/.github/workflows/nvidia-kmod.yml             @hr-mes
/.github/workflows/promote-stable.yml          @hr-mes
/docs/architecture/doc_update_trust.md         @hr-mes

# security: polkit, MAC policy, runtime enforcement, PAM, the Gatekeeper
/system/athanor-bus-api/                       @hr-mes
/forge/specs/athanor-tetragon/                 @hr-mes
/forge/specs/athanor-selinux/                  @hr-mes
/forge/specs/athanor-keylime/                  @hr-mes
/forge/specs/athanor-recovery/**/*.pam         @hr-mes
/.claude/rules/security.md                     @hr-mes

# Shared: the image definition carries PAM and authselect (build and security)
/system/Containerfile                          @hr-mes
/CLAUDE.md                                     @hr-mes
```

A crate or package without an entry above falls to the default owner until a specification
gives it an area. The Ermete-era components without one were retired by ADR-0073.
