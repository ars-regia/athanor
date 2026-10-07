# Derived images

| Field | Value |
| --- | --- |
| Purpose | How to build, sign and follow an image of your own `FROM` an Athanor image, and what it gives up |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-07 (ADR-0083) |
| Depends on | `doc_update_trust.md` UT3 to UT5 (policy, migration, verification), `doc_system_image.md` S2 (the three images) |

Athanor ships one desktop and supports only its own images ([ADR-0083](../decisions/0083-one-desktop-derived-images-open.md)).
Anyone may still build an image on top of one, with other packages, another desktop or
other defaults, and point their machines at it. This page says how, using the same
mechanisms the project's own pipeline and acceptance tests use.

## 1. What a derived image gives up

- **The guarantees of the shell.** The greeter, the lock screen, the SystemPrompter, the
  trusted path and the confined launches are parts of the Athanor shell
  (`doc_shell.md`, `doc_lock_and_prompts.md`, `doc_session_daemons.md`). Another desktop
  has none of them. Replacing the desktop also means replacing the display manager: greetd
  starts the Athanor greeter (`/usr/share/athanor-system-config/greetd.toml`).
- **Support.** Report a problem to the project only when it reproduces on an unmodified
  Athanor image.
- **The project's signature.** The machine trusts what you sign with your key. Keep the
  private key offline; whoever holds it can install any image on your machines as root
  (UT3).
- **Kernel modules signed by the project.** With Secure Boot on, a module the image adds
  loads only when it is signed by a key the machine trusts, which for your own modules
  means a key you enrol as a MOK on each machine.

What it keeps: Athanor's kernel, the hardening and the update service, as long as the
image does not replace them.

## 2. Do not layer packages instead

`rpm-ostree install` on a running machine is not a lighter derived image. bootc then calls
the deployment incompatible and refuses `upgrade` and `switch`, and the update service
reports `local-changes` and downloads nothing (UT3). Applications belong in Flatpak or Nix
(A2-16); changes to the system belong in a derived image.

## 3. Build

Name the repository after the Athanor image you start from: `REGISTRY/OWNER/athanor-system`,
`athanor-system-nvidia` or `athanor-system-nvidia-legacy`. The signature policy pins those
three names under one owner, and any other name is never verified.

Make a key pair once, on a machine you trust:

```bash
skopeo generate-sigstore-key --output-prefix my-image --passphrase-file passphrase
```

Keep `my-image.private` and `passphrase` offline, and put `my-image.pub` next to the
Containerfile under `keys/`:

```dockerfile
FROM ghcr.io/ars-regia/athanor-system:stable
ARG REGISTRY
ARG CREATED

# Your changes.
RUN dnf5 install -y --setopt=install_weak_deps=False <packages> && dnf5 clean all

# Trust your key for your repositories: render the policy for your owner, as the Athanor
# image build does for its own.
COPY keys/ /usr/share/athanor/keys/
RUN /usr/libexec/athanor-update/render-policy --registry "${REGISTRY}" \
        --keys-dir /usr/share/athanor/keys --out /usr/share/athanor/containers --link-etc /etc && \
    bootc container lint

# The update service offers only an image built after the booted one, by this label.
LABEL org.opencontainers.image.created="${CREATED}"
```

```bash
podman build --build-arg REGISTRY=ghcr.io/you \
    --build-arg CREATED="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    -t ghcr.io/you/athanor-system:stable .
```

`CREATED` is not optional. The image inherits the label of its base, and an image whose
build time is not strictly newer than the booted one's is never offered (UT5): without it,
your machines would stop updating after the first switch.

The project keys stay in `/usr/share/athanor/keys` and harm nothing: every signature names
its repository, and the policy refuses a signature made for another one
(`matchRepository`).

## 4. Sign and publish

The machine reads only the classic signature attachment. Sign with skopeo, not with
cosign 3, which writes a format the machine treats as no signature (UT3). skopeo writes the
attachment only for a repository a `registries.d` file names, for example
`registries.d/you.yaml`:

```yaml
docker:
  ghcr.io/you/athanor-system:
    use-sigstore-attachments: true
```

Push under a tag no machine follows, sign that digest, then move the followed tag onto it.
A signature belongs to the digest, so it is already in place when the tag moves; a machine
that checked an unsigned digest would report the update as refused (UT3).

```bash
podman tag ghcr.io/you/athanor-system:stable ghcr.io/you/athanor-system:candidate
podman push ghcr.io/you/athanor-system:candidate
skopeo --registries.d registries.d copy --preserve-digests \
    --sign-by-sigstore-private-key my-image.private --sign-passphrase-file passphrase \
    docker://ghcr.io/you/athanor-system:candidate docker://ghcr.io/you/athanor-system:candidate
skopeo copy --preserve-digests \
    docker://ghcr.io/you/athanor-system:candidate docker://ghcr.io/you/athanor-system:stable
```

## 5. Switch a machine

The booted Athanor image does not know your key, so a plain `bootc switch` would install
your image unverified and record an unverified origin that every later upgrade inherits.
`scripts/switch-verified.sh` verifies the first switch against a key directory you give it,
without changing `/etc`:

```bash
sudo bash scripts/switch-verified.sh ghcr.io/you/athanor-system:stable /path/to/keys
```

`/path/to/keys` holds `my-image.pub`. Reboot afterwards. From then on the machine runs your
image's policy: the update service checks your repository, verifies your signature, and
reports the machine as verified. It follows the tag you switched to.

To go back, run the same script with the Athanor image and no key directory.

## 6. Checked so far

- The update acceptance on the dev VM (`scripts/devvm/acceptance`) builds a derived image
  in this way, signs it with a throwaway key and checks that the machine verifies and
  updates it.
- `render-policy` run with another owner inside an Athanor image renders the three scopes
  for that owner (2026-10-07).
- A desktop swapped in a derived image has not been tried.
