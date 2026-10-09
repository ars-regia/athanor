---
id: ADR-0098
title: "Update delivery, CI and operations decisions (batch 4)"
date: 2026-10-08
status: amended by ADR-0103, ADR-0104, ADR-0107
issues: []
areas: [update, build, signing, process]
---

# 0098. Update delivery, CI and operations decisions (batch 4)

## Context

The review of 2026-10-08 covered `doc_update_delivery.md` (revision 2 of 2026-10-06), `doc_ci.md`
(revision 1 of 2026-10-06) and the operations documents that ADR-0074 and A2-34 promised
(`github-settings.md`, `secrets.md`, `contributing.md`, `ownership.md`, `branching.md`,
`repository-layout.md`). Their open questions were put to the maintainer, who answered on
2026-10-08. Every answer below was the recommendation unless the text says otherwise.
ADR-0096 signs the kernel's OCI artefacts with the project key; its scope needs one
clarification (item 3). ADR-0097, on component owners, is a separate record. Item 6, decided on
2026-10-09, amends the approval budget of A2-27 (ADR-0064), ADR-0080 item 4, ADR-0084 KC1 and
ADR-0088 item 4.

## Decision

On 2026-10-08 the maintainer decided items 1 to 5, and on 2026-10-09 item 6:

1. **The decisions of section 15 of `doc_update_delivery.md`**, taken on 2026-10-06 and recorded
   here as the index asks (revision 3 of the document applies them):
   1. The dwell before an automatic promotion is 24 hours from the last evidence file,
      restarted by a newer complete candidate (UD5).
   2. The NVIDIA variants promote only with `hardware` evidence; without it the default variant
      promotes alone (UD4; item 2 changes how the evidence arrives).
   3. The security class of a release, with the build time that orders releases, is set and
      signed in `signing-images` together with the images (ADR-0080, ADR-0088 item 5, PQ4 of
      `doc_pipeline.md`). Promotion holds no key in any class: `system/promote.sh` signs nothing
      and every promotion is keyless. This replaces the 2026-10-06 form, in which an automatic
      promotion was always of the feature class and a security-class promotion signed an
      attestation in the `signing` environment.
   4. No bridge. Superseded by point 7 on 2026-10-08; the policy still never carries `hr-mes`
      scopes (UD12).
   5. The version serial is the commit count of `HEAD`, so the label is a function of the source
      (UD29).
   6. RPM signing is removed (UD44): RPMs reach a machine only inside the signed image.
   7. A bridge under the previous owner moves the machines installed before the transfer,
      verified at the boot of the bridge (section 5.2, UD45 to UD51).
2. **Points of `doc_update_delivery.md`:**
   - **UD2.** `bootc-image-builder` writes the signed `:stable` reference
     (`ostree-image-signed:docker://<registry>/<image>:stable`) into the installed system. There is
     no first-boot migration on a new install.
   - **UD4.** Hardware evidence from a maintainer machine arrives through a maintainer script under
     `scripts/` that uploads a signed statement. The shipped client gets no test verb. This matches
     PL29 and PL30 of `doc_pipeline.md`.
   - **UD5.** The automatic promotion job runs hourly and is idempotent.
   - **UD32.** chunkah is re-evaluated at a review at 1.0, not when Universal Blue adopts it.
   - **UD34.** The size of every upgrade is recorded. Above the targets of 400 MB (routine update)
     and 1.2 GB (base bump) the job prints a warning; it never blocks.
   - **UD35.** `systemctl soft-reboot` when bootc reports `softRebootCapable`, otherwise a full
     reboot.
   - **UD38.** System extensions (systemd-sysext) stay disabled until after 1.0.
   - **UD39.** "No system component is delivered through Nix" is recorded in this record, consistent
     with the dev mode of `doc_software.md`.
   - **UD46.** Bridge digests are copied with `--preserve-digests`, their signatures travel with
     them, nothing is signed again, and no key exists outside `signing-images`.
   - **UD50.** The bridge stops at the 1.0 release. **This is the maintainer's choice against the
     recommendation**, which was to stop 90 days after the last known machine moves. A machine
     that has not moved by 1.0 then needs `scripts/switch-verified.sh`.
3. **ADR-0096, scope of the key signing.** The signing with the project key covers every OCI
   artefact of the kernel cycle that a machine or the release consumes: `azoth`, `azoth-devel`,
   `azoth-debuginfo`, the MicroVM guest kernel, and also `azoth-boot`, `azoth-nvidia` and
   `azoth-signer`. The keyless signature stays as the build record for each of them, as ADR-0096
   item 2 says for the others.
4. **`doc_ci.md`, CP1.** "A workflow is green or disabled with an open issue that names the cause"
   applies only to the workflows that `doc_pipeline.md` section 3.5 keeps. A workflow it deletes or
   merges is not disabled; it goes with its block.
5. **Operations documents** (the follow-up records of ADR-0074):
   - **GitHub settings (section 6).** The Athanor description is set in `repository.json` (this
     record does not edit `.github/`; the file change follows). `vulnerability_alerts` becomes
     `true`. The environment `delete` is deleted by hand by the maintainer, then the settings are
     exported again. Pages is turned off in `pages.json`; the maintainer then deletes the
     `gh-pages` branch and the `github-pages` environment by hand and exports again.
     `enforce_admins` stays off while there is a single maintainer, because turning it on would
     block every merge (nobody else can approve); it is turned on when a second maintainer joins.
   - **Secrets.** SEC12 is adopted: a dedicated fine-grained token with Administration read and
     write on this repository only, used for `generate-jitconfig` and nothing else, which the
     maintainer creates. `KERNEL_BUMP_TOKEN` is renamed `BOT_PR_TOKEN` in one commit; the rename
     is a later code change that touches `.github/`. The lines that take effect when a second
     maintainer joins stay conditional.
   - **Contributing.** CT7 (red CI) is adopted with the scope of item 4 and a limit of one working
     day. CT6 (code-owner enforcement) stays conditional on a second owner.
   - **Ownership.** OWN1 to OWN4 and the area map (sections 2 and 3) are adopted.
   - **Branching.** BRN1 to BRN6 are adopted. Section 3 keeps `iso-v0` (option A): tag
     `archive/main-2026-08-31` at `4578bb3f` and delete `main`. The name `main` returns with the
     fresh repository at the 1.0 tag, decided by the maintainer on 2026-10-08. Option B (rename
     `iso-v0` to `main`) is the rejected alternative.
   - **Repository layout.** The target layout (`system/` split into `image/` and the crates) is
     adopted. It is executed in a quiet window after the ADR-0096 implementation, with no open pull
     request on the moved paths, as one squash, and the system image check green before the
     merge.
6. **Unattended image signing until 1.0** (decided on 2026-10-09). The environment
   `signing-images` loses its required reviewer until the 1.0 tag, when it returns. Its
   deployment branches and administrator bypass stay as they are. `signing-kernel` keeps its
   required reviewer: a module signed with the
   Machine Owner Key is trusted by Secure Boot on every machine that enrolled the key and is
   withdrawn only by rotating it, while a signed image reaches `:stable` only through the
   promotion and its evidence. The change takes effect only after the image signing job runs in
   `signing-images` again (the end of the rotation in `secrets.md` section 4.1): the `signing`
   environment also holds the kernel keys, so it keeps its reviewer for as long as it exists.
   With one maintainer who may approve their own runs, the reviewer added attention, not a
   second party (KC1), and it held every unattended build cycle at the signing step.
   `bridge` keeps its required reviewer: it is the only control on that path. The job copies
   digests that `verify-system-images` verified onto the previous owner with
   `ATHANOR_BRIDGE_TOKEN` (SEC14), a classic token that, if it belongs to `hr-mes`, reaches every
   package of that account, and machines that do not verify their downloads pull from there
   (UD46); `main`, one of its deployment branches, is not protected yet. The job is skipped
   while `ATHANOR_BRIDGE_REGISTRY` is unset, so the reviewer holds no build cycle today; whether
   it goes is decided again when the bridge is switched on, after `main` is protected or deleted.

## Consequences

- `doc_update_delivery.md` and `doc_ci.md` carry these decisions in revision 3 and revision 2. This
  record decides the points; the maintainer approved both texts on 2026-10-09 by merging the
  pull request that sets their status to approved (ADR-0074 item 5).
- ADR-0096 is amended in one part only: the artefacts its item 1 covers (item 3 above). Its other
  items, the keyless build record, the custody of ADR-0084 and the rejection of a private Fulcio and
  Rekor, are unchanged. `doc_update_trust.md` UT2, `doc_pipeline.md` and `KERNEL.md` take the longer
  list with the implementation of ADR-0096.
- The decision on UD50 changes the grace period of the bridge and nothing else in section 5.2: the
  variable `ATHANOR_BRIDGE_REGISTRY` is cleared at 1.0, and the token `ATHANOR_BRIDGE_TOKEN` is
  deleted with it.
- Follow-ups that these decisions name and this record does not make: the maintainer script of UD4;
  the file changes to `repository.json` and `pages.json` and the hand deletions of item 5; the
  rename of `KERNEL_BUMP_TOKEN`; the removal of `main` from the settings files and workflows once
  the branch is deleted; the execution of the layout move.
- ADR-0084 is amended in one part only: KC1's required reviewer on `signing-images` is
  suspended until the 1.0 tag by item 6. The change to `environments.json` follows
  the end of the rotation, and the maintainer applies it with `ghsettings.py apply`. The same
  pull request changes the D43 check of `scripts/verify.py workflows`, which today requires a
  reviewer on every `signing*` environment, so that it exempts `signing-images` until the 1.0 tag
  and still requires one on `signing-kernel` and `signing`.
- ADR-0080 item 4 (`signing-images` always), the approval budget of A2-27 (ADR-0064) and
  ADR-0088 item 4 change in the same part only: until the 1.0 tag a release cycle asks for one
  approval, `signing-kernel`, and only when the kernel or the NVIDIA modules change. ADR-0088's
  accepted self-review risk and its compensating controls of PL5 then apply to `signing-kernel`
  alone; the deployment branch restriction, the integrity ruleset and the key isolation of PL11
  still bound the unattended image job.
