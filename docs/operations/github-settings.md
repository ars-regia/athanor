# GitHub settings as code

| Field | Value |
| --- | --- |
| Purpose | Which GitHub settings of the repository are kept in files, and how to export, compare and apply them |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 2, 2026-10-08. Sections 1 to 5 are fact; section 6 is decided ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)) and its file changes and hand deletions are not made yet |
| Depends on | `docs/operations/secrets.md` for secret values (separate change) |
| Defines | GHS1 to GHS10 |

## 1. What is managed

The files under [.github/settings/](../../.github/settings/) are the desired state. One file per area, keys sorted, no ids, URLs, timestamps or counts. [ghsettings.py](../../scripts/github-settings/ghsettings.py) reads and writes them through `gh api`.

| Id | File | Content | Read from |
| --- | --- | --- | --- |
| GHS1 | `repository.json` | Default branch, visibility, merge methods and commit titles, auto-merge, update branch, delete branch on merge, features (issues, projects, wiki, discussions), template, sign-off, forking, description, homepage, topics, `security_and_analysis`, Dependabot alerts (`vulnerability_alerts`), private vulnerability reporting | `repos/{r}`, `repos/{r}/vulnerability-alerts` (204 or 404), `repos/{r}/private-vulnerability-reporting` |
| GHS2 | `branch-protection.json` | Classic branch protection of every protected branch, keyed by branch name, including required signatures | `repos/{r}/branches?protected=true`, `.../branches/{b}/protection` |
| GHS3 | `rulesets.json` | Repository rulesets keyed by name: target, enforcement, conditions, rules, bypass actors | `repos/{r}/rulesets`, `repos/{r}/rulesets/{id}` |
| GHS4 | `environments.json` | Environments keyed by name: wait timer, reviewers (by login or team slug), self-review, deployment branch policy and its branch list, admin bypass, and the NAMES of their secrets and variables | `repos/{r}/environments` and its `deployment-branch-policies`, `secrets`, `variables` |
| GHS5 | `labels.json` | Labels: name, colour, description | `repos/{r}/labels` |
| GHS6 | `pages.json` | Pages build type, source, custom domain, HTTPS; `null` when Pages is off | `repos/{r}/pages` |
| GHS7 | `actions.json` | Actions enabled, allowed actions (and the selected list), SHA pinning, default workflow token permissions, Actions approving pull requests, fork pull request approval policy, NAMES of repository secrets and variables | `repos/{r}/actions/permissions` and its `selected-actions`, `workflow`, `fork-pr-contributor-approval`; `repos/{r}/actions/secrets`, `.../variables` |

Secret values are never read, stored or written (GHS8). Variable values are not stored either: the files hold names only.

## 2. Commands

Run from the repository root. `--repo OWNER/NAME` defaults to the repository of the current checkout (`gh repo view`). `--dir` defaults to `.github/settings`.

`export` and `apply` first read `repos/{r}` and stop with exit code 2 and "insufficient rights" unless `permissions.admin` is true: without the admin role GitHub answers 404 on the admin endpoints, which would otherwise read as "off". A 404 counts as "off" only where GitHub documents it so: `vulnerability-alerts` and `pages`. Any other API failure (403, 404, 5xx), an answer that is not JSON, a missing `gh`, and a settings file that is missing, unreadable, not JSON or lacks a key of its area also exit 2 with a message, before the first write.

`diff` only reads and also runs with a read-only GitHub App token, whose answer to `repos/{r}` carries no `permissions.admin`. It first reads `repos/{r}/actions/permissions`, which answers only a token that can read the administration settings, and stops with exit code 2 otherwise, so no 404 of a token without those rights reads as "off".

| Command | Effect | Exit code |
| --- | --- | --- |
| `python3 scripts/github-settings/ghsettings.py export` | Writes the live state to the seven files | 0, or 2 on an error |
| `python3 scripts/github-settings/ghsettings.py diff` | Prints one line per drift, `drift <area> <path>: ...`, then a count. Objects compare by key; lists of names and lists of named entries (environments' reviewers and branch policies, ruleset rules by `type`, required checks by `context`, labels) compare by name, so their order is not drift; anything else compares as a whole value. Read-only: it calls `gh api` with GET only and reads secret and variable names, never values (GHS10) | 0 when equal, 1 on a drift, 2 on an error |
| `python3 scripts/github-settings/ghsettings.py apply` | Prints the plan: first one line per API call (`METHOD path body`, prefixed `DESTRUCTIVE` for a deletion, followed by the ruleset, branch, label or Pages note in parentheses), then one `MANUAL:` line per step only a human can take. Calls nothing that writes | 0, or 2 on an error |
| `python3 scripts/github-settings/ghsettings.py apply --yes` | Prints the same plan, calls and `MANUAL:` lines, then runs the calls in order. Refuses the whole plan, with nothing written, when it holds a `DESTRUCTIVE` call | 0, or 2 on an error or a refused plan |
| `python3 scripts/github-settings/ghsettings.py apply --yes --allow-destructive` | As `--yes`, and also runs the `DESTRUCTIVE` calls | 0, or 2 on an error |

GHS9 `apply` is idempotent: it compares the live state with the files first, so a second run plans no call. After `apply --yes`, run `diff` to confirm.

`apply` changes what the files say, area by area:

| Area | Calls |
| --- | --- |
| Repository | `PATCH repos/{r}` with the changed fields only; `PUT repos/{r}/topics`; `PUT` or `DELETE` on `vulnerability-alerts` and `private-vulnerability-reporting` |
| Branch protection | `PUT .../branches/{b}/protection`; `POST` or `DELETE .../required_signatures`; DESTRUCTIVE `DELETE` of the protection of a branch the file does not list or lists as `{}` or `null` |
| Rulesets | `POST` a new ruleset, `PUT` a changed one, DESTRUCTIVE `DELETE` of one the file does not list |
| Environments | `PUT repos/{r}/environments/{e}`; `POST` and `DELETE` deployment branch policies |
| Labels | `POST` a new label, `PATCH` a changed one, DESTRUCTIVE `DELETE` of one the file does not list |
| Pages | `POST` to enable, `PUT` to change, DESTRUCTIVE `DELETE` when the file holds `null` |
| Actions | `PUT` on `permissions`, `selected-actions`, `workflow` and `fork-pr-contributor-approval` |

Deleting a label removes it from every issue and pull request; deleting a protection or a ruleset opens the branch. Read the plan before `--yes`, and the `DESTRUCTIVE` lines before `--allow-destructive`.

The tests run against a stub `gh` on `PATH` that serves recorded API answers: `python3 -B -m unittest discover -s scripts/tests -p 'test_ghsettings*.py' -v`.

## 3. Token

| Token | Needed for |
| --- | --- |
| Classic token with `repo`, held by a user with the admin role on the repository | `export`, `diff` and `apply` |
| Fine-grained token on the repository: Administration read and write, Environments read and write, Pages read and write, Issues read and write (labels), Secrets read, Variables read, Metadata read | the same; mapping taken from the GitHub REST reference, not exercised |

Verified on 2026-10-06: `export` and `diff` ran with the owner's `gh` login (classic token, scopes `gist`, `read:org`, `repo`, `workflow`, `write:packages`). `apply --yes` has not been run against the live repository.

## 4. Settings the API cannot set

`apply` prints these as `MANUAL:` lines. They need a click in the web interface.

| Setting | UI path |
| --- | --- |
| Environment admin bypass (`can_admins_bypass` in `environments.json`): the REST endpoint takes only wait timer, self-review, reviewers and branch policy | Settings > Environments > _name_ > Deployment protection rules > "Allow administrators to bypass configured protection rules" > Save protection rules |
| Secret values of the repository and of each environment | Settings > Secrets and variables > Actions > Repository secrets, or Settings > Environments > _name_ > Environment secrets > Add secret. Which value goes in which name: `docs/operations/secrets.md` |
| Variable values | Settings > Secrets and variables > Actions > Variables tab, or Settings > Environments > _name_ > Environment variables |

These are listed but not acted on, by choice: an environment the files do not list, and a secret or variable name the files do not list. Deleting an environment destroys its secrets, so `apply` prints a `MANUAL:` line instead (Settings > Environments > _name_ > Delete environment).

Outside the files and left as they are: collaborators and teams, webhooks, deploy keys, autolinks, Actions runners, artifact and log retention, code scanning setup, the social preview image (Settings > General > Social preview, no API).

## 5. Live state at export (2026-10-06)

Facts read by `export`; each one is in the file named. `diff` against `hr-mes/athanor` printed "live state matches the files" and exited 0 right after export.

Since then the files have moved ahead of GitHub (2026-10-07, ADR-0064): the two signing environments of section 7 take over from `signing`, which stays until the image key rotation ends (section 7), and `main` is protected like `iso-v0`. `diff` lists these changes until the maintainer applies them (bootstrap: `docs/operations/secrets.md` section 4).

| Fact | File |
| --- | --- |
| Default branch `iso-v0`; all three merge methods on; auto-merge on; head branches deleted on merge | `repository.json` |
| Description is still `ermete-os` | `repository.json` |
| Dependabot alerts and Dependabot security updates are off; secret scanning and push protection are on; private vulnerability reporting is on | `repository.json` |
| Only `iso-v0` is protected: required checks `Kernel gate` and `Spec gate` (not strict), no review, `enforce_admins` off, force push and deletion off | `branch-protection.json` (now also `main`, section 7) |
| No repository ruleset | `rulesets.json` |
| Environment `signing`: reviewer `hr-mes`, branches `iso-v0` and `main`, admin bypass on, four secrets | `environments.json` until 2026-10-07 (now section 7) |
| Environment `github-pages`: branches `gh-pages` and `main`; no workflow deploys to it since the DNF channel was removed (ADR-0076, decision 2) | deleted by hand, absent from the export of 2026-10-09 |
| Environment `delete`: no rule, no secret | deleted by hand, absent from the export of 2026-10-09 |
| Pages: legacy build from `main:/docs`; `gh api repos/ars-regia/athanor/pages` reports `"status": "errored"` (status is volatile, not stored) | `pages.json`; off since 2026-10-09, when the file became `null` |
| Actions: all actions allowed, SHA pinning not required, default token read-only, Actions cannot approve pull requests, approval required for all external contributors | `actions.json` |

## 6. Open points (decided 2026-10-08, [ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md))

Each one is a change to a file followed by `apply`, or a deletion by hand followed by an export.

| Point | Decision |
| --- | --- |
| Description `ermete-os` | Set the Athanor description in `repository.json` (the file change follows this record) |
| Dependabot alerts off | Set `vulnerability_alerts` to `true` in `repository.json` |
| Environment `delete` has no rule and no secret, and no workflow names it (`grep -rn "environment:" .github/workflows` finds only `signing-kernel` and `signing-images`) | _(Done, export of 2026-10-09)_ The maintainer deletes it by hand, then exports again |
| Pages builds `main:/docs` and errors, and nothing publishes to the `gh-pages` branch since the DNF channel was removed (ADR-0076, decision 2) | Turn Pages off in `pages.json`; the maintainer then deletes the `gh-pages` branch and the `github-pages` environment by hand and exports again _(Done: the maintainer deleted the branch, the environment and the Pages site by hand on 2026-10-09; the export of that day records it)_ |
| `enforce_admins` is off on `iso-v0` and `main`: the admin may push past the required check | Stays off while there is a single maintainer: turned on, it would block every merge, because nobody else can approve. Turn it on when a second maintainer joins. The signing environments bind the admin already (section 7) |
| No scheduled drift check | _(Done in PB2, section 9)_ `maintenance.yml` runs `diff` daily with a read-only token of a GitHub App, not an admin token |

## 7. Signing environments (ADR-0064)

Two environments hold the signing keys, so a release cycle asks for at most two approvals and
no job holds a key it does not use:

| Environment | Secrets | Job |
| --- | --- | --- |
| `signing-kernel` | `SECUREBOOT_SIGNING_KEY`, `MODULE_SIGNING_KEY` | `nvidia-kmod-sign` of `athanor-forge-orchestrator.yml`, only when a kernel or NVIDIA change leaves the signed vmlinuz or modules missing |
| `signing-images` | `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD` | `sign-system-images` of `athanor-forge-orchestrator.yml` |
| `signing` | the four keys it still holds; `MOK_PRIVATE_KEY`, a fifth that no workflow used, was deleted by the maintainer before the export of 2026-10-09 | `sign-system-images` of `athanor-forge-orchestrator.yml`, during the image key rotation only |

`signing` is the environment the split replaces. It holds image key 1, which signs the
transitional release of the image key rotation (`docs/operations/secrets.md` section 4.1), so
`environments.json` declares it as it is live until the rotation ends and `ghsettings.py diff`
does not report it. While `system/keys` holds both image keys, `scripts/verify.py` treats it as
an alias of `signing-images`: its protection rules apply, and its keys are not counted as held
twice. When `athanor-image-1.pub` leaves `system/keys`, the maintainer deletes `signing` and
its entry leaves `environments.json`; an entry left behind fails the lint, because its keys are
then held twice.

`release` holds no key and has the same protection as the signing environments: required
reviewer `hr-mes`, no administrator bypass, branches `iso-v0` and `main`. It gates only the
promotion to `:stable` ([ADR-0104](../decisions/0104-release-workflow-and-stable-gate.md),
amending [ADR-0103](../decisions/0103-audit-5-decisions.md) D2 and D25): the job that moves
`:stable` runs in it after the evidence and the dwell, and the image signing does not wait
for it. Until PB13 of `docs/architecture/doc_pipeline.md` lands, no job uses it.

`signing-kernel` and `signing-images` have the same protection in `environments.json`: required reviewer `hr-mes`; administrator
bypass off (`can_admins_bypass: false`, set by hand: section 4); deployment branches `iso-v0` and
`main`, both protected by `branch-protection.json` (required checks `Kernel gate`, `Spec gate`
and `gate` on `iso-v0`, `Kernel gate` on `main`, section 8; no force push, no deletion). Once the
rotation of `secrets.md` section 4.1 ends, `signing-images` loses the required reviewer until
the 1.0 tag (ADR-0098 item 6); `signing-kernel` keeps it.

`prevent_self_review` stays `false`, deferred until a second reviewer exists (secrets.md
KC1). GitHub refuses the approval of the person who triggered the run, and a release run, or a
re-run of one, is triggered by the maintainer, who is today the only reviewer: with it `true`,
every release the maintainer triggers or re-runs would wait for an approval nobody can give. An
App identity for agents does not change this, because the maintainer still starts and re-runs
releases. It turns `true` when a second required reviewer is added to the signing environments and to
`release`, which waits for the same reason.

`environments.json` is the one place that says which environment holds which key.
`scripts/verify.py workflows` reads it and fails a signing secret read by a job of any other
environment, a secret listed in two environments, a signing environment without a reviewer, with
administrator bypass, or deploying from a branch `branch-protection.json` does not protect by
name. It is a regression guard on the files, not a check of the live settings: `ghsettings.py
diff` is.

## 8. Switching the required check to gate

`branch-protection.json` declares exactly what is applied. On `iso-v0` it requires three checks:
`Kernel gate`, `Spec gate` and `gate`, the aggregate job of `pr.yml` (doc_pipeline.md PL3,
ADR-0075); `main` requires `Kernel gate` until it takes `pr.yml`. The file is applied as soon as
the change that adds `pr.yml` is merged, and it blocks nothing: the three workflows run on every
pull request, so every required check reports. The bots wait for the checks this file requires
(`forge/scripts/bot_merge.py`), so they follow it in every state. The follow-up that removes the
`pull_request` triggers of `kernel-build.yml` and `spec-build-check.yml` removes `Kernel gate` and
`Spec gate` from this file in the same change: a check that is required but never reports would
leave every pull request pending.

| Step | Who | Action |
| --- | --- | --- |
| 1 | maintainer | Merge the change that adds `pr.yml` into `iso-v0` |
| 2 | maintainer | Give every open pull request one new event (a push, or "Update branch"), so `pr.yml` runs on it; check that each shows a `gate` check (`gh pr checks <n>`) |
| 3 | **[M]** maintainer | `python3 scripts/github-settings/ghsettings.py apply`, read the plan (one `PUT .../branches/iso-v0/protection` adding `gate`, no `DESTRUCTIVE` line), then `apply --yes` and `diff`, which must print nothing for branch protection |
| 4 | maintainer | Merge the follow-up (doc_ci.md CP4) that removes the `pull_request` triggers of `kernel-build.yml` and `spec-build-check.yml` and, in the same change, `Kernel gate` and `Spec gate` from `branch-protection.json` and from `CHECK_WORKFLOWS` of `bot_merge.py`, and moves the spec bot merge into `pr.yml`. Before it, kernel and spec changes are built twice |
| 5 | **[M]** maintainer | Apply the file again as in step 3, right after step 4: the plan drops the two legacy contexts |

To roll back step 3, remove `gate` from `branch-protection.json` and apply it again; step 4 is
rolled back by reverting its change and applying the file.

## 9. Drift check, rulesets and the GitHub App (doc_pipeline.md PB2)

GHS10 **Declared, not stored.** A settings file may hold keys GitHub does not store.
`export` keeps them from the file, `apply` never reads them, and `diff` acts on them. The
only one is `personal_tokens` in `actions.json`: the personal access tokens
`FORGE_PAT`, `KERNEL_BUMP_TOKEN` and `SPECS_UPDATE_TOKEN`, which the GitHub App identities
replace (doc_pipeline.md PL5). While `retired` is `false` they are ordinary entries of
`secrets`. Once the App has replaced them, the maintainer sets `retired` to `true` and drops
them from `secrets`; from then on `diff` reports each of them still set as drift, which is
the PB2 gate "the three personal tokens are absent from the secrets list".

**Who applies.** Applying is a maintainer action: `ghsettings.py apply` runs from the
maintainer's machine with their own token. No workflow applies anything to GitHub, and the
settings App below holds no write permission, so the drift check reports and never repairs.

**Daily check.** `.github/workflows/maintenance.yml` runs `ghsettings.py diff` every day
(PL52) and on dispatch. It mints a token of the settings App with
`actions/create-github-app-token`, limited to this repository. The workflow requests no
narrower permissions (the action's v3.2.0 rejects `permission-variables`), so the token
carries exactly the App's permissions, and those permissions are the only control on what
it can do: read access on Actions, Administration, Environments, Pages, Secrets and
Variables (Metadata read is implied), and no write permission. Whoever edits the App
keeps it so; nothing in the repository enforces it. Its key, `SETTINGS_APP_PRIVATE_KEY`
(secrets.md SEC13), then mints nothing that changes the repository. The REST repository
object answers the merge settings (`allow_squash_merge` and the rest) as null to an App
installation token, so `ghsettings.py` reads those ten fields through GraphQL, which
answers them to both tokens. The same token reads a ruleset's bypass actors as null over
REST and each actor as null over GraphQL, which still counts them; reading them needs write
access to the administration settings, which the App does not get. `diff` therefore compares
their number and prints a `note` line saying who they are and their mode were not read, and
`export` refuses to write a ruleset without them; the maintainer's own `diff` compares them in
full (maintainer decision, 2026-10-08). Its client id is the variable
`SETTINGS_APP_CLIENT_ID` (VAR6). A drift fails the job; the `alert` action of PL51 that
turns a failed scheduled job into a `ci-alert` issue is not part of this change.

**Desired state ahead of GitHub.** The files now ask for:

| File | Desired | Live until the maintainer applies it |
| --- | --- | --- |
| `rulesets.json` | ruleset `product-branches` on `refs/heads/iso-v0` and `refs/heads/main`: no deletion, no force push, linear history, a pull request with one approval and no code-owner review (squash only, stale approvals dismissed; ADR-0062 keeps `require_code_owner_review` off while there is one code owner), required check `gate` of GitHub Actions (integration 15368, not strict) (no merge queue yet: see below); bypass by the repository admin role in pull-request mode, so the maintainer merges their own pull requests without a second reviewer but never pushes past the ruleset (PL1, PL2, PL4, PQ5) | no ruleset |
| `actions.json` | secret `SETTINGS_APP_PRIVATE_KEY` and variable `SETTINGS_APP_CLIENT_ID`; `personal_tokens` declared, not yet retired; `sha_pinning_required` stays `false` until PB3 (below) | neither name set |

`gate` is the aggregate job of `pr.yml` (PB1, ADR-0075). It runs `just check`, which tolerates
the findings of `scripts/ci/known-red.txt` until their expiry, so the ruleset adds no
exception of its own for known red checks. `branch-protection.json` requires `gate` next to
`Kernel gate` and `Spec gate` on `iso-v0` (section 8); the ruleset requires `gate` alone on
both branches. Classic branch protection and the ruleset both apply until the maintainer
decides to retire the former; section 8 step 4 and step 5 drop the two legacy contexts, and
they must be done before the merge queue returns (see below).

The ruleset declares no merge queue for now (maintainer decision of 2026-10-08, PIPE-N06): the
required checks do not run on `merge_group`, so a queue could not be satisfied, and `apply`
writes every area at once. The queue returns, in the same change that adds it to
`rulesets.json`, when every required workflow answers `merge_group`; `verify.py workflows`
refuses a ruleset that enables a queue while a required context is reported by no workflow with
that trigger. The queue then is: squash, all green, at most two entries built and merged
together, 360 minutes for checks. The queue needs `merge_group` among the triggers of every
required workflow (PB1's `pr.yml`). SHA pinning enforcement makes GitHub refuse every workflow that uses an action
by tag, so it is enabled only after every `uses:` is pinned by commit SHA (PB3,
`verify.py pinning`); on 2026-10-07 two references are still tags
(`actions/upload-artifact@v4`, `cachix/install-nix-action@v25`). The files declare only what
is meant to be applied now, and `apply` writes a whole area at a time: declaring
`sha_pinning_required: true` today would turn it on with the next `apply` of `actions.json`
and stop those workflows. It stays `false` until PB3 pins the two actions and flips it to
`true` (PL8).

The order of the maintainer's steps, each followed by `ghsettings.py diff`:

1. Protect `main` as `branch-protection.json` describes it (`ghsettings.py apply`), the step of
   the bootstrap (`docs/operations/secrets.md` section 4) still open.
2. Create the settings GitHub App, owned by the organisation, installed on this repository
   only, with the read permissions above; store its client id as `SETTINGS_APP_CLIENT_ID`
   and its private key as `SETTINGS_APP_PRIVATE_KEY` (secrets.md SEC13). Once, right after:
   dispatch `maintenance.yml` (`gh workflow run maintenance.yml --ref iso-v0`) and read its
   log. Confirm that `security_and_analysis` is readable with the App token, so secret
   scanning and push protection are not reported as drift for want of a permission, and that
   the ruleset parameters re-export cleanly (`export` into a scratch directory, then
   `git diff --no-index` against `rulesets.json` shows no change), so a default GitHub adds is
   not reported as drift. A difference in either is fixed in the files or in the App
   permissions before the daily run is trusted.
3. Check that `gate` reports on pull requests and merge groups (section 8, step 2), because
   the ruleset requires it.
4. Precondition: section 8 steps 4 and 5 are done, so `branch-protection.json` no longer
   requires `Kernel gate` and `Spec gate` and the live protection of `iso-v0` and `main` has
   dropped them. Only `pr.yml` has a `merge_group` trigger; `kernel-build.yml` and
   `spec-build-check.yml` do not, so a merge group never gets those two contexts and a queue
   enabled while they are still required cannot be satisfied. Check with
   `ghsettings.py diff` and by reading the required contexts of both branches, then apply
   the ruleset (`ghsettings.py apply`, then `--yes`), which declares no merge queue, and
   re-export to record what GitHub stored.
5. Create the bot App of PL5, move the bots to it, delete the three personal tokens, then
   set `personal_tokens.retired` to `true` and drop them from `secrets`.
6. At the end of the image key rotation (`athanor-image-1.pub` leaves `system/keys`,
   `docs/operations/secrets.md` section 4.1, step 4): delete `signing` and remove its
   entry from `environments.json`.
7. Last: PB3 pins the two actions still referenced by tag and flips `sha_pinning_required`
   to `true` in `actions.json`; the maintainer then applies it.

`prevent_self_review` is not among these steps: it waits for a second reviewer (section 7).
