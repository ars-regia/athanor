# GitHub settings as code

| Field | Value |
| --- | --- |
| Purpose | Which GitHub settings of the repository are kept in files, and how to export, compare and apply them |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-06. Sections 1 to 5 are fact; section 6 is _(Proposal for the maintainer)_ |
| Depends on | `docs/operations/secrets.md` for secret values (separate change) |
| Defines | GHS1 to GHS9 |

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

Every command first reads `repos/{r}` and stops with exit code 2 and "insufficient rights" unless `permissions.admin` is true: without the admin role GitHub answers 404 on the admin endpoints, which would otherwise read as "off". A 404 counts as "off" only where GitHub documents it so: `vulnerability-alerts` and `pages`. Any other API failure (403, 404, 5xx), an answer that is not JSON, a missing `gh`, and a settings file that is missing, unreadable, not JSON or lacks a key of its area also exit 2 with a message, before the first write.

| Command | Effect | Exit code |
| --- | --- | --- |
| `python3 scripts/github-settings/ghsettings.py export` | Writes the live state to the seven files | 0, or 2 on an error |
| `python3 scripts/github-settings/ghsettings.py diff` | Prints a unified diff per area, files on the minus side, live state on the plus side | 0 when equal, 1 on a difference, 2 on an error |
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

The tests run against a stub `gh` on `PATH`: `python3 -B -m unittest discover -s scripts/tests -p test_ghsettings.py -v`.

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

Since then the files have moved ahead of GitHub (2026-10-07, ADR-0064): the two signing environments of section 7 replace `signing`, and `main` is protected like `iso-v0`. `diff` lists these changes until the maintainer applies them (bootstrap: `docs/operations/secrets.md` section 4).

| Fact | File |
| --- | --- |
| Default branch `iso-v0`; all three merge methods on; auto-merge on; head branches deleted on merge | `repository.json` |
| Description is still `ermete-os` | `repository.json` |
| Dependabot alerts and Dependabot security updates are off; secret scanning and push protection are on; private vulnerability reporting is on | `repository.json` |
| Only `iso-v0` is protected: required check `Kernel gate` (not strict), no review, `enforce_admins` off, force push and deletion off | `branch-protection.json` (now also `main`, section 7) |
| No repository ruleset | `rulesets.json` |
| Environment `signing`: reviewer `hr-mes`, branches `iso-v0` and `main`, admin bypass on, four secrets | `environments.json` until 2026-10-07 (now section 7) |
| Environment `github-pages`: branches `gh-pages` and `main` | `environments.json` |
| Environment `delete`: no rule, no secret | `environments.json` |
| Pages: legacy build from `main:/docs`; `gh api repos/ars-regia/athanor/pages` reports `"status": "errored"` (status is volatile, not stored) | `pages.json` |
| Actions: all actions allowed, SHA pinning not required, default token read-only, Actions cannot approve pull requests, approval required for all external contributors | `actions.json` |

## 6. Open points _(Proposal for the maintainer)_

Each one is a change to a file followed by `apply`; none has been made.

| Point | Proposal |
| --- | --- |
| Description `ermete-os` | Set the Athanor description in `repository.json` |
| Dependabot alerts off | Set `vulnerability_alerts` to `true` |
| Environment `delete` has no rule and no secret, and no workflow names it (`grep -rn "environment:" .github/workflows` finds only `signing-kernel` and `signing-images`) | Delete it by hand and re-export |
| Pages builds `main:/docs` and errors, while [call-system-image.yml](../../.github/workflows/call-system-image.yml) line 180 publishes to a `gh-pages` branch | Decide the Pages source (`gh-pages`, or off) and set `pages.json` |
| `enforce_admins` is off on `iso-v0` and `main`: the admin may push past the required check | Decide whether the single admin should be bound by the branch protection, as the signing environments already bind them (section 7) |
| No scheduled drift check | A workflow running `diff` needs an admin token as a secret; decide whether drift detection is worth that token |

## 7. Signing environments (ADR-0064)

Two environments hold the signing keys, so a release cycle asks for at most two approvals and
no job holds a key it does not use:

| Environment | Secrets | Job |
| --- | --- | --- |
| `signing-kernel` | `SECUREBOOT_SIGNING_KEY`, `MODULE_SIGNING_KEY` | `sign` of `nvidia-kmod.yml`, only when a kernel or NVIDIA change leaves the signed vmlinuz or modules missing |
| `signing-images` | `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD` | `sign-system-images` of `call-system-image.yml` |

Both have the same protection in `environments.json`: required reviewer `hr-mes`; administrator
bypass off (`can_admins_bypass: false`, set by hand: section 4); deployment branches `iso-v0` and
`main`, which `branch-protection.json` protects alike (required check `Kernel gate`, no force
push, no deletion).

`prevent_self_review` stays `false` for now. Every push and every run today is started by the
maintainer's own account, agents included, so with it `true` the only reviewer could never
approve a run and the pipeline would lock its own maintainer out. It turns `true` once agents push
with their own GitHub App identity, a planned follow-up: their runs are then approved by the
maintainer as another person.

`environments.json` is the one place that says which environment holds which key.
`scripts/verify.py workflows` reads it and fails a signing secret read by a job of any other
environment, a secret listed in two environments, a signing environment without a reviewer, with
administrator bypass, or deploying from a branch `branch-protection.json` does not protect by
name. It is a regression guard on the files, not a check of the live settings: `ghsettings.py
diff` is.
