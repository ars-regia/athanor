# ISO acceptance under Secure Boot ending in an update to a newer `:stable`: implementation plan

> For the implementer: execute task by task (`superpowers:executing-plans`); each task ends in one commit. The specification is approved and is implemented as written (ADR-0074); a deviation found while working stops the task and goes to the maintainer.

**Requires ADR-0130 (UD18 steps 2 and 3).** It amends step 2 to name the seeded `/etc` file and step 3 to make the `.att` attachment mandatory. Both were decided by the maintainer on 2026-10-23. Until that record is merged, nothing below is implemented (Task 0).

**Goal.** This plan closes the 0.9 exit item of `docs/operations/release-1.0.md` section 4.1, "the ISO test under Secure Boot firmware ending in an update to a newer `:stable`", by implementing UD18 and UD19 of `doc_update_delivery.md` rev 3 (approved 2026-10-09), as amended by ADR-0130.

A new job `upgrade-acceptance` in `accept.yml`:
1. installs the ISO of the current `:stable` on Secure Boot firmware;
2. seeds three files;
3. serves the run's candidate as `:stable` from a local registry;
4. lets the guest's own units download it and calls `Apply()`;
5. checks the updated system;
6. calls `GoBack()` and checks the "from" system again.

It writes `upgrade-acceptance` evidence.

**Architecture.** The job runs in `accept.yml` after the ISO acceptance of the same run, before any promotion: UD4 lists it as a promotion gate, and PL29 places it in `accept.yml`.

The drivers are:
- **The first boot** is driven by `console.py` as today, up to `settings-alive`. In upgrade mode it then ends its serial login and holds: it keeps reading the serial line and types nothing more.
- **The rest** is driven over SSH (UD19) by `scripts/devvm/acceptance/run.sh --ci`. It reuses `lib.sh`'s `expect`, `guest_ssh`, `reboot_guest`, `wait_ssh`, `wait_reboot`, `check_now`, `call_active` and `call_root`, and runs the stages `seed upgrade check goback check-back`.

How the session checks after each reboot are judged:
- `run.sh` fetches the probes of `console.py` (`--print-probe`), runs them over SSH, and turns their output into phases through `console.py`'s own `MARKERS` (`--classify`).
- `verdict.py` judges those phases with its existing greeter, session, settings, MOK and Secure Boot logic, under the prefixes `upgraded-` and `rolled-back-` (UD18 step 5).
- Digest, seed, attestation and held assertions fail inside `run.sh` with `FAIL  <text>`. `verdict.py` reports the first such line.

**Tech stack.** Bash (`run_iso_test.sh`, `lib.sh`, `run.sh`, one new script), Python 3 stdlib (`console.py`, `verdict.py`, tests), QEMU/OVMF secboot, distribution `registry` v3 (`scripts/ci/install-tools.sh`), skopeo, jq on the host, Kickstart.

**Spec.**
- `doc_update_delivery.md`: UD4, UD8, UD18 (steps 1-6, steps 2 and 3 as amended by ADR-0130), UD19, UD20, UD21.
- `doc_update_trust.md` rev 4: UT6 (`Apply()` counts every other session outside the uncounted classes, `GoBack()` is `auth_admin`), UT13 (every image is feature class until 1.0, so the update must reach `downloaded`).
- `doc_pipeline.md` PL29. `release-1.0.md` 4.1. ADR-0074.
- Code read:
  - `scripts/devvm/acceptance/{lib.sh,run.sh}`, `scripts/devvm/devvm.ks`;
  - `forge/test/iso/{run_iso_test.sh,console.py,verdict.py,collaudo.ks,test_verdict.py}`;
  - `scripts/ci/evidence.py`;
  - `athanor-update`: `serve.rs`, `requests.rs`, `check.rs` and `store.rs`.
- Not read: `scripts/devvm/devvm.env`. The local gate refuses to read `*.env` files, so the implementer reads it first.

**Global constraints (AGENTS.md).** No `|| true`, no `2>/dev/null` that hides an error, no `continue-on-error`. Logic lives in scripts and the YAML only calls them. English, Conventional Commits, no attribution. One logical change per commit.

Signing jobs and environments, `release.yml`, `promote-stable.yml` and the image (`system/`) are not touched. Every file below is owned by `@hr-mes` in `.github/CODEOWNERS`, so the implementation PR needs the maintainer's approval at the card's sha. One VM per runner: the job has its own hosted runner.

**Review focus: the failure modes, each named in the report and never passed.**
1. *Update offered but not applied.* `call_active Apply` answers an error (`Blocked`, `NotAuthorized`, `Busy`), or there is no reboot within 10 min. `reboot_into_digest` fails and quotes the reply.
2. *Apply() asks for an administrator because a session is still open.* A tty login is class `user` and counts (`serve.rs` `UNCOUNTED_CLASSES`). The console ends its serial login before `ready` (Task 3), and `stage_upgrade` fails with "a session on ttyS0 is open" if `loginctl` still lists one before `call_active`.
3. *Reboot into the old deployment.* The booted digest is compared with the candidate, never with "changed".
4. *MOK does not trust the new kernel.* shim or GRUB refuse to boot it: the passive console records `secure-boot-refused` (markers `bad shim signature` and `Security Violation`), and `wait_reboot` times out. If the system boots without enforcement, `upgraded-secure-boot` is false.
5. *Registry unreachable, or the attestation unread, reported as pass.*
   - A fallback from the mirror to ghcr.io, where `:stable` is still the "from" digest, leaves the state `none`. `expect` requires `.downloaded.digest == candidate` and prints `.last_error` and `.last_error_host`.
   - A missing `.att` only warns in `check.rs`, so the job asserts it separately (Task 4).
6. *Timeout.* QEMU's `TIMEOUT` covers the install bound plus the stages' bound, so QEMU never dies mid-upgrade (Task 5). Status 124 of either is reported by name.

---

### Task 0: Preconditions (read-only; measured 2026-10-23)
- **ADR-0130 is merged.** It amends UD18 step 2, naming `/etc/security/pwquality.conf.d/50-athanor.conf`, and step 3, making `.att` mandatory. The record is not part of this plan.
- `athanor-iso:stable` exists: `sha256:a8e6ba35…`, label `38796755145`. `athanor-system:stable` is `sha256:68d7c8c8…`, version `43.20261015.365`.
- Run 38796755145 is the Orchestrator at `311ad72e`, which contains #468 (`91f7d873`, Secure Boot case required). Its evidence file was not opened.
- The seeded `/etc` file is generated by `system/Containerfile:288-290`: `printf` writes the fixed lines `# Athanor (system/Containerfile).` and `minlen = 12`. So `:stable` ships it, and every candidate built from an unchanged line ships it identical. An administrator edits it to change the password length rule.
- `accept.yml` `install` runs on `ubuntu-24.04` with `./.github/actions/kvm`, as UD18 says: no divergence.

### Task 1: The test VM answers SSH (UD19)
- `forge/test/iso/collaudo.ks` gains two blocks, each between `# upgrade-acceptance begin` and `# upgrade-acceptance end`.
- **The commands block**, after `services --disabled=sshd` (line 32): `devvm.ks:29`'s `network --bootproto=static --device=link --ip=10.0.2.15 --netmask=255.255.255.0 --gateway=10.0.2.2 --nameserver=10.0.2.3 --activate --onboot=yes`. slirp's DHCP moved the lease from .15 to .16 across a reboot, which breaks the `-:22` forward (devvm.ks:23-28).
- **A last `%post --erroronfail`** that:
  - writes `@COLLAUDO_SSH_KEY@` to `~collaudo/.ssh/authorized_keys` (0600, owner collaudo, `restorecon -R`);
  - writes `/etc/sudoers.d/50-collaudo` with `collaudo ALL=(ALL) NOPASSWD: ALL` (0440), then `visudo -cf` on it;
  - runs `firewall-offline-cmd --zone=home --add-source=10.0.2.2` (the public zone has no ssh, ADR-0089/0116; as devvm.ks:47-48);
  - runs `systemctl enable sshd.service`.
- `run_iso_test.sh`, upgrade case only (`UPDATE_TO` set):
  - creates `keydir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/var/tmp}}/collaudo-ssh.XXXXXX")`, outside OUTPUT_DIR so the private key is never uploaded;
  - runs `ssh-keygen -q -t ed25519 -N '' -C '' -f "$keydir/id"`;
  - takes `cut -d' ' -f1,2` of the `.pub`, checks it against `^ssh-ed25519 [A-Za-z0-9+/]+=*$`, and substitutes it into its copy of the kickstart.
- In every other case the script deletes both blocks from the copy (`sed '/# upgrade-acceptance begin/,/# upgrade-acceptance end/d'`). Then `! grep -q -e @COLLAUDO_SSH_KEY@ -e upgrade-acceptance` must hold, or the script exits 2. The ISO acceptance keeps sshd disabled.
- The network becomes `-netdev user,id=n0,hostfwd=tcp:127.0.0.1:${SSH_PORT}-:22` in the upgrade case, with `SSH_PORT` a free port taken as in `cosign-attest-check.sh`.
- Check: `python3 scripts/verify.py kickstart installer install-channel` and `shellcheck forge/test/iso/run_iso_test.sh`.
- Commit: `test(iso): give the upgrade case's VM a per-run SSH key and passwordless sudo (UD19)`.

### Task 2: `console.py` ends its login, holds, and shares its probes
- With `ATHANOR_CONSOLE_HOLD=1`, `settings-alive` leaves `DECIDED`. After noting it, the console:
  1. types `exit`, clears `tail`, and waits for the getty's next `login:`;
  2. only then writes `OUTPUT/ready`.

  From then on it types nothing, keeps matching `MARKERS` and logging until the socket closes, and ignores the idle give-up.
- New marker pairs in `MARKERS`: `bad shim signature` → `secure-boot-refused`, and `Security Violation` → `secure-boot-refused`. verdict.py adds `secure-boot-refused` to its failure signals.
- New CLI modes:
  - `console.py --print-probe {MOK,GREETER,SESSION,SETTINGS}` writes that probe's bytes.
  - `console.py --classify PREFIX` reads stdin and prints `f"{PREFIX}{phase} {int(time.time())}"`, the `name epoch` format `verdict.py` reads from `phases.txt` (verdict.py:111-114). It goes through a new `phases_in(text: bytes) -> list[str]` that the main loop also uses.
- Tests are new functions in `forge/test/iso/test_verdict.py`, added to `main()`'s tuple. That file is a plain runner, not unittest; `Justfile` runs it.
  - `test_console_logs_out_then_holds`: the fake guest of `test_console_logs_in_opens_settings_and_stops`, with `ATHANOR_CONSOLE_HOLD=1`. Assert:
    - `exit` is the only thing typed after `SETTINGS_ALIVE`;
    - `ready` is absent until the fake guest sends `login:` again;
    - a later `bad shim signature` adds `secure-boot-refused`;
    - the console exits when the socket closes.
  - `test_classify_maps_probe_output`: `b"GREETER_ALIVE 3\r\nSECUREBOOT_ENABLED\r\n"` with `upgraded-` gives two lines, each matching `^upgraded-(greeter-alive|secureboot-enabled) \d+$`.
  - `test_print_probe_is_the_typed_probe`: the output equals `console.MOK_PROBE`.
- Run: `python3 -B forge/test/iso/test_verdict.py`. Expected: each `ok`, then `all checks passed`.
- Commit: `test(iso): end the serial login, hold the console and share its probes`.

### Task 3: `run.sh --ci` with the stages `seed upgrade check goback check-back` (UD18 steps 2-6, UD19)
- `run.sh` parses the mode before anything else: `ci=0; if [[ ${1:-} == --ci ]]; then ci=1; shift; fi`.
  - With `ci=1`:
    - it requires `CI == true`, otherwise `die "--ci runs only in a CI environment"`;
    - it sets `ACC_CI=1`, `STAGES=(seed upgrade check goback check-back)` and `start=${1:-seed}`;
    - it does not call `guard_no_ci`.
  - Otherwise `guard_no_ci` runs first, unchanged, with `start=${1:-install}`.
  - The existing check of `start` against `STAGES` runs after both.
- `--ci` requires these variables, each validated and each named when missing:
  - `ATHANOR_UPDATE_TO` (`^sha256:[0-9a-f]{64}$`);
  - `ATHANOR_UPDATE_REPOSITORY`;
  - `ATHANOR_UPDATE_MIRROR`;
  - `ACC_CI_OUT`, `SSH_PORT`, `ACC_SSH_KEY`.
- **Unverified:** that `devvm.env` honours `SSH_PORT`, the key and `GUEST_USER=collaudo` on a host without the dev VM. That is read first; if it does not, this task starts by making it so.
- `lib.sh`:
  - With `ACC_CI=1`, `tunnel` and `untunnel` return 0: the guest reaches the registry through the mirror drop-in at `10.0.2.2` (UD18 step 3).
  - `booted_digest()` reads `.status.booted.image.imageDigest` from `bootc status --format json`, parsed host-side with jq.
  - `reboot_into_digest CALLER METHOD DIGEST DESCRIPTION` is `request_and_reboot` with the booted digest in place of `marker()`, which exists only in dev-VM images.
  - `disk_floor` writes `df -h` to `$ACC_CI_OUT/df.txt` and fails under 4 GB free (UD21). Each stage calls it first.
  - `ETC_PROBE=/etc/security/pwquality.conf.d/50-athanor.conf` (ADR-0130). It is one variable, so a later amendment is a one-line swap.
  - `session_probes PREFIX`:
    1. For MOK and GREETER: `python3 forge/test/iso/console.py --print-probe X | guest_ssh bash -s | python3 forge/test/iso/console.py --classify PREFIX >> "$ACC_CI_OUT/upgrade-phases.txt"`.
    2. `type_keys` with the password and Return logs in at the greeter. It types through a uinput device in the guest (`scripts/devvm/keyboard_type.py`), where the first boot uses QEMU's monitor `sendkey`.
    3. SESSION and SETTINGS in the same way.
- The stages:
  - `stage_seed`:
    - saves `booted_digest` to `$ACC_STATE/from-digest`, and requires it to differ from `ATHANOR_UPDATE_TO` ("the candidate is :stable already");
    - saves the `sha256sum` of `/usr/etc${ETC_PROBE#/etc}`, the shipped copy;
    - writes a nonce to `~/upgrade-probe` and to `/var/lib/athanor-upgrade-probe/seed` (with `sudo install -D`);
    - appends `# upgrade-acceptance <nonce>` to `$ETC_PROBE`. A comment leaves the rule unchanged.
  - `stage_upgrade`:
    - writes `/etc/containers/registries.conf.d/99-upgrade-acceptance.conf` (`prefix` = `location` = the repository, one `[[registry.mirror]]` = `ATHANOR_UPDATE_MIRROR`, `insecure = true`);
    - stops `athanor-update-check.timer` and waits out a running check, as `stage_apply` does (#547), then runs `check_now`;
    - expects `.update downloaded` and `.downloaded.digest` = the candidate;
    - requires the stored attestation `/var/lib/athanor-update/attestations/<hex>` (`store.rs` `object_dir`) to exist, and the check's journal of this boot to hold no `release attestation could not be fetched` (ADR-0130). Otherwise: "the candidate's attestation was not read";
    - requires `loginctl list-sessions --json=short` (jq on the host) to list no session with `tty == "ttyS0"`;
    - runs `reboot_into_digest call_active Apply "$ATHANOR_UPDATE_TO" "UD18 4: Apply() from the active session boots the candidate"`.
  - `stage_check`:
    - expects `.verified.reason signature`;
    - checks the image name ends with `:stable` and `.signature` is `containerPolicy`;
    - requires the shipped copy's sum to equal the seed's, otherwise "the candidate changed the shipped probe file: the merge is not tested". Then it compares the three seeds with the nonce;
    - runs `session_probes upgraded-`.
  - `stage_goback`: `reboot_into_digest call_root GoBack "$(<"$ACC_STATE/from-digest")" "UD18 6: GoBack() boots the from digest"`. `call_root` is used because `GoBack()` is `auth_admin`, as in the dev VM's `stage_goback`. The mirror drop-in is not removed.
  - `stage_check_back`:
    - asserts the drop-in is present and the mirror's `:stable` is still the candidate. The "from" deployment's `/etc` kept the drop-in written before staging, so the candidate is still offered and `held` means the client refused it;
    - stops the timer, runs `check_now`, and expects `.update held` and `/var/lib/athanor-update/held` = the candidate;
    - checks the seeds;
    - runs `session_probes rolled-back-`.
- Tests in a new `scripts/tests/test_acceptance_ci.py` (unittest, with stub `systemctl`, `gh` and `ssh` on a temporary PATH):
  - `test_ci_mode_refuses_outside_ci`;
  - `test_ci_mode_names_a_missing_variable`;
  - `test_ci_mode_rejects_a_dev_stage`: `--ci install` dies with "unknown stage";
  - `test_ci_mode_does_not_ask_github`: `gh` stub that fails;
  - `test_dev_mode_refuses_beside_a_busy_runner` *(regression guard: passes before this task)*.
- Run: `python3 -B -m unittest discover -s scripts/tests -p 'test_acceptance_ci.py'` (expected `OK`), and `shellcheck scripts/devvm/acceptance/*.sh`.
- Commit: `test(acceptance): run the upgrade and rollback stages over SSH in CI (UD18, UD19)`.

### Task 4: Serve the candidate, run the stages, and judge them
- New `forge/test/iso/serve_candidate.sh REPOSITORY DIGEST WORKDIR`:
  - runs `registry serve` with the config of `cosign-attest-check.sh` on `127.0.0.1:5000`;
  - copies with `forge/scripts/retry.sh skopeo copy --preserve-digests --dest-tls-verify=false` the digest to `:stable`, then `sha256-<hex>.sig` and `sha256-<hex>.att`. **Either one missing is fatal** (ADR-0130);
  - reads `:stable` back and requires DIGEST;
  - writes `registry.pid` and `mirror` (`10.0.2.2:5000/<path>`).
- `run_iso_test.sh`:
  - `UPDATE_TO` and `UPDATE_REPOSITORY` are both set or neither is; without `SECURE_BOOT=1` the script exits 2 ("the upgrade case runs on Secure Boot firmware");
  - in the upgrade case the default `TIMEOUT` is `3600` (estimate): the existing 1500 s install bound plus `STAGES_TIMEOUT=2100` for `run.sh`;
  - it serves the candidate (`trap` kills the registry) and starts `console.py` with `ATHANOR_CONSOLE_HOLD=1`;
  - it waits for `ready` or the console's exit. On `ready` it runs `timeout "$STAGES_TIMEOUT" bash scripts/devvm/acceptance/run.sh --ci > "$output/upgrade.log" 2>&1` and writes the status to `upgrade-status`, then stops QEMU;
  - `verdict_case` gains `upgrade`.
- `verdict.py`:
  - Usage `… [secure-boot [upgrade]]`.
  - The existing greeter, session, settings, MOK and Secure Boot computations move into `stage_checks(phases: dict[str, int], prefix: str) -> dict[str, tuple[bool, str]]`, called with `""` for the existing case.
  - With `upgrade`:
    - `upgrade-stages` requires `upgrade-status` to exist and read 0. A missing file reads "the upgrade stages never ran (no ready)". Otherwise the report quotes the first `FAIL  ` line of `upgrade.log`, and 124 reads "the upgrade stages timed out";
    - `stage_checks` runs on `upgrade-phases.txt` with `upgraded-` and with `rolled-back-`.
- Tests in `test_verdict.py`:
  - `test_upgrade_pass`;
  - `test_upgrade_missing_status_fails`;
  - `test_upgrade_old_deployment_fails`;
  - `test_upgrade_timeout_fails`;
  - `test_upgrade_secure_boot_lost_fails` (`upgraded-secureboot-not-enforced`);
  - `test_upgrade_rollback_session_dead_fails`;
  - `test_upgrade_shim_refusal_fails`;
  - `test_upgrade_requires_secure_boot`;
  - `test_plain_case_unchanged` *(regression guard: passes before this task)*.
- Check: the runner and `shellcheck`. Then run once on the laptop rig or the runner VM: one VM, never beside a runner build. Expected: `verdict.md` `**PASS**`.
- Commit: `test(iso): serve the candidate and judge the upgrade and rollback stages (UD18)`.

### Task 5: The `upgrade-acceptance` evidence gate (UD4)
- `scripts/ci/evidence.py`: add `"upgrade-acceptance"` to `GATES`, with `REQUIRED_CHECKS["upgrade-acceptance"] = ("secure-boot", "upgrade-stages", "upgraded-secure-boot", "rolled-back-secure-boot")`.
- `promotion-record.sh` and `fetch-evidence.sh` are unchanged: UD20 makes the gate required only after two consecutive green orchestrator runs, in a later PR.
- New tests go in the existing `scripts/ci/tests/test_evidence.py`:
  - `test_upgrade_gate_fails_without_rolled_back_secure_boot`;
  - `test_upgrade_gate_passes_with_every_required_check`;
  - `test_iso_gate_unchanged` *(regression guard)*.
- Run: `python3 -B -m unittest discover -s scripts/ci/tests -p test_evidence.py`. Expected: `OK`.
- Commit: `feat(evidence): record the upgrade-acceptance gate of UD4`.

### Task 6: The `upgrade-acceptance` job of `accept.yml` (glue only)
- First, move the inline "Fetch the ISO" step to `forge/test/iso/fetch_iso.sh REF OUT`, verbatim, and use it in `install`. Commit: `refactor(accept): fetch the ISO through a script`.
- The new job: `needs: [install]`, `runs-on: ubuntu-24.04`, `./.github/actions/kvm`, `timeout-minutes: 70`. That is an estimate: QEMU's 60-minute bound plus about 10 minutes of setup, ISO fetch and evidence. Steps:
  - the apt step of `install`, `install-tools.sh`, the registry login and `accept-target.sh`;
  - `fetch_iso.sh` on `athanor-iso:stable`, resolved to its digest and printed in the summary;
  - `SECURE_BOOT=1 UPDATE_TO="$(<target/default-digest)" UPDATE_REPOSITORY="$REGISTRY/athanor-system" bash forge/test/iso/run_iso_test.sh "$ISO_PATH" ./iso-test-upgrade`;
  - `report.sh`, `evidence.py write --gate upgrade-acceptance`, and the artifacts. `df.txt` goes into the summary.
- Check: `actionlint .github/workflows/accept.yml && python3 scripts/verify.py workflows`.
- Commit: `ci(accept): run the upgrade acceptance on Secure Boot firmware (UD18)`.

### Task 7: Prove it
- `gh workflow run accept.yml --ref iso-v0 -f run_id=<newest run> -R ars-regia/athanor`. Two green runs in a row are UD18's acceptance. Three under the timeout, with the `df` record, are UD21's.
- Then update `release-1.0.md` 4.1 from "To add" to "Existing" with an exact-match replacement script. Commit: `docs(release): name the upgrade acceptance as existing`.

---

**Decisions and rejected alternatives**
- **SSH and `run.sh --ci` (UD19).** Rejected: driving the update over the serial console, which deviates from an approved item.
- **Session checks.** The probes of `console.py` run over SSH and are judged by `verdict.py`'s logic. Rejected: rewriting the probes in `run.sh`, which gives two copies that drift.
- **`Apply()` through `call_active`**, the notifier's path. Rejected: `call_root`, which never meets the user's polkit decision.
- **The seeded `/etc` file is a shipped one an administrator edits** (maintainer, 2026-10-23). Rejected: a test-only file in the product image.
- **The mirror drop-in stays for the rollback.** Rejected: removing it, which tests nothing, because a rollback does not merge `/etc`.

**Open questions for the maintainer**
1. UD21 states `timeout-minutes: 60`, and the bounds here sum to 70 (an estimate). Typical runs should take about 40 minutes. Should UD21 name the bound, or should the inner bounds shrink?
2. Not verified on a guest:
   - whether c/image uses the mirror for tag pulls and attachments under the check unit's hardening;
   - whether `type_keys` logs in at the greeter;
   - whether a session inhibitor makes `Apply()` answer `Blocked`;
   - the `loginctl --json` field name `tty` on F43's systemd.
