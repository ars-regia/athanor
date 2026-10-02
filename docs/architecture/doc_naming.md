# Nomi: Athanor e Azoth

Specifica della rinomina del progetto. Stato: **approvata il 2026-09-05**. Prova a
secco eseguita lo stesso giorno su un worktree pulito di `iso-v0`: 225 `git mv`, 540
file riscritti, nessun residuo fuori dalle esclusioni, `verify.py` e `actionlint`
invariati, `cargo metadata --locked --offline` verde. L'esecuzione vera aspetta i passi
della sezione 4.

## 1. Decisione

| Cosa                    | Prima                                                      | Dopo                                        |
| ----------------------- | ---------------------------------------------------------- | ------------------------------------------- |
| Il sistema              | Ermete OS                                                  | **Athanor** (in prosa "Athanor OS")         |
| Il kernel               | kernel Ermete / "Chimera"                                  | **Azoth**                                   |
| Prefisso di crate e RPM | `ermete-`                                                  | `athanor-`                                  |
| Namespace D-Bus, polkit | `org.ermete.*`                                             | `org.athanor.*`                             |
| Repository              | `hr-mes/ermete-os`                                         | `hr-mes/athanor`                            |
| Immagini OCI di sistema | `ghcr.io/hr-mes/ermete-os-*`                               | `ghcr.io/hr-mes/athanor-*`                  |
| Immagini OCI del kernel | `ermete-os-kernel[-devel\|-debuginfo]`, `ermete-os-nvidia` | `azoth[-devel\|-debuginfo]`, `azoth-nvidia` |

Perché: "athanor" è il forno alchemico che mantiene la temperatura da solo per
settimane, il sistema immutabile che si aggiorna e si verifica senza presidio;
"azoth" è il mercurio dei filosofi, l'agente vivo dentro il vaso sigillato. I due
nomi stanno nella stessa tradizione da cui viene "Ermete" e la parola "ermetico"
resta vera del sistema: sigillato, riproducibile.

## 2. Regole di riscrittura

Ordinate: la prima che combacia vince. Le regole del kernel precedono quelle
generiche perché `ermete-kernel` non deve diventare `athanor-kernel`.

| #   | Da                                            | A                             | Note                                                                                                             |
| --- | --------------------------------------------- | ----------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| 1   | `forge/specs/ermete-kernel`                   | `forge/specs/azoth`           | directory e ogni riferimento                                                                                     |
| 2   | `ermete-kernel-microvm`                       | `azoth-microvm`               | RPM del kernel guest                                                                                             |
| 3   | `ermete-kernel-builder`, `ermete-kernel-boot` | `azoth-builder`, `azoth-boot` | immagini locali dei workflow                                                                                     |
| 4   | `ermete-os-kernel`                            | `azoth`                       | pacchetti ghcr; suffissi `-devel`, `-debuginfo` restano                                                          |
| 5   | `ermete-os-nvidia`                            | `azoth-nvidia`                |                                                                                                                  |
| 6   | `%buildid .ermete`                            | `%buildid .azoth`             | NVR `7.1.8-100.azoth.fc43`                                                                                       |
| 7   | `ERMETE_KERNEL_CACHE`                         | `AZOTH_CACHE`                 |                                                                                                                  |
| 8   | `KBUILD_BUILD_USER=ermete`                    | `KBUILD_BUILD_USER=azoth`     | `KBUILD_BUILD_HOST=forge` resta                                                                                  |
| 9   | `ermete-mok`                                  | `athanor-mok`                 | chiave pubblica MOK: il `.pem` lo muove l'utente (sezione 4)                                                     |
| 10  | `/usr/lib/ermete/`                            | `/usr/lib/athanor/`           | e ogni altro percorso `.../ermete/`                                                                              |
| 11  | `hr-mes/ermete-os`                            | `hr-mes/athanor`              | URL del repository                                                                                               |
| 12  | `ermete-os-`                                  | `athanor-`                    | immagini OCI di sistema                                                                                          |
| 12b | `ermete-os` | `athanor` | il nome nudo: directory del clone e riferimenti; le forme con trattino sono già consumate |
| 13  | `org.ermete.`                                 | `org.athanor.`                | D-Bus, polkit, portal (i nomi di servizio `os.ermete.*` seguono la regola 18: `os.athanor.*`)                                                                                            |
| 14  | `Ermete OS`                                   | `Athanor OS`                  | prosa                                                                                                            |
| 15  | `ErmeteOS`                                    | `AthanorOS`                   |                                                                                                                  |
| 16  | `ERMETE`                                      | `ATHANOR`                     | costanti, variabili d'ambiente                                                                                   |
| 17  | `Ermete`                                      | `Athanor`                     | tipi Rust, titoli; nel repo "Ermete" indica sempre il sistema, mai il personaggio (0 occorrenze di "Trismegist") |
| 18  | `ermete`                                      | `athanor`                     | crate, identificatori, unità systemd, utenti di servizio, percorsi                                               |

I nomi di percorso seguono le stesse regole: 76 directory di crate, i file sotto
`system/` (`ermete-install.ks`, `99-ermete-hardening.conf`, `ermete-theme-generator.sh`,
`confidential_computing/ermete-attestation`), `.agents/plugins/ermete-mcp-bundle`,
`.agents/skills/ermete-*`. Si usa `git mv` così la storia segue i file.

## 3. Cosa non cambia

- Le parole generiche: `kernel-build.yml`, `kernel-bump.yml`, `kernel-weekly.yml`,
  il job `Kernel gate` (è il check richiesto dalla protezione del branch), `forge`,
  `system`, le label `kernel-bump` e `system-bump`, i rami `bump/kernel-<data>` e
  `bump/system-<data>`.
- Tutto ciò che è upstream: Fedora, CachyOS, NVIDIA, Firecracker, i nomi dei bcond
  e dei config di `kernel.spec`.
- La storia git. Nessun rewrite, nessun amend: la rinomina è commit nuovi.
- I residui in radice `fix_*.py`, `ab_test*.py` (11 file): fuori dalla riscrittura.
- `docs/architecture/graph-vaults/` (2958 file generati): non si riscrive, si
  rigenera con `/graphify` dopo la rinomina, in un commit a sé. Lo stesso per
  `.codegraph/` (`codegraph init`).
- Il runner `TRISMEGISTUS-WSL` e il proprietario GitHub `hr-mes`.
- I secret (`KERNEL_BUMP_TOKEN`, `FORGE_PAT`, `MOK_PRIVATE_KEY`) e l'environment
  `signing`: i nomi non contengono "ermete".

## 4. Passi dell'utente

In quest'ordine, prima dell'esecuzione dello script:

1. **Commit o stash del lavoro locale**: il working tree ha 194 file modificati e 51
   non tracciati (99 sotto `forge/specs`, il resto sotto `system/`). La rinomina
   riscrive quegli stessi file e sposta le loro directory: ciò che non è committato
   andrebbe in conflitto al primo `git pull`. Lo script rifiuta di partire su un
   albero sporco.
2. **Rinomina del repository** su GitHub: `hr-mes/ermete-os` → `hr-mes/athanor`.
   GitHub reindirizza i vecchi URL e il runner self-hosted resta registrato. In
   locale poi `git remote set-url origin https://github.com/hr-mes/athanor` (lo faccio io).
3. **Chiave pubblica MOK**: `git mv forge/specs/ermete-kernel/keys/mok/ermete-mok.pem
.../athanor-mok.pem` e lo stesso per il `.der`. I miei strumenti non possono
   nominare un percorso `*.pem`, quindi questo `git mv` è tuo; il resto della
   directory lo sposta lo script.
4. **Regole di permesso** in `.claude/settings.local.json`: i deny su
   `forge/specs/ermete-kernel/keys/mok/` vanno aggiornati al nuovo percorso
   `forge/specs/azoth/keys/mok/`, altrimenti la protezione della chiave decade.
5. **Directory del clone**: i file di configurazione degli agenti (`.agents/`) e
   `setup_wsl.sh` citano il percorso locale `.../ermete-os/`, che la regola 12b
   riscrive in `.../athanor/`: rinomina la directory del clone di conseguenza.
6. **Pacchetti ghcr** vecchi (`ermete-os-*`): restano finché non li cancelli. I nuovi
   nascono pubblici al primo push perché il repository è pubblico.

## 5. Meccanica: `scripts/rename.py`

Uno script deterministico, committato con questa specifica, che:

1. rifiuta di partire se `git status --porcelain` non è vuoto;
2. **percorsi**: per ogni percorso tracciato che contiene `ermete` (dal più profondo
   al meno profondo) applica le regole della sezione 2 e fa `git mv`;
3. **contenuti**: per ogni file tracciato di testo (non binario, non in sezione 3)
   applica le regole nell'ordine, una passata, e preserva i fine riga del file;
   riscrive anche `Cargo.lock` (le voci `name = "ermete-…"` dei crate del workspace
   e le loro dipendenze: un lockfile non si rigenera a mano, ma qui cambia solo il
   nome e nessun checksum, perché i crate del workspace non ne hanno);
4. **verifica** e si ferma al primo errore:
   - `git grep -i ermete` vuoto, escluse le voci della sezione 3;
   - `scripts/verify.py` non peggiore di prima: la base è già rossa (polkit 1, paths 21,
     shipped 8, docs 12, panics 1, specs 70) e la rinomina non deve aggiungere nulla;
   - `cargo metadata --locked --offline` sul workspace (i nomi dei crate e le dipendenze
     interne tornano coerenti), `cargo check` sui crate Rust puri in WSL, i crate
     GTK nel builder podman;
   - `python scripts/verify.py` (workflow, polkit, percorsi, file spediti, docs);
   - `actionlint` non peggiore di prima (la base ha 87 avvisi shellcheck rinviati dal
     BLOCCO 1); `just check-syntax` a mano in WSL, perché `just` non è sul runner Windows.
5. Stampa il riepilogo: file spostati, file riscritti, sostituzioni per regola.

Esecuzione su un worktree pulito di `iso-v0`, non sul working tree di lavoro.
Risultato: tre commit, `docs+scripts` (questa specifica e lo script),
`refactor: Ermete OS diventa Athanor, il kernel Azoth` (l'output dello script,
un commit solo perché ogni stato intermedio non compila), `docs: rigenera i
graph-vaults`. Push su `iso-v0`.

## 6. Effetti in CI

- **Kernel Build** ricompila: `build.sh` e il Containerfile sono input del riuso e il
  buildid cambia. Pubblica `ghcr.io/hr-mes/azoth:7.1.8-100.azoth.fc43`, `azoth-devel`,
  `azoth-debuginfo`, il tag `-microvm`; il dispatch NVIDIA pubblica
  `azoth-nvidia:<nvr>-open|legacy`. È il gate della rinomina per il kernel.
- **Kernel Weekly**: `repro` confronta la ricostruzione con il nuovo pubblicato, il
  benchmark riparte con etichetta `published` sul nuovo NVR; la storia precedente
  (`ermete`) resta leggibile negli artefatti dei run passati.
- **Kernel Bump**: la tabella dei pin in `KERNEL.md` e `pins.env` non contengono il
  nome; il bot non cambia.
- **Orchestrator forge**: immagini `athanor-builder`, `athanor-system`; la chiave di
  idempotenza cambia con i contenuti, ricostruisce tutto una volta.

## 7. Quando

Dopo il verde di Kernel Build 33965169123 e del primo report di Kernel Weekly con
la variante `o2` (il dato su `-O3` va raccolto sul kernel di oggi, prima che il
tag cambi), e dopo i passi 1-4 della sezione 4.

## 8. Gate

`git grep -i ermete` vuoto salvo la sezione 3; `verify.py` verde; Kernel Build
verde con i tag `azoth`; `cosign verify` sul nuovo tag; l'orchestrator forge
verde sulle immagini `athanor-*`.
