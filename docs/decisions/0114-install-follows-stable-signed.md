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

Decided by the maintainer on 2026-10-14 (option A of the three put to them).

1. **The kickstarts record the channel in a `%post`.** `system/disk_config/iso.toml`,
   `system/athanor-install.ks` and the acceptance's `forge/test/iso/collaudo.ks` end with one
   identical `%post` that reads the repository from the deployment's origin and runs
   `bootc switch --mutate-in-place --enforce-container-sigpolicy --transport registry
   <repository>:stable`. bootc 1.16 honours the flag in that mode and writes
   `ostree-image-signed:docker://<repository>:stable`; nothing is downloaded at install. The
   repository is read, not written out, so the owner the image was built under is the one
   the shipped policy and `registries.d` name. A missing or unexpected origin fails the
   install.
2. **Users take the ISO that `:stable` names.** The `promote` job of `promote-stable.yml`,
   after `promote.sh`, moves `athanor-iso:stable` to the run's ISO with
   `system/tag-images.sh --iso`. The ISO a user downloads carries, when it is published,
   the build the system images' `:stable` names. `athanor-iso:<run_id>` stays what it was,
   the run's ISO, unpromoted.
3. **The check.** `verify.py install-channel` fails when any kickstart names `:latest` or runs
   `bootc switch` without the flag, and when the three kickstarts lack the `%post` or differ
   in it. The ISO acceptance asks the installed guest for bootc's booted reference and fails
   unless it ends in `:stable` with the signature `containerPolicy` (UD2).

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
  name `tag-images.sh --iso` in the `promote` job as the writer of the ISO's `:stable`.
  ADR-0098 item 2 (UD2) is amended by this record.
- **Open, for the maintainer.** The ISO installs the image through an OCI layout, which changes
  the manifest digest, as the context of `doc_update_trust.md` records. A fresh install is enforcing from its
  first boot with a booted digest no signature names, so the client reads it, from the code
  of `check.rs` and not from a machine, as `NoSignature` rather than `Media`, and
  `migrate.rs` treats an enforcing machine as done. `offerable()` refuses a build not newer
  than the booted one, so the client never offers the same build's signed digest: the machine
  stays on the unsigned digest until the next promotion. `bootc upgrade` run by hand does
  stage it, since the digest differs. `doc_update_trust.md` acceptance item 7 expects `media`
  until a migration and then `signature` with no update between; the client or that item
  needs a decision. This record changes neither.
- **Not verified on a machine.** The acceptance probe and the `%post` are tested with a fake
  `bootc`; no ISO built with them has been installed when this record was written. The upgrade
  case is written and unverified: install from an `athanor-iso:<run_id>` older than the
  current promotion, run `sudo bootc upgrade`, and expect the staged `imageDigest` to equal
  `skopeo inspect --format '{{.Digest}}' docker://<registry>/athanor-system:stable` with the
  signature `containerPolicy`.
