---
paths:
  - ".github/workflows/**"
  - "Justfile"
  - "**/Justfile"
  - "scripts/verify.py"
  - "system/*.sh"
  - "system/tests/**"
---

# CI e build

## Valida in locale, non con un push

I workflow e gli script di pipeline (`system/*.sh`, che portano la logica
che i workflow richiamano) si validano prima di committare:

```
actionlint
python3 scripts/verify.py workflows
bash -n   # su ogni blocco run: non banale
```

`verify.py workflows` esegue `actionlint` solo se è nel PATH; altrimenti passa
con una nota senza controllare nulla. Installa `actionlint`. Per gli script di
pipeline vedi anche i test in `system/tests/`.

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
  che fallisce sta dicendo qualcosa. Gli `|| true` già presenti (ad esempio i
  fallback di `chown` in `call-build-builder.yml` e `call-dag-compile.yml`) sono
  debito noto da risolvere, non un modello da copiare.
- Un commit per problema, non un commit che sistema tutto.
- Ogni workflow del percorso ISO ha un job `lint` che gira per primo.

## Justfile

`just lint`, `just format`, `just check-syntax` sono le porte d'ingresso.
Il Justfile stesso è formattato da `just --unstable --fmt`: se lo modifichi,
`just check-syntax` deve restare verde.
