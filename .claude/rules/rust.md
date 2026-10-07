---
paths:
  - "**/*.rs"
---

# Rust — Athanor

## Concorrenza senza panic

`panic = "abort"` è impostato **sia su dev che su release**. Un panic non si
propaga e non si recupera: termina il processo. In un daemon di sistema questo
significa perdita di servizio, e in `athanor-agentic-kernel` o nel compositor
significa sessione utente persa.

- Mai `.unwrap()` o `.expect()` su `RwLock` / `Mutex`: un lock avvelenato fa
  cadere a cascata tutto ciò che dipende dal daemon. Propaga con `anyhow::Result`.
- Mai `.unwrap()` su `Option` / `Result` in codice che gira in un daemon o nel
  compositor. Nei test va bene.
- Indicizzazione di slice e aritmetica: `release` ha `overflow-checks = true`,
  quindi un overflow che in altri progetti passerebbe silenziosamente qui aborta.
  Usa `checked_*` / `saturating_*` dove l'input non è sotto il tuo controllo.

## Errori

- `thiserror` per i tipi di errore delle librerie, `anyhow` per la propagazione
  nei binari. Sono entrambi già nelle dipendenze del workspace.
- Un `Err` va propagato o gestito, mai inghiottito. Un `catch` vuoto o un
  `let _ =` su un `Result` in un percorso di sicurezza è un difetto.

## Dipendenze

Le versioni stanno in `[workspace.dependencies]` nel `Cargo.toml` di root.
Nei crate usa `nome = { workspace = true }`. **Non aggiungere una dipendenza
diretta con la sua versione**: rompe l'allineamento del workspace.

Prima di introdurre un crate nuovo, verifica che non ce ne sia già uno che fa
la stessa cosa fra le dipendenze del workspace, e chiedi conferma: `deny.toml`
impone vincoli su licenze e provenienza.

## Prima di modificare

Se il simbolo è condiviso fra crate, `codegraph_impact` prima di toccarlo: il
workspace ha decine di membri e una firma cambiata si propaga più lontano di quanto
sembri. Per una modifica ripetuta su più occorrenze, `/ast-refactor`.

## Verifica

`cargo test -p <crate>` sul crate toccato, poi `just lint`.
La formattazione la fa l'hook: non sistemare indentazione a mano.
