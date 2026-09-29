# Updates and trust state: the system side

Status: **consented to by the maintainer on 2026-09-19 at revision 2; this is revision 3,** with decisions D1, D2 and D3 of section 4 taken as recommended. Spike U1 of section 3 ran on the dev VM the same day (`.superpowers/spike-u1-update-trust.md`); revision 3 takes its twelve corrections, four of which change what the code does. D1, D2 and D3 are unaffected. The `auditor` found revision 1 consent-ready with changes: no critical finding, six high and ten medium, all text (`.superpowers/update-trust-security-review.md`). This revision takes them. It changes the signing pipeline, adds a secret and adds root code, so nothing here is built before the maintainer says yes. `doc_shell.md` (SH11, SH12) binds this document with nine constraints; section 2 answers each. Section 3 lists what must be proven on the dev VM before the plan is written, and section 4 the decisions that are the maintainer's.

It is the interim implementation of the Athanor update service of `doc_kernel_profile.md`, section 8 (D31, D36): bootc today, `systemd-sysupdate` with A/B `/usr` later, behind the same interface.

## 1. Context

What ships today, checked on the maintainer's desktop (image 0bd565cd) and in the repository:

- **No update ever runs by itself.** `80-athanor-system.preset` enables `bootc-fetch-apply.timer`, which does not exist. The override of `bootc-fetch-apply-updates.service` in `athanor-base-config` calls `bootc upgrade --stage`, a flag bootc 1.16 does not have.
- **Nothing is verified on the client.** `/etc/containers/policy.json`, owned by `containers-common`, is `insecureAcceptAnything`. The booted reference is `ostree-unverified-registry:ghcr.io/hr-mes/athanor-system:latest`. Athanor ships nothing under `/etc/containers`.
- **The signature cannot be matched.** `forge/scripts/sign_attest.sh` signs keyless (`cosign sign --yes`). CI verifies it with `--certificate-identity-regexp` and the GitHub issuer. `containers-policy.json` matches a Fulcio certificate only by `subjectEmail`, which a GitHub Actions identity does not carry.
- **The version says nothing.** The build sets no version label, so the image inherits the base's: two builds a day apart are both `43.20260916.0`.
- **A machine installed from the ISO boots a digest no signature covers.** The installer converts the image to OCI layout, which changes the manifest digest; the layers are the same. The first `bootc switch --enforce-container-sigpolicy` resolves the tag against the registry again and lands the machine on the signed digest without downloading a layer, which is what makes UT4 cheap.
- **`/run/athanor` has no owner.** The attestation key release creates it with default permissions and writes the released disk key into it. No `tmpfiles.d` entry declares it.
- **`athanor-secure-boot` cannot work.** It is a D-Bus service on `org.athanor.SecureBoot` with no bus policy file, so it cannot own its name; it reads one efivar; it is in `packages.json`.
- **Retention can remove signatures.** `forge/scripts/clean_ghcr.sh` keeps the two newest tagged versions per package, and a cosign signature is a tagged version of the same package.
- A nightly Orchestrator run without a source change publishes no new digest (`has_changes` stays false). Every merge that touches the image does.

What is available: `skopeo` 1.22 and `bootc` 1.16 are in the image; `bootc upgrade` has `--check`, `--download-only` and `--from-downloaded`; `bootc switch` has `--enforce-container-sigpolicy`; the workspace has `zbus` 5.18 on tokio, and `athanor_bus_api::polkit::check_polkit_auth_zbus(conn, sender, action_id, allow_user_interaction)`, which builds the subject from the bus sender.

## 2. Decisions

**UT1. One package, one binary, four units.** `forge/specs/athanor-update` ships the Rust binary `athanor-update` and:

| Unit | Runs | Does |
|---|---|---|
| `athanor-update-check.timer` | 15 minutes after boot, then every 6 hours, with a randomised delay | starts the check |
| `athanor-update-check.service` | `athanor-update check`, oneshot, root | re-derives the verification of the booted digest, checks the registry, downloads, publishes the state |
| `athanor-update-state.service` | `athanor-update check --offline`, oneshot at boot, root, `PrivateNetwork=yes` | the verification of the booted digest that UT5 requires before the network is up |
| `athanor-update.service` | `athanor-update serve`, root, D-Bus activated on `os.athanor.Update1` | the two requests of UT6, then publishes the state |

- Both services take one lock, `/run/athanor-update/lock`, so a request never races a download.
- The broken preset line and the override are deleted; the stock `bootc-fetch-apply-updates.timer` stays disabled by preset, because it applies and reboots.
- **Hardening is measured, and was.** Spike U1 ran each candidate against a real `bootc upgrade --download-only` and a real `--from-downloaded`. Both services ship with `NoNewPrivileges`, `ProtectHome`, `PrivateTmp`, `ProtectKernelTunables`, `ProtectKernelModules`, `ProtectControlGroups`, `ProtectProc=invisible`, `LockPersonality`, `MemoryDenyWriteExecute`, `UMask=0022`, `SystemCallFilter=@system-service @mount`, `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK`, `RestrictNamespaces=~user` and `CapabilityBoundingSet=~CAP_SYS_MODULE CAP_SYS_BOOT CAP_SYS_RAWIO CAP_NET_ADMIN`. Two candidates are dropped: `RestrictSUIDSGID` makes bootc fail on its runtime auth file, and an allow-list `CapabilityBoundingSet` of any size breaks the `setpriv` bootc uses to start its skopeo proxy, so capabilities are subtracted, never enumerated. `ProtectSystem=strict` cannot apply: bootc writes the sysroot.
- **Only the check unit reaches the network.** The serve unit, the one unprivileged callers talk to, runs with `PrivateNetwork=yes`, which `--from-downloaded` tolerates. Neither that nor `IPAddressDeny=any` may go on the check unit: both stop the download.

**UT2. Release images carry a key-based signature beside the keyless one.** *(constraint 1)*

- The maintainer generates the pair offline with `cosign generate-key-pair`. The private key and its password enter the `signing` environment as `COSIGN_PRIVATE_KEY` and `COSIGN_PASSWORD`, and a copy goes to the offline kit with the other project keys. The public key is committed as `system/keys/athanor-image-1.pub`.
- **The signature must be the classic cosign attachment:** an OCI manifest at `<repo>:sha256-<hex>.sig` whose layers are `application/vnd.dev.cosign.simplesigning.v1+json`. It is the only format containers/image reads. cosign 3, the one the pipeline gets from nixpkgs, writes a Sigstore bundle at `<repo>:sha256-<hex>` instead, which every Athanor machine treats as no signature at all, and CI would not notice, because CI verifies with cosign. The key-based step therefore uses `skopeo copy --sign-by-sigstore-private-key`, through `retry.sh`, and the job then verifies its own output the way a machine will: `skopeo copy --policy <the shipped policy>` of the published digest, not `cosign verify`. The job fails when the key is absent; tier and kernel images keep the keyless signature alone.
- The keyless signature and its Rekor entry stay: they are the public record of who built what. The key-based one is what a machine can check offline with a file on disk.
- Signed by digest: the three system images. The ISO is not an update source.
- **The key is alone in its job.** The key-based signature is made in a job of its own that does nothing else: it reads the three digests from a file the build job wrote, logs in, signs and ends. It is the only job that receives `COSIGN_*`. No image build, no SBOM tool and no other third-party binary runs beside the key, because an environment variable is readable by everything the job executes.
- **This key is the whole of client-side trust,** and there is no revocation. Whoever holds it can make any image these machines install as root. UT3's rotation is hygiene, not a response to compromise: an attacker holding key *n* can ship their own key *n+1*. A compromised key is recovered from out of band only, by a new ISO or by one documented command an administrator runs on each machine, and a machine that pulled an attacker's image before that is not recoverable remotely. If the key is ever known to be exposed, the project says so publicly and stops the pipeline. The recovery command is written and tested before the key signs its first image.

**UT3. The policy lives under `/usr` and is scoped to our images.** *(constraints 2, 3)*

- The image carries `/usr/share/athanor/containers/policy.json`, `/usr/share/athanor/containers/registries.d/athanor.yaml` with `use-sigstore-attachments: true`, and the public keys under `/usr/share/athanor/keys/`. They are rendered at image build by one script the RPM ships with its templates, because the registry is a build variable and a key rotation should be a file commit, not an RPM bump. The signing job and the acceptance harness call the same script.
- The container tools read only `/etc/containers`. The image build replaces `/etc/containers/policy.json` with a symbolic link to the file under `/usr` and links the `registries.d` entry; `verify.py shipped` checks both links.
- **`default` is `reject`,** because `bootc switch --enforce-container-sigpolicy` refuses to run against a policy whose top-level default is `insecureAcceptAnything`. Users lose nothing: `transports.docker[""]` and every other transport podman, buildah and skopeo use today (`docker-archive`, `oci`, `oci-archive`, `dir`, `containers-storage`, `docker-daemon`) are `insecureAcceptAnything`, which bootc does not inspect, and the acceptance proves `podman pull`, `podman load` and `podman build` still work. The three system image repositories override that with `sigstoreSigned`, a `keyPaths` list and `matchRepository`. The registry and owner come from the build's variables through a template; no literal `ghcr.io/hr-mes` enters the source.
- **Rotation:** key *n+1* ships in the `keyPaths` of an image signed with key *n*. Key *n* leaves one release after the first image signed with *n+1*. Two keys are therefore in force during a rotation, and the acceptance tests it.
- A local file that replaces the link shadows the policy. The check compares the policy in force with the shipped one by SHA-256 and publishes the result; the badge follows it (`doc_shell.md`, SH12).
- **What the policy does not do.** It constrains three repositories; every other registry name is accepted unsigned. `--enforce-container-sigpolicy` selects the host policy, it does not make an arbitrary reference trustworthy. An administrator with root can point the machine anywhere and is outside this threat model; the machine still refuses to call that state verified, with the reason `reference-out-of-scope`.
- Two `registries.d` files that declare the same scope are a hard error in containers/image, so ours declares only the three repositories.
- **A missing or mistyped `registries.d` entry does not report itself:** a correctly signed image then fails with the same "A signature was required, but no signature exists" as an unsigned one. `verify.py shipped` checks the file, and the pipeline's own verification of UT2 catches it before an image is published.

**UT4. Machines follow a signed reference.** *(constraint 4)*

- **One mechanism for new and existing installs:** `athanor-update-migrate.service`, a oneshot with a stamp file, runs `bootc switch --enforce-container-sigpolicy` to the `stable` tag once the policy is in force and the network is up. No kickstart changes. On the dev VM it took 85 seconds and downloaded nothing; the reference became `ostree-image-signed:docker://…`, and later upgrades kept enforcing. It also moves a machine installed from media off the installer's digest onto the one the signature covers. A failure leaves the old reference, and the badge stays at the exclamation mark with the reason `policy-not-in-force`.

**UT5. "Verified" is recomputed at every check from stored signature material.**

- After a download passes the policy, the check fetches the cosign signature object of that digest (`<repo>:sha256-<hex>.sig`, under 2 kB) with `skopeo copy` into `/var/lib/athanor-update/signatures/<digest>/`. That object is an attachment, not a signed image, and the strict policy would refuse it under its own repository scope. The fetch therefore names `--policy /usr/share/athanor/containers/attachments-policy.json`, whose default is `reject` with an `insecureAcceptAnything` scope for the three repositories, and which `verify.py shipped` checks; `--insecure-policy` is not used. What it fetches is trusted by nothing until the signature over it verifies.
- At every check, and at boot before the network is up, the binary verifies the stored payload with ECDSA P-256 (`p256` and `sha2`, real verification, no shortcut) against the keys named in the `keyPaths` of the shipped policy for that repository. The policy file is the single list of keys; the key directory is only where the files live. When the policy in force is not the shipped one, the reason is `policy-not-in-force` and no key is consulted. It then compares the payload's manifest digest with the booted one, and the repository of the payload's `critical.identity.docker-reference` with the repository the deployment follows; a mismatch is `no-signature`. The tag is not compared: cosign signs the tag it pushed and the machine follows another. The answer is never a stored boolean, and a key that left the image stops verifying by itself.
- The policy is what refuses a bad image; this check only drives the badge. When they disagree the badge is the exclamation mark.
- **Four ways to get the verification wrong, all excluded by test:** the signature object carries one layer per signature, in push order and with no key identifier in the payload, so the verifier tries every key against every layer; the signed bytes are the payload blob exactly as stored, checked against its descriptor's SHA-256, never a re-serialised JSON; the signature is base64 ASN.1 DER, read with `from_der`, with no hand-written parser; `critical.type` must be `cosign container image signature`, and a missing or unknown field is a refusal.
- **A moved tag is a downgrade the policy cannot see,** because a signature covers a digest and never a tag. The check therefore offers and downloads only a digest whose `org.opencontainers.image.created` is strictly newer than the booted deployment's. That label is inside the image configuration, so the signed digest covers it. An older digest is published as `older-than-booted`; it is not a badge state. bootc has no ordering of its own: with the tag pointed at an older image it says "Update available". The booted build time is `status.booted.image.timestamp` of `bootc status --format json`; the candidate's is the label from `skopeo inspect --config`, 54 kB and 70 ms. Neither `bootc upgrade --check`, which has no JSON output, nor `status.booted.cachedUpdate`, which was seen two tag moves stale, is used.
- **A machine installed from the ISO reads "not verified: installed from media"** until the migration of UT4 has run, which needs the network once and no update. Offline, it stays so.
- The statement is about the image the deployment refers to, not a measurement of `/usr` (`doc_shell.md`, SH12).

**UT6. Two requests, no arguments.** *(constraint 5)*

| | `Apply()` | `GoBack()` |
|---|---|---|
| Polkit action | `os.athanor.update.apply` | `os.athanor.update.rollback` |
| Defaults (any / inactive / active) | `auth_admin_keep` / `auth_admin_keep` / `yes`, which are logind's for a reboot on this image | `auth_admin` for all three, never kept |
| Does | refuses unless the state is `downloaded`; `bootc upgrade --from-downloaded`; `login1.Manager.Reboot(false)`; on a refusal, `ostree admin lock-finalization` | `bootc rollback`; records the digest it left as held; asks logind to reboot |

- Bus name `os.athanor.Update1`, owned by root through `/usr/share/dbus-1/system.d/os.athanor.Update1.conf`. The policy allows `send_destination` only together with `send_interface=os.athanor.Update1` and the `send_member` of each method, plus `Introspectable.Introspect`. No property is exported. The activation file names `SystemdService=` and `User=root` and nothing else. Polkit decides who may call.
- The subject is the bus sender, through `check_polkit_auth_zbus`, with user interaction allowed. Never a PID from the caller, never `unix_process`.
- The `.policy` file is installed by the spec, which `verify.py polkit` requires, and both actions are in the `os.athanor.*` namespace it checks.
- **`Apply()` never downloads.** With nothing downloaded it refuses, the timer downloads at its next run, and the shield's button says so. A root service does not open connections on an unprivileged caller's word.
- **The inhibitor check is logind's.** `Apply()` calls `Reboot(false)` and never `RebootWithFlags` with the skip-inhibitors flag. On systemd 258 a root caller is refused by a block inhibitor with `org.freedesktop.login1.BlockedByInhibitorLock`, which the helper reports as it is. `CanReboot` is not used: it answers polkit only and says "yes" with a block inhibitor held. When the reboot is refused after the unlock, the helper runs `ostree admin lock-finalization`, which restores the locked state; `will-apply-at-next-shutdown` therefore describes only the instant between the two calls, and a failure of the re-lock.
- **The held digest is needed and cannot come from bootc:** after a rollback, `bootc upgrade --check` says "No changes" while `--download-only` stages the digest the user just left.
- `GoBack()` targets the immediately previous deployment only and logs the caller's uid at notice.
- **Held digest:** `/var/lib/athanor-update/held` names the digest the user left. The check skips it and offers only a newer one. No request releases it.

**UT7. One state file, written atomically.** *(constraint 6)*

- `tmpfiles.d` declares `/run/athanor-update` and `/var/lib/athanor-update`, both `0755 root root`. `/run/athanor`, which holds the released disk key, is not used.
- `/run/athanor-update/state.json`, `0644 root root`, written to a temporary name and renamed. Readers open it with `O_NOFOLLOW` and check the owner.
- Schema 1: the booted, downloaded and previous deployments (image, digest, version, build time); `verified` with a reason from a closed list (`signature`, `media`, `no-signature`, `key-not-in-policy`, `policy-not-in-force`, `reference-out-of-scope`); the update state (`none`, `available`, `downloaded`, `will-apply-at-next-shutdown`, `refused`, `held`, `older-than-booted`); the policy in force (path, SHA-256, whether it is the shipped one); the four Secure Boot readings of UT8; the newest build time this machine has booted and the last successful check, both persisted under `/var/lib` so they survive a reboot; the last check's result as an error **code** (`none`, `network`, `registry`, `policy`, `storage`, `internal`) and at most a host name. No raw error text and no URL ever enters the file.
- **World-readable is a deliberate trade.** The shield reads the file without a privileged hop, and in exchange every local process learns the machine's patch level and that the user went back. There is no unprivileged alternative: `bootc status` refuses a non-root caller, and `rpm-ostree status --json` has no field for the finalization lock. Nothing else goes in: no user identity, no host name beyond the registry's, no path outside the two directories.
- A small crate, `athanor-trust-state`, parses the file and computes the badge of SH12. The greeter, the applet and the notifier use it, so the rule exists once.

**UT8. Secure Boot is four readings, published separately.** *(constraint 7)* `SecureBoot`, `SetupMode` and `MokSBStateRT` from efivarfs, and `/sys/kernel/security/lockdown`. "On" means 1, 0, absent, and not `none`. The check reads them itself. They never move the badge (SH12).

**UT9. Every published image has its own version.** *(constraint 9)* `system/build-image.sh` sets `org.opencontainers.image.version` to `<base major>.<UTC build date>.<serial>` and `org.opencontainers.image.created`. The serial comes from the caller, the CI run number in the pipeline and `0` in a local build. Ordering uses the build time, never the version string.

**UT10. Retention keeps what a machine can still use.** *(constraint 8)* `clean_ghcr.sh` never deletes a system image digest that is tagged `stable` or `latest`, was tagged so in the last 90 days, or is the previous `stable`; nor any `sha256-<hex>.sig`, `sha256-<hex>` or `.att` tag whose digest it keeps: cosign 3 and containers/image write signatures of one digest to two different tags. It treats signature tags as referrers of their image, as `forge/specs/azoth/retention.sh` already does: a signature is never deleted while its digest is kept. Ninety days is an interim number; the document that sets how long an installed version is supported replaces it.

**UT11. The notifier.** `athanor-update-notify`, a user service in the same package: it watches the state file, sends one notification per downloaded digest per user through `org.freedesktop.Notifications` with the actions "Restart to update" and "Later", one after the first boot into a new deployment, and, while `athanor-bar.service`, which carries the session shield, is not enabled for the user, offers a pending digest again once per session start (`doc_bar.md`, BR8). It calls `Apply()` and `GoBack()` and nothing else. It accepts `ActionInvoked` only from the unique name that owns `org.freedesktop.Notifications` and only for an id it holds, because any session process can emit that signal, and a forged one would summon the administrator prompt of `GoBack()` out of nowhere. It restricts itself with Landlock at start as the greeter does (`ensure_single_threaded`, then the ruleset): read access to the state file and its own libraries, and write access to one directory, `$XDG_STATE_HOME/athanor-update-notify`, where it records per user which digests it has announced.

**UT12. Metered connections.** The check always asks the registry for the digest and build time with `skopeo inspect --config` (UT5), which costs a manifest. It downloads unless NetworkManager's `Metered` is 1 or 3; this desktop reports 4, guessed unmetered, and downloads.

## 3. What spike U1 had to prove (done 2026-09-19; item 6 reasoned from the files, not executed)

1. containers/image accepts an image when any one signature validates (`policy_eval_sigstore.go`), so the keyless signature beside ours is harmless; the spike confirms it once, and that without the key-based signature, or with another key, `bootc upgrade` refuses in a way the binary can map to `refused`.
2. `bootc switch --enforce-container-sigpolicy` to the image name the machine already follows is accepted, or what the working form of the migration is.
3. `skopeo copy --policy <attachments policy> docker://<repo>:sha256-<hex>.sig dir:` fetches the signature object under the strict host policy, and a short test asserts the four points of UT5 on it.
4. `--download-only`: what `bootc status --format json` shows for a locked deployment, that a shutdown leaves the booted version, that `--from-downloaded` works with the network down, and what survives a reboot.
5. After `bootc rollback`, what the next `--check` reports, so the held-digest rule is implemented against real output.
6. How the kickstart leaves a fresh install on the signed reference.
7. Which hardening directives of UT1 survive a real download and a real apply.
8. Whether logind lets a root caller past a block inhibitor, whether bootc can lock a deployment again, and what the state is when the reboot call fails after the unlock.

## 4. Decisions that are the maintainer's (all three taken on 2026-09-19)

- **D1. A `stable` tag.** Users follow `:stable`; `:latest` stays for testing. A manual workflow runs `system/promote.sh <run id>`, which points `stable` at an already signed digest with `skopeo copy`; the signature is by digest, so it carries. Without it every merge that touches the image asks every user to restart. It also gives UT4's migration a new image name to switch to. A moved tag can only move machines forward, because of the build-time rule of UT5. **Recommended.** `doc_kernel_profile.md` leaves channels to release 1.1; this brings one hand-promoted channel forward.
- **D2. `athanor-secure-boot` leaves the image.** It cannot own its bus name, so it has never answered a call, and UT8 replaces its one reading. **Recommended.** Nothing in the repository calls that name. Retiring means the binary, its unit and its bus name leave; the TPM sealing script and unit, the rollback check and the `systemd-pcrphase-sysinit` drop-in stay, and so does the `SOURCES` tree, which `system/Containerfile` reads directly. Which of those the image enables today is checked before the change. The alternative is a bus policy file and a review of what the service claims to attest.
- **D3. Who generates and holds the cosign key:** the maintainer, offline, as with the Secure Boot and module keys. The agent never sees the private key.

Found on the way and out of scope: `athanor-backup`, which ships, guards its methods with `org.athanor.backup.*` actions that no `.policy` file declares, so polkit denies every call, and `verify.py polkit` does not look at the `org.athanor.*` namespace.

## 5. Changes to other documents

- `doc_shell.md`, SH12, "Verified means", and acceptance item 8: a deployment installed from the ISO reads "not verified: installed from media" until the migration of UT4 has run.
- `doc_shell.md`, constraint 7: the sentence about `athanor-secure-boot` points at D2.
- `doc_kernel_profile.md`, section 8: a pointer to this document as the interim update service.
- `doc_system_image.md`: the version label of UT9 and the key-based signature of UT2.

## 6. Acceptance

On the dev VM, with a throwaway key pair:

1. An image with both signatures is downloaded by the timer with no user action; the state reads `downloaded`; a poweroff boots the old version.
2. `Apply()` from the active session reboots into the new version with no password; from an SSH session it asks for administrator authentication. With a reboot inhibitor held, or with nothing downloaded, it refuses and nothing is unlocked. `busctl` cannot reach any other interface or member on the name.
3. After the reboot the state reads verified, with the reason `signature`; with `/etc/containers/policy.json` replaced by a permissive file it reads not verified, `policy-not-in-force`.
4. The same image without its key-based signature, and one signed with another key, are refused; the state reads `refused`, the error code `policy`, and no registry text appears in the file.
5. `GoBack()` asks for administrator authentication from every kind of session, boots the previous digest, and the next three checks do not download the held digest; a newer digest is offered.
6. An image that ships a second public key and is signed with the first is accepted, and so is the next one, signed with the second.
7. A fresh install from the ISO reads not verified, `media`, until the migration has run, and verified, `signature`, after it, with no update in between.
8. The migrated desktop follows the signed reference and keeps updating.
9. With the tag pointed at an older signed digest, the check publishes `older-than-booted` and downloads nothing. A deployment switched to a repository outside the policy reads `reference-out-of-scope`. A valid signature object copied from another of our repositories reads `no-signature`.
10. The recovery command of UT2 moves a machine to a new key with the old one removed.
11. `verify.py polkit`, `verify.py shipped` and the unit tests of `athanor-trust-state` pass; `systemd-analyze security` reports both services below an exposure the plan states.
12. An image whose only signature is a cosign 3 bundle is refused, with the state `refused` and the error code `policy`.
13. With the tag moved back to the digest the user left, three checks download nothing, although `bootc upgrade --download-only` run by hand would stage it.
14. Both bootc calls succeed under the units' own hardening, run as the units run them.
15. With the policy in force, `podman pull` from another registry, `podman load` and `podman build` work as before.
