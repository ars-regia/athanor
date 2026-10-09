# Secrets and key custody

| | |
| --- | --- |
| Purpose | Inventory of every secret, variable and environment the pipeline uses, how to make each one again from zero, and who holds the keys |
| Owner | Maintainer |
| Status | Revision 1, 2026-10-06; section 4 added 2026-10-07; SEC5, SEC6 and RL2 removed on 2026-10-07 with RPM signing (ADR-0076, decision 2). Sections 1, 3 and 4 are facts. Section 2 is decided for the single-maintainer phase (ADR-0084). SEC12 and the rename of `KERNEL_BUMP_TOKEN` are decided on 2026-10-08 ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)); the lines marked _(Proposal)_ in section 2 take effect when a second maintainer joins |
| Depends on | `doc_kernel_build.md` section 6 (key design), `doc_kernel_profile.md` D43 and section 9 (custody, key table), `doc_update_trust.md` UT2, UT3 (image key), decisions A2-27, A2-35, ADR-0062, ADR-0076, ADR-0084 |
| Defines | SEC1-SEC14 (secrets), VAR1-VAR7 (variables), ENV1-ENV7 (environments), KC1-KC8 (custody), RL1-RL8 (recovery) |
| Facts checked with | `git grep` on `origin/iso-v0` at `e238b833`; `gh secret list`, `gh variable list`, `gh api repos/ars-regia/athanor/environments` and its `secrets`, `variables` and `deployment-branch-policies` endpoints, names only; the branches of open PRs #115 (`sign-vmlinuz`), #180 (`a2/delivery`) and #185 (`a2/rpm-sign-job`). The repository was then `hr-mes/athanor`; after the transfer the same `gh` queries on `ars-regia/athanor` on 2026-10-06 return the same names, reviewers and deployment branches |

No value of any secret appears here or was read to write this. Commands below use
`REPO=<owner>/<name>` so they work on a new organisation; today it is `ars-regia/athanor`.

## 1. Inventory

### 1.1 Secrets

Workflow references are `file:line` under `.github/workflows/` at `e238b833` (`origin/iso-v0`).

| Id | Name | Scope | Material | Used by (workflow, job) | Public half in the repository |
| --- | --- | --- | --- | --- | --- |
| SEC1 | `SECUREBOOT_SIGNING_KEY` | environment `signing-kernel` | X.509 private key, RSA 4096, PEM, unencrypted | `athanor-forge-orchestrator.yml` `nvidia-kmod-sign` (:190, environment :203), the sign-kernel job of D43, through `forge/specs/azoth/signer/run.sh sign`: written to a 0600 file for the one command, mounted read-only into the signer image run by digest without network, `sign-kernel.sh vmlinuz` signs the kernel's vmlinuz (`azoth-boot`). No image build sees it; 1.0 has no UKI (A2-8) | `forge/specs/azoth/keys/secureboot/athanor-secureboot.pem` and `.der` (the form `mokutil --import` takes) |
| SEC2 | `MODULE_SIGNING_KEY` | environment `signing-kernel` | X.509 private key, RSA 4096, PEM, unencrypted | `athanor-forge-orchestrator.yml` `nvidia-kmod-sign`, as SEC1, through `forge/specs/azoth/signer/run.sh sign`, as SEC1; `sign-kernel.sh modules` signs the NVIDIA modules | `forge/specs/azoth/keys/modules/athanor-modules.pem`, compiled into Azoth (`kernel-local:42`, `CONFIG_SYSTEM_TRUSTED_KEYS`) |
| SEC3 | `COSIGN_PRIVATE_KEY` | environment `signing-images` | cosign key pair, ECDSA P-256, private half encrypted with SEC4 | `athanor-forge-orchestrator.yml` `sign-system-images` (:299, environment :310) through `system/sign-images.sh:34-49`; the key-based image signature of UT2 | `system/keys/athanor-image-1.pub`, rendered into the image policy (UT3) |
| SEC4 | `COSIGN_PASSWORD` | environment `signing-images` | passphrase of SEC3 | as SEC3 (:335) | none |
| SEC7 | `KERNEL_BUMP_TOKEN` | repository | personal access token | `kernel-bump.yml` `pr` (:204, :208) and `system` (:264, :310); `cosmic-comp-bump.yml` `bump` (:33, :37); `nix-registry-bump.yml` `bump` (:46); as `MERGE_TOKEN` in `spec-build-check.yml` `merge` (:169) and `system-image-check.yml` `merge` (:152) for `forge/scripts/bot_merge.py`. A token, not `GITHUB_TOKEN`, because pull requests opened with `GITHUB_TOKEN` start no checks (`doc_kernel_build.md:457`) | none |
| SEC8 | `SPECS_UPDATE_TOKEN` | repository | personal access token | `forge-util-update-specs.yml` `update-specs` (:32, :45, :62): pushes `chore/update-specs-zero-trust` and opens its pull request | none |
| SEC9 | `FORGE_PAT` | repository | personal access token (classic) | `forge-ghcr-cleanup.yml` `cleanup-janitor` (:32, :39) through `forge/scripts/clean_ghcr.sh`, which deletes container package versions | none |
| SEC10 | `GITHUB_TOKEN` | automatic, per job | GitHub App installation token | most workflows; its scopes are each workflow's `permissions:` block | none |
| SEC12 | runner credential `github-token` | host of the self-hosted runner, not GitHub | GitHub token read by `install.sh` from standard input | `scripts/runner/install.sh:30,52-53` encrypts it with `systemd-creds` (host key and TPM2) into `/etc/credstore.encrypted/athanor-runner.github-token`; `athanor-runner.service:22` loads it; `vm.sh:29,102` uses it only to create a just-in-time runner configuration per job | none |

| SEC13 | `SETTINGS_APP_PRIVATE_KEY` | repository | private key of the settings GitHub App, which holds read permissions only (`github-settings.md` section 9) | `maintenance.yml` `settings-drift`, through `actions/create-github-app-token`, which mints a token for this repository with the App's own permissions, read-only only as long as the App is (`github-settings.md` section 9) | none |
| SEC14 | removed 2026-10-09 (ADR-0103 D4) | | | | |

Keyless signatures (`forge/scripts/sign_attest.sh`, `cosign sign --yes`) use the workflow's
OIDC identity and need no secret.

### 1.2 Variables and environments

No repository variable is set (`gh variable list` is empty), so every default below is in force.

| Id | Name | Default | Used by |
| --- | --- | --- | --- |
| VAR1 | `REGISTRY_HOST` | `ghcr.io` | orchestrator (:100), `call-build-builder.yml:12`, `call-dag-compile.yml:20-21`, `forge-ghcr-cleanup.yml:34,41`, `promote-stable.yml:37,42`, `rust-security-audit.yml:28`, `spec-build-check.yml:106` |
| VAR2 | `KERNEL_REGISTRY` | `ghcr.io/<owner>` (`system/kernel-artifacts.sh:63`) | orchestrator (:153, :206, :268), `kernel-build.yml:55`, `kernel-bump.yml:61,259`, `nvidia-kmod.yml:45`, `call-nvidia-kmod-prepare.yml:37`, `system-image-check.yml:53` |
| VAR3 | `BUILDER_STABLE_TAG` | `latest` | `forge-ghcr-cleanup.yml:25`, `rust-security-audit.yml:28` |
| VAR4 | `RELEASE_BRANCH` | `iso-v0` | PR #180 only: orchestrator, `call-system-image.yml`, `iso-acceptance.yml`, `promote-stable.yml` |
| VAR5 | `PROMOTE_DWELL_HOURS` | `24` | PR #180 only: `promote-stable.yml` |
| VAR6 | `SETTINGS_APP_CLIENT_ID` | none; not set yet | `maintenance.yml` `settings-drift`: client id of the settings GitHub App (SEC13) |
| VAR7 | removed 2026-10-09 (ADR-0103 D4) | | |

| Id | Environment | Secrets | Protection (`.github/settings/environments.json`; `signing` stays beside the split until the image key rotation of section 4.1 ends) | Referenced by |
| --- | --- | --- | --- | --- |
| ENV1 | `signing-kernel` | SEC1, SEC2 | required reviewer `hr-mes`, self-review allowed (`prevent_self_review` false, see `github-settings.md` section 7), no administrator bypass; deployment branches `iso-v0` and `main`, both protected (required checks `Kernel gate`, `Spec gate` and `gate` on `iso-v0`, `Kernel gate` on `main`; no force push, no deletion) | `athanor-forge-orchestrator.yml:203` (`nvidia-kmod-sign`, the sign-kernel job; a manual cycle dispatches the Orchestrator). `scripts/verify.py workflows` fails a signing secret outside a job of the environment that holds it in `environments.json`, a signing job that builds or uses an action other than checkout and artifact transfer, a step that hands a signing secret to anything but a sign script, a signing job in a workflow with `workflow_call` among its triggers, whose keys would be empty unless the caller inherits every secret (actions/runner#4453), and a signing environment with administrator bypass or a deployment branch that `branch-protection.json` does not protect (D43) |
| ENV2 | `stable-override` | none | **missing on GitHub** | PR #180 only (`promote-stable.yml`, checked by `system/require-review.sh`); `doc_update_trust.md` on that branch asks for required reviewers, the release branch only and no administrator bypass |
| ENV3 | `delete` | none | deleted by the maintainer; absent from the export of 2026-10-09 | nothing |
| ENV4 | `github-pages` | none | deleted with the `gh-pages` branch; absent from the export of 2026-10-09 | nothing; it served the DNF channel, removed by ADR-0076 decision 2 |
| ENV5 | `signing-images` | SEC3, SEC4 | required reviewer `hr-mes`, self-review allowed (`prevent_self_review` false, see `github-settings.md` section 7), no administrator bypass; deployment branches `iso-v0` and `main`, both protected (required checks `Kernel gate`, `Spec gate` and `gate` on `iso-v0`, `Kernel gate` on `main`; no force push, no deletion) | `athanor-forge-orchestrator.yml:299` (`sign-system-images`, still in `signing` during the rotation of section 4.1, which moves it back to `signing-images`) |
| ENV7 | removed 2026-10-09 (ADR-0103 D4) | | | |
| ENV6 | `signing` | SEC1 to SEC4 (`MOK_PRIVATE_KEY`, which no workflow used, deleted by the maintainer; absent from the export of 2026-10-09) | as ENV5; declared in `environments.json` only during the image key rotation (section 4.1), as an alias of ENV5 for `scripts/verify.py` | `athanor-forge-orchestrator.yml` `sign-system-images` until step 3 of section 4.1; deleted at step 4 |
| ENV8 | `release` | none | required reviewer `hr-mes`, self-review allowed, no administrator bypass; deployment branches `iso-v0` and `main` | no job yet; the job that moves `:stable` moves behind it, so only a promotion asks for it, and the image signing does not wait for it ([ADR-0104](../decisions/0104-release-workflow-and-stable-gate.md), amending [ADR-0103](../decisions/0103-audit-5-decisions.md) D2, D25) |

### 1.3 Drift between code and GitHub

| Direction | Item | Effect | Action |
| --- | --- | --- | --- |
| files ahead of GitHub | ENV1, ENV5, protection of `main` | the split and the protection exist only in `.github/settings`; GitHub still has `signing` with every key (it stays until section 4.1 ends), and the workflows name the new environments, so a signing job fails to find its keys until they move | the bootstrap of section 4 |
| files ahead of GitHub | SEC13, VAR6 | `maintenance.yml` fails until both exist | step 2 of `github-settings.md` section 9 |
| used, missing | ENV2 | PR #180's override path fails by design until the environment exists | create it when PR #180 merges |
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
- Disk unlock at 1.0: a passphrase is not affected. A TPM plus PIN keyslot from `athanor-uki-enroll` (A2-27 as amended on 2026-10-08) is bound to PCR 7, where shim measures the certificate that verified the kernel: the first boot of a kernel signed with the new key asks for the passphrase or the recovery key, and the administrator runs the tool again. From P4b the PCR policy key decides it (`doc_kernel_profile.md` section 9).

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
| SEC7 | fine-grained: Contents read and write, Pull requests read and write, Issues read and write | pushes `bump/*` branches, `gh label create` (`forge/specs/azoth/open_bump_pr.sh:47`), `gh pr create`, `gh pr merge --auto` (`kernel-bump.yml:244`), merges in `bot_merge.py` |
| SEC8 | fine-grained: Contents read and write, Pull requests read and write | the message in `forge-util-update-specs.yml:35-39` |
| SEC9 | classic: `read:packages`, `delete:packages` (`clean_ghcr.sh:16`) | GitHub Packages accepts only classic tokens |
| SEC12 | decided 2026-10-08 ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)), the maintainer creates it: a dedicated fine-grained token with Administration read and write on this repository only, used for nothing else | `generate-jitconfig` (`vm.sh:102`) is the only call it serves |

`KERNEL_BUMP_TOKEN` now serves six workflows, not only the kernel. Decided 2026-10-08 ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)): it is renamed `BOT_PR_TOKEN`
in one commit that touches every reference. The rename is a later code change under `.github/` and is not made yet.

## 2. Key custody

The custody model is definitive for the single-maintainer phase (ADR-0084, 2026-10-07): two
LUKS2 USB drives, a sealed letter, and a separate holder of the passphrase. It is revisited
when a second maintainer joins. KC1-KC8 apply as written.
Lines marked _(Proposal)_ take effect when a second maintainer joins.

Today: one maintainer holds every key. `signing-kernel` and `signing-images` each have one
required reviewer, who may approve their own runs, because no second signing reviewer is named
(ADR-0062). Decision A2-27 keeps two approvals per release cycle (`sign-kernel` in
`signing-kernel`, then `sign-system-images` in `signing-images`; ADR-0064). `sign-kernel` is the
`nvidia-kmod-sign` job of the Orchestrator (a manual cycle dispatches the Orchestrator), which
runs only when a kernel or NVIDIA change leaves the signed modules or vmlinuz missing; any other
cycle asks for `sign-system-images` alone.

| Id | Rule |
| --- | --- |
| KC1 | **One holder.** The maintainer holds every private key and is the only required reviewer of `signing-kernel`, of `signing` until step 3 of the rotation of section 4.1, and of `release`, which gates the promotion to `:stable` (ADR-0104), with self-review allowed. `signing-images` has no required reviewer from step 3 on, 1.0 included (ADR-0098 item 6, ADR-0104 item 8). Every approval is therefore the maintainer's: until step 3 at most two for the publication of one build (A2-27), `signing-kernel` and `signing`; from step 3 one, `signing-kernel`, and only when the kernel or the modules change. The `release` approval of a promotion is a separate act (ADR-0104 item 7). _(Proposal)_ With a second maintainer, the deputy becomes a holder and a required reviewer of `signing-kernel`, `signing-images` and `stable-override`, and `prevent_self_review` is set: each approval then comes from the holder who did not start the run. |
| KC2 | **Offline backup on two encrypted USB drives.** Each drive is a LUKS2 volume that holds the private keys and passphrases of section 1.4 (SEC1-SEC4) and the fingerprint inventory of KC7. Tokens are not backed up: they are made again (RL1). The LUKS passphrase is long, used for nothing else, and stored on neither drive. The two drives are kept in two separate places, so that one theft, fire or loss cannot take both. Where they are is written only in the sealed instructions of KC8, never in this repository. A drive that is lost, stolen or out of the maintainer's control for any time is a compromise (KC5). _(Proposal)_ With a second maintainer, the keys move to hardware tokens, and the backup becomes an archive encrypted to both holders' OpenPGP keys, one copy kept by each holder. |
| KC3 | **Generation ceremony.** Keys are generated on tmpfs as in section 1.4, and nothing is uploaded until the backup is proven. For each drive in turn: write the keys, `cryptsetup close` the volume, unplug the drive and plug it in again, reopen it by typing the passphrase, and compare each file by SHA-256 with the tmpfs copy. Then prove that the copies work: the public key derived from each private key equals the committed one (X.509 keys, SEC1 and SEC2: `openssl pkey -pubout` against `openssl x509 -pubkey -noout` of the certificate to be committed; cosign key, SEC3: `cosign public-key --key cosign.key` with `COSIGN_PASSWORD`, which `openssl` cannot read, against the `.pub` to be committed), and `cosign.key` signs and verifies a test blob with `COSIGN_PASSWORD`. Only after that does `gh secret set` run, and only after the upload is the plaintext shredded. This changes the order of the blocks of section 1.4, which upload right after generating. The fingerprints go into the inventory (KC7), and the committed certificate or public key is the public record of what was generated. _(Proposal)_ With two holders, both are present, in person or on a call. |
| KC4 | **Maintainer unavailable.** Only the maintainer can approve `signing-kernel`, `release`, which gates the promotion to `:stable` (ADR-0104), and `signing` until step 3 of the rotation of section 4.1. Until step 3 nothing is signed and nothing is promoted until the drives reach someone who can continue; after it `:latest` is still signed with no approval, but no kernel is signed and nothing is promoted to `:stable` (ADR-0098 item 6, ADR-0104 item 8). The repository belongs to the organisation `ars-regia` since 2026-10-06 (`gh api repos/ars-regia/athanor --jq .owner.type` is `Organization`), but the maintainer is its only owner, so nobody else can administer it. The successor is the person KC8 names to receive the drives. _(Proposal)_ Make the successor an owner of the organisation. To rebuild elsewhere, the successor opens a drive with the passphrase (KC8), creates the new `signing-kernel` and `signing-images` environments (RL7) and loads the secrets with the commands of section 1.4. A new owner changes the workflow identity (`https://github.com/<owner>/athanor/...`) that the keyless checks of the CI expect (`system/kernel-artifacts.sh` and its tests) and the registry path of the published images, so it is planned as a signing change. |
| KC5 | **Compromise.** A key counts as compromised when it leaks, when a backup drive is lost, stolen or out of control (KC2), or when a fingerprint check fails (KC7); a lost drive compromises every key it held (SEC1-SEC4). 1. Stop: `gh workflow disable` the workflows that sign, `gh secret delete` the exposed secret. 2. Rotate the key as in section 1.4. 3. Revoke: SEC2, the old certificate goes to `keys/revoked/`; SEC1, MokListX entry or `mokutil --delete` of the old certificate on each machine (D41); SEC3, public notice, then the recovery command of `RECOVERY.md` on each machine (UT2); tokens, revoke in the GitHub settings. 4. Audit: list the deployments of the environment that held the key since the suspected exposure (`gh api "repos/$REPO/deployments?environment=signing-kernel"`, or `signing-images`; before the split, `signing`) and every digest signed in that window. |
| KC6 | **The trusted people change.** When the letter holder or the passphrase holder of KC8 changes, set a new LUKS passphrase on both drives, give it to the new passphrase holder, and renew the letter. A passphrase change does not re-encrypt the LUKS master key, so if anyone may have seen the old passphrase together with a drive, treat it as KC5. _(Proposal)_ With two holders, when a holder leaves: a private key cannot be taken back, so rotate every key that holder could decrypt, then re-encrypt the backup to the new pair of holders. |
| KC7 | **Quarterly check.** Once a quarter, open both drives. Derive the public key of each private key and check it against the inventory and the committed certificate or public key: `openssl pkey -pubout` for the X.509 keys (SEC1, SEC2), `cosign public-key --key <file>` with the key's password in `COSIGN_PASSWORD` for the cosign keys (SEC3; they are in the encrypted sigstore format, which `openssl` cannot read). Image key 1 is archived without its password (section 4.1), so no public key can be derived from it: record the SHA-256 of its file in the inventory and compare it, and check that `system/keys/athanor-image-1.pub` is still committed. Confirm that the letter holder still has the letter and that the passphrase holder of KC8 can still produce the passphrase. Record the date in the private custody log. A drive that does not open is rewritten from the other one the same day, checked again the same way, and the failed drive is destroyed: flash memory fails without warning. A key whose fingerprint does not match is treated as KC5. |
| KC8 | **Sealed instructions.** A sealed letter, kept by a person the maintainer trusts, names the successor (KC4), says where the two drives are, and says who holds the LUKS passphrase. The passphrase is held by a different person or a notary, who gives it only to the successor and only once the maintainer is declared unavailable. The conditions for that declaration are written in the letter. Neither holder alone can open a drive. The letter contains no key and no passphrase, and it is renewed whenever a drive moves, the passphrase changes or one of the people changes (KC6). |

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
   SEC4 into `signing-images` with the commands of section 1.4, from the backup of KC2. Keep
   `signing`: it signs the transitional release with image key 1 and is deleted only at step 4
   of section 4.1.
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
4. **The Orchestrator runs, `nvidia-kmod-sign` is approved, and `azoth-boot` is published.** The
   Orchestrator finds the signed vmlinuz missing and runs the NVIDIA kmod cycle. Its
   `nvidia-kmod-sign` job waits for the `signing-kernel` approval; once approved, `publish` of
   NVIDIA kmod verifies the signed vmlinuz and publishes `azoth-boot` beside the modules.
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
   a release that carries key 2 is promoted to `stable` and the machines meant to keep updating
   without recovery have booted it: bootc checks an image against the policy of the booted
   deployment. `promote.sh` and `sign-images.sh` verify against the keys of the checkout, which
   already holds both, so they cannot tell when this holds: the maintainer decides it. Until
   step 4, the D43 lint of `scripts/verify.py` treats `signing` as `signing-images`; the
   environment has the same reviewer, branches and no administrator bypass.
3. **The images are signed with key 2.** `sign-system-images` moves back to `signing-images`, and
   `sign-images.sh` verifies the signature against `athanor-image-2.pub` alone, so an image still
   signed with key 1 fails there instead of passing a policy that trusts both keys.
4. **Key 1 leaves.** One release after the first image signed with key 2,
   `system/keys/athanor-image-1.pub` is removed, and `signing` is deleted.

A machine that skips the transitional release still trusts key 1 only and refuses the images
signed with key 2: it is moved with `athanor-update recover-key` (`RECOVERY.md`).
