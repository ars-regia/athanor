# Update delivery: from a signed run to every machine

| Field      | Value                                                                                                                                                                                                                                                                             |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose    | How a built and signed system image becomes the `stable` channel, how every machine reaches it with its signature verified, how the update gets smaller and lighter, and what the pipeline must change to produce it efficiently and from pinned inputs                           |
| Owner      | the maintainer (`@hr-mes`)                                                                                                                                                                                                                                                        |
| Status     | draft, rev 2, 2026-10-06: the six open questions of rev 1 are decided (section 15); awaiting the maintainer's approval. Nothing here is built before it                                                                                                                                                                     |
| Depends on | [doc_update_trust.md](doc_update_trust.md) (UT1 to UT13), [doc_ci.md](doc_ci.md) (CI1, CI11, CI12), [doc_system_image.md](doc_system_image.md), [doc_kernel_build.md](doc_kernel_build.md), [transfer-to-organisation.md](../operations/transfer-to-organisation.md) (TO1 to TO8) |
| Defines    | UD1 to UD51                                                                                                                                                                                                                                                                       |

Binding decisions: [A2-4](../decisions/0039-delivery-repairs-before-1-0.md), [A2-5](../decisions/0040-security-updates-at-next-shutdown.md), [A2-8](../decisions/0043-bootc-in-two-steps.md), [A2-13](../decisions/0050-nixpkgs-pin-and-nix-hardening.md), [A2-16](../decisions/0053-nix-for-every-user.md), [A2-17](../decisions/0054-audience-and-support-window.md), [A2-26](../decisions/0063-update-policy.md), [A2-27](../decisions/0064-signing-approvals-and-mok-enrolment.md), [RA-12](../decisions/0028-ghcr-literal-single-exception.md), [ADR-0073](../decisions/0073-component-verdicts.md), [ADR-0075](../decisions/0075-engineering-gates.md), [ADR-0076](../decisions/0076-platform-scope-for-1-0.md).

`doc_update_trust.md` decides what a machine trusts and how it updates. This document implements and sequences it: it does not change the trust model, the policy, the keys or the client's state machine. Where it needs a change in an approved document, section 13 lists it. Choices the maintainer has not made are marked _(Proposal)_; facts not yet observed on this repository are marked _(to verify)_; numbers that are not measured are marked _(estimate)_.

## 1. Scope

The first five priorities of the pipeline audit of 2026-10-06, in order:

1. **Delivery works.** The `stable` channel exists, is promoted by digest from a signed run with evidence, every machine follows it, and the maintainer's two machines move from `ghcr.io/hr-mes` to `ghcr.io/ars-regia` with the digest verified.
2. **An upgrade test in CI.** The previous release is upgraded to the candidate, rebooted, checked, and rolled back, and the result is a required promotion gate.
3. **Machines verify signatures.** The policy of UT3 is in force on every machine, the publication order makes an unsigned image unreachable, and key rotation cannot strand a machine.
4. **Lighter updates.** Reproducible builds, rechunked images, a recorded download size per upgrade, and soft reboots where the kernel does not change.
5. **An efficient, pinned pipeline.** Unchanged inputs build nothing, shared stages build once, every input is pinned by digest, and every image a build consumes is signed.

## 2. Non-goals

- **Trust design.** Keys, policy, verification and the client's requests stay as UT1 to UT13 define them.
- **The sealed composefs backend.** It is release 1.1 (A2-8). This document targets bootc on the ostree backend.
- **Moving system components to Nix.** Section 9.5 records why.
- **ISO signing.** The ISO is installation media, not an update source (UT2). It keeps its published SHA-256.
- **A `testing` channel before 1.0.** `latest` is the only pre-release channel; machines follow `stable`.
- **Kernel builds on hosted runners.** Deferred; section 10.1 records the estimate and the condition to revisit.

## 3. Rationale: what ships today

Checked on 2026-10-06 against branch `iso-v0` at eda61571, the maintainer's desktop and the registry. Source: the pipeline audit of 2026-10-06, stages 1 to 5, re-checked where cited.

| #   | Fact                                                                                                                                                                                                                                                                   | Where                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| F1  | `:stable` does not exist in any of the three system image repositories; `promote-stable.yml` has never run                                                                                                                                                             | registry, Actions history                                              |
| F2  | The build job pushes `:<run_id>` **and** `:latest` before the signing job runs; `:latest` names an unsigned digest for a median of 3.5 min and a maximum of 128 min in the audited runs; the window lasts as long as the `signing` approval waits, which has reached 604 min                                                      | `call-system-image.yml` "Push OS Images", `DEFAULT_TAG=latest`         |
| F3  | `system/promote.sh` verifies the run's digest through the rendered policy, then copies `docker://$repository:$run`, the tag, to `:stable`. The tag can move between the check and the copy                                                                             | `system/promote.sh`                                                    |
| F4  | `promote-stable.yml` needs `lint` only; no acceptance evidence is checked, no attestation is written                                                                                                                                                                   | `promote-stable.yml`                                                   |
| F5  | `athanor-update-migrate.service` fails while `:stable` is absent and restarts every 5 minutes, 288 registry queries a day per machine                                                                                                                                  | `migrate.rs` (`ctx.tools.candidate(&target)?`), unit `RestartSec=5min` |
| F6  | The migration target is `<booted repository>:stable`. Nothing in the client knows that a repository can move                                                                                                                                                           | `migrate.rs`, `sigobj::repository_of`                                  |
| F7  | The maintainer's desktop boots `ostree-unverified-registry:ghcr.io/hr-mes/athanor-system-nvidia:latest` (43.20261004.153) with `nvidia-container-toolkit` layered. bootc reports `image: null` and refuses `upgrade` and `switch`. The image already ships the toolkit | desktop, `bootc status`, `rpm-ostree status`                           |
| F8  | No upgrade test exists. ISO acceptance installs the newest published ISO, weekly; its scheduled run is cancelled when `lint` fails                                                                                                                                     | `iso-acceptance.yml` (`needs: [lint]`, `ISO_TAG: newest`)              |
| F9  | The dev-VM harness covers migrate, download, apply, go-back and rotate, but refuses to run beside a CI job (`guard_no_ci`) and uses its own test registry and keys                                                                                                     | `scripts/devvm/acceptance/run.sh`, `lib.sh`                            |
| F10 | One upgrade between two consecutive runs downloads 26 of 124 layers: 1,905 MB on the default variant, 2,455 MB on `-nvidia`, 2,510 MB on `-nvidia-legacy`. A base bump costs 2,289, 2,839 and 2,894 MB                                                                 | registry, runs 37315915191 and 37384733899                             |
| F11 | The three variants share no layer of their own: the `system` stage is not reused between them                                                                                                                                                                          | registry diff_ids                                                      |
| F12 | Not pinned: tier repositories (`:latest`, `system/Containerfile:23-26`), live `dnf` installs, `nixpkgs#cosign` and `nixpkgs#syft`, `--pull=newer` (`build-image.sh:69`), `bootc-image-builder:latest` (`build_iso.sh:19`). `SOURCE_DATE_EPOCH` is never set            | files cited                                                            |
| F13 | The orchestrator's brain marks every node dirty: `.cache` is git-ignored and `ATHANOR_REDIS_URL` is unset. 2,201 of 2,921 matrix jobs in 30 days were cache hits at about 103 s each, about 65 runner-hours                                                            | `dag_orchestrator.py`, `.gitignore:14`                                 |
| F14 | The builder hash includes `config/packages.json`; 19 of 32 builder tags are the same digest                                                                                                                                                                            | `check_idempotency.sh:58-61`                                           |
| F15 | Tier repository images are unsigned. RPM signing is skipped silently: `if [ -n "${RPM_GPG_KEY}" ]`, and no such secret exists                                                                                                                                          | `call-system-image.yml:94,123`                                         |
| F16 | The kernel builds on the self-hosted runner in a median 3,011 s with no compiler cache, although `doc_kernel_build.md` promises one                                                                                                                                    | kernel runs                                                            |
| F17 | Host versions: bootc 1.16.13, rpm-ostree 2026.1, ostree 2026.2, systemd 258.11, podman 5.8.4                                                                                                                                                                           | desktop, `rpm -q`                                                      |

## 4. Delivery: channels and promotion (priority 1)

### 4.1 Channels

**UD1. Machines follow exactly one channel, `stable`; every published tag has one writer.**

| Tag                                                 | Written by                                       | Followed by                         |
| --------------------------------------------------- | ------------------------------------------------ | ----------------------------------- |
| `:<run_id>`                                         | build job (UD24), once                           | nobody; the immutable name of a run |
| `:latest`                                           | tagging job, after the signature verified (UD25) | test machines that opt in           |
| `:stable`, `:stable-previous`, `:stable-<YYYYMMDD>` | `system/promote.sh` only (UD3)                   | every installed machine (UT4)       |

Acceptance: `python3 scripts/verify.py workflows` gains a check that no workflow step other than the tagging job and `promote.sh` passes `--tag latest`, `:latest` or `:stable` to `podman push` or `skopeo copy`.

**UD2. A machine installed from the ISO follows `:stable` with the signature enforced from its first boot** (A2-4, "kickstart on :stable"). The ISO is built from a promoted digest, and the installed reference is `ostree-image-signed:docker://<registry>/<image>:stable`. _(Proposal)_ The reference is set with bootc-image-builder's target image reference option _(to verify on the pinned builder)_; if it cannot be set, the migration of UT4 sets it at first boot and the installed system downloads nothing (same digest). Acceptance: the ISO acceptance checks `bootc status --format=json` on the installed guest: `.status.booted.image.image.image` ends in `:stable` and `.spec.image.signature` is not `insecure`, or the migration stamp exists within 10 minutes of first boot.

### 4.2 Promotion

**UD3. `system/promote.sh` is the only promoter, and it promotes by digest.** It takes a run id, reads the three digests from that run's `image-digests.txt`, verifies each through the shipped policy (`skopeo copy --policy`, as today), and copies `docker://<repo>@<digest>`, never a tag, to `:stable-previous` (the old `:stable` digest), `:stable-<YYYYMMDD>` and `:stable`. It then re-reads `:stable` and fails unless it names the verified digest. Acceptance: `system/tests/test_promote.sh` _(new)_ runs `promote.sh` against a local registry in which `:<run_id>` is moved to another image between the check and the copy, and asserts that `:stable` names the checked digest.

**UD4. Promotion requires evidence for the same digests.** `promote.sh` refuses unless an evidence file exists for each required gate and names the exact digest being promoted:

- `iso-acceptance` (UD17) for the default variant;
- `upgrade-acceptance` (UD18) for the default variant;
- `signature` (UD25) for all three variants.

An evidence file is JSON written by the gate's script: `{"gate", "digest", "run_id", "verdict", "workflow_run_url", "finished_at"}`. The workflow downloads the files into `artifacts/evidence/`; `promote.sh` reads only that directory (portable pipeline rule). The NVIDIA variants, which the hosted virtual GPU cannot test, also need a `hardware` evidence file for the same digest: a maintainer machine on `:latest` writes it with one command (`athanor-update report-evidence`, a test-mode verb) after a session has started on that digest. Without it, the default variant promotes alone (decision 2 of section 15). Acceptance: `system/tests/test_promote.sh` asserts a refusal for a missing file, a `fail` verdict, and a digest mismatch.

**UD5. Promotion is automatic after the evidence and a dwell; `promote.sh` by hand is the override** (A2-4). A scheduled job of `promote-stable.yml` _(Proposal: hourly)_ selects the newest run whose evidence is complete and whose last evidence file is older than the dwell of 24 hours (decision 1 of section 15), and that is newer than `:stable` by build time (the existing refusal). A newer run with complete evidence restarts the dwell. The manual dispatch keeps its `run_id` input and the same evidence checks; it may shorten the dwell, never skip the evidence. Acceptance: a dry-run mode (`promote.sh --plan`) prints the run it would promote; the job summary records it on each scheduled run.

**UD6. Promotion records what it did.** Each promotion writes `artifacts/promotion.json` (`run_id`, the three digests, the previous `:stable` digests, the evidence files' SHA-256, the trigger: `schedule` or `dispatch`) and attaches it to the promoted digests as a keyless cosign attestation, the public record. The security class of UT13 is a field of a key-signed attestation (decision 3 of section 15): an automatic promotion is always of the feature class and uses no key, which is UT13's reading of an image without a verified class field; a security-class promotion goes through the manual `promote.sh` in the `signing` environment, which signs that attestation. Acceptance: `cosign verify-attestation` with the workflow identity of `promote-stable.yml` passes on the promoted digests.

**UD7. Promotion never moves a machine backwards and never strands one on a key.** Beyond the existing build-time refusal, `promote.sh` refuses a digest whose key-based signature verifies only with a key that the current `:stable` image does not carry in its `keyPaths` (UT3 rotation: key _n+1_ must ship in a promoted image signed with _n_ before an image signed with _n+1_ is promoted). Acceptance: `system/tests/test_promote.sh` with two test keys covers both orders.

**UD8. Retention keeps every promoted digest and its evidence for the support window.** `clean_ghcr.sh` treats `:stable-<YYYYMMDD>` as UT10 treats `:stable`, and keeps the ISO of each promoted run (`athanor-iso:<run_id>`), which UD18 boots. The 90-day figure of UT10 stays until the support-window document replaces it. Acceptance: the janitor's dry run lists no promoted digest or promoted ISO among its deletions.

### 4.3 How machines reach `stable`

**UD9. A missing channel is a wait, not a failure.** `migrate.rs` maps a registry answer of "manifest unknown" for the channel to `Waiting("channel-absent")`, writes no stamp, exits 0, and the unit retries on its timer, not on `Restart=on-failure`. Other errors keep failing. Acceptance: `cargo test -p athanor-update` gains a case with a fake `candidate()` returning manifest unknown; on the dev VM with no `:stable`, `systemctl show -p NRestarts athanor-update-migrate.service` stays 0 for an hour.

**UD10. A machine leaves an unverified reference by itself, once its policy is in force** (UT4, unchanged), and the check names why it waits: `local-changes`, `policy-not-in-force`, `reference-out-of-scope`, `channel-absent`. A machine that migrated and was later switched to a reference that does not enforce the policy reads `origin-not-enforcing`: it is reported, not switched back. The shield shows the reason with its remedy (UD16 for `local-changes`). Acceptance: dev-VM harness stage `migrate`, extended with a machine without `:stable`.

## 5. The namespace move (priority 1)

Athanor runs on two machines, both the maintainer's: the desktop and the laptop `athanor-ref` (maintainer, 2026-10-06). Both are within reach, so the move from `ghcr.io/hr-mes` to `ghcr.io/ars-regia` is a verified step on each machine. Machines installed before the transfer that nobody logs in to follow the previous owner's frozen `:latest`; the bridge of section 5.2 (decided 2026-10-08) moves them without a step on the machine.

**UD11. Each machine moves by digest, verified before the switch.** After TO5 has republished the images under `ars-regia`, on each machine:

1. read the digest of `ghcr.io/ars-regia/<image>:latest` for the machine's variant (`:stable` once it exists);
2. verify it out of band through the new image's rendered policy: `skopeo copy --policy <rendered policy of the new image> docker://<repo>@<digest> dir:<tmp>`;
3. run `sudo bootc switch <repo>@<digest>`. The booted policy names only `hr-mes`, so it cannot enforce this one switch; step 2 is the check, and the switch by digest installs exactly the verified image;
4. after the reboot the new image's policy is in force, and UT4's migration moves the machine to `ostree-image-signed:docker://<repo>:stable` once `:stable` exists.

Acceptance: on each machine `bootc status --format=json` names an `ars-regia` reference and, once `:stable` exists, the shield reads verified.

**UD12. The policy names only the canonical namespace.** The rendered policy keeps its three `sigstoreSigned` scopes under the registry owner the build derives (#230); no `hr-mes` scope is added. Acceptance: `python3 scripts/verify.py shipped` and UD22.

**UD13. The client keeps UT4's target.** `migrate.rs` keeps targeting `<booted repository>:stable`; after UD11 the booted repository is the `ars-regia` one. A booted repository the policy does not name but whose image it pins under another owner is the case of UD47. Acceptance: `cargo test -p athanor-update` (exists).

**UD14. The organisation rename stays blocked until the move is observed** (TO8, unchanged). Evidence: the desktop and the laptop both report `.verified.state == "verified"` on an `ars-regia` reference.

### 5.1 The maintainer's desktop and the layering rule

**UD15. The desktop returns to the image path in three steps.** (This is an operational procedure; it lands in `docs/operations/` with section 13.)

1. `sudo rpm-ostree uninstall nvidia-container-toolkit`. The image already ships the toolkit in its NVIDIA stage; `nvidia-ctk cdi list` after the reboot shows the GPUs _(to verify)_.
2. Reboot. `bootc status --format=json` shows a non-null `.status.booted.image`, and `rpm-ostree status --json --booted` shows no `requested-packages`, `requested-local-packages` or overrides.
3. The desktop moves to `ars-regia` with `sudo bash scripts/switch-verified.sh ghcr.io/ars-regia/athanor-system-nvidia:latest`, which verifies the signature before the switch and records the signed origin (TO6). The bridge of section 5.2 would also move it, after one unverified pull.

Acceptance: `bootc status --format=json | jq -e '.status.booted.image != null'` and the shield reads verified after step 3.

**UD16. No package layering on an Athanor machine.** bootc refuses `upgrade` and `switch` on a layered deployment, so a layered machine stops updating. The rule is enforced by detection and a remedy, not by policy, because root can always layer:

- the check already publishes `local-changes` ahead of every other reason (UT3); the shield and `athanor-update status` _(new verb)_ print the layered packages and the exact `rpm-ostree uninstall` or `rpm-ostree reset` command;
- `docs/operations/` states where each need goes instead: a system component into the image (a pull request), a graphical application into Flatpak, a command-line tool into per-user Nix (A2-16), a service into a Podman container or Quadlet;
- the dev-VM acceptance stage `local-changes` _(new)_ layers a package, checks the reason and the remedy text, removes it, and checks that updates resume.

Acceptance: the dev-VM stage above.

### 5.2 The bridge for machines that follow the previous owner (2026-10-08)

Machines installed before the transfer follow `ghcr.io/hr-mes/athanor-system*:latest`, which no longer moves, and report no update. UD11 reaches only machines the maintainer can log in to; the bridge reaches the others without a step on the machine, and its own policy verifies the move.

**UD45. The bridge is the verified image, tagged under the previous owner.** When the repository variable `ATHANOR_BRIDGE_REGISTRY` names the previous owner as `REGISTRY/OWNER` (`ghcr.io/hr-mes`), CI1 job `bridge-system-images` runs after `tag-system-images` and copies each digest that job moved to `ghcr.io/ars-regia/<image>:latest` onto `<bridge>/<image>:latest`, by digest (`system/tag-images.sh --to`, which copies with `--preserve-digests` and reads the tag back). The same variable is the previous owner the images know: CI5 passes it to `system/build-image.sh`, and `render-policy --moved-from` writes it to `/usr/share/athanor/containers/moved-from` beside the policy, one `REGISTRY/OWNER` per line, empty while the variable is unset. This image side lands in a second pull request, after the athanor-update release whose render-policy takes `--moved-from` is in the published tiers: System Image Check builds a pull request's image from the published tiers, never from the pull request's specs, so a `system/Containerfile` that passes the option before that release fails the check, and only an Orchestrator run after the merge would build it. Until then the images list no previous owner, and the variable stays unset _(second step, not landed)_. Only an image under an owner listed there moves (UD47); a fork stays out of scope. Nothing is built or signed for the bridge itself: it is the image `ars-regia` publishes, with the policy that pins `ars-regia` and its keys, under the same digest. The variable is unset by default, so the job is skipped. The write token is `ATHANOR_BRIDGE_TOKEN`, held by the environment `bridge` (required reviewer `hr-mes`, no administrator bypass, deployment branches `iso-v0` and `main`), so no other job reads it and every run waits for the maintainer's approval. The job runs only on `iso-v0` and `main`, the branches that move `:latest`. Acceptance: `system/tests/test_sign_images.py` (the bridge test: only the verified digests, by digest, under the other owner, and an invalid `--to` refused) and `scripts/verify.py ci workflows`.

**UD46. What is verified, and what is not.** A machine that follows the previous owner unverified, which is every machine installed before the transfer, pulls the bridge through its unverified reference, on the same terms as every image it pulled before: that one download is not verified. Nothing upgrades an unverified machine automatically (`bootc-fetch-apply-updates.timer` is disabled), so the bridge arrives at the next `bootc upgrade` or `rpm-ostree upgrade` the user runs. From the boot of the bridge on, its policy is in force, and the move of UD47 is a `bootc switch --enforce-container-sigpolicy`, verified against the `ars-regia` keys. While the bridge tag and `ars-regia:latest` name the same digest, that switch downloads no new layer and verifies, after the fact, the bytes the machine already runs _(to verify on the dev VM)_. A machine whose origin already enforced a policy that pins the previous owner cannot pull the bridge, whose signatures name `ars-regia` only; none is known (`scripts/switch-verified.sh` exists since 2026-10-07, after the transfer). Such a machine uses `scripts/switch-verified.sh`, or the maintainer also signs the bridge digests for the previous owner (`system/sign-images.sh --registry ghcr.io/hr-mes`, one signing approval) _(Proposal, not built)_.

**UD47. The client follows the owner the policy pins.** `athanor-update migrate`: when the booted repository is an image of a previous owner listed in `/usr/share/athanor/containers/moved-from` (UD45), is not a scope of the shipped policy in force, and exactly one scope has the same registry host and image name (`policy::successor`), the target is that scope with the tag or digest the booted reference names (`:latest` stays `:latest`, `@sha256:...` stays that digest). The image name maps the variant: `athanor-system`, `athanor-system-nvidia` and `athanor-system-nvidia-legacy` each move to the same name; any other name, any other owner (a fork included), another registry host, or two matching scopes stay `reference-out-of-scope`. The move also applies to an enforcing origin out of scope, which is not the signed reference of its image. UT5 holds: an older build is refused (`channel-older-than-booted`), a newer one is locked as an update for the user (UT6), the same build is staged for the next restart. The switch has to stage the target with the policy enforced, or the run fails; the signature object of the new owner is then fetched for the check. The bridge moves only `:latest`: a machine on a run-number tag of the previous owner finds no such tag under the new owner and waits in `successor-absent` (UD49); `scripts/switch-verified.sh` with the `:latest` reference is its way across, and it is then pinned no longer (UD51). Acceptance: `cargo test -p athanor-update` (`migrate::tests`, `policy::tests`).

**UD48. What the user sees.** From the boot of the bridge until the switched deployment boots, the check publishes `verified.reason = owner-moved`, and the shield reads "Not verified: the project moved; this machine is moving with it". While nothing can move it (`successor-absent`, or a queued rollback) the reason is `owner-moved-waiting`, and the shield reads "Not verified: the project moved; this machine cannot follow it yet". While the image under the new owner is the held digest, or one whose deployments failed to boot twice (UD49), the reason is `owner-moved-held`, and the shield reads "Not verified: the project moved; this machine went back and waits for a newer version". The update state is "will apply at next shutdown" (same build) or the notifier's "update ready" (a newer one). After the restart the reference is `ostree-image-signed:docker://ghcr.io/ars-regia/<image>:<tag>` and the shield reads verified. Acceptance: `cargo test -p athanor-update` and `-p athanor-trust-state`; the dev-VM stage `migrate` _(to extend)_.

**UD49. Failure behaviour.** Network or registry down: the unit fails, and `Restart=on-failure` starts it again after five minutes. The image or tag absent under the new owner (`manifest unknown`, `name unknown`, or the answers ghcr.io gives for a package that does not exist: 403 on the pull token, `denied`): a wait, `successor-absent`, exit 0, no stamp, and the migration timer starts it again every six hours; the state reads `owner-moved-waiting`. A machine on a run-number tag of the previous owner waits here for good, since the bridge tags only `:latest`; `scripts/switch-verified.sh` moves it. A signature the policy refuses: the switch fails, nothing is staged, no stamp. Layered packages or a queued rollback: the move waits, as UT4's migration does. The move decides from the deployments, not from the stamp: a machine enforcing on a reference in scope is done and writes the stamp; a staged deployment of the target with the policy enforced waits for the restart (`restart-pending`); anything else switches again. A switch that returns without staging the target fails (code `internal`) and writes nothing. A stamp on an image of the previous owner, written while that owner was the policy's, is ignored, so neither `athanor-update-migrate.service` nor its timer has a condition on the stamp; on any other image the unit ends at the stamp without asking bootc, as it does whenever `moved-from` is empty.

The move respects the held digest of UT6, which nothing releases: the migration never stages it. A user who goes back after the move (`GoBack()`), or greenboot's rollback of the moved deployment (the check of that boot holds the booted digest), leaves the machine on the previous owner with the new owner's digest held. The migration then waits (`held`, exit 0), writes `move-held` under `/var/lib/athanor-update`, and the state reads `owner-moved-held` (UD48). A switched deployment that does not boot at all leaves nothing held, so the migration counts failed boots of each digest in `move-record`, and only failed boots: when the deployment it staged last is gone at the next run, it counts one if that deployment is now the rollback deployment (deployed, then left), and none if it vanished unbooted, as a power cycle discards a staging before it is finalized (a newer build locked until the person presses Apply, or a hard power-off before a clean shutdown; spike U1, section 4). The deployment is named by its ostree checksum and deploy serial, so a deployment that failed earlier and stays the rollback is not counted again. When bootc does not report both, nothing is counted, since the digest alone cannot tell a failed boot from a discarded staging: a deployment that does not boot is still left by greenboot, whose queued return holds the digest (UT6), and a held digest is never staged again. The record is removed with the stamp, so a return after a move that booted is not counted either. After two failed boots of one digest (`migrate::ATTEMPTS`) the migration stops staging it (`boot-failed`, exit 0), writes `move-held`, and the state reads `owner-moved-held`. That record is the migration's own: it never writes the held file, so a digest the person went back from stays held. In every case the move resumes by itself once the tag under the new owner names a newer build, which is not held: it is staged and locked as an update for the user. The shield offers no retry of the held digest (the repair from the shield is decided and not built, UT4); an administrator retries it with `sudo bootc switch --enforce-container-sigpolicy ghcr.io/ars-regia/<image>:<tag>`, the tag the machine follows, and the migration writes the stamp once that deployment boots.

**UD50. When the bridge stops.** The maintainer clears `ATHANOR_BRIDGE_REGISTRY` once every machine known to follow the previous owner (the desktop, the laptop and the dev VM) reports `.verified.state == "verified"` on an `ars-regia` reference, plus a grace period for machines nobody knows of _(Proposal: 90 days, the window of UT10)_. The last bridge stays at the previous owner's `:latest`; nothing deletes it. It keeps working while the `ars-regia` images are signed with a key its policy holds (keys 1 and 2): a rotation away from both ends the bridge for machines that have not moved yet, which then need `scripts/switch-verified.sh`.

**UD51. A verified machine on a run-number tag is pinned, not up to date.** A run-number tag (`:37691917204`) or a digest names one build: nothing newer is ever published under it, so the check finds no update while `:latest` moves on. The dev VM ended there (2026-10-08, runtime audit RT-U7): `scripts/switch-verified.sh` refused a digest, the run tag was the reference left, and the state read verified with `update: none`. The migration stops at a verified booted reference whatever its tag (UT4) and does not move a pinned machine, since a pin can be deliberate. The check publishes `verified.reason = pinned-build` instead (badge at the exclamation mark), and the shield reads "Signed, but pinned to one build: it receives no updates". The check reads a reference as pinned when it is a digest or a run tag of the pipeline (`github.run_id`, digits only); any other tag, `latest`, `stable`, no tag (which is `latest`) or the moving tag of a derived image (`:prod`), follows newer builds. `scripts/switch-verified.sh` accepts only a channel tag and, on a run tag or a digest, says why and names the `:latest` reference to use. Acceptance: `cargo test -p athanor-update` (`migrate::tests`) and `scripts/tests/test_switch_verified.py`.

## 6. Upgrade test in CI (priority 2)

**UD17. ISO acceptance tests the run, not the newest ISO, and runs whatever `lint` says.** `iso-acceptance.yml` gains a `workflow_call` input `run_id` and installs `athanor-iso:<run_id>`. The orchestrator calls it after the tagging job of UD25, on every run that publishes system images. The scheduled weekly run keeps `newest` and loses `needs: [lint]`: `lint` is the pull-request gate (ADR-0075), and a lint failure on `iso-v0` must not hide a boot regression. `verdict.py` writes the evidence file of UD4. Acceptance: one orchestrator run whose ISO acceptance reports the same `run_id` and digest as its `image-digests.txt`.

**UD18. The upgrade test boots the previous release and upgrades it to the candidate through the client's own path.** A new job `upgrade-acceptance` in `iso-acceptance.yml`, on `ubuntu-24.04` with `./.github/actions/kvm`, runs after the ISO acceptance of the same run:

1. **From.** It installs the ISO of the current `:stable` run (UD8 keeps it). Until `:stable` exists, it installs the ISO of the newest earlier run whose signature verifies.
2. **Seed.** As the test user it writes a file under `/home`, a file under `/var/lib/athanor-upgrade-probe/`, and an edit to `/etc/athanor-upgrade-probe.conf` (a file the image ships).
3. **Serve the candidate.** The runner starts a local registry, copies the candidate digest and its `sha256-<hex>.sig` attachment into it under the canonical repository name and tag `stable`, and the guest gets a `registries.conf.d` drop-in that maps the canonical repository to the runner (`10.0.2.2:5000`). The signature identity is the canonical name, so the guest's shipped policy verifies it unchanged.
4. **Upgrade.** The guest's own units do the work: `athanor-update-check.service` downloads, and `Apply()` is called over the bus. No `bootc` command is typed in the guest (as in `run.sh`, item 14 of UT acceptance).
5. **After reboot.** The booted digest is the candidate. The session checks of `verdict.py` pass (greeter, session, settings, no panic). The state reads verified. The three seeded files are intact, and the `/etc` edit survived the three-way merge.
6. **Rollback.** `GoBack()` is called, the guest reboots, and the booted digest is the "from" digest. The state names the candidate as held, and the seeded files are intact.

It writes the `upgrade-acceptance` evidence file of UD4. Acceptance: the job is green on two consecutive orchestrator runs before UD20 makes it required.

**UD19. The harness serves both the dev VM and CI.** The stage functions and helpers of `scripts/devvm/acceptance/lib.sh` (`expect`, `guest_ssh`, `reboot_guest`, `wait_ssh`) are reused. `run.sh` gains `--ci`, which selects the stages `seed upgrade check goback check-back`. `guard_no_ci` applies only to the dev-VM mode: it protects the self-hosted host, and a hosted runner shares no host with anything. `forge/test/iso/run_iso_test.sh` gains an SSH port forward, and `collaudo.ks` gains a per-run public key and a passwordless sudo rule for the test user. Both belong to the test VM only, like its serial console. Acceptance: `bash scripts/devvm/acceptance/run.sh --ci` refuses to run outside a CI environment, and the dev-VM mode still refuses beside a CI run.

**UD20. Both acceptances are required promotion gates.** They are required through UD4, not as branch protection, because they test a digest, not a pull request. Acceptance: UD4's tests.

**UD21. The test fits a hosted runner.** Budget _(estimate)_: install 15 to 17 min (measured on ISO acceptance), download and apply 10 min, two reboots and checks 10 min; `timeout-minutes: 60`. Disk: one ISO, one qcow2 at 40 GiB sparse, and one candidate copy in the local registry (4.1 GB). The job records `df` at each stage in its summary, and a stage that would leave less than 4 GB free fails with that message. Acceptance: three consecutive runs under the timeout, with the free-space record.

## 7. Machines verify signatures (priority 3)

The policy, keys, registries.d and verification are UT2, UT3 and UT5. This section fixes the order in which the pipeline makes them true.

**UD22. Every system image carries the policy of UT3, and the build fails without it.** `verify.py shipped` checks the `/etc/containers/policy.json` link to `/usr/share/athanor/containers/policy.json`, `registries.d/athanor.yaml` with `use-sigstore-attachments: true`, `keyPaths` naming `/usr/share/athanor/keys/athanor-image-1.pub`, and `default: reject`. The system image check job runs the same checks against the built image (`podman run --rm <image> ...`) before push. Acceptance: `python3 scripts/verify.py shipped`, and the image check job.

**UD23. Every switch and upgrade enforces the policy.** The client already passes `--enforce-container-sigpolicy` to `switch` (`tools.rs:264`); once a machine boots `ostree-image-signed:`, `upgrade` keeps enforcing. No code path calls `bootc switch` without the flag; `cargo test -p athanor-update` asserts the argument vector of every `bootc` call. Acceptance: that test.

**UD24. The build job pushes by run id only.** "Push OS Images" pushes `:<run_id>`, never `:latest`. `image-digests.sh` records the digests. Acceptance: UD1's check.

**UD25. Sign, verify, then tag.** The order of a release run becomes:

1. the build job pushes `:<run_id>` and writes `image-digests.txt`;
2. `sign-system-images` (the `signing` environment, the key alone in its job, UT2) signs by digest, then writes the `signature` evidence file;
3. `verify-system-images` (no environment, no secret) pulls each digest anonymously through the shipped policy (`system/verify-images.sh`); it runs also when signing was skipped, so an unsigned image fails the run;
4. `tag-system-images` (no key) copies `docker://<repo>@<digest>` to `:latest` and re-reads it (`system/tag-images.sh`).

The ISO is still built in the build job from `:<run_id>`. Acceptance: in one run, the registry's first write of `:latest` (from the job logs) comes after the `sign-system-images` job finishes. `sign-images.sh` addresses digests, not tags (its tag comparison of today stays as a guard).

**UD26. The transition from unverified references needs no step of its own.** UT4's migration moves every machine whose image carries the policy. A machine on an image older than the policy reaches one through its normal unverified `upgrade` of `:latest` until `:latest` names an image with the policy, then migrates. Images pushed to `:latest` after UD25 are all signed, which closes the unsigned window of F2. Acceptance: dev-VM stage `migrate` (exists).

**UD27. A key rotation is a sequence of releases, each promoted with evidence.** Release _R1_ ships key _n+1_ in `keyPaths` and is signed with _n_. _R1_ is promoted, and its dwell elapses. Release _R2_ is signed with _n+1_ (UD7 refuses _R2_ before _R1_ is `:stable`). Key _n_ leaves `keyPaths` one release later (UT3). Acceptance: dev-VM stage `rotate` (exists) plus UD7's test.

## 8. Lighter updates: reproducible inputs (priority 4a)

A reproducible build is the precondition for small updates: a layer whose bytes change without a source change is downloaded by every machine.

**UD28. Every build input is pinned by digest or hash.**

| Input                          | Today                                   | Pinned as                                                                                                                                                                                         |
| ------------------------------ | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tier repositories              | `@sha256` from the run's `tier-digests.json` (`publish_tiers.sh`; `tier-digests.sh` outside the Orchestrator), unsigned | `@sha256` from a lock file the DAG writes when it publishes a tier (`tier-digests/tier-digests.json`, a file of the run, never committed), read as build arguments. The `${FORGE_REGISTRY}` literal exception of RA-12 is unchanged |
| Fedora packages installed live | `dnf install` from rolling repositories | NEVRA plus SHA-256 lock, served from an OCI mirror by digest, as the NVIDIA RPM mirror already does; a bump bot opens the update pull request                                                     |
| `cosign`, `syft`               | `nixpkgs#cosign`, `nixpkgs#syft`        | the repository's pinned flake (A2-13)                                                                                                                                                             |
| Base image                     | `@sha256` (already)                     | unchanged; the bump bot of A2-17                                                                                                                                                                  |
| `bootc-image-builder`          | `:latest`                               | `@sha256` in `build_iso.sh`, with a bump bot (UD43)                                                                                                                                               |
| `podman build --pull=newer`    | floats                                  | `--pull=missing`; every `FROM` is a digest                                                                                                                                                        |

Acceptance: `python3 scripts/verify.py paths` _(extended)_ fails on any `FROM`, `--mount=...from=` or `podman run` image in `system/` and `forge/scripts/` without `@sha256:`, except the RA-12 tier stages, which must take their digest from the lock.

**UD29. Timestamps come from the commit.** `build-image.sh` exports `SOURCE_DATE_EPOCH` as the commit time of `HEAD` and passes `--source-date-epoch "$SOURCE_DATE_EPOCH" --rewrite-timestamp` to `podman build` (podman 5.8.4 documents both). `org.opencontainers.image.created` becomes the commit time. A rebuild of the same commit is therefore not newer, and `promote.sh` refuses it, which is correct when every input is pinned: a change reaches machines through a commit, a bump pull request included. The serial of UT9's version label changes from the run number to the commit count of `HEAD` (decision 5 of section 15). Acceptance: two builds of the same commit report the same `created` and version labels.

**UD30. The UKI and initramfs stay deterministic.** `assemble_uki.sh` keeps `dracut --reproducible` and runs under the same `SOURCE_DATE_EPOCH`. Any signature in the boot chain that embeds a time is listed in UD31's exceptions with the reason. Acceptance: UD31.

**UD31. A reproducibility check runs weekly and fails on drift.** A hosted job rebuilds the default variant from the commit of the current `:stable` and compares the layer diff_ids with the published image. It fails on any differing layer not on an exception list kept beside the script, each entry with its reason and an issue. It uses `retry.sh` for every download (the weekly kernel check never completed, curl error 23). Acceptance: the job is green, or red with a named layer, on three consecutive weeks.

## 9. Lighter updates: smaller downloads and fewer reboots (priority 4b to 4d)

### 9.1 Rechunking

Upstream state, checked on 2026-10-06:

- **rpm-ostree `compose build-chunked-oci`** was added as experimental in v2025.2 (2025-01-23) and stabilised in v2025.7 (2025-03-28, PR #5326). It gained `user.component` xattr grouping in v2025.11 (2025-09-10) and fixed file labelling in v2025.12 (2025-11-03, PR #5502). Its documentation recommends `--format-version=2` for reproducible output and reuses the layers of a previously chunked image named as `--output`. The host has rpm-ostree 2026.1, whose help defaults to 64 layers and notes that podman 5 handles more than 200.
- **`bootc-base-imagectl rechunk`** (fedora/bootc/base-images, `main`) wraps it: `rpm-ostree experimental compose build-chunked-oci --max-layers=N --bootc --format-version=1`, or chunkah with `--chunkah`.
- **chunkah** (coreos/chunkah, created 2025-12-17, v0.7.1 of 2026-10-05) is a distribution-agnostic successor. It groups by RPM database and `user.component`, supports `SOURCE_DATE_EPOCH`, and defaults to 64 layers. Universal Blue's image template labels it "adventurous" and leaves the chunkah image unpinned ("once mature enough").
- **Universal Blue**'s image template (workflow updated 2026-08-27) runs `rpm-ostree compose build-chunked-oci --max-layers 127 --format-version=2 --bootc` by default. Bluefin's `Justfile` pins `ghcr.io/ublue-os/legacy-rechunk:v1.0.1` by digest. Aurora carries chunkah build scripts (seen in code search, not read in full).
- **Fedora Atomic desktops**, our base, publish images already chunked by `rpm-ostree compose image` (base-atomic: 97 layers).

**UD32. The system image is rechunked with rpm-ostree `compose build-chunked-oci`, format version 2.** _(Proposal: chunkah is re-evaluated when Universal Blue's template makes it the default.)_ The `system` stage (UD40) is built, then rechunked with `--format-version=2 --bootc --max-layers 120`. The previous `:stable` default variant is pulled into local storage under the output name first, so that unchanged components keep their layer digests. The variant stage is then built `FROM` the rechunked digest, adding at most 6 layers. Labels survive (v2025.7, "preserve labels"); UD22's checks run on the rechunked image. Acceptance: `skopeo inspect --raw` of each variant shows at most 126 layers, and the labels of UT9 are present.

**UD33. The first rechunked release is announced as a full download.** Layer identities change once, so each machine downloads the whole image once: 4.1 GB on the default variant and 4.7 GB on NVIDIA. The release notes and the update notice say so; a metered connection defers it (UT12). Acceptance: UD34 records it.

### 9.2 Bytes downloaded per upgrade

**UD34. Every promotion candidate records what an upgrade from `:stable` costs.** `system/upgrade-bytes.sh` reads the digests file of the build job (`image-digests.sh`) and sums the compressed sizes of the candidate's layers whose digests are absent from the from-image, for each variant. The from-image is `:stable`; until the first promotion it does not exist and `:latest`, read before the run moves it, stands in; the file names which one was measured. It also records whether the kernel changed (the `ostree.linux` and `io.athanor.azoth-boot.digest` labels). It writes `artifacts/metrics/upgrade-bytes.json`, and the job summary prints it. The orchestrator keeps the series. Target _(Proposal)_: a routine update (no base bump, no kernel change) at most 400 MB on the default variant, and a base bump at most 1.2 GB, set firmly after ten measured pairs. Acceptance: the file exists for every candidate; the target is reported, not enforced, until the maintainer sets it.

### 9.3 Soft reboot

bootc introduced soft reboots hidden in v1.7.0 (2025-08-25, PR #1392) and un-hid `--soft-reboot` in v1.8.0 (2025-09-05, PR #1580). v1.11.0 (2025-12-05, PR #1768) refuses on SELinux policy changes. The shipped bootc 1.16.13 offers `upgrade --soft-reboot required|auto`, and ostree 2026.2 has `admin prepare-soft-reboot`. A deployment qualifies only when its kernel, initramfs and kernel arguments are unchanged (`softRebootCapable` in `bootc status`). A spike on the desktop recorded `false`.

**UD35. `Apply()` restarts userspace only when the staged deployment qualifies.** _(Proposal: mechanism.)_ When `softRebootCapable` is true for the staged deployment, `Apply()` prepares `/run/nextroot` and asks logind for a reboot with the soft-reboot flag that falls back to a full reboot when no next root is prepared (`RebootWithFlags` with `SD_LOGIND_SOFT_REBOOT_IF_NEXTROOT_SET_UP` _(to verify on systemd 258)_). Otherwise it behaves as UT6. It never calls `bootc upgrade --apply`, because UT6 requires logind's inhibitor check. Spike S1 (section 11, start of P4) verifies that bootc 1.16.13 can prepare without `--apply`, and that staged finalisation happens across a soft reboot. Acceptance: the dev-VM stage `soft-apply` _(new)_ applies a userspace-only update and observes the same `/proc/sys/kernel/random/boot_id` with the new deployment booted.

**UD36. A changed kernel module tree or SELinux policy means a full reboot.** A soft reboot keeps the running kernel and its loaded modules. The NVIDIA kernel modules live in the deployment, not in the boot checksum, so a driver update with an unchanged kernel would pair old modules with new userspace. `Apply()` therefore treats the deployment as qualifying only when `softRebootCapable` is true **and** the ostree directory checksum of `/usr/lib/modules` is equal in the booted and staged commits **and** bootc reports no SELinux policy delta. Acceptance: a unit test of the decision function, and the dev-VM stage of UD35 with a module-only change, which must fully reboot.

**UD37. The user sees which kind of restart is coming.** The state file of UT7 gains `apply_kind: "userspace" | "full"`, an additive field. The notifier and the shield say "Restart the desktop to update" for `userspace` and "Restart to update" for `full`. A security-class update (UT13) applies at the shutdown the user starts and is unaffected. Spike S1 measures the time to the greeter for both kinds on the dev VM. Acceptance: `athanor-trust-state` parses both values, and the dev-VM stage checks the text.

Soft reboot helps only when consecutive images keep the same kernel and initramfs. UD34's metric therefore also records whether the boot checksum changed. If most pairs change it, the cause is investigated before more work goes into the soft-reboot path.

### 9.4 What goes where

**UD38. Each layer of the system has one delivery mechanism.**

| Content                                             | Mechanism                                                                                                                       | Update                          |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| Kernel, drivers, system services, the desktop shell | the signed bootc image                                                                                                          | `stable` channel, A/B, rollback |
| Graphical applications                              | Flatpak                                                                                                                         | per application, no reboot      |
| Command-line tools                                  | per-user Nix, CLI only (A2-16)                                                                                                  | per user, no reboot             |
| Optional system extensions                          | signed sysext, **after 1.0**, with an image policy; until then `systemd-sysext` and `systemd-confext` are disabled in the image | —                               |

The stage-5 inventory of image components gives 24 to keep in the image, 33 obsolete under ADR-0073 (removed through that record, not here), and 1 to move to nixpkgs. Acceptance: the image check job fails if `systemd-sysext.service` or `systemd-confext.service` is enabled without an image policy.

### 9.5 Decision recorded: system components do not move to Nix

**UD39. No system component is delivered through Nix.**

- **Signature.** The bootc image signature covers every system file; a Nix store path on a machine is trusted through `cache.nixos.org`, outside the image key.
- **Rollback.** `GoBack()` would roll back the image and leave the Nix profile, so the two halves could disagree.
- **SELinux.** Store paths carry no labels from the image policy.
- **Reboots.** The reboots that remain are kernel, initramfs and module changes, which Nix cannot deliver either.

The download saving is bounded at about 820 MB, or 11% (stage-5 measurement), which rechunking addresses without these costs. Acceptance: `verify.py shipped` fails if a systemd unit shipped in the image has an `ExecStart` under `/nix/`.

## 10. Efficient, pinned pipeline (priority 5)

**UD40. The `system` stage is built once per run, and the variants derive from its digest.** The build job builds and rechunks `system` once (UD32), then builds `gpu-none`, `gpu-nvidia` and `gpu-nvidia-legacy` `FROM` that digest. Acceptance: the three variants' manifests share every layer digest of the rechunked `system` image (`skopeo inspect` diff_ids); the job time is recorded against the 43 to 61 min of today.

**UD41. The brain asks the registry what is built.** For each node, `dag_orchestrator.py` computes the content hash of `check_idempotency.sh` and asks the registry whether `<node image>:hash-<hash>` exists (`skopeo inspect`, through `retry.sh`). If it exists, the node is clean, and the publish step adds that tag. The `.cache` directory and Redis are removed. Acceptance: an orchestrator run on a documentation-only commit schedules zero build matrix jobs; a test of the brain with a stub registry covers both answers.

**UD42. The builder hash covers only the builder's inputs.** `config/packages.json` leaves `check_idempotency.sh`'s builder hash, because the builder does not read it. Acceptance: a test edits `packages.json` and asserts an unchanged builder hash, and asserts a changed hash for `flake.lock`.

**UD43. Every external image the pipeline runs is pinned, and bumps arrive as pull requests.** `bootc-image-builder` is pinned by digest in `build_iso.sh`. The existing Nix registry bump workflow is extended, or a sibling is added, to open a pull request when a pinned image or the flake moves. The `flatpak` job, a no-op that ends in `|| true`, is removed. Acceptance: UD28's `verify.py paths` check; `grep -n '|| true' .github/workflows/call-system-image.yml` finds nothing.

**UD44. Tier repository images are signed, and the system build verifies them before use.** `forge/scripts/publish_tiers.sh` pushes `:<run_id>` and `hash-<hash>`, records the digest in the run's `tier-digests/tier-digests.json` (UD28), and signs keyless by digest. The system image job verifies each tier digest with `cosign verify` against the workflow identity before building. RPM signing is removed (decision 6 of section 15): the dead `RPM_GPG_KEY` branches go away, because RPMs reach a machine only inside the signed image, ADR-0076 removed the DNF channel, and a signature that is silently skipped protects nothing. Acceptance: the system image job fails when a tier digest's signature does not verify (tested once with a tampered digest on a branch).

### 10.1 Kernel builds: measure before moving

The kernel builds on the self-hosted runner (F16). A hosted build is estimated at 3 to 4.5 hours, and the 14 GB of free disk on a hosted runner is the first blocker. The move is deferred. No compiler cache is used (`doc_kernel_build.md` item 9): it would only speed up a rebuild of the same NVR, and the `inputs` job already skips a build whose inputs match the published kernel. The move is revisited only when a hosted runner is measured building the kernel within the job time limit. This is a sequencing decision, not a requirement of this document.

## 11. Phases

Each phase ends at a gate. The next phase does not start until the gate is green, except P4 and P5, which may run in parallel with P3.

| Phase                                              | Contents                                                         | Exit gate                                                                                                                                                                                                                        | What a user sees                                                                                   |
| -------------------------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| **P0. Repairs**                                    | UD9, UD15 steps 1 and 2, UD24, UD25, UD3, UD43 (builder pin)     | one orchestrator run with `:latest` written after the signature job; `test_promote.sh` green; the desktop shows a non-null image and no layered packages; the migrate unit shows 0 restarts in an hour on a VM without `:stable` | no more failed-unit noise; the desktop updates again                                               |
| **P1. Evidence**                                   | UD17, UD18, UD19, UD21, UD4 (evidence files)                     | two consecutive orchestrator runs with green ISO and upgrade acceptance on their own digests                                                                                                                                     | nothing                                                                                            |
| **P2. `stable` exists**                            | UD1, UD2, UD5, UD6, UD7, UD8, UD10, UD16, UD20, UD22, UD23, UD27 | `:stable` created by `promote.sh` with evidence, then one automatic promotion; a dev VM installed from the ISO follows `:stable` signed                                                                                          | machines move once to `stable`, one download, and the shield turns verified                        |
| **P3. Namespace** (after TO5, the republication) | UD11, UD12, UD13, UD14, UD45 to UD51 | the desktop and the laptop are verified on an `ars-regia` reference; the bridge is stopped by UD50 | one switch per machine, done by the maintainer or, behind the bridge, by the migration |
| **P4. Lighter**                                    | UD28 to UD39                                                     | five candidates with `upgrade-bytes.json`; the weekly reproducibility check green or with named exceptions; a soft reboot observed on the dev VM                                                                                 | one full download (UD33), then smaller updates; "Restart the desktop" when the kernel is unchanged |
| **P5. Pipeline**                                   | UD40, UD41, UD42, UD44, section 10.1                             | a documentation-only commit schedules zero matrix jobs; variants share the `system` layers; tier signatures are verified in the system build                                                                                     | nothing                                                                                            |

The transfer to `ars-regia` took place on 2026-10-06, so P2 creates `:stable` under `ars-regia` only; no `hr-mes:stable` is ever published. Spike S1 (soft reboot, UD35 to UD37) runs on the dev VM at the start of P4.

## 12. Interfaces and failure behaviour

| Interface                                     | Owner                       | Failure behaviour                                                                                                               |
| --------------------------------------------- | --------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `image-digests.txt` (`REPOSITORY TAG DIGEST`) | build job                   | missing or malformed: signing and tagging refuse (exists)                                                                       |
| `artifacts/evidence/<gate>.json`              | each gate's script          | absent, `fail`, or another digest: `promote.sh` refuses, names the gate, exits non-zero                                         |
| `artifacts/promotion.json`                    | `promote.sh`                | a write failure after the copy fails the job; `:stable` is re-read and reported                                                 |
| `tier-digests/tier-digests.json` (run-local)  | DAG publish                 | a digest without a verifying signature: the system build fails                                                                  |
| `artifacts/metrics/*.json`                    | gates                       | reported, never a gate until the maintainer sets a target                                                                       |
| state file `apply_kind`                       | `athanor-update`            | absent (older client): the notifier says "Restart to update"                                                                    |

A failed promotion leaves `:stable` where it was: the copy to `:stable` is the last write. The bridge of section 5.2 fails as UD49 says.

## 13. Changes to other documents

- `doc_update_trust.md`: UT7 gains `apply_kind` (UD37). UT9's serial becomes the commit count of `HEAD` (decision 5). UT10 keeps the promoted ISOs and `:stable-<date>` (UD8). Acceptance items for UD16 and UD35 are added.
- `doc_ci.md`: the release path becomes CI1 build → sign → tag `latest` → CI12 ISO and upgrade acceptance on the run → CI11 automatic promotion after the dwell, with manual override.
- `transfer-to-organisation.md`: TO6 becomes UD11 (by digest, verified out of band, never `:latest`); TO8 cites UD14.
- `doc_kernel_build.md`: item 9 gains the hit-rate record of section 10.1.
- `docs/operations/`: the desktop recovery (UD15) and the layering rule (UD16).

## 14. Risks

| Risk                                                                                    | Effect                                         | Mitigation                                                                                                 |
| --------------------------------------------------------------------------------------- | ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Hosted runner disk or time is too small for the upgrade test                            | P1 cannot close                                | UD21's free-space record; the qcow2 is sparse; the ISO is deleted after install                            |
| Automatic promotion ships a regression the virtual GPU cannot show (NVIDIA)             | NVIDIA machines break after the dwell          | the `hardware` evidence of UD4; one-step `GoBack()`; greenboot holds a bad deployment (UT13)                              |
| Rechunking drops xattrs, SELinux labels or labels                                       | an image that boots wrong or loses its version | UD22 and both acceptances run on the rechunked image; UD32's label check                                   |
| The first rechunked release costs every machine a full download                         | metered users and slow links                   | UD33's notice; UT12 defers on metered connections                                                          |
| A soft reboot pairs old kernel modules with new userspace                               | no GPU after the update                        | UD36's module-tree comparison                                                                              |
| Pinning live packages slows security fixes                                              | a fix waits for a bump pull request            | the bump bot runs daily; a security fix can be promoted through the manual override after its evidence     |
| Key compromise                                                                          | unchanged from UT2: not recoverable remotely   | unchanged                                                                                                  |

## 15. Decisions of the maintainer (2026-10-06)

1. **Dwell before automatic promotion: 24 hours** from the last evidence file, restarted by a newer complete candidate. The maintainer's machines on `:latest` use a release for a day before anyone else (UD5).
2. **The NVIDIA variants promote only with `hardware` evidence** written by a maintainer machine on `:latest` after a session has started on that digest; without it the default variant promotes alone (UD4).
3. **Automatic promotion is always of the feature class and uses no key.** A security-class promotion goes through the manual `promote.sh` in the `signing` environment, which signs the attestation; that approval is needed only when a release answers an advisory (UD6, A2-26, A2-27).
4. **No bridge**, superseded on 2026-10-08 by decision 7. The policy still never carries `hr-mes` scopes (UD12).
5. **The version serial is the commit count of `HEAD`**, so the label is a function of the source (UD29).
6. **RPM signing is removed** (UD44): RPMs reach a machine only inside the signed image, which verifies the signed tier digests.
7. **A bridge, 2026-10-08.** Machines installed before the transfer report `update: none` because the previous owner's `:latest` is frozen. A signed bridge image under the previous owner moves each one, verified at first boot, to its variant under `ars-regia`, and the shield says so (section 5.2, UD45 to UD50). The maintainer's desktop moves by `scripts/switch-verified.sh` (UD15).
