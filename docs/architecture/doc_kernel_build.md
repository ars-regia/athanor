# Athanor OS: Specifica del Kernel (costruzione, pin, manutenzione automatica)

Stato: **approvata il 2026-09-03** (serie `stable` 7.x, `-O2` dal 2026-09-05, debuginfo
come OCI separato con retention di due versioni; dal 2026-09-04 Rust acceso,
ThinLTO e `RANDSTRUCT` spenti, sezione 13). Sostituisce il
README "Testo Sacro" di `forge/specs/azoth/` e lo script
`prepare-chimera.sh`. Il livello funzionale del kernel (eBPF, KVM, Gatekeeper) è
descritto in [doc_kernel_layer.md](doc_kernel_layer.md): questo documento dice
**come il kernel viene costruito, pinnato, firmato e mantenuto**, e quali garanzie
deve dare a quel livello.

Decisioni già prese con il maintainer:

- il kernel è custom e viene prima della v0; la v0 riparte con questo kernel;
- "rolling": segue Fedora stable da vicino, non necessariamente l'ultima minor;
- compatibile con i PC x86-64 dal 2014 in poi, GPU AMD, Intel e NVIDIA;
- massimo tecnico possibile con manutenzione e debito tecnico minimi: il
  sistema si auto-mantiene, l'umano interviene solo quando qualcosa è rosso.

## 1. Principi

1. **Nessuno spec forkato.** Lo spec del kernel Fedora (kernel-ark) ha due ganci
   nativi per esattamente questo uso: `Patch999999: linux-kernel-test.patch`,
   applicato con `git apply` dopo la patch Red Hat, e `Source3001: kernel-local`,
   fuso nei config da `merge.py` con il controllo di coerenza di
   `process_configs.sh`. Athanor fornisce quei due file e i bcond. Lo spec non
   viene mai modificato con `sed`.
2. **Tutto pinnato, tutto verificato.** Un solo file di pin (`pins.env`) e un
   manifest `sources.sha256`. Il SRPM Fedora è verificato con la firma GPG di
   Fedora e con l'hash; il tarball CachyOS con la firma dei suoi maintainer e con
   l'hash; le patch singole con l'hash. Nessuna risoluzione "dinamica" a build
   time: la scelta della versione avviene in una PR, mai nel job di build.
3. **Le ottimizzazioni sono opzioni di prima classe.** Ogni scelta è un'opzione
   Kconfig o una patch che upstream o CachyOS mantengono. Niente `sed` sui
   Makefile, niente `-Wno-error`, objtool acceso. Una patch che non si applica
   fa fallire la build (`git apply` è senza fuzz), non viene "saltata". Una patch
   di terzi che smette di entrare si rinfresca una volta, con revisione umana, e
   la copia rinfrescata entra anch'essa senza fuzz (sezione 8).
4. **I gate falliscono forte.** Config non onorato, patch non applicata, boot
   fallito, kmod NVIDIA che non compila: ognuno è un fallimento della PR di
   bump, con il messaggio esatto. Nessun `|| true`.
5. **Un solo punto di verità per versione.** `pins.env` + `KERNEL.md` generato
   dal bot. Il config effettivo è leggibile sulla macchina accesa
   (`/proc/config.gz`).

## 2. Sorgenti e pin

Directory `forge/specs/azoth/` dopo il blocco:

| File                     | Ruolo                                                                                                                                                                                                                                                                            |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pins.env`               | `FEDORA_KERNEL_NVR` (es. `7.1.12-100.fc43`), `FEDORA_SOURCE_RELEASE` (43), `CACHYOS_RELEASE` (es. `cachyos-7.1.8-1`), `CACHYOS_PATCHES_COMMIT`, `KERNEL_CHANNEL` (`stable` o `lts`)                                                                                              |
| `SOURCES/sources.sha256` | hash del SRPM, del tarball CachyOS, delle patch singole; lo scrive `build.sh --stage manifest`                                                                                                                                                                                   |
| `kernel-local`           | frammento di config, una riga di motivazione per opzione                                                                                                                                                                                                                         |
| `patches.list`           | patch di `CachyOS/kernel-patches` da accodare dopo la base, in ordine                                                                                                                                                                                                            |
| `patches/refreshed/`     | copie rinfrescate delle patch di `patches.list` che non entrano più senza fuzz, stesso percorso relativo; il preambolo registra commit e SHA-256 del file upstream da cui derivano (sezione 8)                                                                                   |
| `patches/`               | patch di Athanor in formato git, applicate dopo `patches.list` in ordine di nome; il messaggio spiega il perché, e ogni patch è candidata all'upstream                                                                                                                           |
| `patches/redhat/`        | patch di Athanor al codice che aggiunge solo la patch Red Hat: vanno soltanto sull'indice Fedora, non sull'albero CachyOS della derivazione del config, e non toccano Kconfig                                                                                                    |
| `fedora-wins.list`       | percorsi in cui un conflitto del merge tra base CachyOS e patch Red Hat si risolve con l'albero Fedora; ogni altro conflitto ferma la build                                                                                                                                      |
| `cmdline`                | riga di comando del kernel, firmata nella UKI (sezione 6)                                                                                                                                                                                                                        |
| `boot.sh`, `boot/`       | la boot matrix (sezione 7, gate 3): ambiente QEMU/OVMF/shim pinnato come il builder, PID 1 dell'initramfs di prova con le asserzioni                                                                                                                                             |
| `builder/Containerfile`  | ambiente Fedora 43 (esiste già), base pinnata per digest                                                                                                                                                                                                                         |
| `builder/toolchain.lock` | gli RPM installati sopra la base del builder, per sha256: strumenti e BuildRequires dello spec pinnato (sezione 5)                                                                                                                                                              |
| `*/*.packages`, `*/*.lock` | per ogni stadio degli ambienti `boot/` e `nvidia/` (`boot/schbench`, `boot/toolchain`, `nvidia/toolchain`), i pacchetti e il loro lock per sha256 (sezione 5)                                                                                                                  |
| `lock.sh`                | rigenera i lock (`generate`), li controlla (`check`) e li installa da koji nei Containerfile (`install`)                                                                                                                                                                        |
| `bconds.sh`              | i bcond di kernel.spec, gli stessi per `dnf builddep`, `rpmbuild` e il lock                                                                                                                                                                                                      |
| `build.sh`               | l'intera build, riproducibile in locale e in CI                                                                                                                                                                                                                                  |
| `build-inputs.py`        | gli input che cambiano gli RPM come JSON: predicato dell'attestazione dei pin e chiave del riuso (sezione 7)                                                                                                                                                                     |
| `keys/`                  | profili e generatore delle chiavi di firma (`profiles/`, `generate.sh`); certificati pubblici della chiave Secure Boot (`secureboot/`), della chiave dei moduli (`modules/`) e delle chiavi ritirate (`revoked/`) (sezione 6); le chiavi private sono nell'environment `signing` |
| `microvm/`               | config e spec del kernel guest (sezione 9)                                                                                                                                                                                                                                       |
| `KERNEL.md`              | cosa c'è nella directory, uso locale, bump; il bot (K5) ne riscrive la tabella dei pin                                                                                                                                                                                           |

Spariscono: `prepare-chimera.sh`, `build-local.sh`, `cachyos-patches/` (1031
file, 7,4 milioni di righe), `patches/0001-acs-override.patch` (rompe
l'isolamento IOMMU, incompatibile con zero-trust), `fedora-nvidia.repo` (vive già
in `forge/assets/repos/`), `README.md`.

**Sorgente 1, Fedora.** SRPM da koji all'NVR pinnato
(`https://kojipkgs.fedoraproject.org/packages/kernel/<ver>/<rel>/src/kernel-<nvr>.src.rpm`).
Quel file non porta firma: koji conserva le copie firmate
(`data/signed/<chiave>/`) solo per le build più recenti e le pota dopo, ma tiene
per sempre l'header di firma in `data/sigcache/<chiave>/src/<nvr>.src.rpm.sig`.
`build.sh` scarica entrambi (hash nel manifest), li ricuce con
`koji.splice_rpm_sighdr` (la stessa funzione con cui koji produce le copie
firmate: il risultato è byte per byte quello del mirror) e verifica con
`rpmkeys --checksig` contro la chiave Fedora della release, la cui impronta è in
`pins.env`. Il SRPM contiene il tarball vanilla, la patch Red Hat, i config,
`merge.py`, `process_configs.sh`: tutto il macchinario di packaging.

**Sorgente 2, CachyOS.** Dal 7.x CachyOS non pubblica più una patch "base" ma il
proprio albero come release firmata su GitHub
(`cachyos-X.Y.Z-N.tar.gz` + `.asc`, chiavi `E18447AC…` e `E8B9AA39…`), che
contiene BBRv3, l'opzione `-O3`, i tuning di scheduler e memoria, `more-uarches`.
La base Athanor è il merge a tre vie, deterministico, di quell'albero sull'albero
Fedora (vanilla + patch Red Hat) con base il tarball vanilla della stessa `X.Y.Z`
(kernel.org, firma PGP): generato in build da input hashati, non conservato nel
repo. Serve il merge, non un `diff` applicato: la patch Red Hat porta backport
(ISP4 AMD in 7.1) che CachyOS ha già, e un diff li aggiungerebbe due volte; il
merge fonde le aggiunte identiche; su quelle divergenti vince Fedora solo per i
percorsi elencati in `fedora-wins.list` (in 7.1: `MAINTAINERS` e `isp4/Kconfig`),
altrove la build si ferma. BORE e
le altre patch scelte arrivano da
`CachyOS/kernel-patches` al commit pinnato, elencate in `patches.list`
(`sched/0001-bore-cachy.patch` è la prima). Il tutto, in ordine, diventa
`linux-kernel-test.patch`.

**Serie del kernel.** CachyOS segue l'ultima stable (oggi 7.2) e la LTS (6.18);
Fedora 43 è sulla 7.1 e verrà ribasata. La coppia va presa **a parità di patch
level**: il diff base è calcolato sul vanilla `X.Y.Z` e entra solo in un albero
`X.Y.Z` (provato in K1: la base 7.1.8 non entra nel 7.1.12 di Fedora, che ha già
il backport ISP4 AMD e altri hunk divergenti). Regola del bot: la release CachyOS
più recente della serie `X.Y`, e l'NVR Fedora stable con lo stesso `X.Y.Z`,
cercato prima nella release Fedora di base (43) e poi nella successiva (i kernel
Fedora sono autonomi: un SRPM di Fedora 44 si ricostruisce e gira su una rootfs
43). Koji conserva per sempre SRPM e header di firma, quindi un NVR non più
"latest" resta pinnabile. Se la coppia non esiste, la PR è rossa e decide
l'umano. `KERNEL_CHANNEL=lts` sposta la stessa logica sulla 6.18.

**Obbligo di bump alla fine vita** (decisione del maintainer, 2026-09-14). Il bot
legge `https://www.kernel.org/releases.json`: se la serie che resterebbe pinnata
non è tra le `stable` e `longterm` non EOL, il job `check` fallisce e nessun altro
bump passa finché una coppia non la sposta. Una serie a fine vita non riceve più
correzioni; restarci in silenzio è il caso della 7.1, EOL il 2026-09-02 mentre
Azoth era fermo a 7.1.8 perché il cron del bot non partiva da `main`.

## 3. La build (`build.sh`)

Gira nel container `builder/Containerfile` sul runner self-hosted (16 core) e
identica in locale. Passi, tutti senza rete tranne i download verificati:

1. scarica nella cache e verifica: hash di ogni file contro il manifest, firma
   PGP del tarball CachyOS e di quello vanilla contro le chiavi vendorizzate,
   firma RPM del SRPM ricucito;
2. scrive `~/.rpmmacros` con `%_topdir` e `%buildid .azoth`; `rpm -i` del SRPM;
   `dnf builddep -y SPECS/kernel.spec` con gli stessi bcond di rpmbuild e ogni
   repository spento, dopo le patch (passo 3) e prima della derivazione del config,
   che deve vedere la toolchain vera (rust-src, bindgen, pahole: `RUST_IS_AVAILABLE`
   e le opzioni che ne dipendono): le BuildRequires le ha già installate il builder
   dal lock (sezione 5), quindi è un controllo, e uno spec che chiede altro ferma
   prep; una patch che non entra ferma prep in pochi secondi;
3. genera `linux-kernel-test.patch`: repo git temporaneo con tre commit (vanilla,
   CachyOS, vanilla + patch Red Hat), `git merge-tree --write-tree` dei due rami
   sopra il vanilla, `patches.list`, `patches/` e `patches/redhat/` applicate
   sull'indice (per ogni voce di `patches.list` la copia in `patches/refreshed/`
   se esiste, rifiutata se registra un altro file upstream o se il file upstream
   entra di nuovo senza fuzz), diff dal commit Fedora al risultato. `patches.list` e `patches/`
   vanno anche sull'albero CachyOS estratto, che serve al passo 4; `patches/redhat/`
   no, perché quell'albero non contiene il codice Red Hat;
4. genera il `kernel-local` completo: il delta Athanor committato, più le opzioni
   che l'albero introduce (`make listnewconfig` sul config Fedora fuso con i
   frammenti clang e con il delta, iterato fino a convergenza) con il valore del
   config CachyOS pinnato o, se assente lì, il default Kconfig. Così il gate
   `-n` di `process_configs.sh` non trova opzioni senza decisione;
5. riduce lo spec a x86_64: gli altri `kernel-*-fedora.config` diventano
   `# EMPTY`, il valore che `process_configs.sh` salta per contratto;
6. `rpmbuild -bp --with toolchain_clang --with clang_lto --without debug
--without tools --without perf --without libperf --without bpftool --without
ynl --without selftests --without doc`: patch e `process_configs.sh -w -n -c`.
   Poi il gate di Athanor: ogni riga del delta committato deve valere nel config
   generato (Fedora segnala i mismatch solo sulle opzioni presenti nel
   risultato, un'opzione caduta per dipendenza non soddisfatta passerebbe in
   silenzio). Config e `kernel-local` finiscono nell'artefatto;
7. stadio `build`: `rpmbuild -bb --noprep` sullo stesso albero. Il debuginfo si
   costruisce e si pubblica a parte: serve a `perf`, `crash`, a un futuro AutoFDO
   e non entra nell'immagine;
8. riproducibilità: `SOURCE_DATE_EPOCH` dalla changelog, `KBUILD_BUILD_USER=azoth`,
   `KBUILD_BUILD_HOST=forge`, `KBUILD_BUILD_TIMESTAMP` derivato. Un job
   settimanale (`kernel-weekly.yml`, job `repro`) ricostruisce lo stesso pin
   con cache vuota e builder ricostruito (sul runner self-hosted: un secondo
   runner quando ci sarà), con la stessa toolchain perché il builder installa
   solo il lock (sezione 5), e confronta con l'OCI pubblicato `config`,
   `System.map`, `vmlinux` sezione per sezione e i moduli senza la firma
   (`repro.py`): la chiave che firma moduli e immagine nasce in ogni build,
   quindi firma dei `.ko` e certificato in `.init.data` sono attesi; ogni altra
   differenza è un bug da aprire, e il job è rosso;
9. ccache su directory persistente del runner (non `actions/cache`): tra due
   patch level cambiano pochi file, la LTO finale no;
10. pubblicazione (job `publish` su runner GitHub, dall'artefatto del job `build`):
    tre pacchetti OCI con i soli RPM dentro, `ghcr.io/hr-mes/azoth`
    (binari), `azoth-devel`, `azoth-debuginfo`, tag `<nvr>`.
    Pacchetti separati e non suffissi del tag, perché la retention di ghcr è per
    pacchetto (`retention.sh`, prima del gate, che così verifica ciò che resta):
    del debuginfo restano le due release più recenti, di kernel e devel tutte; con
    ogni release resta ciò che è raggiungibile dal suo digest, cioè l'indice dei
    referrer che cosign v3 tiene sotto il tag di fallback `sha256-<hex>` (ghcr non
    ha l'API referrers) e i bundle Sigstore che elenca, manifesti senza tag. Tutto
    il resto se ne va: release oltre il limite con i loro referrer, indici
    sostituiti da ogni attestazione successiva, manifesti di un push ripetuto
    dello stesso NVR. Ogni immagine: firma cosign
    keyless (identità OIDC del workflow), SBOM SPDX da syft come attestazione
    `spdxjson`, attestazione custom con i pin (pins.env, hash di manifest, delta,
    patches.list e Containerfile, immagine base del builder, lock della toolchain);
    la principale ha
    anche la provenance SLSA di GitHub (`actions/attest-build-provenance`, commit e
    workflow) nello store attestazioni di GitHub, non nel registro, verificabile
    con `gh attestation verify`. Il gate K2 è il `cosign
verify` e `verify-attestation` nel workflow stesso. `:latest` si muove solo su
    `main`, cioè al merge di una PR di bump. Una PR costruisce e non pubblica.

Il job del kernel è un workflow proprio (`kernel-build.yml`), attivato da cambi in
`forge/specs/azoth/**` e a mano, con hash di idempotenza sugli input:
l'orchestratore non lo ricompila a ogni run, l'immagine di sistema consuma il tag
pinnato in `pins.env`.

## 4. Config (`kernel-local`)

Il config Fedora 43 x86_64 porta già: `SCHED_CLASS_EXT`, `DEBUG_INFO_BTF`,
`BPF_LSM`, `IMA` con `IMA_APPRAISE`, `DM_VERITY_VERIFY_ROOTHASH_SIG`, `FS_VERITY`,
`EROFS`, `INIT_ON_ALLOC_DEFAULT_ON`, `X86_KERNEL_IBT`, `HZ_1000`, `PREEMPT_DYNAMIC`,
`LRU_GEN_ENABLED`, `NTSYNC`, `RUST` (che il delta spegne, sezione 5), `WIREGUARD`,
CAKE e FQ, e la lista LSM
`lockdown,yama,integrity,selinux,bpf,landlock,ipe`. Il frammento Athanor è il
delta, e resta corto:

| Opzione                                               | Valore | Perché                                                                                                                                 |
| ----------------------------------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `SCHED_BORE`                                          | y      | patch CachyOS, responsività desktop                                                                                                    |
| `CC_OPTIMIZE_FOR_PERFORMANCE`                         | y      | `-O2`: la base CachyOS accende `-O3`, ma due A/B di K7 non gli hanno trovato vantaggi (sezione 13, punto 2); `variants/o3` lo rimisura |
| `LTO_NONE`                                            | y      | ThinLTO spento: con `DEBUG_INFO_BTF`, `RUST` richiede `!LTO`; il bcond `clang_lto` resta per il toolchain (sezione 5)                  |
| `RUST`                                                | y      | come Fedora: la porta ai driver che nascono in Rust; con kCFI seleziona `CFI_ICALL_NORMALIZE_INTEGERS`                                 |
| `CFI`                                                 | y      | kCFI, richiede clang; con IBT già attivo                                                                                               |
| `ZERO_CALL_USED_REGS`                                 | y      | hardening a costo trascurabile                                                                                                         |
| `RANDSTRUCT_NONE`                                     | y      | come Fedora: `RUST` dipende da `!RANDSTRUCT`, e il layout randomizzato costa in cache                                                  |
| `MODULE_SIG_FORCE`                                    | y      | ogni modulo firmato: chiave effimera di build per l'albero, MOK per i kmod esterni                                                     |
| `DEFAULT_TCP_CONG`                                    | "bbr3" | BBRv3 dalla base CachyOS                                                                                                               |
| `DEFAULT_FQ`                                          | y      | BBR richiede pacing: FQ come qdisc di default                                                                                          |
| `ZSWAP_COMPRESSOR_DEFAULT_ZSTD`, `ZRAM_DEF_COMP_ZSTD` | y      | compressione memoria zstd di default                                                                                                   |
| `IKCONFIG`, `IKCONFIG_PROC`                           | y      | config verificabile a runtime, usato dall'attestazione                                                                                 |
| `EROFS_FS`                                            | y      | built-in: la rootfs composefs non deve dipendere da un modulo nell'initrd                                                              |

Non si toccano, e il documento lo dice perché il passato li ha toccati:
`OBJTOOL`, `WERROR`, `STACK_VALIDATION`, `DEBUG_INFO_*` (senza DWARF non c'è BTF
e senza BTF non c'è il nervo eBPF), `IOMMU_DEFAULT_PASSTHROUGH`, `X86_NATIVE_CPU`,
la lista LSM, i driver. Opzioni inesistenti (`UKSM`, `ACPI_CUSTOM_METHOD`,
`BCACHEFS_FS` fuori dall'albero dal 6.17) non entrano.

**Livello ISA.** `CONFIG_X86_64_VERSION` non esiste in 7.1 upstream; il kernel
Fedora compila con `-march=x86-64` e `-mtune=generic`. Resta così: nel kernel
v2/v3 non danno nulla di misurabile (il codice generico non usa SIMD, i percorsi
caldi scelgono l'implementazione a runtime) e il baseline è la compatibilità
massima. La decisione v2 riguarda la userland (`forge/config/rpmmacros`), fuori
da questo blocco.

**Il controllo di coerenza** è quello di Fedora: `process_configs.sh` con
`with_configchecks` acceso fallisce se un'opzione del frammento viene scartata
da kconfig. Non serve uno script Athanor.

## 5. Toolchain

clang, lld e llvm di Fedora 43 dal Containerfile, con la base
`registry.fedoraproject.org/fedora:43@sha256:…` pinnata per digest e aggiornata
dal bot. Tutto ciò che il builder installa sopra la base è in
`builder/toolchain.lock`, per nome di file e sha256: gli strumenti di `builder/toolchain.packages`, le
BuildRequires di kernel.spec con i bcond di `bconds.sh` e i pacchetti della base che
aggiornano. `lock.sh install` li scarica da koji, che tiene ogni build (i mirror
tengono solo l'ultima), ricuce l'header di firma che koji conserva in
`data/sigcache` come per il SRPM, controlla sha256 e firma della chiave Fedora e li
installa con ogni repository spento; il lock fa parte degli input del riuso
(`build-inputs.py`). La chiave è quella della release della base, qualunque sia la
release del SRPM pinnato. Lo rigenera `lock.sh generate`, in un container nudo della
base, solo quando cambiano la base, `FEDORA_KERNEL_NVR`, gli strumenti di `toolchain.packages` o
`bconds.sh` (le righe di intestazione del lock); `generate --force` per portare
avanti la toolchain a pin fermi. `build.sh` lo controlla con `lock.sh check` prima di
ogni stadio tranne `manifest`: un lock che non corrisponde a quegli input, o un
builder costruito da un altro lock, ferma la build. Lo stesso vale per gli ambienti
`boot/` e `nvidia/`: ogni stadio dei loro Containerfile ha la sua lista di pacchetti
`<stadio>.packages` e il suo lock `<stadio>.lock` (`boot/schbench`, `boot/toolchain`,
`nvidia/toolchain`), senza SRPM né bcond nell'intestazione; `lock.sh check` li
controlla tutti, quindi un lock di `boot/` o `nvidia/` vecchio ferma anche la build del
kernel. I tre Containerfile si costruiscono con contesto `forge/specs/azoth`
(`podman build -f <ambiente>/Containerfile forge/specs/azoth`), perché `lock.sh` è
condiviso. Senza lock la toolchain seguiva i repository del giorno: il 2026-10-01 il
job `repro` ha trovato `CONFIG_PAHOLE_VERSION` 130 nell'OCI pubblicato e 132 nella
ricostruzione dello stesso pin. `LLVM=1` arriva dal bcond dello spec (`clang_make_opts`). Rust acceso come
in Fedora: in 7.1 `RUST` dipende da `!RANDSTRUCT` e, con `DEBUG_INFO_BTF` acceso
(l'eBPF di Athanor non può rinunciarvi), da `!LTO`, perché pahole non regge i DWARF
fusi da LTO con unità Rust. Quindi ThinLTO e `RANDSTRUCT` restano spenti nel delta,
verificato dal gate. Il bcond `--with clang_lto` resta comunque acceso: è l'unico
con cui kernel.spec passa `HOSTCC=clang CC=clang LLVM=1` a `process_configs.sh`,
senza il quale il config verrebbe valutato con gcc e kCFI sparirebbe; il
frammento LTO che porta con sé è sovrascritto da `kernel-local`. Quando upstream
toglierà il vincolo `!LTO`, ThinLTO si riaccende con due righe del delta; non si
patchano i Makefile per forzarlo.

## 6. Firma e catena di avvio

- **Moduli in albero**: firmati dalla chiave effimera generata dallo spec, il
  cui certificato è dentro il kernel. `MODULE_SIG_FORCE` rende il rifiuto un
  comportamento di compilazione, non di riga di comando.
- **Moduli esterni** (NVIDIA): firmati con la chiave dei moduli del progetto, in
  un job separato che non vede altro. Il suo certificato
  (`keys/modules/athanor-modules.pem`) è compilato nel kernel
  (`CONFIG_SYSTEM_TRUSTED_KEYS`): la fiducia non dipende dal firmware né da
  Secure Boot. La chiave privata (RSA 4096, profilo `keys/profiles/modules.cnf`,
  generata con `keys/generate.sh` il 2026-09-13, copia cifrata fuori da GitHub)
  sta nel secret `MODULE_SIGNING_KEY` dell'environment `signing`, ammesso solo
  ai branch `main` e `iso-v0`. Un secret non è più sicuro per essere nato sul
  runner: conta dove si usa, e chi ne ha la custodia.
- **Chiave Secure Boot**: firma la UKI e la sua policy PCR (`ukify
--pcr-private-key`; è la chiave pubblica con cui `athanor-tpm-luks-seal.sh`
  sigilla LUKS). Profilo `keys/profiles/secureboot.cnf`: non CA, `codeSigning`.
  Secret `SECUREBOOT_SIGNING_KEY`, certificato
  `keys/secureboot/athanor-secureboot.pem` (`.der` per `mokutil --import`). Non
  essendo una CA, anche arruolata resta fuori dal keyring machine
  (`INTEGRITY_CA_MACHINE_KEYRING_MAX`) e finisce nel keyring platform. La patch
  Red Hat verificherebbe i moduli anche con quel keyring (ripiego su `-ENOKEY`):
  `patches/redhat/0001-module-verify-signatures-with-trusted-keyrings-only.patch` lo
  toglie, quindi non può autorizzare un modulo.
- **Revoca**: `keys/revoked/` sono i certificati ritirati, compilati nella
  blacklist del kernel (`CONFIG_SYSTEM_REVOCATION_KEYS`): un modulo firmato con
  uno di loro è rifiutato anche dove quella MOK fosse ancora arruolata. Il primo
  è la MOK unica del 2026-09-04 ("Ermete OS Secure Boot MOK"), che firmava UKI e
  moduli, ritirata il 2026-09-13.
- **UKI**: kernel, initrd, `cmdline` e microcode early in un'unica immagine
  firmata con la chiave Secure Boot dietro lo shim Fedora; la produce la fase
  system-image, perché l'initrd dipende dall'immagine, non dal kernel. Lo spec
  Fedora fornisce già le stringhe SBAT (`kernel.sbat`, `uki.sbat`).
- **Primo avvio**: arruolamento guidato del certificato Secure Boot
  (`mokutil --import`), unica interazione richiesta per avere Secure Boot acceso
  su un PC qualsiasi; i moduli non ne dipendono.
- **`cmdline`** committata: `lockdown=integrity mitigations=auto init_on_alloc=1
randomize_kstack_offset=on page_alloc.shuffle=1 vsyscall=none preempt=full
amd_pstate=active zswap.enabled=1`. Niente `iommu=pt`, niente `mitigations=off`.
- **Rootfs**: dm-verity con roothash firmato da una chiave del progetto nel
  keyring secondario (non quella Secure Boot, che non vi entra),
  fs-verity per composefs, TPM 2.0 per LUKS (`systemd-cryptenroll`) con fallback
  a passphrase sui PC 2014–2016 senza TPM 2.0. Le opzioni kernel ci sono già;
  la parte immagine è del blocco system-image.

## 7. Gate

Ogni PR di bump e ogni cambio in `forge/specs/azoth/**` passa:

1. **build** sul runner self-hosted (60 min misurati su 16 core con ThinLTO);
2. **config**: `process_configs.sh` con controlli accesi;
3. **boot matrix** (job `boot`, runner GitHub-hosted `ubuntu-24.04` con KVM,
   `boot.sh` nell'immagine `boot/Containerfile`): firmware {SeaBIOS, OVMF con
   Secure Boot} × CPU {`-cpu Nehalem`, `-cpu host`}. Nehalem prova che nessuna
   istruzione oltre il baseline è entrata. Il caso UEFI passa dallo shim Fedora
   firmato Microsoft a una UKI di prova (vmlinuz, initramfs di prova, `cmdline`)
   firmata con una MOK effimera arruolata nel varstore OVMF con `virt-fw-vars`:
   la catena della sezione 6 con una chiave usa e getta al posto della MOK del
   progetto, così il gate gira anche sulle PR, senza segreti. Asserzioni
   (`boot/init`, sulla seriale): `uname -r` atteso, `/sys/kernel/btf/vmlinux`,
   `bpftool feature probe`, `/sys/kernel/sched_ext`, lista misure IMA (con
   `ima_policy=tcb` solo nella riga di comando di prova), lockdown `integrity`,
   `tcp_congestion_control=bbr3`, `tainted=0`, `dmesg` senza splat (`BUG:`,
   `WARNING: CPU:`, `Oops:`, `Call Trace:`; gli avvisi hw-vuln come SRSO non lo
   sono); i certificati compilati nel kernel (chiave dei moduli e revocati)
   caricati, per subject key identifier (`Loaded X.509 cert` nel log); in UEFI
   anche `SecureBoot=1` e `MokListRT` presente. `publish`
   dipende da `boot`: senza matrice verde non si pubblica;
4. **kmod NVIDIA** (job `kmod` di `kernel-build.yml`, che chiama il workflow
   riusabile `nvidia-build.yml`; sezione 10): `nvidia-open` (610) e ramo legacy
   580 compilano con `nvidia.sh` contro il `kernel-devel` appena costruito, o
   pubblicato per l'NVR dei pin quando il kernel è riusato, con la toolchain del
   kernel; ogni `.ko` deve portare il vermagic del kernel e i tipi kCFI. Poi, sui
   push, `nvidia-kmod.yml`, avviato da Kernel Build a valle della pubblicazione
   (`workflow_run` vale solo dal branch di default): il job `sign` li firma con la
   chiave dei moduli del progetto e firma una copia con una MOK effimera dal
   profilo Secure Boot; il job `boot` (`boot.sh --mok --insmod`, tutti e quattro
   i casi) arruola quella MOK e nel guest carica il `nvidia.ko` firmato di ogni
   ramo, atteso `ENODEV` (firma accettata, GPU assente), una copia non firmata e
   quella firmata dalla MOK, attese `EKEYREJECTED`: SeaBIOS prova che la fiducia
   non dipende dal firmware, UEFI che una MOK arruolata non la estende; `publish`
   verifica firma e attestazione con cosign;
5. **benchmark di tendenza** (non bloccante): hackbench, schbench, fio null,
   netperf loopback per cinque minuti, risultati come artefatto e grafico nel
   summary. È il numero che ha deciso `-O2` e decide ogni futura opzione;
6. **riproducibilità** settimanale (sezione 3).

**Il check unico.** Il job `gate` di `kernel-build.yml` (check `Kernel gate`)
dipende da tutti gli altri ed è verde solo se `inputs`, `boot` e `kmod` sono
verdi e `build` è verde o saltato per riuso. È l'unico check richiesto dalla
protezione del branch, e Kernel Build parte su ogni PR, senza filtro di
percorsi: così il check esiste sempre e l'auto-merge del bot (sezione 8) ha un
nome solo da aspettare.

**Settimanale (K7).** `kernel-weekly.yml` (domenica, dal branch di default; a
mano con `workflow_dispatch`): job `repro` (gate 6, sezione 3) e job `bench`
(gate 5) su `ubuntu-24.04` con KVM: `bench.sh` avvia il kernel pubblicato in
QEMU (4 vCPU, 4 GiB) con un initramfs che porta hackbench (realtime-tests), schbench
(dal sorgente, commit e hash dell'archivio pinnati in `boot/Containerfile`), fio e netperf;
`bench/init` esegue le prove (30 s ciascuna: hackbench a lavoro fisso, wakeup
p99 e RPS p50 di schbench, IOPS di fio con `ioengine=null`, TCP_STREAM e TCP_RR
di netperf su loopback) e stampa `K7 <metrica> <valore> <unità>`; i
`results.json` restano negli artefatti e `bench-report.py` scrive nel summary
tabella, confronto e un grafico Mermaid per metrica. I runner GitHub cambiano
CPU da un run all'altro: l'andamento è indicativo, la decisione sta nel
confronto A/B nello stesso run, con l'input `variant`: `build.sh --variant
<nome>` fonde `variants/<nome>` sopra `kernel-local` (le righe con lo stesso
simbolo vengono sostituite), buildid `.azoth.<nome>`, mai pubblicato, misurato
accanto al kernel pubblicato. `o3` rimisura `-O3` contro il default `-O2`.

**Riuso.** Il job `inputs` calcola `build-inputs.py` (pin, manifest delle
sorgenti, `kernel-local`, `patches.list`, `patches/`, `fedora-wins.list`, `build.sh`,
Containerfile: solo ciò che cambia gli RPM) e lo confronta con il predicato
dell'attestazione dei pin sull'immagine `azoth:<nvr>`, verificata con
cosign. Se coincidono, `build` e `publish` non partono e la boot matrix usa il
kernel-core dell'immagine pubblicata: un push che tocca solo test, retention o
workflow costa i minuti della matrice, non l'ora di build. Un bump dei pin
ricompila. La prova del riuso è una firma verificata, non un tag.

## 8. Auto-manutenzione: il bot di bump

Workflow `kernel-bump.yml`, giornaliero (`schedule` vale solo dal branch di
default; a mano con `workflow_dispatch` su qualunque branch). Due gruppi, una PR
ciascuno, perché si verificano in modo diverso (`doc_build_ordering.md`, O7 e O8):
il gruppo **kernel** (i job sotto) e il gruppo **system** (`system/Containerfile` e i
lock NVIDIA ripubblicati alla stessa versione, etichetta `system-bump`: System Image
Check costruisce le immagini col kernel pubblicato e una persona rivede la differenza
dei pacchetti, mai auto-merge). Il gruppo kernel ha tre job:

1. **kernel** (runner GitHub-hosted): `bump.py apply --group kernel` legge `pins.env` e
   interroga Bodhi (build `kernel` stable di F43, poi F44), le release GitHub di
   `CachyOS/linux`, `CachyOS/kernel-patches` (testa della directory della
   serie), `CachyOS/linux-cachyos` (il commit di `linux-cachyos/config` vigente
   alla data della release CachyOS: il config con cui CachyOS ha spedito quel
   kernel, non la testa di oggi, che può essere della serie dopo), i tag di
   `NVIDIA/open-gpu-kernel-modules` e l'indice di download NVIDIA (dentro il
   ramo pinnato, 610 e 580: un cambio di ramo è una PR umana), e il registro
   Fedora per il digest dell'immagine base dei tre Containerfile del kernel; un lock
   NVIDIA si rigenera qui solo quando il suo pin si sposta. Coppia kernel
   come in sezione 2; senza coppia il kernel resta dov'è e una nota nel corpo
   della PR dice fin dove arrivano Fedora e CachyOS. Se nulla è cambiato esce;
   altrimenti riscrive `pins.env`, i `FROM` e la tabella dei pin di `KERNEL.md`
   e li passa come artefatto. Una PR aperta alla volta per gruppo (etichetta
   `kernel-bump`): una PR `system-bump` in attesa di revisione non ferma il kernel.
2. **prep** (runner self-hosted, dove il builder e la cache già esistono): nel
   builder, `build.sh --stage manifest` e `nvidia.sh manifest` scaricano i
   sorgenti dei pin nuovi e riscrivono i due manifesti degli hash (il `.run`
   legacy è confrontato con l'hash che NVIDIA pubblica accanto); poi `build.sh
--stage prep`: firme PGP con le chiavi vendorizzate (una rotazione di chiave
   è un prep rosso, mai un'accettazione silenziosa), patch applicate,
   derivazione del config e gate di `kernel-local`. L'esito e le opzioni
   derivate (`listnewconfig` con i valori CachyOS) vanno nel corpo della PR,
   verde o rosso. Prima del builder, `lock.sh generate` riscrive i lock degli
   ambienti i cui input si sono spostati (la base per tutti, il SRPM pinnato per il
   builder): una PR di soli pin NVIDIA o CachyOS li lascia come sono. Se prep si ferma con `refresh needed`, l'esito è `REFRESH`.
3. **pr** (runner GitHub-hosted, con il PAT `KERNEL_BUMP_TOKEN`: le PR aperte
   con il `GITHUB_TOKEN` non fanno partire i check): branch `bump/kernel-<data>`,
   un commit con `pins.env`, i manifesti, i Containerfile, i lock degli ambienti
   e `KERNEL.md`, PR
   verso il branch da cui il bot è partito, con nel corpo la tabella prima/dopo
   dei pin, le note, l'esito di prep e le opzioni derivate; con prep verde
   (`ok`) `gh pr merge --auto --squash`, con prep rosso (`FAIL` o `REFRESH`)
   niente auto-merge: la PR resta aperta finché una persona non corregge e fa
   il merge a mano.

Il gate della PR è il check `Kernel gate` di `kernel-build.yml` (sezione 7),
l'unico richiesto dalla protezione del branch. Con prep verde e il check verde
la PR va in merge da sola; con prep rosso, o con il check rosso, resta aperta
con il log del gate fallito. È l'unico momento in cui serve una persona, e sa
già dove guardare. Al merge il push fa partire
Kernel Build, che pubblica il kernel e alla fine avvia `nvidia-kmod.yml` per
firma, boot e pubblicazione dei moduli. Il cambio di release Fedora della rootfs
(43→44) e il cambio di `KERNEL_CHANNEL` restano PR umane.

**Patch da rinfrescare** (decisione del maintainer, 2026-09-14). Una patch di
`patches.list` che non entra più senza fuzz non si applica con tolleranza: la
tolleranza indovina il punto e un bot che unisce da solo la porterebbe nel kernel
senza che nessuno la guardi. Con esito `REFRESH` la PR ha il titolo che finisce con
`(patch refresh needed)` e **non** ha l'auto-merge. Sul branch della PR, nel
builder, `build.sh --stage refresh --out DIR` ricostruisce l'albero unito vero,
applica ogni patch che non entra con GNU `patch --fuzz=2` come proposta, verifica
che la copia proposta entri senza fuzz in entrambi gli alberi (indice Fedora e
albero CachyOS) e la scrive in `DIR/refreshed/` con il resoconto di GNU patch. Una
persona legge ogni hunk applicato con fuzz, copia il file in
`forge/specs/azoth/patches/refreshed/` con lo stesso percorso di `patches.list`, fa
il commit sul branch e, con il `Kernel gate` verde, fa il merge a mano. Il
preambolo della copia registra il commit di `CachyOS/kernel-patches`, lo SHA-256
del file upstream e i pin su cui è stata rinfrescata. La build rifiuta una copia
che registra un altro file upstream (CachyOS ha cambiato la patch: si rinfresca di
nuovo) e una copia diventata inutile (il file upstream entra di nuovo senza fuzz:
si cancella). Se GNU patch non riesce nemmeno con fuzz, o se i due alberi
richiedono copie diverse, `refresh` si ferma e la patch si rinfresca a mano:
la copia scritta a mano porta lo stesso preambolo (`Refreshed-From`,
`Upstream-SHA256` del file upstream, `Refreshed-On`) e deve entrare senza fuzz
in entrambi gli alberi (indice Fedora e albero CachyOS); se un hunk tocca
codice che esiste solo nell'albero Red Hat, va in `patches/redhat/` invece che
qui (sezione 2). Una copia sotto `patches/refreshed/` senza una voce
corrispondente in `patches.list` ferma la build in ogni stadio che arriva al
ciclo delle patch: va cancellata.

## 9. Kernel guest per le MicroVM

Stessa sorgente e stesso pin, secondo config: `make x86_64_defconfig` +
`kvm_guest.config` + frammento `microvm/kernel-local` (virtio, 9p/virtiofs,
EROFS, dm-verity, BPF, nessun driver fisico, nessun modulo). Spec minimo
`microvm/azoth-microvm.spec` (~100 righe, non ha bisogno del packaging
Fedora: produce `vmlinux` e `bzImage`), pochi minuti di build nello stesso job,
pubblicato in `azoth:<nvr>-microvm`. È il kernel che
`hypervisor-daemon` avvia in Firecracker o cloud-hypervisor; SEV/TDX guest
restano opzioni del frammento per gli host che li hanno.

**Implementazione (K6).** `build.sh`, dopo `rpmbuild -bp`, deriva il config in
una directory oggetto separata (`make O=… x86_64_defconfig kvm_guest.config`,
`merge_config.sh` con `microvm/kernel-local`, `olddefconfig`) e lo verifica riga
per riga come `kernel-local` (`check_delta`): il gate del frammento gira anche
in `prep`, quindi nelle PR del bot. Negli stadi `microvm` e `build` compila con
`rpmbuild -bb microvm/azoth-microvm.spec` (una quarantina di righe:
`%build` con `O=` sull'albero preparato, `%install` di `vmlinux` senza DWARF ma
con simboli e `.BTF`, `bzImage`, `config` e `release` in
`/usr/lib/athanor/microvm/`), pochi minuti prima del kernel principale, che
trova l'albero pulito. Il pacchetto va in `out/microvm/` e nell'OCI
`azoth:<nvr>-microvm` (stesso `publish`: SBOM, firma, attestazione
dei pin, retention come release). Gate (job `boot`, `microvm/boot.sh`):
Firecracker, release GitHub pinnata per hash in `boot/Containerfile`, avvia il
`vmlinux` con una rootfs ext4 di prova (busybox, `microvm/init`) e il guest
chiude con `K6 RESULT ok` dopo le asserzioni: `uname -r` atteso, root su
virtio-blk, BTF, erofs/9p/virtiofs/overlay/ext4, device-mapper con
`DM_VERITY` nel config, niente moduli, kCFI, `dmesg` pulito. Con il kernel
riusato il pacchetto viene dall'OCI pubblicato. Frammento e spec sono input del
riuso (`build-inputs.py`).

## 10. NVIDIA, AMD, Intel

AMD e Intel sono in-tree (`amdgpu`, `radeon`, `i915`, `xe`) con `linux-firmware`
spacchettato per vendor nell'immagine: nessun lavoro nel kernel oltre a non
toglierli. NVIDIA, in un workflow proprio (`nvidia-kmod.yml`) che parte dopo il
kernel:

| Livello         | GPU                              | Meccanismo                                                                                                                                |
| --------------- | -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| default         | tutte                            | `nouveau` in-tree, firmware GSP; NVK in Mesa                                                                                              |
| `nvidia-open`   | Turing 2018+                     | moduli aperti 610.x compilati nel container Fedora contro `kernel-devel`, clang e kCFI coerenti, firmati con la chiave dei moduli         |
| `nvidia-legacy` | Maxwell, Pascal, Volta 2014–2018 | ramo 580, stesso meccanismo; la parte RM è il blob gcc di NVIDIA, senza kCFI né return thunk: rischio noto, verificabile solo su hardware |

Pubblicazione `azoth-nvidia:<kernel-nvr>-<driver>`; le immagini
`athanor-system-nvidia` e `athanor-system-nvidia-legacy` le consumano insieme al firmware e
allo userspace NVIDIA della stessa versione, bloccati per hash in
`system/nvidia/locks/` (docs/architecture/doc_system_image.md, S4-S7). L'immagine
predefinita `athanor-system` non porta moduli NVIDIA. Le versioni del driver
sono pin in `pins.env` (`NVIDIA_OPEN_VERSION` e il commit del tag, che è
annotato e può muoversi; `NVIDIA_LEGACY_VERSION`, con l'hash del `.run` in
`nvidia/sources.sha256`, separato dal manifest del kernel che `build.sh`
verifica per intero e che è un input del riuso), alzate dal bot solo se il
kmod compila. `build-inputs.py` li esclude: non cambiano gli RPM del kernel e
non invalidano il riuso.

**Implementazione (K4).** `nvidia.sh build --driver open|legacy`, nell'immagine
`nvidia/Containerfile` (la base pinnata del builder con la sola toolchain LLVM,
kmod e openssl), estrae l'albero `kernel-devel` dal RPM e compila con Kbuild
(`SYSSRC`, `CC=clang LLVM=1`; `IGNORE_CC_MISMATCH` perché il conftest NVIDIA
pretende la stessa stringa di versione del compilatore, mentre gli hash kCFI
dipendono dai tipi, non dalla versione). Il codice che passa da Kbuild riceve
da solo i flag del kernel. La parte RM dei moduli aperti (`nv-kernel.o`,
`nv-modeset-kernel.o`), che NVIDIA compila fuori da Kbuild con i propri
Makefile, li riceve da `EXTRA_CFLAGS`, letti da `.config` nella grafia di
clang: kCFI, retpoline, return thunk, SLS, IBT, e il padding delle funzioni di
`CALL_PADDING` (`-fpatchable-function-entry=11,11`), senza il quale clang mette
l'hash kCFI in fondo ai 16 byte del preambolo invece che in testa, dove il
kernel e i chiamanti lo leggono: il modulo firma bene ma non carica (`no CFI
hash found`, poi `CFI failure` sull'init). Non è un dettaglio: i Makefile
di NVIDIA provano solo le grafie gcc di retpoline e return thunk, che clang
scarta in silenzio, e senza quei flag objtool conta oltre sedicimila chiamate
indirette e `ret` non mitigati nel solo `nvidia.o`. Gate per ogni `.ko`: il
vermagic del kernel e i preamboli `__cfi_`; per il ramo aperto objtool non
deve trovare `ret` nudi né chiamate o salti indiretti senza retpoline in una
funzione C, che è come si manifesta un flag mancante. Restano circa
cinquecento avvisi, contati e non bloccanti perché sono proprietà del codice
NVIDIA e non dei flag: clang non estende kCFI alle chiamate virtuali né i
return thunk ai thunk del C++ di DisplayPort in `nvidia-modeset.o`, e il RM
ha code di funzione irraggiungibili. `nvidia.sh sign` firma con `scripts/sign-file` del
kernel-devel e l'hash di `CONFIG_MODULE_SIG_HASH`, e rilegge il firmatario con
`modinfo`. Il workflow `nvidia-kmod.yml`: `build` (matrice dei due rami, runner
self-hosted, kernel-devel dall'immagine pubblicata per l'NVR di `nvr.sh`),
`sign` (runner GitHub, environment `signing`: vede solo i `.ko` e la chiave,
montata in sola lettura per la durata del comando), `boot` (la catena della
firma end-to-end in QEMU, gate 4 della sezione 7), `publish` (un'immagine
`scratch` per ramo con `lib/modules/<kver>/extra/nvidia/*.ko`, il layout che
l'immagine di sistema copia e che syft cataloga, più `version` e `kver`;
cosign, SBOM dei moduli,
attestazione dei pin `NVIDIA_*`, retention, gate di verifica). Le patch
`misc/nvidia/*` di CachyOS al commit pinnato non entrano: la prima aggiunge
solo `-mharden-sls=all` alla parte modeset (qui arriva da `EXTRA_CFLAGS`),
l'ultima è un hack sul Makefile che non serve, le tre in mezzo sono correzioni
DSC/DisplayPort: candidate a un `nvidia/patches.list` se servono, non default.

**I tipi kCFI dei moduli aperti.** NVIDIA non compila i suoi moduli con kCFI,
e contengono chiamate indirette verso funzioni di tipo diverso da quello del
punto di chiamata: comportamento indefinito che senza kCFI passa inosservato,
con kCFI una trappola e, con `oops=panic`, un panic all'avvio. La prova su
hardware del 2026-09-23 (immagine `-nvidia` avviata con `cfi=warn`, RTX 3060 e
RTX 4070 Ti SUPER) ha registrato come prima violazione
`nv_drm_connector_mode_valid`, chiamata da `__drm_helper_update_and_validate`
appena `nvidia-drm` registra lo schermo, poi `scanLockState` a ogni modeset;
nessuna nel RM in quell'avvio. Le patch in `nvidia/patches/open/`, applicate da
`nvidia.sh` dopo il clone, correggono i tipi dove sbagliano: i getter bindata
del SEC2 generati con un argomento in meno dello slot NVOC che li contiene, i
distruttori IOM chiamati come `void (*)(Dynamic *)`, la callback del timer
chiamata con un tipo di ritorno diverso, `mode_valid` che restituisce `int`
invece di `enum drm_mode_status`, lo slot `scanLockState` che prende `NvU32`
dove le funzioni di stato prendono `NVEvoLockAction`, il dispatcher dei task
NVSwitch convertito a `nv_q_func_t`. Una patch che non si applica dopo un bump
ferma la build. `nvidia/kcfi_check.py` ferma invece la build se compare una
violazione nuova, leggendo i log di entrambe le parti (RM e Kbuild): confronta
ogni getter bindata con la sua dichiarazione NVOC, l'arità di ogni voce delle
tabelle dei metodi esportati con il suo `paramSize` (resControl la riconverte in
base a quello), ammette da `-Wcast-function-type-strict` solo i cast ragionati
(l'andata e ritorno di quelle tabelle, il parcheggio della callback nel timer) e
non ammette nessun avviso di `-Wincompatible-function-pointer-types-strict`:
un'assegnazione che il C accetta perché un enum è compatibile con il suo intero,
mentre kCFI li distingue. Una volta patchata, la parte Kbuild non emette avvisi,
e il silenzio non prova che i flag le siano arrivati: il controllo legge quindi
il comando che Kbuild salva per ogni oggetto C (`.<oggetto>.o.cmd`) e fallisce
se uno manca di uno dei due avvisi, o se non ne trova nessuno. Il limite: un puntatore a funzione che passa per
`NvP64` o `void *` sfugge a tutti; lo trova solo un avvio sull'hardware con
l'opzione `cfi=warn`, che registra le violazioni invece di fermarsi, senza
ricompilare il kernel. Le patch sono candidate all'upstream; i getter e gli
stati EVO stanno in file generati, quindi la correzione vera è nei generatori
di NVIDIA.

**Il ramo legacy e il kernel 7.1.** Con `CONFIG_CFI=y`, il 7.1 rifiuta in
modpost ogni modulo non GPL, anche uno vuoto che include solo `<linux/mm.h>`:
`KCFI_REFERENCE(__clear_pages_unrolled)` in `asm/page_64.h` mette in ogni
unità di compilazione un riferimento (sezione `.discard.addressable`) al
simbolo, esportato GPL, e modpost lo legge prima che il linker scarti la
sezione. Il riferimento serve solo a vmlinux, per il preambolo kCFI di
`SYM_TYPED_FUNC_START`; in un modulo non risolve nulla. Il difetto è anche in
mainline alla data: `patches/0001-compiler.h-keep-KCFI_REFERENCE-out-of-modules.patch`
lascia `KCFI_REFERENCE` a vmlinux, come già per decompressore e vDSO, ed è
candidata all'upstream.

## 11. Fuori dal kernel

In `system-tweaks` (sysctl.d, modprobe.d): `net.core.default_qdisc=cake` sul
desktop, tunable BORE, `vm.max_map_count`, `kernel.split_lock_mitigate`. In
`forge/config/rpmmacros`: baseline userland v2 e glibc-hwcaps per le librerie
che guadagnano da v3. AutoFDO: `CONFIG_AUTOFDO_CLANG=y` entra quando esiste la
pipeline di profilazione sul kernel Athanor (perf con branch sampling sul
5800X3D, `create_llvm_prof`, profilo committato con hash); nessun profilo
altrui. Variante `PREEMPT_RT` (mainline dal 6.12) e patchset `hardened` di
CachyOS: candidati da valutare con il benchmark, non default.

## 12. Ordine di esecuzione

| Fase | Contenuto                                                                                                                              | Gate                                               |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| K1   | `pins.env`, manifest, `build.sh`, `kernel-local`, `patches.list`; rimozione di script, vendoring e README; workflow `kernel-build.yml` | RPM prodotti in locale e in CI, config onorato     |
| K2   | pubblicazione OCI con cosign, SBOM, SLSA; debuginfo separato                                                                           | `cosign verify` sul tag                            |
| K3   | boot matrix in QEMU con le asserzioni della sezione 7                                                                                  | verde su Nehalem, host, UEFI+SB, BIOS              |
| K4   | `nvidia-kmod.yml` con i due rami, firma MOK                                                                                            | kmod compilati, firmati, accettati sotto SB        |
| K5   | bot di bump con auto-merge                                                                                                             | una PR di bump verde end-to-end                    |
| K6   | kernel guest MicroVM                                                                                                                   | `vmlinux` avvia in Firecracker con rootfs di prova |
| K7   | riproducibilità settimanale, benchmark di tendenza, `-O3` deciso dai numeri                                                            | primo report                                       |

Poi la v0 riprende con `azoth:<nvr>` nell'immagine. Ogni fase è un
insieme di commit verificabili da soli; la specifica si aggiorna se
l'implementazione scopre che un gancio Fedora non è come descritto.

## 13. Decisioni del maintainer (2026-09-03)

1. Serie: `stable` (segue 7.x con CachyOS, bump frequenti); `lts` (6.18) resta
   un valore possibile di `KERNEL_CHANNEL`.
2. `-O3` era acceso come in CachyOS, in attesa del benchmark di K7. Il 2026-09-05 due
   A/B nello stesso run e sulla stessa macchina (EPYC 7763, 4 vCPU, 30 s per prova)
   non hanno mai visto `-O3` vincere oltre il rumore; `-O2` ha vinto hackbench
   entrambe le volte (-7.0%, -2.5%) e una volta TCP_STREAM (+7.0%), pari altrove.
   Decisione: **`-O2`**, kernel più piccolo a parità di resa. `variants/o3` resta
   per rimisurare quando cambiano toolchain o serie.
3. `RANDSTRUCT_FULL` era acceso; il 2026-09-04 la scelta è **Rust acceso, ThinLTO e
   `RANDSTRUCT` spenti**: in 7.1 `RUST` esclude entrambi (sezione 5), il kernel deve
   restare agnostico anche verso i driver futuri in Rust, ThinLTO vale pochi punti
   percentuali che `RANDSTRUCT_FULL` in parte annulla, e questa è la configurazione
   di Fedora e di CachyOS: il delta più corto da mantenere. ThinLTO tornerà quando
   upstream toglierà il vincolo; `RANDSTRUCT` resta escluso per costruzione.
4. Debuginfo pubblicato come OCI separato, retention di due versioni.
5. (2026-09-14) Patch di terzi: niente fuzz nella build, mai. Quando una patch di
   `patches.list` smette di entrare si rinfresca una volta con `build.sh --stage
refresh`, una persona rivede gli hunk applicati con fuzz e la copia va in
   `patches/refreshed/`; la PR di bump in quel caso non ha l'auto-merge (sezione 8).
   Primo caso: la patch BORE della serie 7.2 su `include/linux/sched.h` di
   cachyos-7.2.5-1, che ha aggiunto `struct task_ipi_mask` davanti a `struct
task_struct`.
6. (2026-09-14) Cache kmalloc partizionate in modalità casuale
   (`KMALLOC_PARTITION_RANDOM`, la protezione di `RANDOM_KMALLOC_CACHES` in 7.1). Fedora
   7.2 sceglie la modalità per tipo (`KMALLOC_PARTITION_TYPED`), più forte, che richiede i
   token di allocazione del compilatore (`-falloc-token-max`); il clang di Fedora 43 non li
   ha. `kernel-local` fissa la modalità casuale e `build.sh` si ferma quando il config
   generato riporta `CC_HAS_ALLOC_TOKEN=y`, per togliere il pin e tornare alla modalità per
   tipo scelta da Fedora, invece di restare in silenzio sulla protezione più debole.
7. (2026-09-16) Immagini di sistema: base `base-atomic:43` di Fedora al posto di
   `ermete-base-nvidia`, immagine predefinita con `nouveau` e NVK, varianti
   `athanor-system-nvidia` (pacchetti negativo17) e `athanor-system-nvidia-legacy` (RPM
   Fusion) alla versione esatta dei moduli firmati, con gate e lock per hash
   (docs/architecture/doc_system_image.md).
8. (2026-10-02) Toolchain del builder bloccata per NVR: `builder/toolchain.lock` fissa
   per sha256 ogni RPM sopra la base, e si rigenera con il SRPM pinnato o la base
   (sezione 5), perché la stessa coppia di pin dia lo stesso kernel. (2026-10-03)
   Bloccati allo stesso modo gli ambienti `boot/` e `nvidia/`, un lock per stadio, e
   l'archivio GitHub di schbench pinnato per sha256 come Firecracker.

| `bump.py` | il bot di bump (sezione 8), in due gruppi: kernel (pin nuovi da Bodhi, CachyOS, NVIDIA e registro; riscrive `pins.env`, i `FROM` del kernel e `KERNEL.md`) e system (`system/Containerfile` e i lock NVIDIA ripubblicati) |
