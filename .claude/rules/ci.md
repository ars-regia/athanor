---
paths:
  - ".github/workflows/**"
  - "Justfile"
  - "**/Justfile"
  - "scripts/verify.py"
  - "system/*.sh"
  - "system/tests/**"
---

# CI and build

The area rules are in `.github/workflows/AGENTS.md`; this file adds local checks and traps.

## Validate locally, not with a push

Validate workflows, and the pipeline scripts they call (`system/*.sh`), before committing:

```
actionlint
python3 scripts/verify.py workflows
bash -n <script>   # and on every non-trivial run: block
python3 -B -m unittest discover -s system/tests
```

`verify.py workflows` runs `actionlint` only when it is on `PATH`; otherwise it passes with
a note and nothing of actionlint's is checked. Install `actionlint`, and `shellcheck`, which
it runs on every `run:` block.

Never use a push as the test. A `startup_failure` on GitHub costs more than thirty seconds of
local checks.

## Errors already seen in this repository

- **A step with only `name:`**, no `run:` and no `uses:`. GitHub rejects the **whole file**,
  not just the step, and no workflow in it starts.
- **Empty `if ...; then` / `fi` blocks.** In bash they are syntax errors with exit code 2,
  not silent no-ops.
- **POST requests to external services** for logs. Use `actions/upload-artifact` and
  `$GITHUB_STEP_SUMMARY`.

## Constraints

- Never add `|| true` or `continue-on-error` to make a job pass. The existing `|| true` are
  known debt to resolve, not a model: for example the `chown` fallbacks at
  `call-build-builder.yml:81,88,90` and `call-dag-compile.yml:217,224,226`, and
  `buildah rm "$ctr" || true` at `call-dag-compile.yml:173`;
  `grep -n '|| true' .github/workflows/*.yml` lists them all.
- One commit per problem, not one commit that fixes everything.

## Justfile

`just lint`, `just format` and `just check-syntax` are the entry points. The Justfile itself
is formatted by `just --unstable --fmt`: if you change it, `just check-syntax` must stay
green.

## Known traps

- Push a change that matches a new workflow's trigger before dispatching it. Why: a workflow on a non-default branch is not registered until then, and `gh workflow run` answers 404.
- Keep the `Kernel gate` check running on every PR to `iso-v0`. Why: it is a required status check there.
- Call reusable workflows that need environment secrets with `secrets: inherit`. Why: they see none otherwise.
- Approve or cancel a run waiting on the `signing` environment. Why: it holds its concurrency group and blocks newer runs.
- Capture output before testing it instead of `nm ... | grep -q` under `pipefail`. Why: grep exits early and the pipeline dies of SIGPIPE.
- Decide registry retention by reachability from tagged manifests, never by "untagged". Why: cosign v3 stores signatures as untagged manifests.
- After a synthetic merge of a stacked PR, diff the commits outside the stack. Why: a squash of a stack can silently revert them.
- Avoid pushing to `kernel-build.yml` while a Kernel Build runs. Why: its concurrency group cancels the running build.
- Run JavaScript actions outside the Nix builder container. Why: it has no Node, so they fail inside it.
- Prove a spec change with a rebuilt tier overlay, not System Image Check alone. Why: on a PR it mounts the published overlays, not the PR's specs.
