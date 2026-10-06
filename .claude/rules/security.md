---
paths:
  - "**/polkit*.rs"
  - "**/*.policy"
  - "**/*attestation*/**"
  - "**/*bus-api*/**"
  - "**/*mesh*/**"
  - "**/*cloud-rs/**"
  - "**/*gatekeeper*/**"
  - "system/confidential_computing/**"
---

# Sicurezza — percorsi critici

<!-- I glob sono scritti per FUNZIONE (`*attestation*`, `*bus-api*`, `*mesh*`, `*gatekeeper*`)
     e non per nome di prodotto. La rinomina Ermete -> Athanor del 5 settembre 2026
     aveva gia' spezzato cinque glob su sei scritti con il prefisso vecchio: la
     regola restava nel repository, sembrava configurata, e non si caricava piu'
     su nessuno dei file che deve proteggere. Non reintrodurre prefissi fissi. -->

Stai lavorando su un componente di sicurezza. Qui le regole generali non bastano.

## Niente teatro

Crittografia, validazione dei token, hash e attestazione devono essere **reali**.
`pqc_kyber`, `pqc_dilithium`, `ml-kem`, `sha2`, `zeroize` e `subtle` sono già nel
workspace: usali. Un placeholder, un valore hardcoded o un `return Ok(true)` in un
percorso di autorizzazione **non è una bozza da completare dopo, è una falla**.

Se non puoi implementare la cosa reale in questo passaggio, non scrivere la finta:
fermati e dillo.

## Zero-trust

- Nessun daemon o applicazione fuori da un compartimento o da una MicroVM `crosvm`.
- La policy IPE, il confinamento Landlock e i compartimenti non si aggirano. Se un percorso richiede di saltarli, il percorso è sbagliato.
- Mai `chmod 777`, mai permessi allargati "per far funzionare la cosa".

## Polkit

Il subject polkit deve identificare **il chiamante**, non il bus. È un difetto già
corretto una volta: non reintrodurlo.

Ogni azione polkit dichiarata deve avere il suo file `.policy` installato. Verifica
con `python3 scripts/verify.py polkit`.

## Segreti

Le chiavi non entrano nel repository: `*.key` e `*.pem` sono git-ignored, e ci sono
regole di permesso che ne bloccano la lettura. Se ti serve un valore, chiedi il nome
della variabile d'ambiente, mai il contenuto.

Azzera il materiale sensibile con `zeroize` e confronta con `subtle` per evitare
attacchi a tempo.

## Prima di consegnare

Una modifica qui va rivista dal sub-agente `auditor` con uno scenario di
fallimento concreto, non con un parere. E chiedi conferma all'utente prima di
editare: sono i percorsi che il `CLAUDE.md` marca come da fermarsi.
