# Athanor

OS immutabile, zero-trust, cloud-native. Workspace Rust.
Desktop GTK4/Wayland, nervo eBPF in ring-0, mesh post-quantistica, rootfs immutabile.
Il kernel e' **Azoth** (`forge/specs/azoth`).

**Rinomina in corso.** Il progetto si chiamava Ermete OS fino al 5 settembre 2026
(commit 02bf9c05). La maggior parte dei crate e' oggi `athanor-*`, ma alcuni sono
ancora `ermete-*` (`ermete-shell-rs`, `ermete-settings-rs`, `ermete-daemon-rs`,
`ermete-ebpf-sched`, `ermete-niri`, `ermete-tetragon`). **Non assumere un prefisso:
verificalo.** Non rinominare nulla di tua iniziativa.

<!-- Le convenzioni per area stanno in .claude/rules/ con `paths:`: entrano in
     contesto solo quando apri i file corrispondenti. Qui solo ciò che serve sempre. -->

## Mappa

- `system/` — livello sistema: kernel, eBPF, compositor, bus IPC, attestazione, mesh
- `forge/specs/<nome>/` — pacchetti e crate applicativi: shell, settings, dock, store, daemon, gatekeeper, e le spec RPM
- `docs/architecture/*.md` — documenti di architettura. Leggi quello dell'area **prima** di modificarla, mai all'avvio

<!-- Nessun conteggio qui dentro di proposito: i numeri di crate e documenti cambiano
     ogni settimana e invecchiano in silenzio. Per la struttura reale usa il grafo
     (`codegraph_explore`), non una lista scritta a mano. -->
- `scripts/verify.py` — verificatore del progetto: workflow, polkit, percorsi, file spediti, documentazione

**Mai entrare in `docs/architecture/graph-vaults/`**: sono 2958 file generati dal
grafo. Interrogali con `/graphify query`, non aprirli.

## Stato corrente

Branch `iso-v0`. Obiettivo: **un'ISO che si avvia e mostra il greeter**, non la 1.0.
Il piano operativo è in `NEXT.md`, a blocchi ordinati con un gate ciascuno: non
passare al blocco successivo finché il gate non è verde. Il contesto diagnostico
è in `ANALISI_2026-09-02.md`.

Il working tree ha centinaia di file modificati non committati. **Verificalo
sempre con `git status` prima di qualunque operazione git distruttiva.**

## Comandi

- Lint: `just lint` — copre forge, system e la sintassi del Justfile
- Formattazione: `just format`
- Verifica sintassi senza modificare: `just check-syntax`
- Verificatore di progetto: `python3 scripts/verify.py` (o `workflows`, `kickstart`, `polkit`, `paths`, `shipped`, `docs`, `services`).
  `workflows` chiama `actionlint` (che a sua volta passa shellcheck sui blocchi `run:`) e `kickstart` chiama
  `ksvalidator` di `pykickstart` sui file `.ks`: se il binario manca il controllo passa con una nota, quindi installali.
- Test: `cargo test -p <crate>`. 52 file contengono test; non esiste una suite unica, mira al crate.
  I test python stanno in `<area>/tests/` e girano con `python3 -B -m unittest discover -s <dir>`.
- Panic letto da un QR code: `python3 scripts/decode-drm-panic.py '<url drm-panic|payload z|->'` stampa
  il log del kernel che drm_panic ha disegnato a schermo, senza passare il payload a un browser.
- Build completa: `just all` — lunga, chiedi prima di lanciarla

## Comandi shell: niente `cd`

La sessione gira già nella root del progetto. **Non premettere `cd <percorso> &&`**
a un comando: i percorsi relativi dopo un `cd` non sono verificabili staticamente
contro le regole di permesso, e ogni comando cosi' costruito richiede
un'approvazione manuale che non servirebbe.

Usa percorsi relativi alla root (`grep -n x .github/workflows/*.yml`), oppure
`git -C <sub>` e `cargo -p <crate>` quando devi agire su un sottoprogetto.
Se ti serve davvero un'altra directory, usa un percorso assoluto nel comando
invece di cambiare directory.

## Standing rules

<!-- Regole decise dall'utente, non apprendimenti. Stavano nella memoria automatica,
     che Claude riscrive e pota da solo: una direttiva permanente li' dentro puo'
     sparire. Vivono qui. -->

- **English on GitHub, Italian in chat.** Commit messages, PRs, issues, workflow
  output, code comments and new documentation are written in English, enterprise
  tone. Conversation with the user stays in Italian. Never rewrite history to
  translate what is already published. *(2026-09-06)*
- **Formal, idiomatic solutions.** Prefer the best-practice fix over the minimal
  patch, for maintainability. No `|| true`, no `continue-on-error`, no band-aid
  that hides a failure instead of resolving it. *(2026-09-03)*
- **Pipeline portable, GitHub as glue.** Logic lives in scripts under the repo, the
  workflow YAML only checks out, calls them and uploads their output: no `run:` block
  beyond a few lines. Steps exchange data through files in a known directory, not
  through `$GITHUB_OUTPUT` or artifacts alone. No hard-coded `ghcr.io/hr-mes`: a
  variable with a default. Prefer a standard mechanism (OCI, cosign with a key, a file
  on disk) over one that exists only on GitHub. Applies to new code and to any file
  touched anyway; no refactoring for its own sake. *(2026-09-10)*

## Limiti inviolabili

- **Modello di minaccia a tre livelli** (`docs/architecture/doc_threat_model.md`): il codice dell'utente non confinato è l'utente; le applicazioni confinate (Flatpak e classe `confined` del broker) sono non fidate e non scrivono i percorsi di persistenza; root e l'immagine sono di `doc_kernel_profile.md`. Una spec dichiara contro quale livello vale ogni sua garanzia.
- **Ogni servizio systemd spedito dal repository imposta `NoNewPrivileges=yes` o `CapabilityBoundingSet=` (allow-list)**, scritto in chiaro (non dedotto da `DynamicUser=` o simili); lo verifica `python3 scripts/verify.py services`. Un'eccezione va nella lista `SERVICE_EXEMPT` con il motivo. Mai `chmod 777`, mai root diretto.
- **Niente finte implementazioni nella sicurezza**: crittografia, validazione dei token e hash devono essere reali. Un placeholder in un percorso di sicurezza è un bug, non una bozza.
- **`panic = "abort"` su dev e release**: un panic non è recuperabile, termina il processo. Nei daemon questo significa perdita di servizio.
- Modifiche a `system/athanor-bus-api/src/polkit.rs`, al Gatekeeper (`forge/specs/athanor-gatekeeper-rs`) o all'attestazione (`system/confidential_computing/athanor-attestation`): fermati e chiedi prima di editare.

## Protocollo scratch

Script temporanei, binari di prova e log vanno in `/.scratch/`, che è git-ignored.
Mai committarli. Nella root ci sono già `fix_*.py` e `ab_test*.py` di sessioni
passate: sono residui, non toccarli e non prenderli a modello.
