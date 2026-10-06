---
paths:
  - ".github/workflows/**"
  - "Justfile"
  - "**/Justfile"
  - "scripts/verify.py"
---

# CI e build

## Valida in locale, non con un push

I workflow del percorso ISO si validano prima di committare:

```
actionlint
python3 scripts/verify.py workflows
bash -n   # su ogni blocco run: non banale
```

Non usare il push come test. Un `startup_failure` su GitHub costa più di trenta
secondi di verifica locale.

## Errori già visti su questo repository

- **Step con solo `name:`**, senza `run:` né `uses:`. GitHub rifiuta l'**intero
  file**, non solo lo step: i workflow non partono affatto. Se ricostruisci uno
  step, dagli un corpo o rimuovilo. Mai lasciarlo vuoto.
- **Blocchi `if ...; then` / `fi` vuoti**. In bash sono errori di sintassi con
  uscita 2, non no-op silenziosi.
- **POST verso servizi esterni** per i log. Usa `actions/upload-artifact` e
  `$GITHUB_STEP_SUMMARY`.

## Vincoli

- Mai aggiungere `|| true` o `continue-on-error` per far passare un job. Un job
  che fallisce sta dicendo qualcosa.
- Un commit per problema, non un commit che sistema tutto.
- Ogni workflow del percorso ISO ha un job `lint` che gira per primo.

## Justfile

`just lint`, `just format`, `just check-syntax` sono le porte d'ingresso.
Il Justfile stesso è formattato da `just --unstable --fmt`: se lo modifichi,
`just check-syntax` deve restare verde.

## Known traps

- Push a change that matches a new workflow's trigger before dispatching it. Why: a workflow on a non-default branch is not registered until then, and `gh workflow run` answers 404.
- Keep the `Kernel gate` check running on every PR to `iso-v0`. Why: it is a required status check there.
- Call reusable workflows that need environment secrets with `secrets: inherit`. Why: they see none otherwise.
- Approve or cancel a run waiting on the `signing` environment. Why: it holds its concurrency group and blocks newer runs.
- Capture output before testing it instead of `nm ... | grep -q` under `pipefail`. Why: grep exits early and the pipeline dies of SIGPIPE.
- Decide registry retention by reachability from tagged manifests, never by "untagged". Why: cosign v3 stores signatures as untagged manifests.
- After a synthetic merge of a stacked PR, diff the commits outside the stack. Why: a squash of a stack can silently revert them.
- Avoid pushing to `kernel-build.yml` while a Kernel Build runs. Why: the push cancels it.
- Run JavaScript actions outside the Nix builder container. Why: it has no Node, so they fail inside it.
- Prove a spec change with a rebuilt tier overlay, not System Image Check alone. Why: on a PR it mounts the published overlays, not the PR's specs.
