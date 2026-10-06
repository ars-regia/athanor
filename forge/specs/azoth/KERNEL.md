# azoth

Il kernel di Athanor OS: il pacchetto `kernel` di Fedora ricostruito con clang, senza LTO
(Rust con DEBUG_INFO_BTF richiede !LTO finche' pahole non regge il DWARF unito da LTO, vedi
`kernel-local`) sopra la base CachyOS (BORE, tunable; -O2 al posto del loro -O3, deciso dall'A/B), con l'hardening in piu' di Athanor. La
specifica e' `docs/architecture/doc_kernel_build.md`; qui c'e' solo cosa sta in questa
directory e come si usa.

| File | Ruolo |
|------|-------|
| `pins.env` | i pin: NVR Fedora (stesso patch level della release CachyOS), release CachyOS, commit del config e delle patch |
| `SOURCES/sources.sha256` | hash di ogni file che build.sh scarica; lo scrive `build.sh --stage manifest` |
| `SOURCES/keys/{cachyos,kernel.org}/` | chiavi pubbliche che firmano i tarball CachyOS e vanilla |
| `keys/` | `profiles/` e `generate.sh`: le chiavi di firma del progetto; certificati pubblici in `secureboot/` (vmlinuz, secret `SECUREBOOT_SIGNING_KEY`), `modules/` (moduli esterni, compilato nel kernel, secret `MODULE_SIGNING_KEY`) e `revoked/` (compilati nella blacklist del kernel); i secret stanno nell'environment `signing` |
| `kernel-local` | delta Kconfig di Athanor sul config x86_64 di Fedora |
| `patches.list` | patch di CachyOS/kernel-patches applicate sopra la base |
| `patches/refreshed/` | copie rinfrescate e riviste delle patch di `patches.list` che non entrano più senza fuzz; il preambolo registra il file upstream da cui derivano (`build.sh --stage refresh`, spec sezione 8) |
| `patches/` | patch di Athanor, in formato git, applicate dopo quelle di CachyOS |
| `patches/redhat/` | patch di Athanor al codice della patch Red Hat: solo sull'albero Fedora, mai su Kconfig |
| `fedora-wins.list` | percorsi in cui un conflitto tra base CachyOS e patch Red Hat si risolve con l'albero Fedora |
| `cmdline` | la riga di comando base usata dalla matrice di avvio, generata da `athanor-kernel-profile/profile.toml` (`kernel_profile.py generate`); non modificarla a mano |
| `build.sh` | dai pin agli RPM: stadi `manifest` (scarica i sorgenti dei pin e scrive il loro manifesto), `prep` (sorgenti, patch, gate dei config), `refresh` (propone le copie rinfrescate delle patch di CachyOS che non entrano senza fuzz), `microvm` (prep e il solo kernel guest) e `build` (entrambi i kernel); `--variant NOME` per una variante di `variants/` |
| `variants/` | frammenti che sovrascrivono righe di `kernel-local` per il confronto A/B del benchmark (`o3`: -O3 al posto di -O2); buildid `.azoth.NOME`, mai pubblicati |
| `repro.py` | la riproducibilita': due build dello stesso pin a confronto (config, System.map, vmlinux per sezioni, moduli senza firma) |
| `bench.sh`, `bench/init`, `bench-report.py` | il benchmark di tendenza: kernel in QEMU/KVM con hackbench, schbench, fio, netperf; tabelle, confronto A/B e grafici dai `results.json` |
| `build-inputs.py` | gli input della build come JSON: predicato dell'attestazione dei pin e chiave del riuso in CI |
| `bump.py` | il bot di bump, in due gruppi (`--group kernel\|system`): `check` (JSON delle modifiche) e `apply` (gruppo kernel: riscrive pins.env, i `FROM` dei Containerfile del kernel e la tabella dei pin qui sotto; gruppo system: i `FROM` di `system/Containerfile` e i lock NVIDIA ripubblicati; stampa il corpo della PR); `verify` controlla i lock NVIDIA |
| `nvr.sh` | l'NVR del kernel derivato dai pin, lo stesso che rpmbuild produce e che i tag OCI usano |
| `builder/Containerfile`, `builder/toolchain.packages`, `builder/toolchain.lock` | l'ambiente: Fedora pinnata per digest piu' la toolchain LLVM e le BuildRequires dello spec, ogni RPM per sha256 nel lock |
| `lock.sh`, `boot/*.packages`, `boot/*.lock`, `nvidia/toolchain.*` | i lock degli ambienti, uno per stadio di Containerfile: `lock.sh generate` riscrive quelli i cui input sono cambiati (base, lista dei pacchetti, e per il builder SRPM pinnato e bcond; `--force`: anche a input fermi), `lock.sh check` li controlla tutti in `build.sh` |
| `bconds.sh` | i bcond di kernel.spec, gli stessi per `dnf builddep`, `rpmbuild` e il lock |
| `boot.sh` | la boot matrix: dal kernel-core a quattro avvii QEMU con le asserzioni della spec |
| `boot/Containerfile`, `boot/init` | l'ambiente della boot matrix (qemu, OVMF, shim, ukify, Firecracker, strumenti di benchmark) e il PID 1 dell'initramfs di prova |
| `microvm/kernel-local`, `microvm/azoth-microvm.spec` | il kernel guest per le MicroVM (spec, sezione 9): frammento sopra x86_64_defconfig + kvm_guest.config e lo spec minimo che mette vmlinux, bzImage, config e release in `/usr/lib/athanor/microvm/` |
| `microvm/boot.sh`, `microvm/init` | il gate del kernel guest: vmlinux in Firecracker con una rootfs ext4 di prova, `K6 RESULT ok` sulla seriale |
| `nvidia.sh` | i moduli kernel NVIDIA, rami `open` (610) e `legacy` (580), contro il kernel-devel: `build`, `sign` e `manifest` (l'hash del `.run` legacy) |
| `nvidia/Containerfile`, `nvidia/sources.sha256` | l'ambiente di nvidia.sh (la toolchain LLVM del kernel, kmod, openssl) e l'hash del `.run` legacy |
| `sign-kernel.sh` | le firme degli artefatti del kernel (D43): vmlinuz con sbsign, moduli NVIDIA con sign-file dopo l'allow-list `check-modules`, `verify` contro il vmlinuz dell'RPM; gira solo nell'immagine del signer |
| `signer/Containerfile`, `signer/toolchain.*`, `signer/publish.sh`, `signer/run.sh`, `signer/cosign.pin` | il signer: immagine con sbsigntools e il sign-file di kernel-devel dal lock e `sign-kernel.sh` dentro, taggata dall'hash dei suoi input (`publish.sh`) e usata per digest (`signer/image.digest`); `run.sh` la lancia senza rete, con gli input in sola lettura, negli step senza chiave (`prepare`, `inputs`, che risolve di nuovo il kernel con cosign scaricato per sha256) e nello step con le chiavi (`sign`) |

## Pin correnti

<!-- pins:begin (table written by bump.py apply) -->
| pin | value |
| --- | --- |
| `FEDORA_KERNEL_NVR` | `7.2.8-100.fc43` |
| `FEDORA_KEY_FPR` | `c6e7f081cf80e13146676e88829b606631645531` |
| `KERNEL_CHANNEL` | `stable` |
| `CACHYOS_RELEASE` | `cachyos-7.2.8-1` |
| `CACHYOS_CONFIG_COMMIT` | `6676e72b85eb9e30d079a8c3dcdf93aedd1e8226` |
| `CACHYOS_PATCHES_COMMIT` | `17bb0bb818d283d0dc6e0280a2e9d5b95e66808b` |
| `NVIDIA_OPEN_VERSION` | `615.71.09` |
| `NVIDIA_OPEN_COMMIT` | `61dcc93722ecb418bb5f2e00923f05b4b8051dd1` |
| `NVIDIA_LEGACY_VERSION` | `580.178.04` |
<!-- pins:end -->

## Uso locale

```sh
podman build -t localhost/azoth-builder -f forge/specs/azoth/builder/Containerfile forge/specs/azoth
mkdir -p "$HOME/.cache/azoth" out
podman run --rm -v "$PWD:/forge" -v "$HOME/.cache/azoth:/var/cache/azoth" \
  -w /forge localhost/azoth-builder \
  bash forge/specs/azoth/build.sh --stage prep --out /forge/out
```

`prep` dura pochi minuti e lascia in `out/` il config generato e il `kernel-local`
completo delle opzioni derivate; `build` produce gli RPM (un'ora su 16 core) in
`out/kernel`, `out/devel`, `out/debuginfo`, con l'NVR in `out/nvr`. La CI e'
`.github/workflows/kernel-build.yml`: ricompila solo se `build-inputs.py` non
coincide con l'attestazione dei pin dell'immagine `<nvr>` gia' pubblicata; altrimenti
la boot matrix gira sul kernel-core pubblicato e non si pubblica nulla.

Se `prep` si ferma con `refresh needed` (una patch di `patches.list` non entra piu'
senza fuzz):

```sh
podman run --rm -v "$PWD:/forge" -v "$HOME/.cache/azoth:/var/cache/azoth" \
  -w /forge localhost/azoth-builder \
  bash forge/specs/azoth/build.sh --stage refresh --out /forge/out
```

Le copie da rivedere finiscono in `out/refreshed/`, da copiare sotto
`patches/refreshed/` dopo aver riletto gli hunk applicati con fuzz.

## Boot matrix

```sh
podman build -t localhost/azoth-boot -f forge/specs/azoth/boot/Containerfile forge/specs/azoth
podman run --rm --device /dev/kvm -v "$PWD:/forge" -w /forge localhost/azoth-boot bash forge/specs/azoth/boot.sh --rpms /forge/out --out /forge/boot-out
```

Quattro avvii, firmware {SeaBIOS, OVMF con Secure Boot via shim} x CPU {Nehalem,
host}, ognuno con le asserzioni di `boot/init` (uname, BTF, bpftool, sched_ext, IMA,
lockdown, BBR v3, taint, dmesg; in UEFI anche Secure Boot acceso e MOK arruolata).
Serve solo il kernel-core: `--rpms` accetta l'`out/` di build.sh o una directory con
il solo RPM. Senza `/dev/kvm` (WSL, podman machine) aggiungi `--accel tcg`: minuti
invece di secondi, e `host` diventa `max`. Log seriali e riepilogo in `boot-out/`.
Con `--mok CERT` arruola altri certificati in MokList e con `--insmod FILE.ko:ERRNO`
carica moduli nel guest, in tutti i casi, pretendendo l'errno di insmod: `ENODEV` per un
modulo firmato con la chiave dei moduli compilata nel kernel, senza il suo hardware,
`EKEYREJECTED` per uno non firmato o firmato da una MOK arruolata. Ogni caso verifica
anche che i certificati di `keys/modules` e `keys/revoked` siano stati caricati. E' la
prova della catena dei moduli esterni (spec, sezione 7, gate 4).

## Kernel guest MicroVM

`build.sh --stage microvm` fa il prep e compila solo il kernel guest (pochi minuti): il
pacchetto `azoth-microvm` finisce in `out/microvm/`, con `build` accanto agli
RPM del kernel. Il gate:

```sh
podman run --rm --device /dev/kvm -v "$PWD:/forge" -w /forge localhost/azoth-boot bash forge/specs/azoth/microvm/boot.sh --rpms /forge/out --out /forge/boot-out/microvm
```

Firecracker vuole KVM: senza `/dev/kvm` il gate gira solo in CI.

## Settimanale: riproducibilita' e benchmark

`.github/workflows/kernel-weekly.yml` (domenica, dal branch di default; a mano con
`workflow_dispatch`, anche con la variante `o3` per rimisurare -O3). In locale:

```sh
podman run --rm -v "$PWD:/forge" -w /forge localhost/azoth-builder python3 forge/specs/azoth/repro.py --a /forge/out-a --b /forge/out-b --out /forge/repro-out
podman run --rm --device /dev/kvm -v "$PWD:/forge" -w /forge localhost/azoth-boot bash forge/specs/azoth/bench.sh --rpms /forge/out --out /forge/bench-out
```

`--a` e `--b` sono due directory di RPM dello stesso pin (la B con il kernel-devel, da cui
viene `scripts/extract-vmlinux`). Il benchmark senza KVM (`--accel tcg`) prova solo
l'initramfs: i numeri misurerebbero l'emulatore.

## Moduli NVIDIA

```sh
podman build -t localhost/azoth-nvidia -f forge/specs/azoth/nvidia/Containerfile forge/specs/azoth
podman run --rm -v "$PWD:/forge" -v "$HOME/.cache/azoth:/var/cache/azoth" \
  -w /forge localhost/azoth-nvidia \
  bash forge/specs/azoth/nvidia.sh build --driver open --devel /forge/out/devel --out /forge/nvidia-out
```

`--driver open` (610, GitHub al commit pinnato) o `legacy` (580, il `.run` nel
manifest degli hash); `--devel` e' una directory con il `kernel-devel-*.rpm` (l'`out/`
di build.sh, o l'immagine `azoth-devel:<nvr>`). I `.ko` finiscono in
`nvidia-out/<driver>/lib/modules/<kver>/extra/nvidia/` (il layout che l'immagine di
sistema copia) con il vermagic del kernel e i preamboli kCFI, senza firma:
`nvidia.sh sign --key K --cert C --devel DIR --out DIR` li firma con sign-file del
kernel-devel, in locale con una chiave effimera, in CI con la chiave dei moduli del
progetto (workflow `.github/workflows/nvidia-kmod.yml`, che poi li carica in QEMU con
`boot.sh --mok --insmod` prima di pubblicarli).

## Pubblicazione

Ogni push su `main` o `iso-v0` che tocca questa directory costruisce e pubblica quattro
OCI con i soli RPM dentro, tag `<nvr>` (quello che stampa `bash nvr.sh`):

| Immagine | Contenuto |
|----------|-----------|
| `ghcr.io/ars-regia/azoth` | kernel, core, modules, modules-core/extra/internal, uki-virt |
| `ghcr.io/ars-regia/azoth-devel` | kernel-devel, per i kmod esterni (NVIDIA, fase K4) |
| `ghcr.io/ars-regia/azoth:<nvr>-microvm` | azoth-microvm: vmlinux, bzImage, config e release del kernel guest (sezione 9) |
| `ghcr.io/ars-regia/azoth-debuginfo` | debuginfo, restano le due versioni piu' recenti |
| `ghcr.io/ars-regia/azoth-nvidia` | i `.ko` NVIDIA firmati, tag `<nvr>-k<12 cifre esadecimali del digest del kernel>-<branch>-<versione>` (pubblicata da `nvidia-kmod.yml`, non da Kernel Build) |

Ognuna e' firmata con cosign keyless dall'identita' del workflow, porta un SBOM SPDX
e un'attestazione custom con i pin (`pins.json`: pins.env, hash del manifest, del
delta e del Containerfile, immagine base del builder); la principale ha anche la
provenance SLSA di GitHub. `:latest` si muove solo sul branch di default del repository. Verifica:

```sh
cosign verify --certificate-identity-regexp '^https://github.com/ars-regia/athanor/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "ghcr.io/ars-regia/azoth:$(bash nvr.sh)"
gh attestation verify "oci://ghcr.io/ars-regia/azoth:$(bash nvr.sh)" --repo ars-regia/athanor
```

## Bump

Il bot (`kernel-bump.yml`, spec sezione 8) apre ogni giorno, dal branch di default, una
PR con i pin nuovi, i manifesti rigenerati, l'esito di `prep` e le opzioni derivate, e
le mette l'auto-merge sul check `Kernel gate` di Kernel Build, ma solo se `prep` e' verde e nessun lock NVIDIA si muove: altrimenti la fonde una persona. A mano, nella stessa
sequenza:

1. `python3 bump.py check --group kernel` mostra cosa muoverebbe;
   `python3 bump.py apply --group kernel` riscrive `pins.env`, i `FROM` dei Containerfile
   del kernel e la tabella dei pin qui sopra. La base di `system/Containerfile` e i lock
   NVIDIA ripubblicati sono del gruppo `system` (`--group system`), in una PR a parte. Un pin scelto
   a mano (per esempio un cambio di serie) si scrive in `pins.env` e basta.
2. Nel builder: `build.sh --stage manifest --out DIR` e `nvidia.sh manifest --out DIR`
   scaricano i sorgenti dei pin e scrivono `DIR/sources.sha256`, da copiare in
   `SOURCES/` e in `nvidia/`.
3. `prep` verde, poi `build`. Se il gate del config elenca opzioni nuove o mancanti,
   la decisione va in `kernel-local`.
