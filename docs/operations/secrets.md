# Secrets and key custody

| | |
| --- | --- |
| Purpose | Inventory of every secret, variable and environment the pipeline uses, how to make each one again from zero, and who holds the keys |
| Owner | Maintainer |
| Status | Revision 1, 2026-10-06; section 4 added 2026-10-07; SEC5, SEC6 and RL2 removed on 2026-10-07 with RPM signing (ADR-0076, decision 2). Sections 1, 3 and 4 are facts. Section 2 and every line marked _(Proposal)_ await the maintainer |
| Depends on | `doc_kernel_build.md` section 6 (key design), `doc_kernel_profile.md` D43 and section 9 (custody, key table), `doc_update_trust.md` UT2, UT3 (image key), decisions A2-27, A2-35, ADR-0076 |
| Defines | SEC1-SEC12 (secrets), VAR1-VAR5 (variables), ENV1-ENV5 (environments), KC1-KC6 (custody), RL1-RL8 (recovery) |
| Facts checked with | `git grep` on `origin/iso-v0` at `bd1f0e4a`; `gh secret list`, `gh variable list`, `gh api repos/ars-regia/athanor/environments` and its `secrets`, `variables` and `deployment-branch-policies` endpoints, names only; the branches of open PRs #115 (`sign-vmlinuz`), #180 (`a2/delivery`) and #185 (`a2/rpm-sign-job`) |

No value of any secret appears here or was read to write this. Commands below use
`REPO=<owner>/<name>` so they work on a new organisation; today it is `ars-regia/athanor`.

## 1. Inventory

### 1.1 Secrets

Workflow references are `file:line` under `.github/workflows/` at `bd1f0e4a`.

| Id | Name | Scope | Material | Used by (workflow, job) | Public half in the repository |
| --- | --- | --- | --- | --- | --- |
| SEC1 | `SECUREBOOT_SIGNING_KEY` | environment `signing-kernel` | X.509 private key, RSA 4096, PEM, unencrypted | `nvidia-kmod.yml` `sign` (:130, environment :139), the sign-kernel job of D43, through `forge/specs/azoth/signer/run.sh sign`: written to a 0600 file for the one command, mounted read-only into the signer image run by digest without network, `sign-kernel.sh vmlinuz` signs the kernel's vmlinuz (`azoth-boot`). No image build sees it; 1.0 has no UKI (A2-8) | `forge/specs/azoth/keys/secureboot/athanor-secureboot.pem` and `.der` (the form `mokutil --import` takes) |
| SEC2 | `MODULE_SIGNING_KEY` | environment `signing-kernel` | X.509 private key, RSA 4096, PEM, unencrypted | `nvidia-kmod.yml` `sign` (:130, environment :139) through `forge/specs/azoth/signer/run.sh sign`, as SEC1; `sign-kernel.sh modules` signs the NVIDIA modules | `forge/specs/azoth/keys/modules/athanor-modules.pem`, compiled into Azoth (`kernel-local:42`, `CONFIG_SYSTEM_TRUSTED_KEYS`) |
| SEC3 | `COSIGN_PRIVATE_KEY` | environment `signing-images` | cosign key pair, ECDSA P-256, private half encrypted with SEC4 | `call-system-image.yml` `sign-system-images` (:427, environment :433) through `system/sign-images.sh:26-41`; the key-based image signature of UT2 | `system/keys/athanor-image-1.pub`, rendered into the image policy (UT3) |
| SEC4 | `COSIGN_PASSWORD` | environment `signing-images` | passphrase of SEC3 | as SEC3 (:453) | none |
| SEC7 | `KERNEL_BUMP_TOKEN` | repository | personal access token | `kernel-bump.yml` `pr` (:203, :207) and `system` (:263, :309); `cosmic-comp-bump.yml` `bump` (:33, :37); `nix-registry-bump.yml` `bump` (:46); as `MERGE_TOKEN` in `spec-build-check.yml` `merge` (:144) and `system-image-check.yml` `merge` (:152) for `forge/scripts/bot_merge.py`. A token, not `GITHUB_TOKEN`, because pull requests opened with `GITHUB_TOKEN` start no checks (`doc_kernel_build.md:447`) | none |
| SEC8 | `SPECS_UPDATE_TOKEN` | repository | personal access token | `forge-util-update-specs.yml` `update-specs` (:32, :45, :62): pushes `chore/update-specs-zero-trust` and opens its pull request | none |
| SEC9 | `FORGE_PAT` | repository | personal access token (classic) | `forge-ghcr-cleanup.yml` `cleanup-janitor` (:32, :39) through `forge/scripts/clean_ghcr.sh`, which deletes container package versions | none |
| SEC10 | `GITHUB_TOKEN` | automatic, per job | GitHub App installation token | most workflows; its scopes are each workflow's `permissions:` block | none |
| SEC12 | runner credential `github-token` | host of the self-hosted runner, not GitHub | GitHub token read by `install.sh` from standard input | `scripts/runner/install.sh:30,46` encrypts it with `systemd-creds` (host key and TPM2) into `/etc/credstore.encrypted/athanor-runner.github-token`; `athanor-runner.service:22` loads it; `vm.sh:29,102` uses it only to create a just-in-time runner configuration per job | none |

Keyless signatures (`forge/scripts/sign_attest.sh`, `cosign sign --yes`) use the workflow's
OIDC identity and need no secret.

### 1.2 Variables and environments

No repository variable is set (`gh variable list` is empty), so every default below is in force.

| Id | Name | Default | Used by |
| --- | --- | --- | --- |
| VAR1 | `REGISTRY_HOST` | `ghcr.io` | orchestrator (:46), `call-build-builder.yml:12`, `call-dag-compile.yml:35-36`, `forge-ghcr-cleanup.yml:34,41`, `fuzzing.yml:36`, `promote-stable.yml:37,42`, `rust-security-audit.yml:22,139,193`, `spec-build-check.yml:100` |
| VAR2 | `KERNEL_REGISTRY` | `ghcr.io/<owner>` (`system/kernel-artifacts.sh:56`) | orchestrator (:135, :186), `kernel-build.yml:54`, `kernel-bump.yml:60,258`, `nvidia-kmod.yml:46`, `system-image-check.yml:53` |
| VAR3 | `BUILDER_STABLE_TAG` | `latest` | `forge-ghcr-cleanup.yml:25`, `fuzzing.yml:36`, `rust-security-audit.yml:22,139,193` |
| VAR4 | `RELEASE_BRANCH` | `iso-v0` | PR #180 only: orchestrator, `call-system-image.yml`, `iso-acceptance.yml`, `promote-stable.yml` |
| VAR5 | `PROMOTE_DWELL_HOURS` | `24` | PR #180 only: `promote-stable.yml` |

| Id | Environment | Secrets | Protection (`.github/settings/environments.json`; GitHub still has the single `signing` environment until the bootstrap of section 4) | Referenced by |
| --- | --- | --- | --- | --- |
| ENV1 | `signing-kernel` | SEC1, SEC2 | required reviewer `hr-mes`, self-review allowed (`prevent_self_review` false, see `github-settings.md` section 7), no administrator bypass; deployment branches `iso-v0` and `main`, both protected (required check `Kernel gate`, no force push, no deletion) | `nvidia-kmod.yml:139` (`sign`, the sign-kernel job). `scripts/verify.py workflows` fails a signing secret outside a job of the environment that holds it in `environments.json`, a signing job that builds or uses an action other than checkout and artifact transfer, a step that hands a signing secret to anything but a sign script, and a signing environment with administrator bypass or a deployment branch that `branch-protection.json` does not protect (D43) |
| ENV2 | `stable-override` | none | **missing on GitHub** | PR #180 only (`promote-stable.yml`, checked by `system/require-review.sh`); `doc_update_trust.md` on that branch asks for required reviewers, the release branch only and no administrator bypass |
| ENV3 | `delete` | none | none; created 2026-08-08 | nothing |
| ENV4 | `github-pages` | none | deployment branches `gh-pages` and `main` | no workflow names it; it served the DNF channel, removed by ADR-0076 decision 2, and the maintainer deletes it with the `gh-pages` branch |
| ENV5 | `signing-images` | SEC3, SEC4 | required reviewer `hr-mes`, self-review allowed (`prevent_self_review` false, see `github-settings.md` section 7), no administrator bypass; deployment branches `iso-v0` and `main`, both protected (required check `Kernel gate`, no force push, no deletion) | `call-system-image.yml:433` (`sign-system-images`); PR #185 adds `sign-repo` and SEC5, SEC6 |

### 1.3 Drift between code and GitHub

| Direction | Item | Effect | Action |
| --- | --- | --- | --- |
| files ahead of GitHub | ENV1, ENV5, protection of `main` | the split and the protection exist only in `.github/settings`; GitHub still has `signing` with every key, and the workflows name the new environments, so a signing job fails to find its keys until they move | the bootstrap of section 4 |
| used, missing | ENV2 | PR #180's override path fails by design until the environment exists | create it when PR #180 merges |
| present, unused | ENV3 | none | delete it, or say what it is for |
| other | SEC9 | `forge-ghcr-cleanup.yml` fails on every run (37173567085, 36288233693, 35483291172) | outside this runbook |
| other | `system/cosign.pub` | an old public key (2026-07-24, blob `ef686642`), not the image key (`athanor-image-1.pub`, blob `48cdddb2`). `system/athanor-store/src/main.rs:39` reads `/etc/athanor/keys/cosign.pub`, and `git grep` finds nothing that installs that file | maintainer to decide: retire it or make the store use the image key |

### 1.4 Generation, rotation and holders

Generate every private key on tmpfs (`$XDG_RUNTIME_DIR`), never on the btrfs disk, where
copy-on-write makes `shred` ineffective. Record the fingerprints the commands print.
Every block below starts by running this preamble in the same shell; it fails if
`XDG_RUNTIME_DIR` or `REPO` is unset, and every path uses `${KEYDIR:?}` so none can resolve to `/`:

```bash
umask 077
: "${REPO:?set REPO=<owner>/<name>}"
KEYDIR=$(mktemp -d -p "${XDG_RUNTIME_DIR:?}" athanor-keys.XXXXXX)
```

When done: `shred -u "${KEYDIR:?}"/*.key` and `rm -r "${KEYDIR:?}"` (tmpfs: the pages are freed).
Upload with `gh secret set NAME --env ENVIRONMENT --repo "$REPO" < FILE` into the environment section 1.2 names for the secret, which never echoes the value.
Today every key and token is held by the maintainer alone (`hr-mes`).

**SEC1, Secure Boot key.** Parameters: `forge/specs/azoth/keys/profiles/secureboot.cnf`
(CN "Athanor Secure Boot Signing Key", not a CA, `codeSigning`), RSA 4096, SHA-256, 3650 days
(`keys/generate.sh:39`). `generate.sh` refuses to overwrite a certificate, so remove the old
one first.

```bash
# preamble of section 1.4 first
git rm forge/specs/azoth/keys/secureboot/athanor-secureboot.pem forge/specs/azoth/keys/secureboot/athanor-secureboot.der
bash forge/specs/azoth/keys/generate.sh secureboot --key-dir "${KEYDIR:?}"
gh secret set SECUREBOOT_SIGNING_KEY --env signing-kernel --repo "$REPO" < "${KEYDIR:?}/athanor-secureboot.key"
```

Consequences of a rotation:

- Every machine with Secure Boot on trusts the old certificate through MokList. It must enrol the new one (`mokutil --import athanor-secureboot.der`, then MokManager at the console at the next boot) **before** it boots an image signed with the new key, or shim refuses that boot.
- The installer enrols the certificate of the image it installs (A2-35), so new installations need nothing.
- The repository holds one Secure Boot certificate (`forge/specs/azoth/signer/run.sh` signs and verifies against it, `nvidia-publish.sh` attests its hash and `system/kernel-artifacts.sh` checks that hash). A staged rotation, where release N ships the new certificate for enrolment and release N+1 is the first one it signs, needs a change there first. _(Proposal)_
- Disk unlock is not affected at 1.0: TPM sealing is disabled until 1.1 (A2-27). From P4b the PCR policy key decides it (`doc_kernel_profile.md` section 9).

**SEC2, module signing key.** Parameters: `profiles/modules.cnf` (CN "Athanor Kernel Module
Signing Key", not a CA, `digitalSignature`), RSA 4096, SHA-256, 3650 days. `build.sh:290-293`
requires exactly one certificate in `keys/modules/` and at least one in `keys/revoked/`.

```bash
# preamble of section 1.4 first
git mv forge/specs/azoth/keys/modules/athanor-modules.pem forge/specs/azoth/keys/revoked/athanor-modules-$(date -I).pem
bash forge/specs/azoth/keys/generate.sh modules --key-dir "${KEYDIR:?}"
gh secret set MODULE_SIGNING_KEY --env signing-kernel --repo "$REPO" < "${KEYDIR:?}/athanor-modules.key"
```

Consequences: the certificate is compiled into Azoth, so a rotation is a kernel rebuild. Order:
merge, Kernel Build publishes, NVIDIA kmod re-signs, the Orchestrator rebuilds the images. No
machine needs any action, because the kernel and its modules always arrive in the same image.
The old certificate moves to `keys/revoked/`, so the new kernel refuses anything it signed.

**SEC3 and SEC4, image signing key** (`doc_update_trust.md` UT2, D3).

```bash
# preamble of section 1.4 first
cosign generate-key-pair --output-key-prefix "${KEYDIR:?}/cosign"   # prompts for the password: that is COSIGN_PASSWORD
cp "${KEYDIR:?}/cosign.pub" system/keys/athanor-image-<n>.pub
gh secret set COSIGN_PRIVATE_KEY --env signing-images --repo "$REPO" < "${KEYDIR:?}/cosign.key"
gh secret set COSIGN_PASSWORD --env signing-images --repo "$REPO"   # prompts, nothing echoed
```

Rotation follows UT3: key n+1 ships in the policy of an image signed with key n, and key n
leaves one release after the first image signed with n+1. There is no revocation: after a
compromise, recovery is out of band (`forge/specs/athanor-update/RECOVERY.md`,
`athanor-update recover-key`), see KC5.

**SEC7, SEC8, SEC9, SEC12, tokens.** A token is made again in the GitHub settings of the account
that owns it, then stored with `gh secret set NAME --repo "$REPO"` (SEC12: piped into `sudo
scripts/runner/install.sh --image <golden image>`, `scripts/runner/README.md:65-74`). A rotation
has no consequence beyond the runs that fail while the secret is stale. The type and scopes of
the stored tokens cannot be read back from GitHub; the minimum each needs, from what the code does:

| Id | Minimum permissions on this repository only | Why |
| --- | --- | --- |
| SEC7 | fine-grained: Contents read and write, Pull requests read and write, Issues read and write | pushes `bump/*` branches, `gh label create` (`forge/specs/azoth/open_bump_pr.sh:47`), `gh pr create`, `gh pr merge --auto` (`kernel-bump.yml:243`), merges in `bot_merge.py` |
| SEC8 | fine-grained: Contents read and write, Pull requests read and write | the message in `forge-util-update-specs.yml:35-39` |
| SEC9 | classic: `read:packages`, `delete:packages` (`clean_ghcr.sh:16`) | GitHub Packages accepts only classic tokens |
| SEC12 | _(Proposal)_ a dedicated fine-grained token with Administration read and write on this repository only, used for nothing else | `generate-jitconfig` (`vm.sh:102`) is the only call it serves |

_(Proposal)_ `KERNEL_BUMP_TOKEN` now serves six workflows, not only the kernel; a name such as
`BOT_PR_TOKEN` would say so. Renaming touches every reference in one commit.

## 2. Key custody _(Proposal)_

Today: one maintainer holds every key, with an offline backup held by the key custodian;
`signing-kernel` and `signing-images` each have one required reviewer, who may approve their own runs. Decision A2-27 keeps two approvals per release cycle (`sign-kernel` in `signing-kernel`, then
`sign-system-images` in `signing-images`; ADR-0064). `sign-kernel` is the `sign` job of `nvidia-kmod.yml`, which runs only
when a kernel or NVIDIA change leaves the signed modules or vmlinuz missing; any other cycle
asks for `sign-system-images` alone. Everything below is a proposal.

| Id | Proposal |
| --- | --- |
| KC1 | **Two holders,** the maintainer and one deputy, both required reviewers of `signing-kernel`, `signing-images` and `stable-override`. Once there are two, set `prevent_self_review`: a run is approved by the holder who did not start it. The two approvals of A2-27 then each need the second person. |
| KC2 | **Offline encrypted backup in two places.** The private keys of section 1.4, encrypted to both holders' OpenPGP keys, each on a hardware token: `tar -C "${KEYDIR:?}" -c ... \| gpg --encrypt -r <holder A> -r <holder B> -o <archive>`. Two copies, in two separate places, each kept by a different holder. Tokens are not backed up: they are made again (RL1). A private custody runbook, outside this public repository, records where each backup is and what it holds. |
| KC3 | **Generation ceremony.** Keys are generated on tmpfs as in section 1.4, with both holders present (in person or on a call). The archive of KC2 is written and test-decrypted before the plaintext is shredded. The committed certificate or public key is the record of what was generated. |
| KC4 | **Maintainer unavailable.** The deputy approves releases (KC1) and holds a backup copy (KC2), so the pipeline continues. The repository belongs to the organisation `ars-regia` since 2026-10-06 (`gh api repos/ars-regia/athanor --jq .owner.type` is `Organization`), but the maintainer is its only owner, so nobody else can administer it yet. Make the deputy an organisation owner. To rebuild elsewhere, the deputy decrypts the copy and loads the secrets into the new `signing-kernel` and `signing-images` environments with the commands of section 1.4. |
| KC5 | **Compromise.** 1. Stop: `gh workflow disable` the workflows that sign, `gh secret delete` the exposed secret. 2. Rotate the key as in section 1.4. 3. Revoke: SEC2, the old certificate goes to `keys/revoked/`; SEC1, MokListX entry or `mokutil --delete` of the old certificate on each machine (D41); SEC3, public notice, then the recovery command of `RECOVERY.md` on each machine (UT2); tokens, revoke in the GitHub settings. 4. Audit: list the deployments of the environment that held the key since the suspected exposure (`gh api "repos/$REPO/deployments?environment=signing-kernel"`, or `signing-images`; before the split, `signing`) and every digest signed in that window. |
| KC6 | **A holder leaves.** A private key cannot be taken back. Rotate every key that holder could decrypt, then re-encrypt the backup to the new pair of holders. |

## 3. Recovery after a loss

A loss here means no copy is left, neither on GitHub nor in a backup. A secret on GitHub cannot be
read back, so the backup of KC2 is the only copy that can be restored.

| Id | Lost | Can it be made again? | What the loss forces |
| --- | --- | --- | --- |
| RL1 | SEC7, SEC8, SEC9, SEC12 | yes, in the GitHub settings | nothing beyond the failed runs |
| RL3 | SEC2 | yes | a kernel rebuild (section 1.4); no machine action |
| RL4 | SEC1 | a new key, never the old one | every machine with Secure Boot on re-enrols, with someone at the console for MokManager. Images signed with the old key keep booting where it is enrolled. Machines with Secure Boot off need nothing |
| RL5 | SEC3 or SEC4 | a new key, never the old one | the new key cannot reach machines in an image signed with the old one (UT3 needs the old key), so every installed machine stops accepting updates. Each one is recovered out of band: `athanor-update recover-key` (`RECOVERY.md`) or a reinstall from a new ISO. This is the costliest loss |
| RL6 | the private key of the MOK of 2026-09-04, retired and held by no environment | not needed | nothing for its loss. A **leak** still matters: shim on a machine that still has the 2026-09-04 MOK enrolled boots what it signs, and kernels built before its revocation (2026-09-13) accept modules it signs. Its exposure stays relevant until that MOK is deleted (`mokutil --delete`) or listed in MokListX on every machine that enrolled it |
| RL7 | an environment (ENV1, ENV2, ENV5) | yes | recreate it with its reviewers and deployment branches as in section 1.2, then load its secrets from the backup |
| RL8 | the backup itself, with the GitHub secrets still in place | yes, for the backup | a GitHub secret cannot be exported, so rotate SEC1, SEC2 and SEC3 at a planned date while their old keys still sign, and back up the new keys (KC2) |

## 4. Bootstrap after merge

The merge of the sign-only signing cycle (D43, ADR-0064) leaves the pipeline waiting on five
steps, in this order. Until step 4 is done, System Image Check fails every system pull request:
`system/kernel-artifacts.sh check-plan` finds no `azoth-boot` for the pinned kernel and stops,
because the Orchestrator, not a pull request, publishes the signed vmlinuz.

1. **The maintainer creates the secrets of the two environments.** Create `signing-kernel` and
   `signing-images` as `.github/settings/environments.json` describes them
   (`docs/operations/github-settings.md` section 7: reviewer, branches `iso-v0` and `main`,
   administrator bypass off by hand), then load SEC1 and SEC2 into `signing-kernel` and SEC3 and
   SEC4 into `signing-images` with the commands of section 1.4, from the backup of KC2. Delete
   `signing` only after a run of each signing job has passed with the new environments.
2. **`azoth-signer.yml` publishes the signer image.** It runs on the push of the merge
   (`forge/specs/azoth/signer/**`, `lock.sh` and `sign-kernel.sh` are in its paths), or by hand:
   `gh workflow run azoth-signer.yml --ref iso-v0 --repo "$REPO"`. It needs no key and no
   approval.
3. **Commit the digest into `forge/specs/azoth/signer/image.digest`.** The run's step summary
   names it (`sha256:` and 64 hex digits, alone on the line). The file does not exist when the
   cycle merges, and until it is committed `signer/run.sh` fails closed in every stage before
   any container runs: NVIDIA kmod `prepare` fails, so `sign` never asks for the approval and
   nothing is signed. Every stage also verifies, before the pull, that `azoth-signer.yml`
   signed that digest on `iso-v0` or `main`. A run started before this commit, the one the
   merge itself starts included, fails that way; start the Orchestrator again once the digest
   is on the branch.
4. **The Orchestrator runs, NVIDIA kmod `sign` is approved, and `azoth-boot` is published.** The
   Orchestrator finds the signed vmlinuz missing and calls NVIDIA kmod. Its `sign` job waits for
   the `signing-kernel` approval; once approved, `publish` verifies the signed vmlinuz and
   publishes `azoth-boot` beside the modules.
5. **The system images build.** In the same Orchestrator run, the system stage copies the signed
   vmlinuz from `azoth-boot` by digest, and `sign-system-images` waits for the `signing-images`
   approval. From then on a cycle without a kernel or NVIDIA change asks only for that approval.

### 4.1 Image key rotation, October 2026

The password of image key 1 (SEC4) was lost before the bootstrap (RL5). Key 1 still signs
from the old `signing` environment, so the rotation of `doc_update_trust.md` UT3 replaces it
before that environment is deleted, and no installed machine needs the out-of-band recovery:

1. **Key 2 exists.** It was generated into the KC2 backup with a random password that lives only
   there and in `signing-images`, which holds key 2 as SEC3 and SEC4. Key 1 is archived without
   its password. `system/keys/athanor-image-2.pub` is committed, so every image built from then
   on trusts both keys.
2. **A transitional release is signed with key 1.** `sign-system-images` runs in `signing` until
   a release that carries key 2 is published and installed.
3. **The images are signed with key 2.** `sign-system-images` moves back to `signing-images`.
4. **Key 1 leaves.** One release after the first image signed with key 2,
   `system/keys/athanor-image-1.pub` is removed, and `signing` is deleted together with
   `MOK_PRIVATE_KEY`.

A machine that skips the transitional release still trusts key 1 only and refuses the images
signed with key 2: it is moved with `athanor-update recover-key` (`RECOVERY.md`).
