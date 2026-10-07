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
- **Stop and ask** before changing a job that uses the `signing` environment or the secrets
  it holds (`docs/operations/secrets.md`).
- **Never hide a failure.** No `|| true`, no `continue-on-error`. The existing `|| true` are
  known debt, not a model (`grep -n '|| true' .github/workflows/*.yml`).
- **A step with only `name:`** makes GitHub reject the whole file, and an empty
  `if ...; then` / `fi` is a bash syntax error. `verify.py workflows` catches both.
- **Never send logs to an external service.** Use `actions/upload-artifact` and
  `$GITHUB_STEP_SUMMARY`.
- **Keep the `Kernel gate` check running on every pull request**: it is the one required
  status check on `iso-v0` (`.github/settings/branch-protection.json`).
- **Reusable workflows that need environment secrets** are called with `secrets: inherit`.
- **A workflow on a non-default branch is not registered** until a push matches its
  trigger; `gh workflow run` answers 404 before that.

## Checks

A push is not a test. Before committing:

```bash
actionlint
python3 scripts/verify.py workflows ci registry
```

`verify.py workflows` runs `actionlint` only when it is on `PATH`; without it the check
passes with a note. CI installs actionlint 1.7.9 with shellcheck (`call-lint.yml`).
