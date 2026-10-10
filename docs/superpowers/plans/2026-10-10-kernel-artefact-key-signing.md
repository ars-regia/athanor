# Kernel artefact project-key signing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every OCI artefact of the kernel cycle that an image or the release consumes is signed with the project image key in `sign-system-images`, and every check after that step verifies it against `system/keys/athanor-image-*.pub` (issue #141).

**Architecture:** The keyless job that already resolves the kernel (`kernel-artifacts-final`) writes two lists in the digests-file format of `image-digests.sh`: every kernel artefact of the release, and those not yet signed with a project key. `sign-system-images` validates the second list against a fixed set of kernel repositories and signs it with the same `skopeo copy --sign-by-sigstore-private-key` call it uses for the system images. `verify-system-images` then checks the first list with `cosign verify --key`, the command `KERNEL.md` documents for a verifier outside GitHub.

**Tech Stack:** bash, skopeo, cosign v3.1.3 (pinned by `sigstore/cosign-installer`), GitHub Actions, Python `unittest` with the offline fakes of `system/tests`.

**Spec:** `docs/decisions/0096-kernel-artefacts-project-key.md` (ADR-0096), its scope in `docs/decisions/0098-update-delivery-ci-operations-batch-4.md` item 3, and `docs/architecture/doc_update_trust.md` UT2.

## Global Constraints

- Signed artefacts (ADR-0098 item 3): `azoth`, `azoth-devel`, `azoth-debuginfo`, the MicroVM guest kernel (`azoth:<nvr>-microvm`), `azoth-boot`, `azoth-nvidia` (both module branches) and `azoth-signer`.
- Same tool and format as the system images: `skopeo copy --sign-by-sigstore-private-key`, the classic attachment at `<repo>:sha256-<hex>.sig` (UT2). Never `cosign sign`.
- The signing job runs no tool beside skopeo and holds the key alone (UT2 "The key is alone in its job", D43). Anything that needs cosign runs in a job without the key.
- The keyless signature stays as the build record. Checks before the signing step keep verifying it (ADR-0096 item 2).
- No new approval: the kernel artefacts are signed in the existing `sign-system-images` job (ADR-0096 item 1, ADR-0064).
- Stop-and-ask files: `system/sign-images.sh`, `system/image-digests.sh --check` and the `sign-system-images` job of `.github/workflows/athanor-forge-orchestrator.yml`. Show the maintainer the diff before the push.
- No `|| true`, no `2>/dev/null` hiding an error, no `continue-on-error`. Every message and comment is in English.
- Approved documents (`doc_update_trust.md`, `doc_pipeline.md`, `doc_ci.md`) change only through the pull request the maintainer approves.

## Review Focus

1. **Re-signing grows the signature manifest.** containers/image copies the existing signatures of `repo@digest` and appends the new one, so signing the same kernel digest on every nightly run would add one layer per run. The unsigned list (Task 1) exists for this. Test: an artefact already signed with a project key is not listed as unsigned.
2. **A registry or Rekor outage read as "unsigned"** would only add a signature, never skip one. Still, the probe classifies cosign's two observed "no key signature" messages and fails on anything else (Task 1 test).
3. **`--new-bundle-format` is deprecated in cosign 3.1.3**, and a later cosign may stop reading the classic attachment. The pipeline pins cosign through `cosign-installer`, and a cosign bump that breaks it fails `verify-system-images` loudly. Test: the fake refuses a call without `--new-bundle-format=false`, as real cosign does ("expected key signature, not certificate").
4. **A digests file that names a repository outside the kernel set**, or another registry, never reaches skopeo (Task 2 test).
5. **A run whose signing job was skipped or failed ends red**: `verify-system-images` runs anyway and refuses a kernel artefact without a key signature (Task 3 test).

Observed facts, 2026-10-10, cosign v3.1.3 against ghcr.io:
- `cosign verify --key system/keys/athanor-image-1.pub --new-bundle-format=false --insecure-ignore-tlog=true ghcr.io/ars-regia/athanor-system@sha256:64e40ca7…` verifies the skopeo signature (rc 0).
- The same call on `azoth@sha256:d2b4083f…`, which has no key signature, prints `Error: no signatures found` (rc 10).
- The call with key 2 on an image signed with key 1 prints `Error: no matching signatures: invalid signature when validating ASN.1 encoded signature` (rc 12).
- Without `--new-bundle-format=false` the call fails with `expected key signature, not certificate`.
- `vars.KERNEL_REGISTRY` is unset, so the kernel registry equals the image registry `ghcr.io/<owner>`.

---

### Task 1: Key-signature probe and the release lists in `kernel-artifacts.sh`

**Files:**
- Modify: `system/kernel-artifacts.sh` (header usage, constants after `UNVERIFIED`, `probe_signed`, new `release_digests` and `verify_key`, dispatch)
- Modify: `system/tests/fake_registry.py` (`cosign --key`, fixture key `key_signatures`)
- Test: `system/tests/test_kernel_artifacts.py` (new class `ProjectKey`)

**Interfaces:**
- Produces: `kernel-artifacts.sh signed REF key` prints `signed` or `unsigned`; exit 1 on any other cosign failure.
- Produces: `kernel-artifacts.sh release-digests ALL UNSIGNED`, after `require-ready`. It writes lines `REPOSITORY TAG DIGEST`, the format of `image-digests.sh`, in this order: azoth, azoth-devel, azoth-boot, azoth-nvidia open, azoth-nvidia legacy, azoth `<nvr>-microvm`, azoth-debuginfo, azoth-signer (tag `image.digest`).
- Produces: `kernel-artifacts.sh verify-key FILE` exits 0 when every line is signed with a project key, 1 otherwise. It prints one `signed with a project key:` line per artefact.
- Environment: `KERNEL_KEYS_DIR` (default `system/keys`).

- [ ] **Step 1: Teach the fake cosign key verification**

In `system/tests/fake_registry.py`, add this to the docstring's fixture keys after `sigstore_keys`:

```
  key_signatures {"registry/repo@digest": "file name of the .pub whose private half signed it"}
                `cosign verify --key` accepts it only with that key, and only with
                --new-bundle-format=false (cosign v3 otherwise looks for a bundle)
```

In `cosign()`, insert after the `signature_transient_errors` block and before `regex = …`:

```python
    if "--key" in args:
        if "--new-bundle-format=false" not in args:
            return fail("Error: no matching attestations: expected key signature, not certificate\nerror during command execution: no matching attestations: expected key signature, not certificate")
        signer = fx.get("key_signatures", {}).get(ref)
        if signer is None:
            return fail("Error: no signatures found\nerror during command execution: no signatures found", 10)
        if signer != pathlib.Path(args[args.index("--key") + 1]).name:
            return fail("Error: no matching signatures: invalid signature when validating ASN.1 encoded signature\nerror during command execution: no matching signatures: invalid signature when validating ASN.1 encoded signature", 12)
        return 0
```

- [ ] **Step 2: Write the failing tests**

Append to `system/tests/test_kernel_artifacts.py`:

```python
MICROVM = "sha256:" + "6" * 64
DEBUGINFO = "sha256:" + "7" * 64
SIGNER_DIGEST = (ROOT / "forge/specs/azoth/signer/image.digest").read_text().strip()


def releasable():
    """published() plus the artefacts resolve() does not record: guest kernel, debuginfo, signer."""
    fx = published()
    fx["tags"][f"{REG}/azoth:{NVR}-microvm"] = MICROVM
    fx["tags"][f"{REG}/azoth-debuginfo:{NVR}"] = DEBUGINFO
    fx["signatures"][f"{REG}/azoth@{MICROVM}"] = KERNEL_BUILD
    fx["signatures"][f"{REG}/azoth-debuginfo@{DEBUGINFO}"] = KERNEL_BUILD
    fx["signatures"][f"{REG}/azoth-signer@{SIGNER_DIGEST}"] = SIGNER + "iso-v0"
    return fx


class ProjectKey(Tool):
    def setUp(self):
        super().setUp()
        self.keys = self.dir / "keys"
        self.keys.mkdir()
        for name in ("athanor-image-1.pub", "athanor-image-2.pub"):
            (self.keys / name).write_text("-----BEGIN PUBLIC KEY-----\n")
        self.env["KERNEL_KEYS_DIR"] = str(self.keys)

    def lists(self, fx):
        self.registry(fx)
        self.assertEqual(self.run_script("require-ready").returncode, 0)
        r = self.run_script("release-digests", str(self.dir / "all.txt"), str(self.dir / "unsigned.txt"))
        return r

    def test_signed_with_any_project_key(self):
        ref = f"{REG}/azoth@{KERNEL}"
        for signer, verdict in (("athanor-image-1.pub", "signed"), ("athanor-image-2.pub", "signed"), ("other.pub", "unsigned"), (None, "unsigned")):
            with self.subTest(signer=signer):
                self.registry({"key_signatures": {ref: signer} if signer else {}, "errors": []})
                r = self.run_script("signed", ref, "key")
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertEqual(r.stdout.strip(), verdict)

    def test_an_outage_is_an_error_not_unsigned(self):
        ref = f"{REG}/azoth@{KERNEL}"
        self.registry({"errors": [ref]})
        r = self.run_script("signed", ref, "key")
        self.assertEqual(r.returncode, 1)
        self.assertNotIn("signed", r.stdout)

    def test_release_lists_every_kernel_artefact(self):
        r = self.lists(releasable())
        self.assertEqual(r.returncode, 0, r.stderr)
        want = [
            f"{REG}/azoth {NVR} {KERNEL}",
            f"{REG}/azoth-devel {NVR} {DEVEL}",
            f"{REG}/azoth-boot {boot_tag()} {BOOT}",
            f"{REG}/azoth-nvidia {tag('open')} {MODULE['open']}",
            f"{REG}/azoth-nvidia {tag('legacy')} {MODULE['legacy']}",
            f"{REG}/azoth {NVR}-microvm {MICROVM}",
            f"{REG}/azoth-debuginfo {NVR} {DEBUGINFO}",
            f"{REG}/azoth-signer image.digest {SIGNER_DIGEST}",
        ]
        self.assertEqual((self.dir / "all.txt").read_text().splitlines(), want)
        self.assertEqual((self.dir / "unsigned.txt").read_text().splitlines(), want)

    def test_an_artefact_already_signed_with_the_key_is_not_signed_again(self):
        fx = releasable()
        fx["key_signatures"] = {f"{REG}/azoth@{KERNEL}": "athanor-image-1.pub"}
        r = self.lists(fx)
        self.assertEqual(r.returncode, 0, r.stderr)
        unsigned = (self.dir / "unsigned.txt").read_text()
        self.assertNotIn(f"{REG}/azoth {NVR} {KERNEL}", unsigned)
        self.assertEqual(len(unsigned.splitlines()), 7)

    def test_a_debuginfo_without_its_build_record_is_refused(self):
        fx = releasable()
        del fx["signatures"][f"{REG}/azoth-debuginfo@{DEBUGINFO}"]
        r = self.lists(fx)
        self.assertEqual(r.returncode, 1)
        self.assertIn("azoth-debuginfo", r.stderr)
        self.assertFalse((self.dir / "all.txt").exists())

    def test_verify_key_refuses_an_unsigned_artefact(self):
        fx = releasable()
        fx["key_signatures"] = {f"{REG}/azoth@{KERNEL}": "athanor-image-2.pub"}
        self.registry(fx)
        listed = self.dir / "all.txt"
        listed.write_text(f"{REG}/azoth {NVR} {KERNEL}\n{REG}/azoth-devel {NVR} {DEVEL}\n")
        r = self.run_script("verify-key", str(listed))
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"signed with a project key: {REG}/azoth:{NVR}@{KERNEL}", r.stdout)
        self.assertIn("azoth-devel", r.stderr)
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `python3 -m unittest discover -s system/tests -p test_kernel_artifacts.py -k ProjectKey -v`
Expected: FAIL. `signed REF key` exits 2 (usage), and `release-digests` is unknown.

- [ ] **Step 4: Implement**

In the header of `system/kernel-artifacts.sh`, after the `signed REF kernel|modules|signer` entry:

```
#   signed REF key                      signed or unsigned with a project key, any
#                                       athanor-image-*.pub of $KERNEL_KEYS_DIR (ADR-0096)
#   release-digests ALL UNSIGNED        after require-ready: write every kernel artefact a release
#                                       consumes ("REPOSITORY TAG DIGEST", image-digests.sh) to
#                                       ALL, and those without a project key signature to UNSIGNED;
#                                       the guest kernel, debuginfo and signer, which the state
#                                       file does not record, must carry their keyless build record
#   verify-key FILE                     exit 1 unless every artefact of FILE is signed with a
#                                       project key
```

After the `UNVERIFIED=` line:

```bash
KEYS_DIR=${KERNEL_KEYS_DIR:-$ROOT/system/keys}
# cosign v3 reports a kernel artefact without a signature by the given key with these messages
# (observed 2026-10-10). Any other failure is an error.
KEY_UNVERIFIED='no signatures found|no matching signatures: invalid signature when validating ASN\.1 encoded signature$'
```

Replace the first line of `probe_signed`'s body (`local status=0 regex`) so the function begins:

```bash
probe_signed() {
  [[ $2 != key ]] || { probe_key_signed "$1"; return; }
  local status=0 regex
```

Add before `probe_predicates`:

```bash
probe_key_signed() { # probe_key_signed REF: signed when any project key verifies REF
  local key status keys=("$KEYS_DIR"/athanor-image-*.pub)
  [[ -f ${keys[0]} ]] || die "no athanor-image-*.pub under $KEYS_DIR"
  for key in "${keys[@]}"; do
    status=0
    # skopeo's sigstore attachment is the classic format and has no Rekor entry: the key is the
    # trust root (UT2), so the transparency log is not consulted.
    cosign verify --key "$key" --new-bundle-format=false --insecure-ignore-tlog=true "$1" > /dev/null 2> "$TMP/err" || status=$?
    if [[ $status -eq 0 ]]; then
      echo signed
      return 0
    fi
    grep -qE "$KEY_UNVERIFIED" "$TMP/err" || { cat "$TMP/err" >&2; return 1; }
  done
  echo unsigned
}
```

Add after `check_plan`:

```bash
release_digests() { # release_digests ALL UNSIGNED
  [[ $# -eq 2 ]] || usage
  [[ $(get state) == ready ]] || die "release-digests needs state=ready: run require-ready first"
  local nvr ref repo tag digest verdict line
  nvr=$(get nvr)
  local -a lines=(
    "$REGISTRY/azoth $nvr $(get kernel_digest)"
    "$REGISTRY/azoth-devel $nvr $(get devel_digest)"
    "$REGISTRY/azoth-boot $(get boot_tag) $(get boot_digest)"
    "$REGISTRY/azoth-nvidia $(get nvidia_open_tag) $(get nvidia_open_digest)"
    "$REGISTRY/azoth-nvidia $(get nvidia_legacy_tag) $(get nvidia_legacy_digest)"
  )
  # Not in the state file: resolved by tag and held to their keyless build record, as every
  # check before the release signing step is (ADR-0096 item 2).
  for ref in "azoth:$nvr-microvm" "azoth-debuginfo:$nvr"; do
    digest=$(ask digest "$REGISTRY/$ref")
    [[ -n $digest ]] || die "$REGISTRY/$ref is not published"
    verdict=$(ask signed "$REGISTRY/${ref%%:*}@$digest" kernel)
    [[ $verdict == signed ]] || die "$REGISTRY/$ref is not signed by Kernel Build"
    lines+=("$REGISTRY/${ref%%:*} ${ref#*:} $digest")
  done
  digest=$(< "$ROOT/forge/specs/azoth/signer/image.digest")
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || die "forge/specs/azoth/signer/image.digest is not a digest: '$digest'"
  verdict=$(ask signed "$REGISTRY/azoth-signer@$digest" signer)
  [[ $verdict == signed ]] || die "$REGISTRY/azoth-signer@$digest is not signed by azoth-signer.yml"
  lines+=("$REGISTRY/azoth-signer image.digest $digest")
  printf '%s\n' "${lines[@]}" > "$1.tmp"
  : > "$2.tmp"
  for line in "${lines[@]}"; do
    read -r repo tag digest <<< "$line"
    verdict=$(ask signed "$repo@$digest" key)
    # Signing a signed digest again appends a signature to its .sig manifest on every run.
    [[ $verdict == signed ]] || echo "$line" >> "$2.tmp"
  done
  mv "$1.tmp" "$1"
  mv "$2.tmp" "$2"
}

verify_key() { # verify_key FILE
  [[ $# -eq 1 && -s $1 ]] || usage
  local -a lines
  local line repo tag digest verdict failed=0
  mapfile -t lines < "$1"
  for line in "${lines[@]}"; do
    read -r repo tag digest <<< "$line"
    verdict=$(ask signed "$repo@$digest" key)
    if [[ $verdict == signed ]]; then
      echo "signed with a project key: $repo:$tag@$digest"
    else
      echo "kernel-artifacts: $repo:$tag@$digest carries no project key signature" >&2
      failed=1
    fi
  done
  return "$failed"
}
```

In the dispatch, after `check-plan) check_plan "$@" ;;`:

```bash
  release-digests) release_digests "$@" ;;
  verify-key) verify_key "$@" ;;
```

`signed) [[ $# -eq 2 ]] || usage; ask signed "$1" "$2" ;;` already passes `key` through `probe signed REF key`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python3 -m unittest discover -s system/tests -p test_kernel_artifacts.py -v`
Expected: every test PASS, `ProjectKey` included.

- [ ] **Step 6: Run the whole system suite and the linters**

Run: `python3 -m unittest discover -s system/tests` and `shellcheck system/kernel-artifacts.sh`
Expected: OK, and no shellcheck finding.

- [ ] **Step 7: Commit**

```bash
git add system/kernel-artifacts.sh system/tests/fake_registry.py system/tests/test_kernel_artifacts.py
git commit -m "feat(kernel): list the kernel artefacts a release signs with the project key (ADR-0096)"
```

### Task 2: Signing the kernel list in `sign-images.sh` [stop-and-ask: diff before push]

**Files:**
- Modify: `system/image-digests.sh` (new `--check-kernel FILE` mode)
- Modify: `system/sign-images.sh` (optional `--kernel-digests FILE`)
- Test: `system/tests/test_sign_images.py`

**Interfaces:**
- Consumes: the UNSIGNED list of Task 1. It may be empty when every artefact is already signed.
- Produces: `image-digests.sh --registry R --check-kernel FILE` exits 0 for an empty file or for lines naming only `R/{azoth,azoth-devel,azoth-debuginfo,azoth-boot,azoth-nvidia,azoth-signer}` by digest, each `repo@digest` once. Otherwise it exits 2 before anything is signed.
- Produces: `sign-images.sh --registry R [--kernel-digests FILE] DIGESTS_FILE`, which signs the system images, then each kernel line, printing `signed: repo@digest`.

- [ ] **Step 1: Write the failing tests**

In `system/tests/test_sign_images.py`, add after `NAMES`:

```python
KERNEL_LINES = [
    f"{REG}/azoth 7.2.9-100.azoth.fc43 sha256:{'a' * 64}",
    f"{REG}/azoth 7.2.9-100.azoth.fc43-microvm sha256:{'b' * 64}",
    f"{REG}/azoth-signer image.digest sha256:{'c' * 64}",
]
```

Add to `SignImages`:

```python
    def kernel_file(self, lines):
        path = self.dir / "artifacts" / "kernel-unsigned.txt"
        path.write_text("".join(f"{line}\n" for line in lines))
        tags = json.loads((self.state / "tags.json").read_text())
        for line in lines:
            repo, tag, digest = line.split()
            tags[f"{repo}:{tag}"] = digest
        (self.state / "tags.json").write_text(json.dumps(tags))
        return path

    def sign_kernel(self, path, **env):
        return subprocess.run(
            ["bash", str(SIGN), "--registry", REG, "--kernel-digests", str(path), str(self.file)],
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def test_the_kernel_artefacts_are_signed_after_the_images(self):
        self.digests()
        r = self.sign_kernel(self.kernel_file(KERNEL_LINES))
        self.assertEqual(r.returncode, 0, r.stderr)
        signed = json.loads((self.state / "signed.json").read_text())
        self.assertEqual(signed, list(self.tags.values()) + [line.split()[2] for line in KERNEL_LINES])
        self.assertEqual(r.stdout.count("signed: "), 6)

    def test_an_empty_kernel_list_signs_the_images_alone(self):
        self.digests()
        r = self.sign_kernel(self.kernel_file([]))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads((self.state / "signed.json").read_text()), list(self.tags.values()))

    def test_a_kernel_line_outside_the_kernel_set_is_refused_before_signing(self):
        for line in (
            f"{REG}/athanor-system 412 sha256:{'d' * 64}",
            f"ghcr.io/elsewhere/azoth 7.2.9 sha256:{'d' * 64}",
            f"{REG}/azoth 7.2.9 latest",
            KERNEL_LINES[0],
        ):
            with self.subTest(line=line):
                self.digests()
                lines = [line] if line != KERNEL_LINES[0] else [line, line]
                path = self.dir / "artifacts" / "kernel-unsigned.txt"
                path.write_text("".join(f"{x}\n" for x in lines))
                (self.state / "calls.log").unlink(missing_ok=True)
                r = self.sign_kernel(path)
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertFalse((self.state / "calls.log").exists())
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `python3 -m unittest discover -s system/tests -p test_sign_images.py -k kernel -v`
Expected: FAIL. `sign-images.sh` prints its usage and exits 2 on `--kernel-digests`.

- [ ] **Step 3: Implement `--check-kernel` in `system/image-digests.sh`**

Add to the header, after the `--check` paragraph:

```
# With --check-kernel it reads the list of kernel artefacts the signing job signs with the
# project key (system/kernel-artifacts.sh release-digests, ADR-0096): every line names a kernel
# repository under the registry the caller gives, by digest, each repo@digest once. The list
# may be empty: every artefact was already signed.
#        image-digests.sh --registry REGISTRY/OWNER --check-kernel FILE
```

Parse the option: `check_kernel=''` in the variable line, and `--check-kernel) check_kernel=$2 ;;` in the `case`. Add `--check-kernel FILE` to `usage`. Then, before `if [[ -n $check ]]; then`:

```bash
kernel=(azoth azoth-devel azoth-debuginfo azoth-boot azoth-nvidia azoth-signer)
if [[ -n $check_kernel ]]; then
  [[ -n $registry && -z $tag && -z $out && -z $variants && -z $check ]] || usage
  [[ -f $check_kernel ]] || { echo "${0##*/}: $check_kernel is missing" >&2; exit 2; }
  declare -A seen=()
  while read -r repository tag digest; do
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $tag ]] || { echo "${0##*/}: malformed line in $check_kernel: '$repository $tag $digest'" >&2; exit 2; }
    [[ ${repository%/*} == "$registry" && " ${kernel[*]} " == *" ${repository##*/} "* ]] ||
      { echo "${0##*/}: $check_kernel names $repository, not a kernel repository under $registry" >&2; exit 2; }
    [[ -z ${seen[$repository@$digest]:-} ]] || { echo "${0##*/}: $check_kernel names $repository@$digest twice" >&2; exit 2; }
    seen[$repository@$digest]=1
  done < "$check_kernel"
  exit 0
fi
```

- [ ] **Step 4: Implement `--kernel-digests` in `system/sign-images.sh`**

Header: add the kernel artefacts to the first paragraph ("…of the published system images and, with --kernel-digests, of the kernel artefacts a release consumes (ADR-0096)…"), and change the usage line to `sign-images.sh --registry REGISTRY/OWNER [--kernel-digests FILE] DIGESTS_FILE`.

Replace the argument check with:

```bash
kernel=''
if [[ $# -eq 5 && $3 == --kernel-digests ]]; then
    kernel=$4
    set -- "$1" "$2" "$5"
fi
[[ $# -eq 3 && $1 == --registry && -n $2 && -s $3 ]] || {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER [--kernel-digests FILE] DIGESTS_FILE" >&2
    exit 2
}
```

After `bash "$root/system/image-digests.sh" --registry "$registry" --check "$digests"`:

```bash
[[ -z $kernel ]] || bash "$root/system/image-digests.sh" --registry "$registry" --check-kernel "$kernel"
```

After `render-policy`:

```bash
if [[ -n $kernel ]]; then
    # Machines never pull the kernel artefacts, so the shipped registries.d does not name them:
    # this file, written only here, lets skopeo write their sigstore attachments. The repository
    # list is its own command so that a failure of cut or sort fails the job (pipefail).
    cut -d' ' -f1 "$kernel" | sort -u > "$work/kernel-repositories"
    {
        echo docker:
        while read -r repository; do
            printf '  %s:\n    use-sigstore-attachments: true\n' "$repository"
        done < "$work/kernel-repositories"
    } > "$work/policy/registries.d/athanor-kernel.yaml"
fi
```

Move the signing loop into a function and call it for both files:

```bash
sign_file() {
    while read -r repository _ digest; do
        # skopeo writes the sigstore attachment only where registries.d enables it: the rendered
        # one does, for exactly these repositories, and the runner's default does not. A copy onto
        # a digest reference fails unless the manifest still has that digest.
        bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --preserve-digests --sign-by-sigstore-private-key "$work/key" --sign-passphrase-file "$work/passphrase" \
            "docker://$repository@$digest" "docker://$repository@$digest"
        echo "signed: $repository@$digest"
    done < "$1"
}
sign_file "$digests"
[[ -z $kernel ]] || sign_file "$kernel"
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python3 -m unittest discover -s system/tests -p test_sign_images.py -v`
Expected: every test PASS.

- [ ] **Step 6: Lint, full suite, show the diff**

Run: `shellcheck system/sign-images.sh system/image-digests.sh`, `shfmt -d system/sign-images.sh`, `python3 -m unittest discover -s system/tests`.
Then show the maintainer `git diff origin/iso-v0 -- system/sign-images.sh system/image-digests.sh` in chat before the push.

- [ ] **Step 7: Commit**

```bash
git add system/image-digests.sh system/sign-images.sh system/tests/test_sign_images.py
git commit -m "feat(signing): sign the kernel artefacts of a release with the project key (ADR-0096)"
```

### Task 3: Orchestrator wiring [stop-and-ask: diff before push]

**Files:**
- Modify: `.github/workflows/athanor-forge-orchestrator.yml` (jobs `kernel-artifacts-final`, `sign-system-images`, `verify-system-images`)
- Test: `python3 scripts/verify.py workflows`, `actionlint`

**Interfaces:**
- Consumes: `release-digests` and `verify-key` (Task 1), and `sign-images.sh --kernel-digests` (Task 2).
- Produces: the artifact `kernel-digests`, with `kernel-digests.txt` (every artefact) and `kernel-unsigned.txt` (to sign).

- [ ] **Step 1: `kernel-artifacts-final` writes and uploads the lists**

After its `Require ready` step:

```yaml
      - name: Kernel artefacts the release signs with the project key (ADR-0096)
        run: bash system/kernel-artifacts.sh release-digests kernel-artifacts/kernel-digests.txt kernel-artifacts/kernel-unsigned.txt
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-digests
          path: |
            kernel-artifacts/kernel-digests.txt
            kernel-artifacts/kernel-unsigned.txt
          if-no-files-found: error
```

- [ ] **Step 2: `sign-system-images` signs the unsigned list**

Add a download step after `📎 Digests recorded by the build job`:

```yaml
      - name: 📎 Kernel artefacts without the project key signature
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-digests
          path: kernel
```

Change the signing command to:

```yaml
        run: bash system/sign-images.sh --registry "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}" --kernel-digests kernel/kernel-unsigned.txt artifacts/image-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"
```

Extend the job comment: "It also signs the kernel artefacts that carry no project key signature yet (ADR-0096); the list comes from a keyless job and is checked by image-digests.sh --check-kernel."

- [ ] **Step 3: `verify-system-images` checks every kernel artefact**

Add `- uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6` after its checkout, the same download step as in Step 2, and, after the image verification:

```yaml
      - name: 🔎 Kernel artefacts carry the project key signature (ADR-0096)
        run: bash system/kernel-artifacts.sh verify-key kernel/kernel-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"
```

Add to the job comment: "It also verifies every kernel artefact of the release with cosign verify --key, the command KERNEL.md gives a verifier outside GitHub."

- [ ] **Step 4: Check the workflow**

Run: `actionlint .github/workflows/athanor-forge-orchestrator.yml` and `python3 scripts/verify.py workflows`
Expected: both clean. D43 still holds: the signing job runs only skopeo.

- [ ] **Step 5: Show the diff and commit**

Show `git diff origin/iso-v0 -- .github/workflows/athanor-forge-orchestrator.yml` in chat, then:

```bash
git add .github/workflows/athanor-forge-orchestrator.yml
git commit -m "ci(orchestrator): sign and verify the kernel artefacts with the project key (ADR-0096)"
```

### Task 4: Documents and issue #141

**Files:**
- Modify: `docs/architecture/doc_update_trust.md` (UT2), `docs/architecture/doc_pipeline.md` (PL17), `docs/architecture/doc_ci.md` (line 72), `forge/specs/azoth/KERNEL.md` (verification block)

Edit with an exact-replace Python script, then `git diff --numstat` (repository rule for these files).

- [ ] **Step 1: UT2.** Replace "Kernel images keep it alone too until the release signing step of ADR-0096 lands, and from then on are signed with the project key when released." with "The kernel artefacts of a release (ADR-0096, ADR-0098 item 3) get the same key-based signature in the same job, from a list a keyless job writes (`system/kernel-artifacts.sh release-digests`), and `verify-system-images` checks them with `cosign verify --key`; their keyless signature stays as the build record." Replace "Signed by digest: the three system images." with "Signed by digest: the three system images and the kernel artefacts of the release."

- [ ] **Step 2: PL17.** Append: "The kernel artefacts follow the same rule from the release signing step on (ADR-0096); checks before that step keep verifying the keyless record."

- [ ] **Step 3: doc_ci.md line 72.** Replace "This is the decided target and is not landed." with "`sign-system-images` signs them, from the list `kernel-artifacts-final` writes, and `verify-system-images` verifies them."

- [ ] **Step 4: KERNEL.md.** Replace the `cosign verify --certificate-identity …` line of the verification block with the key-based command, and keep the `gh attestation verify` line as the build record:

```sh
cosign verify --key system/keys/athanor-image-2.pub --new-bundle-format=false --insecure-ignore-tlog=true \
  "ghcr.io/ars-regia/azoth:$(bash nvr.sh)"
```

Add one sentence after the block, in the file's language: the key signature is added by the release signing step, so a kernel published since the last Orchestrator run carries only its keyless build record. Use key 1 while the rotation of `secrets.md` section 4.1 is open.

- [ ] **Step 5: Check and commit**

Run: `git diff --numstat` (only these four files, no reformatting) and `python3 scripts/verify.py docs`.

```bash
git add docs/architecture/doc_update_trust.md docs/architecture/doc_pipeline.md docs/architecture/doc_ci.md forge/specs/azoth/KERNEL.md
git commit -m "docs(trust): kernel artefacts carry the project key signature (ADR-0096)"
```

The pull request body says "Closes #141" only after the first Orchestrator run on `iso-v0` shows `signed with a project key:` for all eight artefacts in `verify-system-images`.
