# Athanor OS: Platform Experience (Boot, Sicurezza Hardware e User-Land)

Questo documento colma la distanza tra l'infrastruttura di basso livello (Kernel, Mesh, IPC) e l'esperienza tangibile dell'utente finale. Dettaglia la catena di fiducia hardware (Boot) e le interfacce primarie con cui l'utente interagisce dal momento in cui preme il pulsante di accensione.

---

## 1. Boot Flow, UKI e TPM 2.0 (La Catena di Fiducia)

In Athanor OS, l'avvio del sistema non è un semplice caricamento di file dal disco, ma un rigoroso processo crittografico di validazione continua (Measured Boot).

### Unified Kernel Image (UKI)
Abbandoniamo la classica frammentazione (kernel, initramfs, parametri di boot separati) a favore di un singolo file binario firmato digitalmente: l'**UKI**. 
L'intero sistema operativo base viene pacchettizzato in questo payload e firmato crittograficamente (Secure Boot). Se un singolo bit viene alterato, il firmware UEFI della macchina si rifiuta fisicamente di avviarlo.

### Sigillo TPM 2.0 (Trusted Platform Module)
La partizione dei dati dell'utente (`/var/home`), gestita tramite *Bcachefs*, è interamente crittografata con LUKS2.
Invece di chiedere password complesse a ogni avvio, Athanor OS sfrutta il chip TPM 2.0 della scheda madre:
- Durante l'avvio, il sistema misura lo stato del firmware, del bootloader e del kernel UKI registrandoli nei PCR (Platform Configuration Registers) del TPM.
- Le chiavi di decrittazione del disco sono "sigillate" (sealed) all'interno del TPM.
- Il TPM decritta il disco **solo se** l'hash del sistema operativo in fase di avvio corrisponde esattamente a quello firmato in origine. Se un attaccante tenta di manomettere il kernel o di avviare il disco da una chiavetta live, il TPM si rifiuta di rilasciare la chiave crittografica.

---

## 2. L'Esperienza OOBE (Out-Of-Box Experience) e il Greeter

L'impatto iniziale dell'utente con Athanor OS è gestito dal demone `athanor-greeter`, scritto in Rust nativo.

### Il Primo Avvio (OOBE)
> **Superseded** by `doc_first_run.md` (2026-10-05). First run is not part of the greeter, generates no mesh key and signs in to no service (FR19). The text below is kept as history.

Al primo boot di una macchina vergine, il Greeter non mostra un generico desktop vuoto. Lancia un flusso isolato e blindato per:
1. Creare l'account utente amministratore (in un ambiente in cui le password sono gestite con primitive di *Zeroing* in RAM tramite `ZeroizeOnDrop` per prevenire dump della memoria).
2. Generare la coppia di chiavi ellittiche **X25519** che fungeranno da identità crittografica inviolabile per il Mesh.
3. Chiedere il login opzionale a Cloudflare Zero Trust per innescare immediatamente l'adesione della macchina allo Swarm globale.

### Il Login Quotidiano
Il `athanor-greeter` sfrutta Wayland per presentare una schermata di login a latenza zero. Non carica pesanti dipendenze X11 o Web. Valida le credenziali e sblocca il portachiavi (Keyring) dell'utente in un'unica transazione atomica, passando il controllo al compositor `Niri`.

---

## 3. La Dotazione User-Land (App e Desktop)

Athanor OS è per sua natura un sistema ostile al software legacy. Il filesystem principale è immutabile e non prevede l'installazione di programmi tramite i classici `apt` o `dnf` in user-space.

### Flatpak come Standard Assoluto
Tutte le applicazioni grafiche utente (Browser, Editor, Media Player) esistono esclusivamente sotto forma di **Container Flatpak** (o OCI) sottoposti a verifica SLSA Level 4.
Queste app operano in un sandbox stretto (Bubblewrap). Se scarichi un PDF maligno tramite il browser, l'exploit rimane confinato nel filesystem effimero del browser e non può toccare la root del sistema o i tuoi documenti personali senza passare per i portali XDG (`xdg-desktop-portal-athanor`), i quali operano sotto una rigorosa policy **Fail-Closed Zero-Trust** (nessun permesso viene accordato in caso di errore di sistema).

### Le App Predefinite Minimali
Al primo avvio, l'OS fornisce un set di strumenti essenziali curati per non violare l'isolamento:
- **Browser:** Una versione hardenizzata (spesso basata su Firefox/LibreWolf) fornita via Flatpak.
- **Terminal/IDE:** Ambienti di sviluppo forniti tramite podman/toolbx, che permettono all'utente di distruggere e ricreare macchine virtuali di sviluppo senza mai "sporcare" l'OS host.
- **Interface:** COSMIC on cosmic-comp, with Athanor's own surfaces replacing COSMIC's one stage at a time. See [doc_shell.md](doc_shell.md), which supersedes the niri and Relm4 panel described here before 2026-09-18.

---

## 4. Il Portale Web Integrato (Astro.js)

L'anello di congiunzione tra l'utente avanzato e la documentazione del sistema è un vero e proprio portale web servito localmente in `localhost`, eliminando la necessità di cercare wiki online.

### Motore Astro.js e Ricerca Pagefind
Il codice in `system/portal/` genera un sito web statico compresso ad altissime prestazioni. Utilizza `Pagefind` per offrire una barra di ricerca istantanea (Zero-JS) che indicizza tutta l'architettura dell'OS, i log di sistema e i comandi utili.

### Ricerca e Documentazione (Zero-JS e Zero-AI)
L'aspetto pi� avanzato del portale � la sua staticit� estrema. Invece di affidarsi a instabili demoni di traduzione AI (ormai rimossi dall'OS) o pesanti framework JavaScript, il portale utilizza **Astro.js e Pagefind**. L'indicizzazione e la traduzione dei documenti avvengono in fase di build statica, permettendo una ricerca fulminea e a zero overhead sulla macchina locale.

