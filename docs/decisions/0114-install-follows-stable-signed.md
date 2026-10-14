---
id: ADR-0114
title: "Installed systems follow :stable with the signature policy, set by the kickstart's %post"
date: 2026-10-14
status: accepted
issues: [146]
areas: [update, build, release, security]
---

# 0114. Installed systems follow :stable with the signature policy, set by the kickstart's %post

## Context

[ADR-0098](0098-update-delivery-ci-operations-batch-4.md) item 2 decided that
`bootc-image-builder` writes the signed `:stable` reference into the installed system, and
`doc_update_delivery.md` UD2 left the builder option that sets it _(to verify on the pinned
builder)_. There is no such option:

- The builder's installer kickstart (`osbuild/images`,
  `pkg/manifest/anaconda_installer_iso_tree.go`, `bootcInstallerKickstartStages`) installs the
  image from the ISO through `ostreecontainer` with OCI transport and signature verification
  off, then runs a `%post` with `bootc switch --mutate-in-place --transport registry` to the
  reference it was given, the run's own `athanor-system:<run_id>`. Its own comment calls this a
  workaround for the missing `--target-imgref` in Anaconda (osbuild/image-builder issue 380).
  It takes no signature policy and no separate target reference.
- Anaconda's `ostreecontainer` runs `ostree container image deploy` with
  `--no-signature-verification` whatever the transport; ostree-ext verifies only with
  `--enforce-container-sigpolicy`, which Anaconda never passes.

So every machine installed from the ISO followed `ostree-unverified-registry:` on the tag of
the run that built the ISO, and `system/athanor-install.ks` followed `:latest` unverified.

Commit 101e5781 removed a second `bootc switch` from the ISO's `%post` because it named
`:latest` while the disk held the ISO's build: the machine recorded a tag that by then
pointed elsewhere, and an older ISO gave a machine that announced itself as current.

## Decision

The maintainer decided on 2026-10-14, choosing option A of the three put to them:

1. **The kickstarts switch the installed system to `:stable` with the signature policy
   enforced**, in a `%post` (`bootc switch --mutate-in-place --enforce-container-sigpolicy`).
2. **The ISO published for users carries a promoted digest.**

## Mechanism

This record implements the two decisions as follows.

1. **One `%post`.** `system/disk_config/iso.toml`, `system/athanor-install.ks` and the
   acceptance's `forge/test/iso/collaudo.ks` end with one identical `%post --erroronfail` that
   reads the repository from the deployment's origin and runs
   `bootc switch --mutate-in-place --enforce-container-sigpolicy --transport registry
   <repository>:stable`. bootc 1.16 honours the flag in that mode and writes
   `ostree-image-signed:docker://<repository>:stable`; nothing is downloaded at install. The
   repository is read, not written out, so the owner the image was built under is the one
   the shipped policy and `registries.d` name. A missing or unexpected origin fails the
   install.
2. **The promotion names the ISO, and a job of its own moves it.** `system/fetch-evidence.sh`
   fetches the run's `iso-digest.txt`, the file `accept.yml` read to find the ISO it tested.
   `scripts/ci/promotion-record.sh` checks that it names the athanor-system digest being
   promoted and that `athanor-iso:<run_id>` still names it, and writes it into the record as
   `iso`, with that athanor-system digest as `iso_system`. The bundle signs the record, and
   the job summary the `release` approval covers prints both. `promote.sh` recomputes
   the record before it moves anything, so it refuses when the ISO is gone or moved. The `iso`
   job of `promote-stable.yml` (`needs: [promote]`, environment `release`) then verifies the
   approved plan against the signed copy off GitHub, as the `promote` job does, and runs
   `system/promote-iso.sh`. It refuses unless `athanor-system:stable` still names
   `iso_system`, so a late re-run of an older promotion's `iso` job never moves the ISO
   behind the system images, and moves `athanor-iso:stable` to `iso` with
   `system/tag-images.sh --iso`, which refuses when `athanor-iso:<run_id>` names another
   digest. Re-running the failed jobs retries the ISO alone.
3. **The checks.** `verify.py install-channel` fails when any kickstart names `:latest` or runs
   `bootc switch` without the flag, and when the three kickstarts lack the `%post`, run it
   without `--erroronfail`, or differ in it; a comment that quotes the command does not count.
   The ISO acceptance asks the installed guest for bootc's booted reference and fails unless
   it ends in `:stable` with the signature `containerPolicy` (UD2).

## Why 101e5781's concern no longer applies

The `%post` records a channel, not a claim about the disk. bootc reads the booted digest from
the deployment, and `bootc upgrade` compares it with the digest the channel names, so a
machine installed from an older ISO finds `:stable` ahead of it and takes it, verified,
instead of announcing itself current. The ISO users are given moves only with `:stable`, so
at publication its build and the channel agree. The concern was a moving tag with nothing
verifying it; here the tag is the one promotion moves, and every pull through it is checked
against the shipped policy.

## Alternatives rejected

- **B: keep the run's tag and let `athanor-update-migrate` move the machine on first boot.**
  It leaves a first-boot migration on every new install, the step ADR-0098 removed, and an
  unverified reference until it runs.
- **C: wait for the builder to gain a target reference (osbuild/image-builder issue 380).**
  Open upstream with no date; installed machines would keep following an unverified tag.
- **Build the ISO from the promoted digest in `promote-stable.yml`.** It moves an hour-long
  build into the job that waits on the `release` approval, for the same result as tagging the
  run's ISO, which the acceptance already passed.

## Consequences

- `doc_update_delivery.md` UD2 names the `%post` as the mechanism and drops the
  _(to verify)_ note; `doc_pipeline.md` PL32 and PL35 and the `athanor-iso` row of `doc_ci.md`
  name `tag-images.sh --iso` in the `iso` job as the writer of the ISO's `:stable`.
  ADR-0098 item 2 (UD2) is amended by this record.
- The ISO installs the image through an OCI layout, which changes the manifest digest, as the
  context of `doc_update_trust.md` records: a fresh install is enforcing from its first boot
  on a digest no signature names. The maintainer chose on 2026-10-14 to fix the client, so a
  fresh install moves to the signed digest of its own build at the first check (pull request
  `fix/update-same-build-signed`, which merges before this record's).
- **Not verified on a machine.** The acceptance probe and the `%post` are tested with a fake
  `bootc`; no ISO built with them has been installed when this record was written. The upgrade
  case is written and unverified: install from an `athanor-iso:<run_id>` older than the
  current promotion, run `sudo bootc upgrade`, and expect the staged `imageDigest` to equal
  `skopeo inspect --format '{{.Digest}}' docker://<registry>/athanor-system:stable` with the
  signature `containerPolicy`.
