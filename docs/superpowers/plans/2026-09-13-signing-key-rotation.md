# Signing Key Rotation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire the single project MOK of 2026-09-04 and replace it with two keys: a Secure Boot key (UKI and PCR policy) and a kernel module signing key whose certificate is compiled into Azoth, so the NVIDIA modules load whatever the firmware state.

**Architecture:** The module certificate enters the kernel through `CONFIG_SYSTEM_TRUSTED_KEYS`; `build.sh` adds it to the tree it diffs into `linux-kernel-test.patch`. The retired MOK goes into the kernel blacklist through `CONFIG_SYSTEM_REVOCATION_KEYS`. The Secure Boot key is not a CA, so `CONFIG_INTEGRITY_CA_MACHINE_KEYRING_MAX=y` (already on) keeps an enrolled copy of it out of the machine keyring: it can never authorise a module. The boot matrix proves all three properties.

**Tech Stack:** Bash, OpenSSL 3, Kconfig, GitHub Actions, QEMU/OVMF/shim (`forge/specs/azoth/boot.sh`).

**Spec:** `docs/architecture/doc_kernel_build.md`, sections 6 (signing), 7 (gates 3 and 4) and 10 (NVIDIA).

## Global Constraints

- Private keys never enter the repository, a log or this conversation; they reach CI only as secrets of the `signing` environment (branches `main`, `iso-v0`).
- New secrets: `SECUREBOOT_SIGNING_KEY`, `MODULE_SIGNING_KEY`. `MOK_PRIVATE_KEY` is deleted only after Task 8 is green.
- Keys: RSA 4096, SHA-256 certificates, 3650 days (neither shim nor the kernel enforce expiry).
- Certificates: `forge/specs/azoth/keys/secureboot/athanor-secureboot.{pem,der}`, `forge/specs/azoth/keys/modules/athanor-modules.pem`, `forge/specs/azoth/keys/revoked/*.pem`.
- Code comments, commits and workflow output in English; the Italian documents (`doc_kernel_build.md`, `KERNEL.md`) stay Italian.
- No `|| true`, no `continue-on-error`. Never push `forge/**` while an Orchestrator run is in progress.
- Commits end with the session attribution lines.

---

### Task 1: Key profiles and generator

**Files:**
- Create: `forge/specs/azoth/keys/profiles/secureboot.cnf`
- Create: `forge/specs/azoth/keys/profiles/modules.cnf`
- Create: `forge/specs/azoth/keys/generate.sh`

**Interfaces:**
- Produces: `generate.sh secureboot|modules --key-dir DIR` writes `DIR/athanor-<profile>.key` and `keys/<profile>/athanor-<profile>.pem` (+ `.der` for secureboot). `profiles/secureboot.cnf` is also used by Tasks 4 and 5 for ephemeral test MOKs.

- [ ] **Step 1: Write `profiles/secureboot.cnf`**

```ini
# Secure Boot signing key of Athanor (docs/architecture/doc_kernel_build.md, section 6):
# signs the UKI, which shim verifies through MokList, and its PCR policy (ukify
# --pcr-private-key). Not a CA: the kernel admits into the machine keyring only CA
# certificates (CONFIG_INTEGRITY_CA_MACHINE_KEYRING_MAX), so an enrolled copy of this key
# can never authorise a kernel module. boot.sh and nvidia-kmod.yml build their ephemeral
# test MOKs from this same profile.
[ req ]
default_bits = 4096
default_md = sha256
distinguished_name = req_distinguished_name
prompt = no
string_mask = utf8only
x509_extensions = exts

[ req_distinguished_name ]
CN = Athanor Secure Boot Signing Key

[ exts ]
basicConstraints = critical,CA:FALSE
keyUsage = digitalSignature
extendedKeyUsage = codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
```

- [ ] **Step 2: Write `profiles/modules.cnf`**

```ini
# Kernel module signing key of Athanor (docs/architecture/doc_kernel_build.md, section 6):
# signs the external modules (NVIDIA); its certificate is compiled into the kernel
# (kernel-local, CONFIG_SYSTEM_TRUSTED_KEYS). The extensions are those of the key the
# kernel generates for its own modules (certs/default_x509.genkey).
[ req ]
default_bits = 4096
default_md = sha256
distinguished_name = req_distinguished_name
prompt = no
string_mask = utf8only
x509_extensions = exts

[ req_distinguished_name ]
CN = Athanor Kernel Module Signing Key

[ exts ]
basicConstraints = critical,CA:FALSE
keyUsage = digitalSignature
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
```

- [ ] **Step 3: Write `generate.sh`**

```bash
#!/usr/bin/env bash
# Generates one of the project signing keys (docs/architecture/doc_kernel_build.md,
# section 6) from its OpenSSL profile in profiles/. The private key goes to KEY-DIR, which
# must lie outside the repository; the public certificate goes to keys/<profile>/ in PEM,
# and in DER as well for secureboot, the form mokutil --import takes. Prints what to
# record: the SHA-256 fingerprint and the subject key identifier, the id the kernel logs
# a compiled-in certificate with.
#
# Usage: generate.sh secureboot|modules --key-dir DIR
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
die() { echo "error: $*" >&2; exit 1; }

PROFILE=${1:-}
[[ $# -gt 0 ]] && shift
KEY_DIR=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --key-dir) KEY_DIR=$2; shift 2 ;;
    *) die "unknown argument: $1" ;;
  esac
done
[[ ($PROFILE == secureboot || $PROFILE == modules) && $KEY_DIR ]] \
  || { echo "usage: generate.sh secureboot|modules --key-dir DIR" >&2; exit 2; }

REPO=$(git -C "$HERE" rev-parse --show-toplevel)
KEY_DIR=$(realpath -m "$KEY_DIR")
[[ $KEY_DIR/ != "$REPO"/* ]] || die "--key-dir must lie outside the repository: $KEY_DIR"
(umask 077 && mkdir -p "$KEY_DIR")

KEY=$KEY_DIR/athanor-$PROFILE.key
CERT=$HERE/$PROFILE/athanor-$PROFILE.pem
[[ ! -e $KEY ]] || die "$KEY exists: a key is never overwritten"
[[ ! -e $CERT ]] || die "${CERT#"$REPO"/} exists: retire the previous certificate first"
mkdir -p "$HERE/$PROFILE"

umask 077
openssl req -new -x509 -newkey rsa:4096 -sha256 -days 3650 -nodes \
  -config "$HERE/profiles/$PROFILE.cnf" -keyout "$KEY" -out "$CERT"
chmod 644 "$CERT"
if [[ $PROFILE == secureboot ]]; then
  openssl x509 -in "$CERT" -outform DER -out "${CERT%.pem}.der"
  chmod 644 "${CERT%.pem}.der"
fi

echo "private key: $KEY"
echo "certificate: ${CERT#"$REPO"/}"
openssl x509 -in "$CERT" -noout -subject -enddate -fingerprint -sha256 \
  -ext basicConstraints,keyUsage,extendedKeyUsage,subjectKeyIdentifier
```

- [ ] **Step 4: Verify the generator with throwaway keys**

Run:
```bash
chmod +x forge/specs/azoth/keys/generate.sh
bash forge/specs/azoth/keys/generate.sh modules --key-dir .scratch/keytest; echo "exit $?"
bash forge/specs/azoth/keys/generate.sh secureboot --key-dir /tmp/claude-keytest
bash forge/specs/azoth/keys/generate.sh modules --key-dir /tmp/claude-keytest
bash forge/specs/azoth/keys/generate.sh modules --key-dir /tmp/claude-keytest2; echo "exit $?"
```
Expected: the first call exits 1 with `--key-dir must lie outside the repository` and creates no directory. The second and third print `CA:FALSE`, `Digital Signature`, a subject key identifier, and `Code Signing` for secureboot only. The fourth exits 1 with `exists: retire the previous certificate first`.

Then clean up and confirm nothing is left:
```bash
rm -rf /tmp/claude-keytest /tmp/claude-keytest2 forge/specs/azoth/keys/secureboot forge/specs/azoth/keys/modules
git status --short forge/specs/azoth/keys
```
Expected: only `?? forge/specs/azoth/keys/generate.sh` and `?? forge/specs/azoth/keys/profiles/`.

- [ ] **Step 5: Lint and commit**

Run: `just lint`. Expected: no shellcheck finding on `generate.sh`.

```bash
git add forge/specs/azoth/keys/generate.sh forge/specs/azoth/keys/profiles
git commit -m "build(kernel): add the profiles and the generator of the project signing keys"
```

---

### Task 2: Generate the keys and register the secrets (the user runs this)

**Files:**
- Create: `forge/specs/azoth/keys/secureboot/athanor-secureboot.pem`, `.der`
- Create: `forge/specs/azoth/keys/modules/athanor-modules.pem`
- Move: `forge/specs/azoth/keys/mok/athanor-mok.pem` → `forge/specs/azoth/keys/revoked/ermete-mok-2026-09-04.pem`
- Delete: `forge/specs/azoth/keys/mok/athanor-mok.der`
- Modify: `.gitignore:40-42`

**Interfaces:**
- Produces: the certificate paths of Global Constraints; secrets `SECUREBOOT_SIGNING_KEY`, `MODULE_SIGNING_KEY` in the `signing` environment.

- [ ] **Step 1: Generate both keys** in a tmpfs directory: the plaintext never touches the btrfs disk, where copy-on-write makes `shred` ineffective, and it vanishes with the session. The only persistent copy is the encrypted one on the `ATHANOR-KIT` USB stick (Step 3).

```bash
KEYDIR=/run/user/1000/athanor-keys
bash forge/specs/azoth/keys/generate.sh secureboot --key-dir "$KEYDIR"
bash forge/specs/azoth/keys/generate.sh modules --key-dir "$KEYDIR"
```
Record both SHA-256 fingerprints.

- [ ] **Step 2: Register the secrets** (the values go from file to GitHub; nothing is echoed):

```bash
gh secret set SECUREBOOT_SIGNING_KEY --env signing < "$KEYDIR/athanor-secureboot.key"
gh secret set MODULE_SIGNING_KEY --env signing < "$KEYDIR/athanor-modules.key"
gh secret list --env signing
```
Expected: three secrets, `MOK_PRIVATE_KEY` still among them. Nothing uses the new ones yet, so nothing breaks.

- [ ] **Step 3: Encrypted copy outside GitHub, then destroy the plaintext**

```bash
KIT=/run/media/hr-mes/ATHANOR-KIT
tar -C "$KEYDIR" -c athanor-secureboot.key athanor-modules.key \
  | gpg --symmetric --cipher-algo AES256 -o "$KIT/athanor-signing-keys-2026-09-13.tar.gpg"
sync
gpg --decrypt "$KIT/athanor-signing-keys-2026-09-13.tar.gpg" | tar -t
```
Expected: the two file names, decrypted from the stick with the passphrase. Then `shred -u "$KEYDIR"/*.key && rmdir "$KEYDIR"` (tmpfs: the pages are freed) and unmount the stick.

- [ ] **Step 4: Retire the old certificate in the tree and admit the new ones in `.gitignore`**

```bash
mkdir -p forge/specs/azoth/keys/revoked
git mv forge/specs/azoth/keys/mok/athanor-mok.pem forge/specs/azoth/keys/revoked/ermete-mok-2026-09-04.pem
git rm forge/specs/azoth/keys/mok/athanor-mok.der
```
In `.gitignore` replace:
```
# Il certificato pubblico della MOK di progetto: solo la parte pubblica, la chiave
# privata sta nell'environment GitHub `signing` (docs/architecture/doc_kernel_build.md, sezione 6).
!forge/specs/azoth/keys/mok/athanor-mok.pem
```
with:
```
# The public certificates of the project signing keys; the private keys live in the
# GitHub environment `signing` (docs/architecture/doc_kernel_build.md, section 6).
!forge/specs/azoth/keys/secureboot/*.pem
!forge/specs/azoth/keys/modules/*.pem
!forge/specs/azoth/keys/revoked/*.pem
```

- [ ] **Step 5: Verify and commit**

Run: `git status --short forge/specs/azoth/keys .gitignore`
Expected: the two new PEMs and the DER added, the rename, the deleted DER, `.gitignore` modified; no `.key` anywhere.

```bash
git add .gitignore forge/specs/azoth/keys
git commit -m "build(kernel): split the project MOK into a Secure Boot key and a module signing key"
```

---

### Task 3: Compile the module certificate and the revocation list into Azoth

**Files:**
- Modify: `forge/specs/azoth/kernel-local:38`
- Modify: `forge/specs/azoth/build.sh` (after the Athanor patches loop, before `g diff --binary`)
- Modify: `forge/specs/azoth/build-inputs.py`

**Interfaces:**
- Consumes: `keys/modules/*.pem` (exactly one), `keys/revoked/*.pem` (one or more).
- Produces: in-tree `certs/athanor-modules.pem`, `certs/athanor-revoked.pem`; reuse key field `keys_sha256`.

- [ ] **Step 1: `kernel-local`, after `CONFIG_MODULE_SIG_FORCE=y`**

```
# External modules (NVIDIA) are trusted through the project module signing certificate
# compiled into the kernel, whatever the firmware state; retired project keys are
# revoked. build.sh places both files in the tree from keys/modules and keys/revoked.
CONFIG_SYSTEM_TRUSTED_KEYS="certs/athanor-modules.pem"
CONFIG_SYSTEM_REVOCATION_LIST=y
CONFIG_SYSTEM_REVOCATION_KEYS="certs/athanor-revoked.pem"
```
Dependencies already hold in the Fedora config: `SYSTEM_BLACKLIST_KEYRING=y`, `PKCS7_MESSAGE_PARSER=y`. `check_delta` fails the build if any line does not survive.

- [ ] **Step 2: `build.sh`, right after the `for p in "${ATHANOR_PATCHES[@]}"` loop**

```bash
# The certificates kernel-local compiles in (CONFIG_SYSTEM_TRUSTED_KEYS and
# CONFIG_SYSTEM_REVOCATION_KEYS, paths relative to the tree): added to the index, so
# linux-kernel-test.patch carries them into the tree rpmbuild prepares.
mapfile -t MODULE_CERTS < <(find "$HERE/keys/modules" -name '*.pem' | sort)
mapfile -t REVOKED_CERTS < <(find "$HERE/keys/revoked" -name '*.pem' | sort)
[[ ${#MODULE_CERTS[@]} -eq 1 ]] || die "keys/modules: expected one certificate, found ${#MODULE_CERTS[@]}"
[[ ${#REVOKED_CERTS[@]} -ge 1 ]] || die "keys/revoked: no certificate, CONFIG_SYSTEM_REVOCATION_KEYS needs one"
# awk 1 rather than cat ends every file with a newline: a PEM without one would glue its
# END line to the next BEGIN, and extract-cert stops at the first unparsable block without
# an error, dropping the remaining certificates.
add_to_tree() { # add_to_tree PATH FILE...: the concatenation of FILE... at PATH in the index
  g update-index --add --cacheinfo "100644,$(awk 1 "${@:2}" | g hash-object -w --stdin),$1"
}
add_to_tree certs/athanor-modules.pem "${MODULE_CERTS[@]}"
add_to_tree certs/athanor-revoked.pem "${REVOKED_CERTS[@]}"
```

- [ ] **Step 3: `build-inputs.py`**

In the docstring replace `source manifest, config delta, patches, merge rules,` with `source manifest, config delta, patches, the certificates compiled in (keys/modules, keys/revoked), merge rules,`. In the dict, after `"patches_sha256": {...},` add:

```python
            "keys_sha256": {
                p.relative_to(k).as_posix(): sha(p.relative_to(k))
                for d in ("modules", "revoked")
                for p in sorted((k / "keys" / d).glob("*.pem"))
            },
```

- [ ] **Step 4: Verify**

Run:
```bash
python3 forge/specs/azoth/build-inputs.py | python3 -c 'import json,sys; print(json.load(sys.stdin)["keys_sha256"])'
bash -n forge/specs/azoth/build.sh && just lint
```
Expected: two entries, `keys/modules/athanor-modules.pem` and `keys/revoked/ermete-mok-2026-09-04.pem`; lint clean. The real proof is the CI build (Task 8): `check_delta` plus the certificate assertions of Task 4.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/azoth/kernel-local forge/specs/azoth/build.sh forge/specs/azoth/build-inputs.py
git commit -m "build(kernel): compile the module signing certificate and the revoked keys into Azoth"
```

---

### Task 4: Boot matrix proves the compiled-in trust

**Files:**
- Modify: `forge/specs/azoth/boot/init`
- Modify: `forge/specs/azoth/boot.sh`

**Interfaces:**
- Produces: `k3.certs=<skid>,<skid>` on every test command line; `k3.insmod` on every case, BIOS included; `--mok` now means "enrolled MOKs that must not authorise modules".

- [ ] **Step 1: `boot/init`** — parse the new parameter: change `uname_expected='' sb='' insmod_list=''` to `uname_expected='' sb='' insmod_list='' certs_list=''` and add to the `case`:
```sh
    k3.certs=*) certs_list=${w#*=} ;;
```
After `insmod_expect() {...}` add:
```sh
cert_loaded() { # cert_loaded SKID: the kernel loaded a compiled-in certificate with this key
  # id; x509_load_certificate_list logs "Loaded X.509 cert '<CN>: <skid in hex>'" for the
  # trusted keyring and the revocation list alike.
  dmesg | grep -E "Loaded X.509 cert '.*: $1'"
}
```
After the `check dmesg ...` line add:
```sh
for skid in $(echo "$certs_list" | tr ',' ' '); do
  check "cert-$skid" cert_loaded "$skid"
done
```

- [ ] **Step 2: `boot.sh`**

Replace the header lines 9-11 and 19-22 with:
```bash
# With --insmod it also exercises the external module chain (section 7, gate 4) in every
# case: a .ko signed with the module signing key compiled into the kernel must load
# (ENODEV: good signature, no GPU), and any other must be rejected (EKEYREJECTED).
```
```bash
#   --mok    certificate (PEM) to enrol in MokList besides the ephemeral one of the UKI,
#            to prove an enrolled MOK does not authorise modules
#   --insmod module to load in the guest and the errno expected from insmod (ENODEV,
#            EKEYREJECTED, or 0), in every case
```
Replace the `TEST_CMDLINE=` line with:
```bash
# The certificates the kernel must have compiled in (kernel-local), by subject key
# identifier, the id the kernel logs them with: the module signing one and the revoked.
skid() { openssl x509 -in "$1" -noout -ext subjectKeyIdentifier | tail -n 1 | tr -d ' :' | tr 'A-F' 'a-f'; }
K3_CERTS=''
for cert in "$HERE"/keys/modules/*.pem "$HERE"/keys/revoked/*.pem; do
  K3_CERTS+="${K3_CERTS:+,}$(skid "$cert")"
done
TEST_CMDLINE="$CMDLINE console=ttyS0,115200 panic=-1 ima_policy=tcb k3.uname=$KVER k3.certs=$K3_CERTS"
```
Replace the comment `# parameter lists file:errno and goes only into the command line of the UKI (UEFI cases).` with `# parameter lists file:errno and goes into the command line of every case.` and, right after the `K3_INSMOD` loop's `done`, add:
```bash
TEST_CMDLINE+="${K3_INSMOD:+ k3.insmod=$K3_INSMOD}"
```
In the `ukify build` call change `--cmdline "$TEST_CMDLINE k3.sb=1${K3_INSMOD:+ k3.insmod=$K3_INSMOD}"` to `--cmdline "$TEST_CMDLINE k3.sb=1"`.
Change the ephemeral MOK so it has the Secure Boot key's shape:
```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$HERE/keys/profiles/secureboot.cnf" \
  -subj '/CN=Athanor OS K3 test MOK/' -keyout "$WORK/mok.key" -out "$OUT/mok.pem" 2> /dev/null
```
Replace the MokList comment with:
```bash
# MokList: the ephemeral MOK of the UKI and those of --mok. shim copies it to MokListRT;
# none of them is a CA, so the kernel keeps them out of the machine keyring and they
# verify boot artefacts, never modules.
```

- [ ] **Step 3: Failing run against the published kernel (no certificate compiled in)**

```bash
mkdir -p .scratch/k3/out
ctr=$(podman create ghcr.io/hr-mes/azoth:7.1.8-100.azoth.fc43 /kernel-core)
podman cp "$ctr:/." .scratch/k3/out/ && podman rm "$ctr"
podman build -t localhost/azoth-boot forge/specs/azoth/boot
podman run --rm --device /dev/kvm -v "$PWD:/forge" -w /forge localhost/azoth-boot \
  bash forge/specs/azoth/boot.sh --rpms /forge/.scratch/k3/out --out /forge/.scratch/k3/boot-out --case bios-host
```
Expected: `K3 FAIL cert-<module skid>` and `K3 FAIL cert-00719df9e5029ff44ef08fb1f136265370c31db1`, `bios-host: FAIL`. Every other check `ok`. This proves the assertion bites; it turns green on the kernel of Task 8.

- [ ] **Step 4: Lint and commit**

Run: `just lint`. Expected: clean.
```bash
git add forge/specs/azoth/boot/init forge/specs/azoth/boot.sh
git commit -m "test(kernel): assert the compiled-in certificates and load modules in every boot case"
```

---

### Task 5: NVIDIA kmod workflow signs with the module key and proves separation

**Files:**
- Modify: `.github/workflows/nvidia-kmod.yml` (header, `sign`, `boot`)
- Modify: `forge/specs/azoth/nvidia.sh:27-28` (comment)

**Interfaces:**
- Consumes: secret `MODULE_SIGNING_KEY`; `keys/modules/athanor-modules.pem`; `keys/profiles/secureboot.cnf`; `boot.sh --mok/--insmod` of Task 4.
- Produces: artifact `nvidia-mok-signed` (`mok/open/...`, `mok/test-mok.pem`).

- [ ] **Step 1: Header comment** — replace from `# .ko files and the project MOK. boot: in QEMU with Secure Boot and the project MOK` to `# GPU) and an unsigned copy must be rejected (EKEYREJECTED). publish: OCI` with:
```yaml
# .ko files and the module signing key. boot: in QEMU, every case of the matrix: the
# signed nvidia.ko of each branch must pass the signature check against the certificate
# compiled into the kernel (ENODEV: no GPU), while an unsigned copy and a copy signed by
# an enrolled Secure Boot-profile MOK must be rejected (EKEYREJECTED). publish: OCI
```

- [ ] **Step 2: `sign` job** — before the signing step insert:
```yaml
      - name: Sign a copy with an enrolled-MOK-shaped key
        # The negative sample of the boot job: nvidia.ko of the open branch signed by an
        # ephemeral key with the Secure Boot profile, whose certificate boot.sh enrols in
        # MokList. The kernel must refuse it: a Secure Boot key never authorises a module.
        run: |
          set -euo pipefail
          mkdir -p mok
          cp -a out/open mok/open
          openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
            -config forge/specs/azoth/keys/profiles/secureboot.cnf -subj '/CN=Athanor OS K3 test MOK/' \
            -keyout "$RUNNER_TEMP/test-mok.key" -out mok/test-mok.pem 2> /dev/null
          podman run --rm \
            -v "$GITHUB_WORKSPACE:/forge" \
            -v "$RUNNER_TEMP/test-mok.key:/run/test-mok.key:ro" \
            -w /forge localhost/azoth-nvidia \
            bash forge/specs/azoth/nvidia.sh sign --key /run/test-mok.key \
              --cert mok/test-mok.pem --devel /forge/devel --out /forge/mok

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-mok-signed
          path: mok/
          if-no-files-found: error
```
Replace the signing step with:
```yaml
      - name: Sign with the module signing key
        # The key lives in a 0600 file in the runner's tmp for the duration of the
        # command, mounted read-only in the container; the public certificate is in the
        # repository and compiled into the kernel (kernel-local).
        env:
          MODULE_SIGNING_KEY: ${{ secrets.MODULE_SIGNING_KEY }}
        run: |
          set -euo pipefail
          [[ -n ${MODULE_SIGNING_KEY:-} ]] || { echo "MODULE_SIGNING_KEY is not available to this job: check the signing environment" >&2; exit 1; }
          umask 077
          printf '%s\n' "$MODULE_SIGNING_KEY" > "$RUNNER_TEMP/module.key"
          trap 'rm -f "$RUNNER_TEMP/module.key"' EXIT
          podman run --rm \
            -v "$GITHUB_WORKSPACE:/forge" \
            -v "$RUNNER_TEMP/module.key:/run/module.key:ro" \
            -w /forge localhost/azoth-nvidia \
            bash forge/specs/azoth/nvidia.sh sign --key /run/module.key \
              --cert forge/specs/azoth/keys/modules/athanor-modules.pem --devel /forge/devel --out /forge/out
```

- [ ] **Step 3: `boot` job** — after the `nvidia-open-unsigned` download add:
```yaml
      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        # The negative check of separation: nvidia.ko signed by an enrolled MOK.
        with:
          name: nvidia-mok-signed
          path: mok
```
Replace the `Run boot.sh with the signed modules` step with:
```yaml
      - name: Run boot.sh with the signed modules
        # Every case: the trust in the module signing key is compiled into the kernel, so
        # SeaBIOS proves it holds without firmware, and the UEFI cases prove that a MOK
        # enrolled in MokList does not extend it.
        run: |
          set -euo pipefail
          mkdir -p boot-out
          ko="lib/modules/${nvr}.x86_64/extra/nvidia/nvidia.ko"
          podman run --rm --device /dev/kvm -v "$GITHUB_WORKSPACE:/forge" -w /forge localhost/azoth-boot \
            bash forge/specs/azoth/boot.sh --rpms /forge/out --out /forge/boot-out \
              --mok mok/test-mok.pem \
              --insmod "signed/open/${ko}:ENODEV" \
              --insmod "signed/legacy/${ko}:ENODEV" \
              --insmod "unsigned/open/${ko}:EKEYREJECTED" \
              --insmod "mok/open/${ko}:EKEYREJECTED"
```

- [ ] **Step 4: `nvidia.sh` comment** — replace:
```bash
#   --key/--cert  private key and certificate (PEM or DER) of the MOK: in CI the project
#              one from the `signing` environment, locally an ephemeral one
```
with:
```bash
#   --key/--cert  private key and certificate (PEM or DER) of the signer: in CI the
#              project module signing key (`signing` environment, keys/modules), locally
#              an ephemeral one
```

- [ ] **Step 5: Lint and commit**

Run: `just lint` and `python3 scripts/verify.py workflows`. Expected: clean.
```bash
git add .github/workflows/nvidia-kmod.yml forge/specs/azoth/nvidia.sh
git commit -m "ci(nvidia): sign the modules with the module signing key and prove key separation"
```

---

### Task 6: System image signs the UKI with the Secure Boot key

**Files:**
- Modify: `.github/workflows/call-system-image.yml:22-24, 241-242, 277-291`
- Modify: `system/Containerfile:1-2` (comment)

**Interfaces:**
- Consumes: secret `SECUREBOOT_SIGNING_KEY`; `keys/secureboot/athanor-secureboot.pem`. `assemble_uki.sh` is unchanged: it reads `/run/secrets/uki_key` and `uki_cert`.

- [ ] **Step 1: Secret declaration** — replace:
```yaml
      MOK_PRIVATE_KEY:
        description: "Project MOK private key (PEM) that signs the UKI; supplied by the signing environment"
```
with:
```yaml
      SECUREBOOT_SIGNING_KEY:
        description: "Secure Boot private key (PEM) that signs the UKI and its PCR policy; supplied by the signing environment"
```

- [ ] **Step 2: Job comment** — replace:
```yaml
    # The signing environment holds MOK_PRIVATE_KEY, the project MOK that signs the
    # UKI and the external modules (forge/specs/azoth/KERNEL.md).
```
with:
```yaml
    # The signing environment holds SECUREBOOT_SIGNING_KEY, the Secure Boot key that
    # signs the UKI and its PCR policy (forge/specs/azoth/KERNEL.md).
```

- [ ] **Step 3: Build step** — replace `MOK_PRIVATE_KEY: ${{ secrets.MOK_PRIVATE_KEY }}` with `SECUREBOOT_SIGNING_KEY: ${{ secrets.SECUREBOOT_SIGNING_KEY }}`; the presence check with:
```bash
          [[ -n ${SECUREBOOT_SIGNING_KEY:-} ]] || { echo "SECUREBOOT_SIGNING_KEY is not available to this job: check the signing environment" >&2; exit 1; }
          # The Secure Boot private key and its certificate reach assemble_uki.sh as build
          # secrets (/run/secrets/uki_key and /run/secrets/uki_cert): they never enter a layer.
```
(removing the two old `# The MOK private key ...` comment lines), and the two `--secret` lines with:
```bash
            --secret "id=uki_key,env=SECUREBOOT_SIGNING_KEY" \
            --secret "id=uki_cert,src=forge/specs/azoth/keys/secureboot/athanor-secureboot.pem" \
```

- [ ] **Step 4: `system/Containerfile`** — replace `# The NVIDIA modules are built for the kernel NVR of the pins, signed with the` / `# project MOK by nvidia-kmod.yml and published as azoth-nvidia:<nvr>-<branch>: a` with `# The NVIDIA modules are built for the kernel NVR of the pins, signed with the` / `# project module signing key by nvidia-kmod.yml and published as azoth-nvidia:<nvr>-<branch>: a`.

- [ ] **Step 5: Verify no reference is left, lint, commit**

Run:
```bash
grep -rnE 'MOK_PRIVATE_KEY|keys/mok|athanor-mok' --exclude-dir=graph-vaults --exclude-dir=.git . | grep -v docs/superpowers/plans
just lint
```
Expected: hits only in `docs/architecture/doc_kernel_build.md` and `forge/specs/azoth/KERNEL.md` (Task 7). Lint clean.
```bash
git add .github/workflows/call-system-image.yml system/Containerfile
git commit -m "ci(image): sign the UKI with the Secure Boot key"
```

---

### Task 7: Documentation

**Files:**
- Modify: `docs/architecture/doc_kernel_build.md` (line 61 table row, section 6, section 7 gates 3-4, section 10 table)
- Modify: `forge/specs/azoth/KERNEL.md:13, 74-83, 120-128`

- [ ] **Step 1: `doc_kernel_build.md` line 61** — replace the `keys/mok/` row with:
```
| `keys/`                  | profili e generatore delle chiavi di firma (`profiles/`, `generate.sh`); certificati pubblici della chiave Secure Boot (`secureboot/`), della chiave dei moduli (`modules/`) e delle chiavi ritirate (`revoked/`) (sezione 6); le chiavi private sono nell'environment `signing` |
```

- [ ] **Step 2: section 6** — replace the `**Moduli esterni**` and `**UKI**` bullets and the `**Primo avvio**` bullet with:
```markdown
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
  Secret `SECUREBOOT_SIGNING_KEY`, certificato `keys/secureboot/athanor-secureboot.pem`
  (`.der` per `mokutil --import`). Non essendo una CA, anche arruolata resta
  fuori dal keyring machine (`INTEGRITY_CA_MACHINE_KEYRING_MAX`): non può
  autorizzare un modulo.
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
```
In the `**Rootfs**` bullet replace `con roothash firmato dalla stessa chiave del progetto` with `con roothash firmato da una chiave del progetto nel keyring secondario (non quella Secure Boot, che non vi entra)`.

- [ ] **Step 3: section 7** — in gate 3 replace `in UEFI anche \`SecureBoot=1\` e \`MokListRT\` presente.` with `i certificati compilati nel kernel (chiave dei moduli e revocati) caricati, per subject key identifier (\`Loaded X.509 cert\` nel log); in UEFI anche \`SecureBoot=1\` e \`MokListRT\` presente.` In gate 4 replace from `il job \`sign\` li firma con la MOK del` to `\`EKEYREJECTED\`;` with:
```markdown
il job `sign` li firma con la chiave dei moduli del progetto e firma una copia
con una MOK effimera dal profilo Secure Boot; il job `boot` (`boot.sh --mok
--insmod`, tutti e quattro i casi) arruola quella MOK e nel guest carica il
`nvidia.ko` firmato di ogni ramo, atteso `ENODEV` (firma accettata, GPU
assente), una copia non firmata e quella firmata dalla MOK, attese
`EKEYREJECTED`: SeaBIOS prova che la fiducia non dipende dal firmware, UEFI che
una MOK arruolata non la estende;
```

- [ ] **Step 4: section 10 table** — in the `nvidia-open` row replace `firmati MOK` with `firmati con la chiave dei moduli`.

- [ ] **Step 5: `KERNEL.md`** — replace line 13 with:
```
| `keys/` | `profiles/` e `generate.sh`: le chiavi di firma del progetto; certificati pubblici in `secureboot/` (UKI e policy PCR, secret `SECUREBOOT_SIGNING_KEY`), `modules/` (moduli esterni, compilato nel kernel, secret `MODULE_SIGNING_KEY`) e `revoked/` (compilati nella blacklist del kernel); i secret stanno nell'environment `signing` |
```
Replace lines 80-83 (`Con \`--mok CERT\` arruola ...` to `(spec, sezione 7, gate 4).`) with:
```markdown
Con `--mok CERT` arruola altri certificati in MokList e con `--insmod FILE.ko:ERRNO`
carica moduli nel guest, in tutti i casi, pretendendo l'errno di insmod: `ENODEV` per un
modulo firmato con la chiave dei moduli compilata nel kernel, senza il suo hardware,
`EKEYREJECTED` per uno non firmato o firmato da una MOK arruolata. Ogni caso verifica
anche che i certificati di `keys/modules` e `keys/revoked` siano stati caricati. E' la
prova della catena dei moduli esterni (spec, sezione 7, gate 4).
```
Replace lines 125-128 (`\`nvidia.sh sign --key K ...\` to `Boot con \`boot.sh --mok --insmod\` prima di pubblicarli).`) with:
```markdown
`nvidia.sh sign --key K --cert C --devel DIR --out DIR` li firma con sign-file del
kernel-devel, in locale con una chiave effimera, in CI con la chiave dei moduli del
progetto (workflow `.github/workflows/nvidia-kmod.yml`, che poi li carica in QEMU con
`boot.sh --mok --insmod` prima di pubblicarli).
```

- [ ] **Step 6: Verify and commit**

Run:
```bash
grep -rnE 'MOK_PRIVATE_KEY|keys/mok|athanor-mok' --exclude-dir=graph-vaults --exclude-dir=.git . | grep -v docs/superpowers/plans
python3 scripts/verify.py docs && python3 scripts/verify.py paths
```
Expected: no hit; verify.py shows no new failure against its known baseline.
```bash
git add docs/architecture/doc_kernel_build.md forge/specs/azoth/KERNEL.md
git commit -m "docs(kernel): document the Secure Boot key, the module signing key and the revocation"
```

---

### Task 8: Cut-over and end-to-end verification

- [ ] **Step 1: PR gate without publishing.** Push a branch and open a PR into `iso-v0`; Kernel Build runs on PRs and publishes nothing.
```bash
git push -u origin HEAD:signing-key-rotation
gh pr create --base iso-v0 --head signing-key-rotation --title "Split the project MOK into a Secure Boot key and a module signing key" --body-file <(printf '%s\n' "Retires the single project MOK of 2026-09-04. The module signing certificate is compiled into Azoth (CONFIG_SYSTEM_TRUSTED_KEYS) and the retired MOK is revoked (CONFIG_SYSTEM_REVOCATION_KEYS); the UKI is signed with a dedicated Secure Boot key. Plan: docs/superpowers/plans/2026-09-13-signing-key-rotation.md")
gh pr checks --watch
```
Expected: `Kernel gate` green; the boot job log shows `K3 ok   cert-<module skid>` and `K3 ok   cert-00719df9…` in all four cases.

- [ ] **Step 2: Merge** when no Orchestrator run is in progress (`gh run list --workflow athanor-forge-orchestrator.yml --status in_progress`). The push on `iso-v0` starts Kernel Build (publish), then NVIDIA kmod (dispatched by Kernel Build), and in parallel an Orchestrator run that still takes the previous kernel.

- [ ] **Step 3: Watch the chain.** `gh run watch` on Kernel Build, then on NVIDIA kmod. Expected in the kmod `boot` log: `insmod-0 ok`, `insmod-1 ok` (ENODEV), `insmod-2 ok`, `insmod-3 ok` (EKEYREJECTED) in `bios-*` and `uefi-*`.

- [ ] **Step 4: Rebuild the image on the new kernel and modules**, once NVIDIA kmod has published: `gh workflow run athanor-forge-orchestrator.yml --ref iso-v0`, then `gh run watch`. Expected: green; the tier 0 repository reports `[CACHE MISS] Pulling azoth:7.1.8-100.azoth.fc43` (new revision label).

- [ ] **Precondition for any machine with Secure Boot already on** (not the desktop, where it is off): enrol the new certificate *before* upgrading, while the old image still runs: `sudo mokutil --import forge/specs/azoth/keys/secureboot/athanor-secureboot.der`, confirm in MokManager at the next boot, then upgrade. Otherwise shim refuses the new UKI and bootc falls back to the previous deployment.

- [ ] **Step 5: Deploy on the desktop and verify**
```bash
sudo bootc upgrade && systemctl reboot
# after the reboot
lsmod | grep -E '^nvidia'
modinfo -F signer nvidia
journalctl -k -b 0 | grep -E "Loaded X.509 cert|Key was rejected"
openssl x509 -in /etc/pki/uki/uki-signing.crt -noout -subject
```
Expected: `nvidia`, `nvidia_drm`, `nvidia_modeset`, `nvidia_uvm` loaded; signer `Athanor Kernel Module Signing Key`; both certificates loaded and no rejection; UKI certificate `CN=Athanor Secure Boot Signing Key`.

---

### Task 9: Retire the old key

- [ ] **Step 1:** Only after Task 8 Step 5 is green: `gh secret delete MOK_PRIVATE_KEY --env signing` and `gh secret list --env signing` (expected: the two new secrets only).
- [ ] **Step 2:** The user destroys the encrypted offline copy of the 2026-09-04 key. It stays revoked in every kernel from now on.
- [ ] **Step 3:** Secure Boot on the desktop is a separate step, not part of this plan: `sudo mokutil --import forge/specs/azoth/keys/secureboot/athanor-secureboot.der`, confirm in MokManager at the next boot, enable Secure Boot in the firmware.
