# ISO acceptance under Secure Boot ending in an update to a newer `:stable`: implementation plan

> For the implementer: execute task by task (`superpowers:executing-plans`); each task ends in one commit. The specification is approved and is implemented as written (ADR-0074); a deviation found while working stops the task and goes to the maintainer.

**Goal.** This plan closes the 0.9 exit item of `docs/operations/release-1.0.md` section 4.1, "the ISO test under Secure Boot firmware ending in an update to a newer `:stable`", by implementing UD18 and UD19 of `doc_update_delivery.md` rev 3 (approved 2026-10-09).

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
- **The first boot** is driven by `console.py` exactly as today, up to `settings-alive`. In upgrade mode it then holds: it keeps reading the serial line passively and types nothing more.
- **The rest** is driven over SSH (UD19) by `scripts/devvm/acceptance/run.sh --ci`. It reuses `lib.sh`'s `expect`, `guest_ssh`, `reboot_guest`, `wait_ssh`, `wait_reboot`, `check_now`, `call_active` and `call_root`, and runs the stages `seed upgrade check goback check-back`.

How the session checks after each reboot are judged:
- `run.sh` fetches the probes of `console.py` (`--print-probe`), runs them over SSH, and turns their output into phases through `console.py`'s own `MARKERS` (`--classify`).
- `verdict.py` judges those phases with its existing greeter, session, settings, MOK and Secure Boot logic, under the prefixes `upgraded-` and `rolled-back-` (UD18 step 5).
- Digest, seed and held assertions fail inside `run.sh` with `FAIL  <text>`, as every other acceptance stage does. `verdict.py` reports the first such line.

**Tech stack.** Bash (`run_iso_test.sh`, `lib.sh`, `run.sh`, one new script), Python 3 stdlib (`console.py`, `verdict.py`, tests), QEMU/OVMF secboot, distribution `registry` v3 (`scripts/ci/install-tools.sh`), skopeo, jq on the host, Kickstart.

**Spec.**
- `doc_update_delivery.md`: UD4, UD8, UD18 (steps 1-6), UD19, UD20, UD21.
- `doc_update_trust.md` rev 4: UT6 (`Apply()`/`GoBack()`, polkit), UT13 (every image is feature class until 1.0, so the update must reach `downloaded`).
- `doc_pipeline.md` PL29. `release-1.0.md` 4.1. ADR-0074.
- Code read: `scripts/devvm/acceptance/{lib.sh,run.sh}`, `forge/test/iso/{run_iso_test.sh,console.py,verdict.py,collaudo.ks,test_verdict.py}`, `scripts/ci/evidence.py`, `athanor-update` `requests.rs` and `check.rs`.
- Not read: `scripts/devvm/devvm.env`, which defines `guest_ssh`, `ssh_key`, `SSH_PORT`, `GUEST_USER` and `die`. The local gate refuses to read `*.env` files, so the implementer reads it first.

**Global constraints (AGENTS.md).** No `|| true`, no `2>/dev/null` that hides an error, no `continue-on-error`. Logic lives in scripts and the YAML only calls them. English, Conventional Commits, no attribution. One logical change per commit.

Signing jobs and environments, `release.yml` and `promote-stable.yml` are not touched. `system/Containerfile` is touched in Task 1 only, for a file under `/etc`. Every file below is owned by `@hr-mes` in `.github/CODEOWNERS`, so the implementation PR needs the maintainer's approval at the card's sha. One VM per runner: the job has its own hosted runner.

**Review focus: the failure modes, each named in the report and never passed.**
1. *Update offered but not applied.* `call_active Apply` answers an error (`Blocked`, `NotAuthorized`, `Busy`), or there is no reboot within 10 min. `reboot_into_digest` fails and quotes the reply.
2. *Reboot into the old deployment.* The booted digest is compared with the candidate, never with "changed".
3. *MOK does not trust the new kernel.* shim or GRUB refuse to boot it: the passive console records `secure-boot-refused` (markers `bad shim signature` and `Security Violation`), and `wait_ssh` times out. If the system boots without enforcement, `upgraded-secure-boot` is false.
4. *Registry unreachable reported as pass.* c/image falls back from the mirror to ghcr.io, where `:stable` is still the "from" digest, so the state is `none`. `expect` requires `.downloaded.digest == candidate` and prints `.last_error` and `.last_error_host`.
5. *Timeout.* `run.sh --ci` runs under `timeout 2400`. Status 124 is reported as "the upgrade stages timed out", never as a pass.

---

### Task 0: Preconditions (read-only; measured 2026-10-23)
- `athanor-iso:stable` exists: `sha256:a8e6ba35…`, label `38796755145`. `athanor-system:stable` is `sha256:68d7c8c8…`, version `43.20261015.365`.
- Run 38796755145 is the Orchestrator at `311ad72e` (2026-10-15). That commit contains #468 (`91f7d873`, Secure Boot case required), so its acceptance ran the Secure Boot case. Its evidence file was not opened.
- `grep -rn athanor-upgrade-probe` finds the name only in the spec: **the image does not ship `/etc/athanor-upgrade-probe.conf`**. Task 1 adds it, and the job cannot pass until a `:stable` carrying it exists (open question 2).
- `accept.yml` `install` runs on `ubuntu-24.04` with `./.github/actions/kvm`, as UD18 says: no divergence.

### Task 1: The image ships `/etc/athanor-upgrade-probe.conf` (UD18 step 2)
- `system/Containerfile`: one `RUN printf` that writes a two-line file, then `test -s`. The comment says that nothing reads the file, and that the upgrade acceptance edits it to prove an `/etc` edit survives the three-way merge.
- Check: `python3 scripts/verify.py containerfile` (or the check `--list` names for it) is green. `system/check-image-*.sh` stays green on a local build if the rig is free.
- Commit: `feat(image): ship /etc/athanor-upgrade-probe.conf for the upgrade acceptance (UD18)`.

### Task 2: The test VM answers SSH (UD19)
- `forge/test/iso/collaudo.ks` gains a last `%post --erroronfail` between `# upgrade-acceptance begin` and `# upgrade-acceptance end`. It:
  - writes `@COLLAUDO_SSH_KEY@` to `~collaudo/.ssh/authorized_keys` (0600, owner collaudo, `restorecon -R`);
  - writes `/etc/sudoers.d/50-collaudo` with `collaudo ALL=(ALL) NOPASSWD: ALL` (0440), then `visudo -cf` on it;
  - runs `systemctl enable sshd.service`.
- `run_iso_test.sh`, upgrade case only (`UPDATE_TO` set):
  - generates `ssh-keygen -t ed25519 -N '' -f "$RUNNER_TEMP/collaudo-ssh/id"`, outside OUTPUT_DIR so the private key is never uploaded;
  - validates the public key against `^ssh-ed25519 [A-Za-z0-9+/]+=*$`;
  - substitutes it into its copy of the kickstart.
- In every other case the script deletes the section from the copy, then `grep -q @COLLAUDO_SSH_KEY@` must find nothing. The ISO acceptance keeps sshd disabled.
- The network becomes `-netdev user,id=n0,hostfwd=tcp:127.0.0.1:${SSH_PORT}-:22` in the upgrade case. `SSH_PORT` is a free port taken as in `cosign-attest-check.sh`.
- Check: `python3 scripts/verify.py kickstart` and `shellcheck forge/test/iso/run_iso_test.sh`.
- Commit: `test(iso): give the upgrade case's VM a per-run SSH key and passwordless sudo (UD19)`.

### Task 3: `console.py` holds after the first boot, and exposes its probes
- With `ATHANOR_CONSOLE_HOLD=1`, `settings-alive` leaves `DECIDED`. The console notes it, writes `OUTPUT/ready`, and from then on types nothing. It keeps matching `MARKERS` and logging until the serial socket closes. In hold mode the idle give-up does not apply.
- New marker pairs in `MARKERS`: `bad shim signature` → `secure-boot-refused`, and `Security Violation` → `secure-boot-refused`. verdict.py adds `secure-boot-refused` to its failure signals.
- New CLI modes for `run.sh`:
  - `console.py --print-probe {MOK,GREETER,SESSION,SETTINGS}` writes that probe's bytes to stdout.
  - `console.py --classify PREFIX` reads stdin and prints `PREFIX + phase` for each `MARKERS` hit, one per line, through a new `phases_in(text: bytes) -> list[str]` that the main loop also uses.
- Tests are new functions in `forge/test/iso/test_verdict.py`, added to `main()`'s tuple. That file is a plain runner, not unittest; `Justfile` already runs it.
  - `test_console_holds_after_settings_in_hold_mode`: the fake guest of `test_console_logs_in_opens_settings_and_stops`, with `ATHANOR_CONSOLE_HOLD=1`. Assert that `ready` exists, that no byte is typed after `SETTINGS_ALIVE`, that a later `bad shim signature` adds `secure-boot-refused`, and that the console exits when the socket closes.
  - `test_classify_maps_probe_output`: `b"GREETER_ALIVE 3\r\nSECUREBOOT_ENABLED\r\n"` with prefix `upgraded-` gives `["upgraded-greeter-alive", "upgraded-secureboot-enabled"]`.
  - `test_print_probe_is_the_typed_probe`: `--print-probe MOK` output equals `console.MOK_PROBE`.
- Run: `python3 -B forge/test/iso/test_verdict.py`. Expected: each `ok`, then `all checks passed`.
- Commit: `test(iso): hold the console after the first boot and share its probes`.

### Task 4: `run.sh --ci` with the stages `seed upgrade check goback check-back` (UD18 steps 2-6, UD19)
- `run.sh`: with `--ci` as the first argument, it requires `CI == true`, otherwise `die "--ci runs only in a CI environment"`. It sets `ACC_CI=1` and `STAGES=(seed upgrade check goback check-back)`, and does not call `guard_no_ci`. Without `--ci`, `guard_no_ci` runs first, unchanged, so the dev-VM mode still refuses beside a busy self-hosted runner.
- `--ci` requires these variables, each validated and each named when missing:
  - `ATHANOR_UPDATE_TO` (`^sha256:[0-9a-f]{64}$`);
  - `ATHANOR_UPDATE_REPOSITORY`;
  - `ATHANOR_UPDATE_MIRROR`;
  - `ACC_CI_OUT`, `SSH_PORT`, `ACC_SSH_KEY`.
- `lib.sh`:
  - With `ACC_CI=1`, `tunnel` and `untunnel` return 0: the guest reaches the registry through the mirror drop-in at `10.0.2.2` (UD18 step 3), not through the dev VM's reverse tunnel.
  - New `booted_digest()` reads `guest_ssh sudo bootc status --format json | jq -r .status.booted.image.imageDigest`.
  - New `reboot_into_digest CALLER METHOD DIGEST DESCRIPTION`: `request_and_reboot` with the booted digest in place of `marker()`. The acceptance marker exists only in dev-VM images.
  - New `disk_floor`: it prints `df -h` to `$ACC_CI_OUT/df.txt` and fails when less than 4 GB is free (UD21). It is called at the start of each stage.
  - New `session_probes PREFIX`:
    1. For MOK and GREETER: `python3 forge/test/iso/console.py --print-probe X | guest_ssh bash -s | python3 forge/test/iso/console.py --classify PREFIX >> "$ACC_CI_OUT/upgrade-phases.txt"`.
    2. `type_keys` with the password and Return, to log in at the greeter, as `console.py` does through the monitor.
    3. SESSION and SETTINGS in the same way.
- The stages:
  - `stage_seed`:
    - saves `booted_digest` to `$ACC_STATE/from-digest`, and requires it to differ from `ATHANOR_UPDATE_TO` ("the candidate is :stable already");
    - writes a nonce to `~/upgrade-probe` and to `/var/lib/athanor-upgrade-probe/seed`;
    - requires `/etc/athanor-upgrade-probe.conf` to exist ("the from image does not ship the probe file", Task 1), then appends `edited=<nonce>` to it.
  - `stage_upgrade`:
    - writes `/etc/containers/registries.conf.d/99-upgrade-acceptance.conf` (`prefix` = `location` = the repository, one `[[registry.mirror]]` = `ATHANOR_UPDATE_MIRROR` with `insecure = true`);
    - stops `athanor-update-check.timer` and waits out a running check, as `stage_apply` does (#547), then runs `check_now`;
    - expects `.update downloaded` and `.downloaded.digest` = the candidate;
    - runs `reboot_into_digest call_active Apply "$ATHANOR_UPDATE_TO" "UD18 4: Apply() from the active session boots the candidate"`.
  - `stage_check`:
    - expects `.verified.reason signature`;
    - checks the image name ends with `:stable` and `.signature` is `containerPolicy` in `bootc status`;
    - compares the three seeds with the nonce;
    - runs `session_probes upgraded-`.
  - `stage_goback`: removes the mirror drop-in, then runs `reboot_into_digest call_root GoBack "$(<"$ACC_STATE/from-digest")" "UD18 6: GoBack() boots the from digest"`. It reuses `call_root` from `stage_goback` of the dev VM, because `GoBack()` is `auth_admin`.
  - `stage_check_back`:
    - stops the timer and runs `check_now`;
    - expects `.update held`, and `/var/lib/athanor-update/held` = the candidate;
    - checks the seeds;
    - runs `session_probes rolled-back-`.
- Tests in a new `scripts/tests/test_acceptance_ci.py` (unittest, with stub `systemctl`, `gh` and `ssh` on a temporary PATH):
  - `test_ci_mode_refuses_outside_ci`: `CI` unset gives exit ≠ 0 and the message.
  - `test_ci_mode_names_a_missing_variable`.
  - `test_dev_mode_refuses_beside_a_busy_runner`: the `systemctl` stub reports active and `gh` prints `1`, so the run stops on "busy".
  - `test_ci_mode_does_not_ask_github`: `gh` stub that fails, under `--ci` with `CI=true`; the refusal is the missing variable, not the runner check.
- **Unverified:** that `devvm.env` honours the overrides above on a host without the dev VM. That is read first; if it does not, Task 4 starts by making it so.
- Run: `python3 -B -m unittest discover -s scripts/tests -p 'test_acceptance_ci.py'` (expected `OK`), and `shellcheck scripts/devvm/acceptance/*.sh`.
- Commit: `test(acceptance): run the upgrade and rollback stages over SSH in CI (UD18, UD19)`.

### Task 5: Serve the candidate, run the stages, and judge them
- New `forge/test/iso/serve_candidate.sh REPOSITORY DIGEST WORKDIR`:
  - starts the registry config and `registry serve` of `cosign-attest-check.sh` on `127.0.0.1:5000`, the port UD18 names;
  - copies with `forge/scripts/retry.sh skopeo copy --preserve-digests --dest-tls-verify=false` the digest to `:stable`, plus the `sha256-<hex>.sig` attachment and the `.att` attachment. UD18 names `.sig` only, but `check.rs` also fetches `.att`;
  - reads `:stable` back and requires DIGEST;
  - writes `registry.pid` and `mirror` (`10.0.2.2:5000/<path>`).
- `run_iso_test.sh`:
  - `UPDATE_TO` and `UPDATE_REPOSITORY` are both set or neither is; without `SECURE_BOOT=1` the script exits 2 ("the upgrade case runs on Secure Boot firmware", release-1.0.md 4.1);
  - it serves the candidate (`trap` kills the registry) and starts `console.py` with `ATHANOR_CONSOLE_HOLD=1`;
  - it waits for `ready` or the console's exit, then runs `timeout 2400 bash scripts/devvm/acceptance/run.sh --ci > "$output/upgrade.log" 2>&1`, writes its status to `upgrade-status`, then stops QEMU;
  - `verdict_case` gains `upgrade`.
- `verdict.py`:
  - Usage `… [secure-boot [upgrade]]`.
  - The existing greeter, session, settings, MOK and Secure Boot computations move into `stage_checks(phases: set[str], prefix: str) -> dict[str, tuple[bool, str]]`. The existing case calls it with `""`, so its output is byte-identical (an existing test asserts that).
  - With `upgrade` it adds:
    - `upgrade-stages`: `upgrade-status` is 0. Otherwise the report quotes the first `FAIL  ` line of `upgrade.log`, and 124 reads "the upgrade stages timed out";
    - `stage_checks(upgrade_phases, "upgraded-")` and `stage_checks(upgrade_phases, "rolled-back-")`, giving `upgraded-secure-boot`, `rolled-back-secure-boot` and the rest.
- Tests in `test_verdict.py`:
  - `test_upgrade_pass`;
  - `test_upgrade_old_deployment_fails`: the report quotes `FAIL  UD18 4: … booted sha256:<from>`;
  - `test_upgrade_timeout_fails`;
  - `test_upgrade_secure_boot_lost_fails`: `upgraded-secureboot-not-enforced`;
  - `test_upgrade_rollback_session_dead_fails`;
  - `test_upgrade_shim_refusal_fails`;
  - `test_upgrade_requires_secure_boot`: `upgrade` without `secure-boot` exits 2;
  - `test_plain_case_unchanged`.
- Check: the runner and `shellcheck`. Then run once on the laptop rig or the runner VM: one VM, never beside a runner build. Expected: `verdict.md` `**PASS**`.
- Commit: `test(iso): serve the candidate and judge the upgrade and rollback stages (UD18)`.

### Task 6: The `upgrade-acceptance` evidence gate (UD4)
- `scripts/ci/evidence.py`: add `"upgrade-acceptance"` to `GATES`, with `REQUIRED_CHECKS["upgrade-acceptance"] = ("secure-boot", "upgrade-stages", "upgraded-secure-boot", "rolled-back-secure-boot")`.
- `promotion-record.sh` and `fetch-evidence.sh` are unchanged: UD18 and UD20 make the gate required only after two consecutive green orchestrator runs, in a later PR.
- New `scripts/tests/test_evidence.py`: `test_upgrade_gate_fails_without_rolled_back_secure_boot`, `test_upgrade_gate_passes_with_every_required_check`, `test_iso_gate_unchanged`. Expected: `OK`.
- Commit: `feat(evidence): record the upgrade-acceptance gate of UD4`.

### Task 7: The `upgrade-acceptance` job of `accept.yml` (glue only)
- First, move the inline "Fetch the ISO" step to `forge/test/iso/fetch_iso.sh REF OUT`, verbatim, and use it in `install`. Commit: `refactor(accept): fetch the ISO through a script`.
- The new job: `needs: [install]`, `runs-on: ubuntu-24.04`, `./.github/actions/kvm`, `timeout-minutes: 60` (UD21, an estimate). Steps:
  - the apt step of `install`, `install-tools.sh` for `registry`, the registry login and `accept-target.sh`;
  - `fetch_iso.sh` on `athanor-iso:stable`, resolved to its digest and printed in the summary;
  - `SECURE_BOOT=1 UPDATE_TO="$(<target/default-digest)" UPDATE_REPOSITORY="$REGISTRY/athanor-system" bash forge/test/iso/run_iso_test.sh "$ISO_PATH" ./iso-test-upgrade`;
  - `report.sh`, `evidence.py write --gate upgrade-acceptance`, and the artifacts. `df.txt` goes into the summary (UD21).
- Check: `actionlint .github/workflows/accept.yml && python3 scripts/verify.py workflows`.
- Commit: `ci(accept): run the upgrade acceptance on Secure Boot firmware (UD18)`.

### Task 8: Prove it
- `gh workflow run accept.yml --ref iso-v0 -f run_id=<newest run> -R ars-regia/athanor`. Two green runs in a row are UD18's acceptance. Three under the timeout, with the `df` record, are UD21's.
- Then update `release-1.0.md` 4.1 from "To add" to "Existing" with an exact-match replacement script. Commit: `docs(release): name the upgrade acceptance as existing`.

---

**Decisions and rejected alternatives**
- **SSH and `run.sh --ci` (UD19).** Rejected: driving the update over the serial console, which deviates from an approved item.
- **Session checks.** The probes of `console.py` run over SSH and are judged by `verdict.py`'s logic. Rejected: rewriting the probes in `run.sh`, which gives two copies that drift.
- **`Apply()` through `call_active`**, the active session as the notifier. Rejected: `call_root`, which never meets the user's polkit decision. `GoBack()` uses `call_root`, because the action is `auth_admin`.
- **The from-digest comes from the guest's `bootc status`.** Rejected: taking it from the registry tag, which can move during the run.

**Open questions for the maintainer**
1. A `/etc` file that exists only for a test ships in the product image (Task 1): is that acceptable, as UD18 implies?
2. Bootstrap: today's `:stable` lacks the probe file, so the job fails at `seed` until a build with Task 1 is promoted on its two current gates. Is that order acceptable?
3. `.att` is served beside `.sig`, a small extension of UD18 step 3 because the client fetches both. Should the spec text be amended?
4. Not verified on a guest:
   - whether c/image uses the mirror for tag pulls and reads the attachments from it under the check unit's hardening;
   - whether `type_keys` logs in at the greeter;
   - whether `Apply()` is `Blocked` by a session inhibitor;
   - the 60-minute budget.
