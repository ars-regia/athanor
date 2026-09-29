# NEXT — verso l'ISO v0

Obiettivo corrente: **un'ISO che si avvia e mostra il greeter.** Non la 1.0.
Roadmap completa e motivazioni in chat con claude-7f; stato in `python3 scripts/verify.py`.

Lavora sul branch `iso-v0`. Esegui i blocchi **nell'ordine**, uno per sessione,
in plan mode. Ogni blocco ha un gate: non passare al successivo finché non è verde.

Prima di tutto, se non è ancora stato fatto:

```bash
git checkout -b iso-v0
git add scripts/verify.py forge/specs/*/*/os.ermete.*.policy \
        forge/specs/ermete-{daemon,gatekeeper,cloud,mdm,lvfs,store}-rs/*.spec
git commit -m "fix(polkit): dichiara e installa le azioni polkit mancanti; le spec leggono i file reali"
git add system/ermete-bus-api/src/polkit.rs $(git diff --name-only | grep -E 'src/(dbus|bedrock|main|dbus_interface|daemon)\.rs$')
git commit -m "fix(security): il subject polkit identifica il chiamante, non il bus"
```

---

## BLOCCO 1 — Tappa 0: far partire il CI

```
Contesto: repo ermete-os, branch iso-v0. Leggi ANALISI_2026-09-02.md §2.2, poi
esegui `python3 scripts/verify.py workflows` per l'elenco esatto.

OBIETTIVO — una sola cosa: i workflow devono essere accettati da GitHub e leggibili.
Nessuna feature, nessun refactor, nessuna modifica alla logica dei job oltre a
quanto elencato.

Problema 1 — 14 step con solo `name:`, senza `run:` né `uses:`. GitHub rifiuta
l'INTERO file ("Required property is missing: uses"): i workflow non partono.
Introdotti il 2026-08-31 da 2596fda7. Sono tutti lo step "Set Default Rust
Toolchain". Per ciascuno: ricostruiscilo (dtolnay/rust-toolchain o un `rustup
default` esplicito) oppure rimuovilo. Non lasciare step vuoti.

Problema 2 — 7 blocchi `if ...; then` / `fi` vuoti in rust-security-audit.yml.
In bash sono errori di sintassi (exit 2), non no-op. Ricostruisci l'installazione
di sccache / cargo-deny / cargo-vet / bpf-linker o togli il blocco.

Problema 3 — rimuovi ogni POST verso webhook.site e lo step "Old dpaste".
Sostituisci con actions/upload-artifact e $GITHUB_STEP_SUMMARY.

Problema 4 — aggiungi un job `lint` che gira per primo in ogni workflow del
percorso ISO (orchestrator, build-builder, dag-compile, system-image) e fallisce se
`actionlint` o `python3 scripts/verify.py workflows` trovano qualcosa.

VINCOLI
- Valida in locale: actionlint, `bash -n` su ogni blocco run:. Non fare push per
  testare.
- Nessun `|| true`, nessun continue-on-error aggiunto.
- Un commit per problema.

GATE: `python3 scripts/verify.py workflows` esce 0, e dopo il push
`gh run list --branch iso-v0` non mostra startup_failure.
```

---

## BLOCCO 2 — Tappa 1 + 2: fondamenta e DAG stretto

Prima, a mano, dieci minuti:

```bash
skopeo inspect --no-tags docker://ghcr.io/hr-mes/ermete-base-nvidia:latest | head
# oppure: gh api /user/packages/container/ermete-base-nvidia/versions
```

> Obsolete since 2026-09-16: the system image is built on Fedora's
> `quay.io/fedora-ostree-desktops/base-atomic:43` (docs/architecture/doc_system_image.md, S1),
> so the `ermete-base-nvidia` commands above no longer apply. Kept as history.

Se risponde, vai avanti. Se NON esiste: fermati e dillo a claude-7f — è un blocco
diverso (ricostruire la base o passare a quay.io/fedora/fedora-bootc:43 per la v0).

```
Contesto: repo ermete-os, branch iso-v0. Obiettivo: la build più piccola che
produce un'immagine avviabile. Tutto ciò che non serve al boot esce da QUESTA
build, non dal progetto — è un branch, si ripristina con git.

DA FARE
1. In forge/config/packages.json togli da custom_packages E dai tier:
   mesh-bus, cluster-mesh, mesh-sync, cloud-rs, ai-daemon, ermete-compositor,
   hypervisor-daemon, ermete-net-unikernel, ermete-telemetry, init-oracle,
   sysmon-ebpf, mdm-rs, ermete-greeter, kernel.
   NON toccare il tooling di tier0 (rust-toolchain, mold, sccache, buildah,
   osbuild, cargo-tools, just, uki-tools, ...): serve per costruire il resto.
2. In system/Containerfile togli `rosenpass.service` e `keylime_agent.service`
   dalla riga `systemctl enable`: quei pacchetti hanno una spec ma non sono nel
   DAG, e se la base non li porta il RUN abortisce. Lascia tutto il resto.
3. Verifica che forge/scripts/dag_orchestrator.py produca una matrice coerente
   senza i pacchetti tolti (eseguilo in locale se possibile).
4. Verifica che ogni pacchetto rimasto nel DAG abbia la sua directory in
   forge/specs — `python3 scripts/verify.py shipped` deve scendere, non salire.

VINCOLI
- Nessuna modifica ai crate Rust in questo blocco.
- Nessun `|| true` nel Containerfile.
- Un commit: "build(iso-v0): DAG ridotto alla prova di boot".

GATE: la matrice del DAG contiene solo i pacchetti attesi; `verify.py shipped`
è sceso. Poi: `gh workflow run ermete-forge-orchestrator.yml --ref iso-v0`,
e si legge `gh run view --log-failed`. Fallirà: è normale. Riproduci lo step
nel container ghcr.io/hr-mes/ermete-os-builder in locale prima di correggere.
```

---

## BLOCCO K — il kernel, prima della Tappa 3

Decisione del 2026-09-03: il kernel custom è il primo passo di Ermete OS; la v0
riprende dopo, con il nostro kernel nell'immagine. La specifica approvata è
`docs/architecture/doc_kernel_build.md`: fasi K1–K7, una per sessione, ognuna con
il suo gate (sezione 12). Uso locale e bump a mano in
`forge/specs/ermete-kernel/KERNEL.md`.

---

## Dopo

- **Tappa 3** — cicli di correzione sul DAG finché `ghcr.io/hr-mes/ermete-os-system:<run_id>`
  esiste e `cosign verify` passa.
- **Tappa 4** — ISO: lo step BIB `anaconda-iso`. Se rifiuta `--rootfs=bcachefs`,
  per la v0 usa `btrfs`. Scorciatoia locale: `system/scripts/build_bib.sh <img> <tag> iso` in WSL2.
- **Tappa 5** — boot in VM (mai sul disco per primo), poi hardware. Gate: greeter, login, Settings.
  **Greeter: VERDE il 2026-09-09** — acceptance run 34400804630 su `athanor-iso:34394726609`
  (`greeter reached: greeter-alive`, 152 s dal primo boot; screenshot rivisti). Restano per
  il login e per la pulizia, nessuno bloccante: la card mostra "greetd daemon" invece
  dell'utente umano (`sys/auth.rs:39` legge `$USER`, che sotto greetd è `greetd`); barra
  CSD "Athanor Greeter" con ✕ perché cage non ha layer-shell (sparisce con la Tappa 6, nel
  frattempo `set_decorated(false)` + fullscreen); font delle icone assente (riquadri
  esadecimali); stringhe in italiano ed etichetta "WAYLAND • NIRI" con `lang en_US`;
  `GSK_RENDERER=ngl` è il nome vecchio; il greeter chiede il protocollo Session Lock che
  cage non ha. Login e Settings del gate: prossimi.
- **Tappa 6** — compositore di sessione: **cosmic-comp** (System76, Rust + Smithay) al posto
  di niri. Deciso il 2026-09-09 per l'utente che arriva da Windows/macOS: finestre
  flottanti, barra con minimizza/ripristina, Alt-Tab, aggancio; niri è tiling a scorrimento
  per disegno e il flottante vi è un'eccezione. Greeter e sessione sullo stesso compositore
  (modello cosmic-greeter, greetd resta): via cage e wlroots dal percorso di avvio, che il
  2026-09-09 hanno prodotto un difetto di rendering che non vale per la sessione.
  Fino ad allora **nessuna nuova funzione sull'IPC di niri**: oggi lo nominano 28 file su 185
  (shell 14, settings 5, dock 5, daemon 2, niri-ipc 1), tutto il resto è GTK4 + layer-shell
  e gira invariato. Primo passo: spike di 1–2 giorni, shell e dock sopra cosmic-comp via
  layer-shell, con davanti un utente abituato a Windows. Poi: backend IPC su
  `cosmic-workspace`/`toplevel-info`, niri e `config.kdl` rimossi, residui `athanor-ags`
  cancellati, niri oggi non è nemmeno un RPM. Gate: sessione cosmic-comp con shell, dock e
  Settings; niri, cage e athanor-ags assenti dall'immagine.
  **Greeter su cosmic-comp: VERDE il 2026-09-10** — spike locale (cosmic-comp 1.6.0-3 annidato in
  cage headless: il client riceve `wayland-1`, layer surface accettata, il compositore esce
  con il client), poi commit `d9eb3228` (system-config -25: `cosmic-comp --no-xwayland
athanor-shell-rs --greeter`, sonda wlroots rimossa, cosmic-comp in packages.json) e collaudo
  `34413831911` su `athanor-iso:34409767682`: PASS, `greeter-alive`, 148 s dal primo boot,
  greeter a schermo intero senza barra CSD. Fatti utili per la sessione: cosmic-comp espone
  layer-shell, toplevel-info, toplevel-management e workspace a ogni client senza
  `wp_security_context` (state.rs, `client_not_sandboxed`), quindi shell e dock non hanno
  bisogno di cosmic-session; `athanor-shell.service`/`athanor-dock.service` vivono in
  `athanor-system-services`, che non è nel DAG della v0; la "patch" floating-first di
  `athanor-niri` è un segnaposto di 17 righe. Prossimo: la sessione (`athanor-session` →
  cosmic-comp + unit utente, `sys/auth.rs:160` manda ancora `XDG_CURRENT_DESKTOP=niri`), poi
  cage via dall'immagine.

  **Sessione su cosmic-comp: commit `6a84f676` (2026-09-10)** — `athanor-session` →
  `cosmic-comp /usr/bin/athanor-desktop`; `athanor-desktop` pubblica il display al manager
  utente e attende `athanor-session.target`, fermarlo è il logout. Corsa risolta con un gate
  `athanor-desktop.service` (oneshot, `wayland-info` fino a che il compositore risponde) che
  il target richiede e shell/dock ordinano dopo con `Requisite=`. Rimossi: `niri-session.target`,
  il preset utente (che abilitava anche un `athanor-wallpaper.service` inesistente) e l'alias
  `athanor-ags.service`; l'hook usbguard ora è in %files. Validato in locale fin dove il nesting
  consente (contratto greetd, import env, gate, target su/giù); le unit sul socket di cosmic-comp
  le prova il collaudo su KMS. Boot verificato non rotto: greeter ancora verde sull'immagine
  con la sessione (34488203834; il FAIL 34484197437 era una VM instabile, stallo in
  local-fs/sysinit prima di greetd — rilanciare prima di incolpare una modifica).

  **niri e cage via dall'immagine: `add82378` + `25cc4e08` (2026-09-10)** — spec `athanor-niri`
  cancellata, `niri` tolto da packages.json e dai Requires di shell, daemon e system-config;
  `cage` tolto da packages.json e dal recovery, che ora usa `cosmic-comp --no-xwayland
athanor-recovery-ui`: un solo compositore per greeter, sessione e recovery. Build:
  orchestrator 34492718633.

  **Collaudo del login (2026-09-10)** — il greeter mostrava "greetd daemon" perché
  `discover_target_user` riconosceva solo l'utente `greeter`: ora qualunque account di
  sistema (uid < 1000) fa cercare la persona in /etc/passwd; il campo password ha il focus
  iniziale; la sessione richiesta a greetd dice `XDG_CURRENT_DESKTOP=Athanor`. Il collaudo
  ISO, dopo `GREETER_ALIVE`, digita la password via monitor QEMU (`sendkey`), chiede alla
  macchina via seriale `SESSION_ALIVE` (sessione wayland di collaudo su seat0 + target, shell
  e dock attivi dopo 15 s), scatta `screen-session.png`, e il verdetto PASSA solo con la
  sessione. Poi: il backend IPC su cosmic-workspace/toplevel-info.

  **Login: VERDE il 2026-09-10** — collaudo 34532199424 su `athanor-iso:34528079682`, PASS
  con `session-alive`, greeter a 103 s dal primo boot, sessione 116 s dopo il greeter;
  screenshot: barra della shell, widget del desktop, dock con terminale e Firefox. La catena
  di difetti, tutti nelle unit utente di `athanor-system-services` e tutti trovati facendo
  girare il collaudo: (1) `SystemCallFilter=@system-service` uccideva shell e dock con
  SIGSYS — la diagnostica non diceva quale syscall, ora chiede `ausearch -m SECCOMP` e
  `coredumpctl`: era `mincore` (27), che systemd ha tolto dal gruppo e che lo stack GL
  chiama al primo frame; ammesso esplicitamente (c68af35f); (2) `ProtectHome=read-only`
  lasciava la shell senza posto per `widgets.json` e la cronologia delle notifiche:
  `ConfigurationDirectory=`/`StateDirectory=athanor`, cronologia in `$XDG_STATE_HOME`
  (stesso commit); (3) `MemoryDenyWriteExecute=true` faceva segfault entrambe le unit al
  primo shader compilato da llvmpipe (ip = indirizzo del fault su una pagina appena
  scritta): tolto con motivazione, stesso precedente di athanor-scudo su greetd. Resta del
  gate della Tappa 5: **Settings**. Note dallo screenshot, non bloccanti: due icone del dock
  sono segnaposto; il dock avverte `gdk_wayland_toplevel_compute_size: size.width > 0`.

  **Settings: VERDE il 2026-09-11 — GATE DELLA TAPPA 5 CHIUSO.** Collaudo 34588077759 su
  `athanor-iso:34581338485` (l'ISO col fix della shell, 490b0694): PASS con greeter a 103 s,
  sessione 116 s dopo, Settings 81 s dopo la sessione. Il passo Settings del collaudo
  (547fde2a, b3a69528) avvia `athanor-settings-rs` come unit transiente del manager utente
  dal login seriale, attende l'oggetto D-Bus della finestra (`/os/athanor/Settings/window/N`,
  esportato da GTK) e risponde `SETTINGS_ALIVE` se la unit è ancora attiva cinque secondi
  dopo; lo screenshot, preso all'arrivo della risposta, mostra la finestra "Impostazioni di
  Sistema" sopra il desktop. Note dallo screenshot, non bloccanti: la finestra è più alta
  dello schermo (1280x800) e il dock la copre; si apre direttamente sulla pagina Rete senza
  navigazione visibile; il primo tentativo (34581940472) rispondeva sul processo e non
  sulla finestra, e la finestra non c'era ancora — da qui la sonda sull'oggetto D-Bus.

  **Desktop COSMIC al posto di shell e dock (decisione 2026-09-11).** Il rework della shell
  non si fa: Fedora 43 impacchetta l'intero desktop COSMIC 1.6.0, la stessa release del
  cosmic-comp in immagine, e adottarlo costa un commit invece di settimane. In immagine
  entrano cosmic-panel (pannello e dock, applet come figli), cosmic-bg, cosmic-settings-daemon,
  cosmic-notifications, cosmic-osd, cosmic-launcher (avviato da cosmic-comp sul tasto Super,
  via `system_actions`), cosmic-settings, cosmic-term e le icone; ognuno dei demoni è una
  unit utente di `athanor-system-services` (1.0.1-10) voluta da `athanor-session.target`,
  con la stessa sandbox delle unit di shell e dock, che spariscono. `athanor-shell-rs`
  resta in immagine solo per il greeter; `athanor-settings-rs` resta per le pagine
  zero-trust e per il passo Settings del collaudo, che non cambia. *Aggiornamento
  2026-09-17:* `athanor-settings-rs` è uscita dall'immagine per motivi di sicurezza
  (firma CRDT con chiave generata a ogni scrittura, token Cloudflare sulla riga di comando
  di curl), e da allora il passo Settings del collaudo avvia `cosmic-settings`. La sonda
  verifica che il nome `com.system76.CosmicSettings` sia posseduto sul bus di sessione, che
  la unit sia ancora attiva 20 s dopo e che il suo journal non contenga panic, abort o core
  dump; **non** verifica la finestra, che resta visibile solo nello screenshot. Il gate
  torna alla nostra Settings, con un controllo sul toplevel Wayland, quando l'applicazione
  sarà riprogettata. La sessione annuncia
  `XDG_CURRENT_DESKTOP=Athanor:COSMIC`. Con questo decadono: il backend IPC su protocolli
  cosmic della Tappa 6 (il pannello COSMIC li parla già), la disconnessione Wayland su Esc
  (era in gtk4-layer-shell, che il pannello non usa), il widget Hardware finto, il redraw
  a 50 fps. Il collaudo ora chiede `cosmic-panel.service` e `cosmic-bg.service`. Se in
  futuro si vorrà una shell propria, si scrive in libcosmic sopra questo desktop, un pezzo
  alla volta, senza buttare nulla. Gate: collaudo verde con pannello COSMIC e Settings,
  poi installazione sul disco fisico dell'utente (rischio vero: NVIDIA e MOK, non la UI).

  **Desktop COSMIC: VERDE il 2026-09-11** — commit `531c74c5`, orchestrator 34594986990,
  collaudo 34601095629 su `athanor-iso:34594986990`: PASS al primo giro, greeter a 103 s,
  sessione 117 s dopo, Settings 81 s dopo la sessione. Screenshot: pannello in alto con
  orologio e area di stato, dock in basso con Firefox e la Settings aperta, finestra
  Settings con decorazioni del compositore (titolo, minimizza, massimizza, chiudi). Da
  sistemare, nessuno bloccante: l'ala sinistra del pannello è vuota (i bottoni vogliono
  cosmic-app-library e cosmic-workspaces, non in immagine); i preferiti del dock sono
  segnaposto (default COSMIC che punta a app assenti: si scrive
  `com.system76.CosmicAppList/v1/favorites` con le nostre); lo sfondo è tinta unita
  (nessuno sfondo Pop in immagine: `com.system76.CosmicBackground/v1/all` verso un file
  nostro). Prossimo: la VM locale sulla stessa immagine, poi l'installazione sul disco fisico.

  **Desktop COSMIC completo: VERDE il 2026-09-11** — collaudo 34639821395 su
  `athanor-iso:34633460472`: PASS, pannello con Workspaces e Applications a sinistra,
  dock con launcher, griglia app, Firefox, file manager, editor, terminale, store e
  toggle, tutte icone vere. La caccia ai bottoni vuoti ha rivelato un difetto grosso
  della forge, non tre pacchetti mancanti. **Come i pacchetti entrano nell'immagine**:
  tre canali, non uno. (1) l'immagine base `ermete-base-nvidia` (repo separato, `FROM`
  del Containerfile) porta Fedora e il grosso del desktop COSMIC come **binari Fedora
  ufficiali**; (2) i tier-repo portano solo i pacchetti **con spec locale**; (3) i
  flatpak sul sistema installato. Le quattro liste `upstream_*` di `packages.json` **non
  installavano niente**: le leggeva solo `dag_orchestrator` (per il grafo) e
  `fetch_repo_rpms`, che cercava un'immagine rolling per-pacchetto **mai pubblicata**,
  falliva in silenzio e restituiva successo — così **50 pacchetti su 85 mancavano
  dall'immagine con build verdi** (cosmic-launcher/settings/term/files, mpv, ffmpeg,
  thunar, btop, i font, qemu-kvm). Fix: `c403fd12` installa gli upstream con un
  `dnf5 install` esplicito nel Containerfile (Fedora + RPM Fusion, `--allowerasing` per
  ffmpeg), guidato dal manifest; `3148e1e9` esclude `kernel-forge` (kernel esterno,
  `azoth:<nvr>`) dal nuovo controllo fail-loud che rende fatale un'immagine di pacchetto
  custom mancante — il silenzio ingoiato era ciò che nascondeva i 50 buchi. **Debito
  zero-trust** (decisione utente 2026-09-11): gli upstream sono binari Fedora per ora
  (firma GPG = provenienza); vanno ricompilati da sorgente nel builder e serviti dai
  tier-repo, e allora il nome passa in `custom_tier*` con una spec. Vedi
  [[athanor-forge-package-channels]]. Prossimo: installazione sul disco fisico.

  **VM locale Hyper-V (2026-09-11)** — `athanor-disk.vhdx` installata da `athanor-iso:34528079682`,
  Gen2, Secure Boot off, 4 vCPU, 6 GB, Default Switch; strumenti in `/.scratch/vm/`
  (bridge WMI elevato, tastiera via codici VK, mouse via uinput, SSH per chiave). Greeter e
  sessione partono a velocità nativa; un'ora di uso con la shell ha trovato quello che il
  collaudo CI, con il suo screenshot a pochi secondi dal login, non vede:
  (4) **la shell moriva ogni 60–130 s per systemd-oomd** (peak ~840 MB, `MemoryHigh=768M`),
  27 riavvii in un'ora: `sys/sandbox.rs` applicava una policy Landlock di sola lettura
  limitata a `/usr`, `/etc`, `$XDG_RUNTIME_DIR`, quindi la shell non poteva leggere la
  propria `$XDG_CONFIG_HOME/athanor` (`theme.css: Permission denied` a ogni avvio, tema
  dinamico mai applicato) e `desktop_widgets::load_config`, su qualunque errore di lettura,
  riscriveva il layout di default: scrittura → evento del file monitor → `reload_widgets` →
  lettura negata → scrittura, migliaia di volte al secondo, ogni giro con due timer in più
  che tenevano vivo l'albero dei widget. Fix: Landlock confina le _scritture_ all'insieme
  scrivibile della unit (config/state dir, runtime dir, /tmp) e lascia le letture alla unit;
  il default viene scritto solo se il file manca; i timer tengono riferimenti deboli
  (athanor-shell-rs 1.0.0-29). (5) `athanor-backup-hourly.service` chiamava `dbus-send`,
  che non è nell'immagine: `busctl call` (athanor-backup 1.0.0-4).
  Trovati e non ancora trattati: `athanor-journal-seal` fallisce perché il systemd di Fedora
  è compilato senza FSS (`journalctl --setup-keys`: "Compiled without forward-secure sealing
  support") — la unit va tolta o condizionata; `systemd-modules-load` fallisce sui moduli
  nvidia ("Key was rejected by service") con Secure Boot off — da verificare su hardware con
  la MOK arruolata, e comunque i moduli andrebbero caricati da udev, non forzati da
  modules-load.d su macchine senza GPU; **la shell perde la connessione Wayland chiudendo
  un popover** ("Lost connection to Wayland compositor", exit 1, 3 volte su 4 con Esc su
  Wi-Fi, control center, spotlight; verificato sull'immagine 34581338485): con
  `WAYLAND_DEBUG=1` la sequenza è `zwlr_layer_surface_v1.destroy()` e _poi_
  `wl_surface.attach(nil)+commit()` sulla stessa superficie, cioè un commit su una
  superficie senza più ruolo, e cosmic-comp chiude la connessione senza inviare
  `wl_display.error` (nessuna riga nel journal nemmeno con `RUST_LOG=debug`). È il
  difetto noto di gtk4-layer-shell su COSMIC (pop-os/cosmic-comp#2159, marzo 2026, lo
  stesso di Ghostty; l'ordine giusto è unmap, poi destroy del ruolo, poi della
  superficie), ancora aperto in gtk4-layer-shell 1.3.0 ad agosto 2026. Rimedio da
  scegliere: patch a gtk4-layer-shell nel nostro DAG (ordine di teardown), oppure la
  decisione del blocco shell (cosmic-panel non usa GTK). Finché resta, ogni popover
  chiuso può riavviare la barra (2 s di buco);
  il widget "Hardware" mostra valori finti (`get_memory_usage` → `(4096, 16384)`);
  il dock lancia le app via IPC niri (`NIRI_SOCKET` assente: ogni click è a vuoto — Tappa 6)
  e ha `org.gnome.Terminal.desktop` fissato mentre l'immagine ha `foot`; la barra ridisegna
  a ~50 fps anche a riposo (animazione dell'indicatore, costa CPU su llvmpipe);
  `ethernet.cloned-mac-address=random` (`99-mac-randomization.conf`) lascia la VM senza
  rete su Hyper-V, che scarta i frame con MAC diverso da quello assegnato — decisione di
  prodotto (MAC casuale anche su ethernet vale il costo su switch gestiti e prenotazioni
  DHCP?); `vconsole.keymap=` vuoto nella riga di comando del kernel installato; mcelog
  fallisce in VM ("CPU is unsupported", innocuo). Requisito x86-64-v3: sotto un
  hypervisor senza AVX2 la shell muore in SIGILL senza messaggio. `bootc status` nella
  VM: `/etc/containers/policy.json` dell'immagine è `insecureAcceptAnything`, quindi
  `bootc switch`/`upgrade` accetta qualunque immagine senza verificare la firma cosign
  che la pipeline produce (da chiudere con una policy `sigstoreSigned` sulla chiave del
  progetto); la versione dichiarata dell'immagine è ancora `latest.20260705` della base.
  L'aggiornamento di una VM installata da ISO non è `bootc upgrade` (il tag è fissato al
  run id) ma `bootc switch --apply ghcr.io/hr-mes/athanor-system:<run id>`.
  **DNS: il sistema installato non risolve nulla sulla rete dell'utente.**
  `99-dns-tls.conf` impone `DNSOverTLS=yes` (stretto) con DNSSEC su 1.1.1.1/9.9.9.9, e
  il resolver del DHCP (senza DoT) viene comunque provato per primo sul link: la rete
  di casa dell'utente rifiuta la porta 853 (verificato anche dall'host Windows), quindi
  `bootc switch` falliva con "lookup ghcr.io: i/o timeout". Un OS che senza 853 non ha
  DNS è inusabile su molte reti (ISP e aziende la bloccano): decisione di prodotto tra
  `opportunistic` (cifra quando può, degrada in chiaro con avviso) e stretto con un
  fallback esplicito. Nella VM: drop-in `99-vm-dns.conf` con `opportunistic` e
  `ipv4.ignore-auto-dns yes` sulla connessione.
  Sulla LAN dell'utente il DHCP imposta un hostname transitorio preso dal reverse DNS del
  provider (`customer.….isp.starlink.com`): il sistema installato non ha hostname statico
  (`hostnamectl`: unset), il kickstart o l'installer dovrebbe fissarlo.

## BLOCCO P — kernel and platform profile

Specification: [docs/architecture/doc_kernel_profile.md](docs/architecture/doc_kernel_profile.md), approved at gate P0 on 2026-09-14. Blocks run in order behind their gates (section 15); spike S1 runs alongside P1–P4a.

- **P1**: `forge/specs/athanor-kernel-profile`, holding `profile.toml`, `kernel_profile.py` (validator over every role combination and generator of the effective profiles), `athanor-profile-check` and the acceptance marker `profile-ok`. Plan: [docs/superpowers/plans/2026-09-14-kernel-profile-p1.md](docs/superpowers/plans/2026-09-14-kernel-profile-p1.md). Gate: ISO acceptance PASS with `profile-ok`.
- **P2**: kernel build profile (section 5) and boot matrix assertions. Gate: Kernel Build gate green.

## BLOCCO S — shell, stage 1

Specification: [docs/architecture/doc_shell.md](docs/architecture/doc_shell.md), approved on 2026-09-18 after two review rounds. COSMIC stays underneath; stage 1 adds a design system, the update experience, the trust state and the greeter's seal. Packages run in this order, each with its own plan:

- [x] **Spikes P1–P3** (GTK4 applet in cosmic-panel; gtk4/relm4 bump on the greeter; headless screenshots). Each gates the plan named in section 3 of the spec. Ran on 2026-09-18.
- [x] **1b-system**: `doc_update_trust.md` written, audited and consented to by the maintainer; then key-based signature, policy, download timer, state file, helper, notifier. Dev VM acceptance passed 2026-09-24; the project key and its policy went in on 2026-09-25. The first signed Orchestrator run and the first promotion follow the merge.
- [x] **1a**: design system and the greeter on it. PR #55.
- [ ] **1d**: the seal in the greeter. It replaces 1b-shield, whose applet in cosmic-panel is not built: the session shield is born in our bar (BLOCCO S2).
- [x] **1c**: layout document, three presets, two knobs, chooser. PR #63; its translator to cosmic-panel is a bridge until the switch of stage 2.

Gate: section 7 of the spec, on a fresh install in the dev VM and on the maintainer's desktop upgraded in place.

## BLOCCO S2 — shell, stage 2

Specification: [docs/architecture/doc_shell.md](docs/architecture/doc_shell.md), revision 5 (2026-09-25), sections 3 and 8. Of COSMIC only cosmic-comp stays; stage 2 brings our bar and dock, enabled by hand until they pass the replacement rule of SH1. Packages run in this order, each with its own plan:

- [x] **Spike P4**: compositor privileges and the second connection, five questions (section 3 of the spec). Gates 2a. Run 2026-09-25: all five answered, revision 5 stands; the licence of `cosmic-client-toolkit` is open for 2a.
- [x] **2a**: `athanor-compositor-client` and the boundary check in `scripts/verify.py`.
- [x] **`doc_bar.md`**: specification of the bar and the dock, written after P4 and reviewed. Gates 2b. Approved 2026-09-25 (revision 1).
- [ ] **2b**: our bar, with tray, notifications and the shield.
- [ ] **2c**: our dock.
- [ ] **Switch**: bar and dock become the default; cosmic-panel, cosmic-applets, cosmic-notifications and the translator leave the image.

Gate: section 8 of the spec, after the switch, on a fresh install in the dev VM and on the maintainer's desktop upgraded in place.

## Shell — lacune funzionali verso un utente Windows/macOS

> Superseded on 2026-09-18 by BLOCCO S: the critical analyses behind `doc_shell.md` found this shell to be a niri satellite whose styling never rendered. The three gaps below stay valid as requirements for the stage that owns each surface.

Analisi statica della shell 2026-09-10 (`athanor-shell-rs`, 83 file, ~15.6k righe vive):
sana — zero unwrap/expect/panic/unsafe nel codice vivo, clippy pulito, niente TODO,
codice morto `snap_overlay*` rimosso (2aaae73f). Per ampiezza batte le shell open-source
artigianali, è alla pari con COSMIC come portata, sotto GNOME/KDE/macOS solo sulla coda
lunga e la rifinitura. Le tre lacune più sentite da chi arriva da Windows/macOS, tutte
piccole e indipendenti dal compositore — da fare dopo che la sessione cosmic-comp è verde:

- [ ] **On-screen keyboard.** Manca del tutto (0 file). Blocca l'uso touch e il login su
      hardware senza tastiera fisica. GNOME/KDE/COSMIC ce l'hanno. È la lacuna più grave
      perché può rendere una macchina inutilizzabile, non solo scomoda.
- [ ] **Night light / temperatura colore.** Manca (0 file). Regolazione attesa ovunque;
      su Wayland si fa con `wlr-gamma-control` — cosmic-comp lo espone, quindi è un pannello
      nel control center + un client gamma, niente di compositore-specifico.
- [ ] **Profili di alimentazione** (prestazioni / bilanciato / risparmio). Manca (0 file).
      Si appoggia a `power-profiles-daemon` via D-Bus, come batteria e upower già fanno.

Note minori dalla stessa analisi, non bloccanti: `sys/auth.rs` sblocca il keyring in
silenzio (`let _ = proxy.unlock_keyring`) — un fallimento non è segnalato, vale almeno un
`warn`; accessibilità (`voiceover.rs` + pagine a11y nei settings) è agli inizi.
Metodo di validazione deciso: sviluppo su Windows, build nel builder podman, ogni feature
passa il collaudo CI (screenshot rivisti dall'utente).

## Pipeline — portabilità dal CI (dopo il gate della Tappa 5)

Misura del 2026-09-10: 5186 righe di script portabili contro 3325 di YAML, di cui 1429
di shell inline nei `run:`; 33 script su 41 non usano nulla di GitHub. La regia
(`needs`, matrici, `workflow_call`) resta nel dialetto di Actions per scelta: non vale
un orchestratore proprio. Il debito è lo shell inline, concentrato in due file, e va
estratto quando quei file si toccano comunque, non prima.

- [ ] `call-dag-compile.yml` (385 righe inline): ciclo rpmbuild per tier e publish su
      ghcr in uno script chiamato dal YAML, provabile in locale nel builder.
- [ ] `call-system-image.yml` (231 righe inline): costruzione dell'immagine e firma
      in script; `sign_attest.sh` e `sbom_rootfs.sh` esistono già, il resto li segue.
- [ ] Origine del registro come variabile unica con default `ghcr.io/hr-mes`, usata da
      workflow, `athanor-install.ks`, `athanor-store` e `athanor-forge.repo`.
- [ ] Per la 1.0, non per la v0: radice di fiducia della firma fuori dall'OIDC di GitHub
      (chiave propria con HSM, o Fulcio/Rekor privati). È sovranità, non portabilità.

Le 76 spec segnalate da `verify.py specs` non bloccano il boot: vanno nella v1,
insieme alla compilazione della Fase 1 e al boot test della Fase 2
(prompt in PIANO_RIPARTENZA.md).
