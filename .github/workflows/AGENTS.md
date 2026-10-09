# .github/workflows: area rules

These rules add to the root `AGENTS.md`. Every workflow, secret and variable is described
in `docs/architecture/doc_ci.md`: `python3 scripts/verify.py ci` fails when one is missing
there, or when the document names a workflow file that does not exist.

- **YAML is glue.** Logic lives in scripts under the repository (`system/*.sh`,
  `forge/scripts/`, `scripts/`); a `run:` block is a few lines that call them. Steps exchange
  data through files in a known directory, not only through `$GITHUB_OUTPUT` or artifacts.
  Prefer a standard mechanism (OCI, cosign with a key, a file on disk) to one that exists
  only on GitHub.
- **No literal registry owner.** Images come from `REGISTRY_HOST` and the repository owner,
  never `ghcr.io/ars-regia` written out (`verify.py registry`).
- **Stop and ask** before changing a job that uses the `signing-kernel`, `signing-images`
  or `signing` environment, or the secrets they hold (`docs/operations/secrets.md`).
- **Signing secrets go by name, never `secrets: inherit`.** A signing job reads its secret only
  in the `env` of the step that runs a sign script, in a workflow that is not `workflow_call`;
  signing jobs live in `athanor-forge-orchestrator.yml` (`verify.py workflows`, D43).
- **Never hide a failure.** No `|| true`, no `continue-on-error`. The existing `|| true` are
  known debt, not a model (`grep -n '|| true' .github/workflows/*.yml`).
- **A step with only `name:`** makes GitHub reject the whole file, and an empty
  `if ...; then` / `fi` is a bash syntax error. `verify.py workflows` catches both.
- **Never send logs to an external service.** Use `actions/upload-artifact` and
  `$GITHUB_STEP_SUMMARY`.
- **Keep the required checks running on every pull request**: `Kernel gate`, `Spec gate` and
  `gate` on `iso-v0`, `Kernel gate` on `main` (`.github/settings/branch-protection.json`), and
  `gate` through the `product-branches` ruleset (`rulesets.json`).
- **A workflow on a non-default branch is not registered** until a push matches its
  trigger; `gh workflow run` answers 404 before that.

## Traps already seen

- A run waiting on a signing environment holds its concurrency group and blocks newer runs:
  ask the maintainer to approve or cancel it.
- A push to `kernel-build.yml` while a Kernel Build runs cancels that build (its concurrency
  group cancels in progress).
- A squash of a stacked pull request can silently revert commits outside the stack: after a
  synthetic merge, diff those commits.
- The Nix builder container has no Node: JavaScript actions run outside it.
- System Image Check on a pull request mounts the published tier overlays, not the pull
  request's specs: prove a spec change with a rebuilt tier overlay.

## Checks

A push is not a test. Before committing:

```bash
actionlint
python3 scripts/verify.py workflows ci registry
```

`verify.py workflows` runs `actionlint` only when it is on `PATH`; without it the check
passes with a note; `just check` requires it. CI installs actionlint 1.7.9
(`scripts/ci/install-tools.sh`). Check every pipeline script a workflow calls with `bash -n`.
