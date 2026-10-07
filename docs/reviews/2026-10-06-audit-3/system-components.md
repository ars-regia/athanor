# System components: verdicts for system/ (iso-v0) and the Ermete-era system/ of main

Snapshot: 91aefb9f. Scope: every crate, directory and file under `system/` of `iso-v0`, plus the `system/` components that existed on `origin/main` (Ermete era) and are gone from iso-v0: `ermete-ai-daemon`, `ermete-compositor`, `ermete-shell-rs`, `ermete-updater-rs`, `portal`, `config`, `bcachefs-root.mount`, `ARCHITECTURE.md`. Evidence comes from reading the code, `forge/config/packages.json`, `forge/specs/*.spec`, `system/Containerfile`, `docs/architecture/components.toml`, `docs/decisions/`, `cargo metadata --offline --no-deps` and `python3 scripts/verify.py paths shipped panics polkit`. Nothing was built or run.

## Summary

- `system/` holds two different populations. The Athanor-era shell libraries (`apps`, `compositor-client`, `i18n`, `layout`, `portal`, `preview`, `preview-render`, `search`, `style`, `trust-state`, `unit`) and the image pipeline scripts are specified, shipped, tested and of good quality: KEEP.
- The Ermete-era platform crates (`greeter`, `hypervisor-daemon`, `confidential_computing`, `mesh-bus`, `cluster-mesh`, `store`, `agentic-kernel`, `ebpf`, `ebpf-sched`, `net-unikernel`, `telemetry`, `init-oracle`, `oobe`) are not shipped. They have no live specification, and six of them contain fake security mechanisms: hard-coded "trusted" verdicts, zero session keys, zero signatures, self-signed nonces, a placeholder image digest and an unauthenticated listener. None of them should be made real in its current form. The product needs these functions are covered by upstream projects the accepted decisions already name: greetd + systemd-homed/cryptenroll, Keylime, kernel WireGuard + a Headscale/NetBird-class coordinator (A2-12), scx_loader, and firewalld/nftables.
- For every component that exists in both eras, the iso-v0 version is better or equal. The one notable security improvement is the hypervisor's private polkit check: on main it trusted the bus daemon's uid 0, and iso-v0 moved it to the shared `athanor-bus-api` helper. The `system/` components that exist only on main are dead, or are template content, and none is worth bringing back.
- One crate needs to be made real rather than retired: `athanor-bus-api`. It is the polkit subject helper that shipped `athanor-update` depends on. It has no specification, carries unrelated mesh/telemetry/shared-memory code, and has a documented PID-reuse window. Its path is protected (CLAUDE.md), so this needs a maintainer decision.
- Two decisions must be amended before the retirements: doc_software decision 5 keeps `athanor-store` storage for the fleet, and components.toml calls `ebpf-sched` "out-of-1.0" while doc_kernel_profile D5 retires it. `confidential_computing` and `bus-api` are protected paths, so changes there need the maintainer.

## Verdict table

| Id     | Component                              | Era                      | Shipped                  | Spec / decision                             | Verdict                                       | Severity |
| ------ | -------------------------------------- | ------------------------ | ------------------------ | ------------------------------------------- | --------------------------------------------- | -------- |
| CSY-01 | system/athanor-greeter                 | both                     | no (EXEMPT)              | none; superseded by greeter-ui + greetd     | RETIRE                                        | critical |
| CSY-02 | system/athanor-hypervisor-daemon       | both                     | no (EXEMPT)              | out-of-1.0, issue 57                        | RETIRE                                        | critical |
| CSY-03 | system/confidential_computing          | both                     | no (not exempt)          | missing; A2-12, 0049                        | RETIRE (protected path)                       | critical |
| CSY-04 | system/athanor-mesh-bus                | both                     | no (EXEMPT)              | out-of-1.0, issue 57; A2-12                 | REPLACE-UPSTREAM                              | critical |
| CSY-05 | system/athanor-cluster-mesh            | both                     | no (EXEMPT)              | out-of-1.0, issue 57                        | RETIRE                                        | critical |
| CSY-06 | system/athanor-store                   | both                     | no (not exempt)          | doc_software decision 5                     | RETIRE                                        | critical |
| CSY-07 | system/athanor-bus-api                 | both                     | yes (via athanor-update) | missing                                     | MAKE-REAL                                     | high     |
| CSY-08 | system/athanor-ebpf-sched              | both                     | no (not exempt)          | D5 retired vs components.toml out-of-1.0    | REPLACE-UPSTREAM                              | high     |
| CSY-09 | system/athanor-agentic-kernel          | both                     | no (not exempt)          | missing                                     | RETIRE                                        | high     |
| CSY-10 | system/Justfile                        | both                     | n/a                      | missing                                     | MERGE-INTO root Justfile                      | high     |
| CSY-11 | system/athanor-install.ks              | iso-v0                   | n/a (manual ISO)         | doc_first_run FR21, doc_software SWb        | MERGE-INTO system/disk_config/iso.toml        | medium   |
| CSY-12 | system/ebpf                            | both                     | no (not exempt)          | missing                                     | REPLACE-UPSTREAM                              | medium   |
| CSY-13 | system/athanor-net-unikernel           | both                     | no (EXEMPT)              | missing                                     | RETIRE                                        | medium   |
| CSY-14 | system/athanor-telemetry               | both                     | no (EXEMPT)              | contradicts A2-19                           | RETIRE                                        | medium   |
| CSY-15 | system/athanor-init-oracle             | both                     | no (EXEMPT)              | missing                                     | RETIRE                                        | medium   |
| CSY-16 | system/athanor-style                   | both (iso-v0 far larger) | yes                      | doc_shell SH5, SH4                          | KEEP (drop pre-Calmo modules)                 | medium   |
| CSY-17 | system/scripts                         | both                     | assemble_uki.sh only     | doc_kernel_profile, doc_software decision 6 | KEEP assemble_uki.sh, RETIRE the rest         | medium   |
| CSY-18 | system/sysctl.d                        | both                     | no                       | missing; doc_kernel_profile P3              | MERGE-INTO forge/specs/athanor-kernel-profile | medium   |
| CSY-19 | system/athanor-oobe                    | both                     | no (outside workspace)   | doc_first_run decision 9 retires it         | RETIRE                                        | low      |
| CSY-20 | system/cosign.pub                      | both                     | no                       | missing                                     | RETIRE                                        | low      |
| CSY-21 | system/athanor-compositor-client       | iso-v0                   | yes                      | doc_shell SH2                               | KEEP                                          | low      |
| CSY-22 | system/athanor-apps                    | iso-v0                   | yes                      | doc_bar BR3                                 | KEEP                                          | low      |
| CSY-23 | system/athanor-i18n                    | iso-v0                   | yes                      | doc_languages LN2                           | KEEP                                          | low      |
| CSY-24 | system/athanor-layout                  | iso-v0                   | yes                      | doc_shell SH6                               | KEEP                                          | low      |
| CSY-25 | system/athanor-portal                  | iso-v0                   | yes                      | doc_portal PT2                              | KEEP                                          | low      |
| CSY-26 | system/athanor-preview                 | iso-v0                   | yes                      | doc_launcher LA6                            | KEEP                                          | low      |
| CSY-27 | system/athanor-preview-render          | iso-v0                   | yes                      | doc_launcher LA6                            | KEEP                                          | low      |
| CSY-28 | system/athanor-search                  | iso-v0                   | yes                      | doc_launcher LA2                            | KEEP                                          | low      |
| CSY-29 | system/athanor-trust-state             | iso-v0                   | yes                      | doc_update_trust UT7                        | KEEP                                          | low      |
| CSY-30 | system/athanor-unit                    | iso-v0                   | yes                      | doc_kernel_profile section 10, SH8          | KEEP                                          | low      |
| CSY-31 | system/Containerfile                   | both (rewritten)         | the image                | doc_system_image S2                         | KEEP                                          | low      |
| CSY-32 | system/build-image.sh                  | iso-v0                   | pipeline                 | doc_system_image S8                         | KEEP                                          | low      |
| CSY-33 | system/image-digests.sh                | iso-v0                   | pipeline                 | doc_update_trust UT2                        | KEEP                                          | low      |
| CSY-34 | system/kernel-artifacts.sh             | iso-v0                   | pipeline                 | doc_build_ordering O3                       | KEEP                                          | low      |
| CSY-35 | system/package-delta.sh                | iso-v0                   | pipeline                 | doc_system_image section 4                  | KEEP                                          | low      |
| CSY-36 | system/promote.sh                      | iso-v0                   | pipeline                 | doc_update_trust UT4                        | KEEP                                          | low      |
| CSY-37 | system/sign-images.sh                  | iso-v0                   | pipeline                 | doc_update_trust UT2                        | KEEP                                          | low      |
| CSY-38 | system/disk_config                     | both                     | ISO build                | doc_first_run FR21                          | KEEP                                          | low      |
| CSY-39 | system/keys                            | iso-v0                   | yes (image)              | doc_update_trust UT2                        | KEEP                                          | low      |
| CSY-40 | system/nvidia                          | iso-v0                   | yes (variants)           | doc_system_image S7                         | KEEP                                          | low      |
| CSY-41 | system/tests                           | iso-v0                   | CI                       | doc_build_system section 6                  | KEEP                                          | low      |
| CSY-42 | system/README.md                       | iso-v0                   | n/a                      | n/a                                         | KEEP (one correction)                         | low      |
| CSY-43 | system/ermete-compositor (main only)   | Ermete                   | was not                  | none; cosmic-comp chosen                    | RETIRE (stay deleted)                         | low      |
| CSY-44 | system/ermete-shell-rs (main only)     | Ermete                   | n/a                      | RA-5                                        | RETIRE (stay deleted)                         | low      |
| CSY-45 | system/ermete-ai-daemon (main only)    | Ermete                   | n/a                      | none                                        | RETIRE (stay deleted)                         | low      |
| CSY-46 | system/ermete-updater-rs (main only)   | Ermete                   | was not                  | RA-6                                        | RETIRE (stay deleted)                         | low      |
| CSY-47 | system/portal (main only)              | Ermete                   | was not                  | none                                        | RETIRE (stay deleted)                         | low      |
| CSY-48 | system/config (main only)              | Ermete                   | was not                  | session compositor decision                 | RETIRE (stay deleted)                         | low      |
| CSY-49 | system/bcachefs-root.mount (main only) | Ermete                   | was not                  | RA-7                                        | RETIRE (stay deleted)                         | low      |
| CSY-50 | system/ARCHITECTURE.md (main only)     | Ermete                   | n/a                      | RA-1                                        | RETIRE (stay deleted)                         | low      |

Severity count: critical 6, high 4, medium 8, low 32.

## Findings

### CSY-01 system/athanor-greeter: RETIRE

- **Severity:** critical
- **Category:** fake security mechanism (authentication, TPM, attestation)
- **Where:** `system/athanor-greeter/src/auth.rs:16`, `src/tpm.rs:106,162`, `src/attestation.rs:104-105`, `src/main.rs:129,172`
- **Evidence:** Authentication tries the PAM services `login`, `system-auth`, `passwd`, `sudo` and `other` in turn (`auth.rs:16`), and the first one that accepts wins. That widens the policy to the weakest stack, and `other` is normally deny-all only by convention. The "TPM unseal" is `SHA256(username || password || [0u8; 32])` (`tpm.rs:162`): no TPM object is involved. The PCRs are read with an empty selection and discarded. `verify_boot_chain` always returns `is_trusted: true` (`tpm.rs:106`). The fallback attestation report claims `hardware_enclave_active: true, secrets_released: true` (`attestation.rs:104-105`), and `main.rs:129` turns either flag into `attestation_verified`. The password is read from the `GREETER_PASSWORD` environment variable (`main.rs:172`). The crate is in `experimental/EXEMPT`, and no spec in packages.json ships it. The shipped greeter is `greeter-ui` on greetd. Era: both versions are the same code with renames (+26/-29).
- **Standard:** OWASP ASVS V2 (authentication) and V6 (stored cryptography); polkit and PAM least privilege; the CLAUDE.md rule "no fake implementations in security".
- **Recommendation:** Delete the crate and its forge spec. TPM-bound unlock, if wanted, belongs to `systemd-cryptenroll` (disk) or `systemd-homed` (home), as doc_first_run decision 2 and D42 already frame it. Boot-chain trust belongs to measured boot plus the UKI PCR policy (doc_kernel_profile section 9).
- **Needs a decision:** no (already superseded by the shipped greeter-ui).

### CSY-02 system/athanor-hypervisor-daemon: RETIRE

- **Severity:** critical
- **Category:** fake security mechanism (post-quantum handshake, attestation, secret release)
- **Where:** `system/athanor-hypervisor-daemon/src/attestation.rs:104-130,139,272,366`
- **Evidence:** `verify_pqc_hardware_handshake` (`:104`) generates a fresh local Dilithium keypair, signs the nonce with it, and then verifies that signature against the remote public key (`:130`). It can only succeed if the two keys are equal, so it proves nothing about the peer. `verify_keylime_tpm` (`:139`) returns Trusted whenever PCRs can be read, with no reference values. The "measurement" is the SHA-256 of `/proc/self/exe` (`:272`). The "secret release" writes a random key to a file under `/run/athanor`. The test at `:366` asserts `res.is_err()` and then propagates `res?`, which contradicts itself. Not shipped (EXEMPT). Out of 1.0 per components.toml (issue 57). Era: main's private polkit copy returned true when `conn.peer_creds()` had uid 0. Those are the bus daemon's credentials, not the caller's (Suspected bypass). iso-v0 uses the shared `athanor-bus-api` helper, so iso-v0 is better.
- **Standard:** OWASP ASVS V6 and V9; SLSA (provenance of measurements); the CLAUDE.md rule "no fake implementations in security".
- **Recommendation:** Delete the crate and its spec. MicroVM isolation for 1.0 is Flatpak/bubblewrap plus Landlock (RA-18). Later VM work should use upstream libvirt or crosvm, with attestation through Keylime (decision 0049). Close issue 57 against the retirement.
- **Needs a decision:** no. Issue 57 already tracks the fake post-quantum authentication.

### CSY-03 system/confidential_computing: RETIRE

- **Severity:** critical
- **Category:** fake security mechanism (SEV-SNP, TDX and Keylime attestation)
- **Where:** `system/confidential_computing/athanor-attestation/src/verifier.rs:37-118`, `src/cvm_manager.rs:208-224,489,499`, `src/key_release.rs:57-58`
- **Evidence:** The SEV-SNP check verifies a signature over the measurement only, against a configured "remote pubkey" (`verifier.rs:37,81`). It does not check the report body against the VCEK, ASK and ARK chain. The TDX check passes `report.report_mac_struct[..64]`, which is a MAC structure, as an ECDSA signature (`verifier.rs:118`). The Keylime path POSTs to `https://keylime.athanor.local:8881/v1/quotes/verify`, a host that does not exist (`cvm_manager.rs:208`), and fills the PCRs with the strings `"verified_pcr0"`, `"verified_pcr7"` and `"verified_pcr10"` (`:222-224`). Key release shells out to `sev-guest-unseal` and `tdx-guest-unseal`, tools that exist nowhere (`key_release.rs:57-58`). The tests write to `/tmp` (verify.py paths: `cvm_manager.rs:489,499`). The crate has its own `Cargo.lock`. It has no reverse dependency, no spec (verify.py shipped fails on it), and its components.toml status is "missing". Era: iso-v0 +88/-31 against main, the same design.
- **Standard:** OWASP ASVS V6; AMD SEV-SNP ABI and Intel TDX DCAP verification models; the CLAUDE.md rule "no fake implementations in security".
- **Recommendation:** Delete the directory. When doc_fleet needs confidential VMs, use REPLACE-UPSTREAM: Keylime for TPM attestation (installed and disabled per decision 0049), and the Confidential Containers Trustee/KBS or `snpguest` for SEV-SNP and TDX report verification.
- **Needs a decision:** yes. `system/confidential_computing/athanor-attestation` is a protected path in CLAUDE.md ("stop and ask"), so the maintainer must approve the deletion.

### CSY-04 system/athanor-mesh-bus: REPLACE-UPSTREAM

- **Severity:** critical
- **Category:** fake security mechanism (session keys, signatures)
- **Where:** `system/athanor-mesh-bus/src/tunnel.rs:113-131`, `src/crdt_broadcaster.rs:191`
- **Evidence:** When the Rosenpass PSK is missing, the session key falls back to `vec![0u8; 32]` and the peer is marked Authenticated without any signature check (`tunnel.rs:113-131`). `initiate_handshake` sends `sender_node_id: "local_node"` with a zero session key. CRDT broadcasts carry `pqc_sig = vec![0u8; 64]` (`crdt_broadcaster.rs:191`), and their nonce and Kyber signature are also zero. `verify_signature` runs Ed25519 over keys named Dilithium. The crate has 2812 lines and 57 `unsafe` blocks without SAFETY comments. Not shipped (EXEMPT). Out of 1.0 (issue 57). Era: iso-v0 +48/-124, the same design.
- **Standard:** OWASP ASVS V6 and V9; Rust API Guidelines (unsafe documentation); the CLAUDE.md rule "no fake implementations in security".
- **Recommendation:** Delete the crate. A2-12 already chose the fleet transport: kernel WireGuard with a Headscale- or NetBird-class coordinator, Cloudflare Access for identity, and Rosenpass as the post-quantum PSK provider. Synchronisation, if needed, goes to Syncthing or a CRDT library behind that tunnel, not to a home-made transport.
- **Needs a decision:** no (A2-12 decides the replacement; doc_fleet supersedes D26-D38).

### CSY-05 system/athanor-cluster-mesh: RETIRE

- **Severity:** critical
- **Category:** fake functionality behind an unauthenticated listener
- **Where:** `system/athanor-cluster-mesh/src/main.rs:64-65`, `src/swarm_ipc.rs:41-42,134`, `src/discovery.rs:66-67`, `src/npu_scheduler.rs:113`
- **Evidence:** A TCP server on port 51823 binds to the CGNAT 100.64/10 address and falls back to `0.0.0.0` (`swarm_ipc.rs:41-42`, `discovery.rs:66-67`). It accepts every handshake without authentication. `execute_local_npu_layers` (`npu_scheduler.rs:113`) runs no inference, and the reply is the constant `"Distributed NPU Swarm Llama 3.2 token output"` (`swarm_ipc.rs:134`). The node id is random. Not shipped (EXEMPT). Out of 1.0 (issue 57).
- **Standard:** OWASP ASVS V9 (communications) and V1 (architecture); systemd hardening (no network listener without authentication).
- **Recommendation:** Delete the crate and its spec. Distributed inference, if the fleet ever wants it, has an upstream answer: llama.cpp RPC behind the A2-12 WireGuard mesh.
- **Needs a decision:** no.

### CSY-06 system/athanor-store: RETIRE

- **Severity:** critical
- **Category:** placeholder in a security path (image verification)
- **Where:** `system/athanor-store/src/main.rs:39,193`
- **Evidence:** The install path runs `cosign verify` on a tag and then pulls `@sha256:PINNED_IMMUTABLE_HASH_PLACEHOLDER` (`main.rs:193`). The digest that gets used is not the one that was verified, and it is a literal placeholder. The key path is `/etc/athanor/keys/cosign.pub` (`:39`), but the Containerfile installs `system/keys/` to `/usr/share/athanor/keys/`, never `system/cosign.pub`. The crate can also delete the Flathub remote. It runs a Dilithium check over a synthetic payload. Its io_uring/CRDT storage is referenced only by `mesh-bus/storage_bridge`. It has no spec and is not exempt (verify.py shipped fails). doc_software decision 5 removed its install commands and keeps the storage "for the fleet". Era: iso-v0 +23/-22, the same code.
- **Standard:** SLSA (verify what you deploy, by digest); OWASP ASVS V10 (code integrity); the CLAUDE.md rule "no fake implementations in security".
- **Recommendation:** Delete the crate together with `system/cosign.pub` (CSY-20). Software installation is Flatpak plus the signed bootc image (doc_software). If the fleet later needs storage, it gets a specification of its own.
- **Needs a decision:** yes. Amend doc_software decision 5, which keeps the storage.

### CSY-07 system/athanor-bus-api: MAKE-REAL

- **Severity:** high
- **Category:** unspecified security-critical library
- **Where:** `system/athanor-bus-api/src/polkit.rs:35-48,97-137`, `src/lib.rs:24-28`, `src/shm_ring.rs`, `src/socket.rs`
- **Evidence:** `polkit.rs` (172 lines) is real and tested. `check_polkit_auth_zbus` and `check_subject` build a `system-bus-name` subject. `unix_process_subject` (`:131`) resolves the caller with `GetConnectionCredentials`, but sends `start-time = 0` (`:48`). A `ponytail:` note at `:137` admits the resulting PID-reuse window. Shipped `athanor-update` (`serve.rs`) calls it. So do `cloud-rs`, `lvfs-rs`, `mdm-rs`, and the retirement candidates `telemetry`, `hypervisor`, `ebpf-sched` and `mesh-bus`. The rest of the crate does not belong in a polkit helper: mesh, telemetry and AI payload types, a 687-line `unsafe` SPSC ring (`shm_ring.rs`) and socket framing (`socket.rs`), used only by components this report retires. components.toml status: missing ("polkit subject check every system service uses"). Era: iso-v0 +210/-8, and iso-v0 is better: main had no shared helper, and the hypervisor kept a private, bypassable copy.
- **Standard:** polkit least privilege (prefer `system-bus-name` subjects, or `unix-process` with pidfd/start-time); Rust API Guidelines (crate scope); ADR for the security decision.
- **Recommendation:** Write a short spec (one section in doc_update_trust or a dedicated doc) for the polkit helper. Remove `shm_ring`, `socket` and the mesh/telemetry/AI types when their consumers retire. Close the PID-reuse window: use the `system-bus-name` subject everywhere a bus name exists, and use `pidfd` or the real start time for the `unix-process` path. Evaluate the `zbus_polkit` crate as an upstream alternative (Suspected available; not checked offline).
- **Needs a decision:** yes. `system/athanor-bus-api/src/polkit.rs` is protected in CLAUDE.md.

### CSY-08 system/athanor-ebpf-sched: REPLACE-UPSTREAM

- **Severity:** high
- **Category:** contradictory status; broken authorisation
- **Where:** `system/athanor-ebpf-sched/src/dbus_interface.rs:40`, `src/bpf_trace.rs:39`, `src/sched_ext.rs:162-165`; `docs/architecture/components.toml` (ebpf-sched entry); `docs/architecture/doc_kernel_profile.md:135,1114`
- **Evidence:** The crate enforces `os.athanor.ebpfsched.update` (`dbus_interface.rs:40`), but no `.policy` file declares that action, so polkit always denies it (verify.py polkit). It registers a scheduler control name on the session bus, loads BPF objects from `target/` paths (verify.py paths: `bpf_trace.rs:39`, `sched_ext.rs:162,163,165`), and carries an "AI bridge". No spec, not exempt (verify.py shipped). doc_kernel_profile D5 (`:135`) and `:1114` retire it in favour of BORE plus `scx_loader`, but components.toml lists it as "out-of-1.0, issue 123". Decision 0033 (RA-17) already dropped its stash.
- **Standard:** ADR consistency (one source of truth); polkit least privilege (declared actions); Fedora Packaging Guidelines (no build-tree paths at runtime).
- **Recommendation:** Delete the crate. Ship `scx_loader` with `scx_lavd` or `scx_bpfland` under the BORE kernel, as D5 says. Correct components.toml to "retired" and close issue 123.
- **Needs a decision:** no (D5 decides). The components.toml correction follows from it.

### CSY-09 system/athanor-agentic-kernel: RETIRE

- **Severity:** high
- **Category:** misleading security claims; dead code
- **Where:** `system/athanor-agentic-kernel/src/main.rs`, `src/ebpf_monitor.rs:18`, `src/hot_patcher.rs`, `src/ai_predictor.rs`
- **Evidence:** A 2-second loop applies whitelisted sysctls when thresholds trip. On a "TCP scan" it blocks the source IP through `ATHANOR_AI_GATEWAY`, which defaults to `127.0.0.1`. It turns on a "zero-trust" XDP mode by loading `target/bpfel-unknown-none/release/ebpf-core` (`ebpf_monitor.rs:18`), and it ignores the `setrlimit` result. `hot_patcher.rs` (590 lines) is a real aya kprobe/uprobe/freplace injector that `main` never calls: a live-patching primitive with no consumer. `ai_predictor.rs` has an `unsafe impl aya::Pod` without a SAFETY comment. 927 lines in all. No spec, not exempt, not shipped.
- **Standard:** systemd hardening (no runtime sysctl mutation by a daemon); Rust API Guidelines (unsafe documentation); OpenSSF Best Practices (no unused attack surface).
- **Recommendation:** Delete the crate. Static sysctl hardening goes to the kernel profile (CSY-18). Network blocking goes to firewalld/nftables (CSY-12). Runtime detection is Tetragon, which decision 0048 made real.
- **Needs a decision:** no.

### CSY-10 system/Justfile: MERGE-INTO root Justfile

- **Severity:** high
- **Category:** broken documented entry point
- **Where:** `system/Justfile` (recipes `build`, `_build-bib`, `build-unikernel`, `secureboot-key-audit`); `Justfile:8,44-66`
- **Evidence:** The root `Justfile` imports it (`mod system`) and calls `just system/build` for `system-build`, `disk-qcow2` and `disk-iso`. `build` runs `scripts/build_container.sh`, which runs `podman build .` with `system/` as the context and passes no `AZOTH_NVR`. The Containerfile copies repo-root paths (`COPY forge/specs/azoth/pins.env`, `COPY system/scripts/`) and tests `/usr/lib/modules/${AZOTH_NVR}.x86_64/vmlinuz` (`Containerfile:84`). This recipe therefore cannot build the image (Suspected; not run, because builds are out of scope). The real entry point is `system/build-image.sh`, used by CI. `build-unikernel` targets the non-existent `athanor-unikernel-daemon`. `secureboot-key-audit` runs `chmod` and `mv` on private keys under `/etc/pki` on the host, which no flow uses. `_build-bib` duplicates `forge/scripts/build_iso.sh`.
- **Standard:** Diátaxis (a how-to must work); "Pipeline portable, GitHub as glue" standing rule (one script per task, the same locally and in CI).
- **Recommendation:** Delete `system/Justfile`. In the root Justfile, make `system-build` call `system/build-image.sh` and `disk-iso` call `forge/scripts/build_iso.sh`. Drop the `unikernel` and key-audit recipes. Keep `check`/`lint` coverage through the root `just lint`.
- **Needs a decision:** no.

### CSY-11 system/athanor-install.ks: MERGE-INTO system/disk_config/iso.toml

- **Severity:** medium
- **Category:** divergent duplicate configuration
- **Where:** `system/athanor-install.ks:26,37,40-41`; `system/disk_config/iso.toml`; `docs/architecture/doc_software.md:275,349`; `docs/architecture/doc_first_run.md:23,352,373`
- **Evidence:** This is the kickstart for a manual Anaconda install. It pins `ghcr.io/hr-mes/athanor-system:latest`, an unsigned moving tag rather than `:stable` (`:26`). It enables `sshd` and `systemd-homed` (`:37`) and claims that homed encrypts the user's home (`:40-41`). The product ISO is built from `iso.toml`, which enables neither and whose kickstart carries only `xconfig --startxonboot` and the fstab `%post`. doc_software decision 2 turns remote login off on new installs and doc_first_run corrects the homed comment, so the two install paths already disagree. components.toml status: missing.
- **Standard:** ISO 29148 (one requirement, one source); systemd hardening (no network service on by default without a decision).
- **Recommendation:** Keep a single kickstart source: `iso.toml`, which bootc-image-builder consumes. Delete `athanor-install.ks`, or generate it from `iso.toml` if a manual path is still wanted. Apply doc_software SWb and doc_first_run FR21 there.
- **Needs a decision:** no. doc_first_run doubt 7 leaves "whether the file stays" open, and this recommends that it goes.

### CSY-12 system/ebpf: REPLACE-UPSTREAM

- **Severity:** medium
- **Category:** duplicate of an upstream mechanism; build-tree paths
- **Where:** `system/ebpf/ebpf-core` (364 lines), `system/ebpf/ebpf-loader/src/main.rs:68,72`
- **Evidence:** A real aya XDP program with an IPv4 blocklist and a port allowlist. It is compiled with `#![allow(clippy::all, warnings, unsafe_code)]`. The loader embeds the object with `include_bytes_aligned!("../../target/bpfel-unknown-none/...")` (verify.py paths `main.rs:68,72`) and defaults to the interface `eth0`. No spec, not exempt, not shipped. Its only consumer is `agentic-kernel` (CSY-09).
- **Standard:** Fedora Packaging Guidelines (no build-tree paths); OpenSSF Best Practices (prefer maintained upstream components).
- **Recommendation:** Delete it. Host filtering is `firewalld` over nftables (`nftables` is already in `upstream_core`). Any eBPF security observation is Tetragon (decision 0048).
- **Needs a decision:** no.

### CSY-13 system/athanor-net-unikernel: RETIRE

- **Severity:** medium
- **Category:** dead code with no consumer
- **Where:** `system/athanor-net-unikernel/src`; `forge/specs/athanor-net-unikernel/athanor-net-unikernel.spec:25,30`
- **Evidence:** A smoltcp userspace TCP/IP stack on a TAP device that falls back to loopback, polled every 5 ms. MicroVM registration is duplicated. Nothing consumes it. The forge spec does `cd /forge/system/...` and installs from `target/release`, but the package is not in packages.json (EXEMPT). The "Level 12 unikernel" build script and recipe (CSY-10, CSY-17) target a component that does not exist.
- **Standard:** OpenSSF Best Practices (no unused attack surface); Fedora Packaging Guidelines (build in `%build`).
- **Recommendation:** Delete the crate, its spec, `system/scripts/build_unikernel.sh` and the root `unikernel` recipe. Guest networking for any future VM uses passt or a network namespace.
- **Needs a decision:** no.

### CSY-14 system/athanor-telemetry: RETIRE

- **Severity:** medium
- **Category:** contradicts an accepted decision
- **Where:** `system/athanor-telemetry/src`; `docs/decisions/` A2-19 (0056), A2-10 (0045)
- **Evidence:** It tails `journalctl`, flags "anomalies", and calls InitOracle, falling back to "simulated execution". A2-19 decides there is no telemetry, and "report a problem" goes through `athanor-profile-check`. A2-10 already deleted the telemetry document. Not shipped (EXEMPT). Era: iso-v0 +60/-110, the same design.
- **Standard:** ADR (an accepted decision binds); GDPR-style data minimisation as reflected in A2-19.
- **Recommendation:** Delete the crate and its spec.
- **Needs a decision:** no (A2-19).

### CSY-15 system/athanor-init-oracle: RETIRE

- **Severity:** medium
- **Category:** duplicate of systemd; unsafe fallback path
- **Where:** `system/athanor-init-oracle/src/systemd_manager.rs:13`
- **Evidence:** It polls `systemctl` every 60 seconds and writes units to `/etc/systemd/system`. When that fails, it falls back to `/tmp/systemd/system` with `systemctl --user` (verify.py paths `:13`), a world-writable location. Not shipped (EXEMPT).
- **Standard:** systemd hardening (`Restart=`, `OnFailure=`, `StartLimit*`); greenboot for boot health on an image-based OS.
- **Recommendation:** Delete it. Recovery is `Restart=`/`OnFailure=` in each unit, plus greenboot-style health checks on the bootc deployment.
- **Needs a decision:** no.

### CSY-16 system/athanor-style: KEEP (drop the pre-Calmo modules)

- **Severity:** medium
- **Category:** legacy code inside a kept crate
- **Where:** `system/athanor-style/src/lib.rs:1-17`, `src/appearance_engine.rs:399`, `src/accent_engine.rs`, `src/glass.rs`, `Cargo.toml`; `forge/specs/athanor-recovery/athanor-recovery-1.0.0/src/main.rs:358`
- **Evidence:** `calmo.rs` and `calmo/` (the token generator, its contrast check and tests) are the visual identity. CI runs `calmo/tests`, `contrast.py` and `generate.py --check` (`call-lint.yml:82-84`), and `calmo.spec` uses the generator. `lib.rs` says the three other modules (`accent_engine`, 420 lines; `appearance_engine`, 665 lines; `glass`, 73 lines) are the pre-Calmo glass theme, kept only for `athanor-recovery`, with all warnings silenced. `appearance_engine.rs:399` falls back to `/tmp/athanor_active_layout.toml` (verify.py paths). They pull `zbus` and `tokio` with `features = ["full"]` into a GTK styling crate. The callers of `load_glass_theme` are recovery (`main.rs:358`), oobe (CSY-19) and the old `athanor-shell-rs` dock. Era: main had 7 files and iso-v0 has 154, so iso-v0 is better.
- **Standard:** Rust API Guidelines (a crate does one thing); doc_shell SH4 (re-skin recovery, then delete).
- **Recommendation:** Keep the crate. Re-skin recovery on Calmo per SH4, then delete `accent_engine`, `appearance_engine` and `glass`, and remove `zbus`, `tokio` and `gtk4-layer-shell` from its dependencies. Update the Cargo `description`, which still advertises "Glassmorphism".
- **Needs a decision:** no (SH4 already plans it).

### CSY-17 system/scripts: KEEP assemble_uki.sh, RETIRE the rest

- **Severity:** medium
- **Category:** dead scripts copied into the image build
- **Where:** `system/scripts/`; `system/Containerfile:150,249,262`; `Justfile:66`; `docs/architecture/doc_software.md:23,339`
- **Evidence:** The Containerfile copies the whole directory to `/scripts` (`:150`), runs only `assemble_uki.sh` (`:249`), and deletes `/scripts` (`:262`). The other scripts:
  - `provision_flatpak.sh` is never run, and doc_software decision 6 deletes it.
  - `athanor-theme-generator.sh` and `matugen_theme.template` are referenced only by the old `athanor-shell-rs` palette, at `/usr/bin` paths no package installs.
  - `build_unikernel.sh` targets a component that does not exist.
  - `build_bib.sh` and `build_container.sh` serve only `system/Justfile` (CSY-10).

  `assemble_uki.sh` is specified (doc_kernel_profile, the PCR policy row near `:700`). The directory's components.toml status is missing.

- **Standard:** OpenSSF Best Practices (no unused code in the build context); SLSA (a minimal build input set).
- **Recommendation:** Keep `assemble_uki.sh`. Move it beside `build-image.sh`, or name it in components.toml with its spec. Delete the other six files, and change the Containerfile to `COPY system/scripts/assemble_uki.sh`.
- **Needs a decision:** no.

### CSY-18 system/sysctl.d: MERGE-INTO forge/specs/athanor-kernel-profile

- **Severity:** medium
- **Category:** a hardening file that nothing installs
- **Where:** `system/sysctl.d/99-athanor-hardening.conf`; `forge/specs/athanor-kernel-profile/SOURCES/usr/share/athanor/kernel-profile/base.json:164-168`; `forge/specs/athanor-system-tweaks/SOURCES/usr/lib/sysctl.d/99-network-security.conf`; `docs/architecture/doc_kernel_profile.md` (Sysctl and memory, P3)
- **Evidence:** It sets `kptr_restrict=2`, `dmesg_restrict=1`, `yama.ptrace_scope=2`, `unprivileged_bpf_disabled=1` and `rp_filter=1`. Neither the Containerfile nor any spec installs it (components.toml: "nothing installs"). doc_kernel_profile records that `ptrace_scope` is 0 on the machine. `base.json` already specifies `dmesg_restrict` and `kptr_restrict`. `athanor-system-tweaks` ships `rp_filter` with Italian comments. A reader assumes hardening that is not in force.
- **Standard:** systemd sysctl.d conventions (vendor files in `/usr/lib/sysctl.d`, owned by a package); KSPP recommended settings.
- **Recommendation:** Move the values that P3 accepts (`ptrace_scope`, `unprivileged_bpf_disabled`) into the kernel profile's sysctl set, which `athanor-kernel-profile` ships and tests. Then delete the directory.
- **Needs a decision:** no (P3 owns the sysctl set; `ptrace_scope=2` versus 1 is a P3 choice).

### CSY-19 system/athanor-oobe: RETIRE

- **Severity:** low
- **Category:** prototype already retired by a spec
- **Where:** `system/athanor-oobe/src/main.rs:103,617`; root `Cargo.toml` (`exclude`)
- **Evidence:** It is excluded from the workspace. It writes `/etc/athanor/oobe.json`, falling back to `/tmp/athanor` (verify.py paths `:103`). Its telemetry switch defaults to on, and nothing reads the file. doc_first_run (FR1, decision 9) retires it. A2-22 makes the installer the Anaconda web UI.
- **Standard:** ADR (execute accepted decisions).
- **Recommendation:** Delete the crate as FR1 says.
- **Needs a decision:** no.

### CSY-20 system/cosign.pub: RETIRE

- **Severity:** low
- **Category:** orphan key
- **Where:** `system/cosign.pub`; `system/athanor-store/src/main.rs:39`; `system/Containerfile:171`
- **Evidence:** Its only reader is `athanor-store`, which expects it at `/etc/athanor/keys/cosign.pub`. The image installs `system/keys/` (`athanor-image-1.pub`) to `/usr/share/athanor/keys/` instead. The key is therefore never on a machine. Root cause shared with CSY-06.
- **Standard:** doc_update_trust UT2 (one image key, one location).
- **Recommendation:** Delete it together with `athanor-store`.
- **Needs a decision:** no (follows CSY-06).

### CSY-21 system/athanor-compositor-client: KEEP

- **Severity:** low
- **Category:** keep; checker false positive
- **Where:** `system/athanor-compositor-client/src/shortcuts.rs:236,284,293,299`, `src/connection.rs:531-640`
- **Evidence:** It is the only crate that speaks the COSMIC Wayland protocols (SH2), used by the bar, dock, launcher, layout-chooser and apps. It has 61 tests. Its `unsafe` is confined to a GLib `GSource` in `connection.rs`, each block with a SAFETY comment. verify.py `panics` flags `shortcuts.rs:236,284,293,299` as `expect` calls over budget, but they are the RON parser's own `s.expect(':')` method, which returns `Result`. That is a false positive of the checker.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it. Teach `scripts/verify.py panics` to match `.expect(` only on `Option`/`Result` receivers, or rename the parser method (for example `eat`).
- **Needs a decision:** no.

### CSY-22 system/athanor-apps: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-apps`
- **Evidence:** Specified (doc_bar BR3) and used by the bar and dock. 30 tests. No non-test `unwrap`, `unsafe` or `/tmp` paths.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-23 system/athanor-i18n: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-i18n`
- **Evidence:** Specified (doc_languages LN2). Used by greeter-ui, layout-chooser, bar, apps, dock, launcher and preview. 10 tests. Clean non-test code.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-24 system/athanor-layout: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-layout`
- **Evidence:** Specified (doc_shell SH6). Parses TOML through the `toml` crate (`favorites.rs:86`, `document.rs:103`). 69 tests. `vendor/10-athanor.toml` is installed by `bar.spec`.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-25 system/athanor-portal: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-portal`
- **Evidence:** Specified (doc_portal PT2/PT5). The library of the shipped `xdg-desktop-portal-athanor`. 7 tests. Clean non-test code. It shares a name with main's unrelated `system/portal` (CSY-47), an Astro site.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-26 system/athanor-preview: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-preview/src/render.rs:79`
- **Evidence:** Specified (doc_launcher LA6). Used by the launcher. 16 tests. The only `unsafe` match is a comment explaining why the code avoids `sysconf` because the workspace denies unsafe code.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-27 system/athanor-preview-render: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-preview-render`
- **Evidence:** Specified (doc_launcher LA6), a per-request render helper built by `launcher.spec`. 4 tests. Clean non-test code.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-28 system/athanor-search: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-search`
- **Evidence:** Specified (doc_launcher LA2), used by the launcher and preview, and reused by doc_files FM11. 81 tests. Clean non-test code.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-29 system/athanor-trust-state: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-trust-state`
- **Evidence:** Specified (doc_update_trust UT7, SH12). Used by bar, update and update-notify. `update.spec` runs its tests. 11 tests. Clean non-test code.
- **Standard:** Rust API Guidelines.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-30 system/athanor-unit: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/athanor-unit/src/sandbox.rs`, `src/crash_loop.rs`
- **Evidence:** Specified (doc_kernel_profile section 10 for Landlock self-confinement; SH8 for the crash-loop counter). Used by shelld, bar, apps, dock, launcher, preview, search and the portal. doc_files, doc_osd and doc_portal reuse it. 24 tests. Clean non-test code.
- **Standard:** Rust API Guidelines; systemd hardening.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-31 system/Containerfile: KEEP

- **Severity:** low
- **Category:** keep; supply-chain polish
- **Where:** `system/Containerfile:11,48,89,102,113,150`
- **Evidence:** Specified (doc_system_image S2). The base image and the Fedora builder are pinned by digest, the NVIDIA modules by digest, and the image is checked with `bootc container lint`. Re-audit decision 12 accepts the hard-coded owner in the four tier mounts. Those mounts use `:latest` tags rather than digests (`:48,89,102,113`), so an image build is not reproducible from its inputs alone. Era: rewritten on iso-v0; main's version is not worth comparing.
- **Standard:** SLSA Build L3 (pinned, recorded inputs); OpenSSF Scorecard (Pinned-Dependencies).
- **Recommendation:** Keep it. Pass the tier repository digests as build arguments, as is already done for the NVIDIA modules. Copy only `assemble_uki.sh` instead of `system/scripts/` (CSY-17).
- **Needs a decision:** no.

### CSY-32 system/build-image.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/build-image.sh`
- **Evidence:** Specified (doc_system_image S8). 100 lines, `set -euo pipefail`. The one image build used by `call-system-image.yml` and `system-image-check.yml`. Tested by `system/tests/test_build_image.py`.
- **Standard:** "Pipeline portable, GitHub as glue" standing rule.
- **Recommendation:** Keep it, and make it the only local entry point (CSY-10).
- **Needs a decision:** no.

### CSY-33 system/image-digests.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/image-digests.sh`
- **Evidence:** Specified (doc_update_trust UT2). 27 lines, strict mode. Called by `call-system-image.yml`.
- **Standard:** SLSA (record the digests that are signed).
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-34 system/kernel-artifacts.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/kernel-artifacts.sh:39,55,58`
- **Evidence:** Specified (doc_build_ordering O3). 474 lines, strict mode. Called by five workflows and `fetch_repo_rpms.sh`. Tested by `test_kernel_artifacts.py`. The owner comes from `GITHUB_REPOSITORY_OWNER`, with a default.
- **Standard:** SLSA (verified inputs).
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-35 system/package-delta.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/package-delta.sh`
- **Evidence:** Specified (doc_system_image section 4). 28 lines, strict mode. Called by `system-image-check.yml`. It has no test of its own.
- **Standard:** doc_build_system section 6 (scripts carry tests).
- **Recommendation:** Keep it. Add a small unittest in `system/tests` if the script grows.
- **Needs a decision:** no.

### CSY-36 system/promote.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/promote.sh`
- **Evidence:** Specified (doc_update_trust UT4). 75 lines, strict mode. Called by `promote-stable.yml`. Tested by `test_promote.py`.
- **Standard:** SLSA (promote only signed artifacts).
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-37 system/sign-images.sh: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/sign-images.sh`
- **Evidence:** Specified (doc_update_trust UT2). 70 lines, strict mode. Called by `call-system-image.yml` in the job that holds the key. Tested by `test_sign_images.py`.
- **Standard:** SLSA; Sigstore cosign key-based signing.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-38 system/disk_config: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/disk_config/iso.toml`, `disk.toml`, `defs/athanor-43.yaml`
- **Evidence:** Specified (doc_first_run FR21). `iso.toml` is the product ISO's kickstart, used by `forge/scripts/build_iso.sh`. `disk.toml` deliberately declares no user. Era: iso-v0 +193/-8, better documented.
- **Standard:** bootc-image-builder configuration.
- **Recommendation:** Keep it, and make it the single kickstart source (CSY-11).
- **Needs a decision:** no.

### CSY-39 system/keys: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/keys/athanor-image-1.pub`; `system/Containerfile:171-172`
- **Evidence:** Specified (doc_update_trust UT2). It holds only the public image key, installed to `/usr/share/athanor/keys/` and rendered into the signature policy.
- **Standard:** Sigstore/containers-policy.json key trust.
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-40 system/nvidia: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/nvidia/` (`gate.sh`, `lock.py`, `locks/`, `mirror*.sh`, `azoth-nvidia-kmod.spec`, `tests/`)
- **Evidence:** Specified (doc_system_image S7). The Containerfile runs `gate.sh` at `:232-233`, and CI runs `system/nvidia/tests` (`call-lint.yml:54`).
- **Standard:** SLSA (locked vendor inputs).
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-41 system/tests: KEEP

- **Severity:** low
- **Category:** keep
- **Where:** `system/tests/`
- **Evidence:** Specified (doc_build_system section 6). Five unittest modules plus `fake_registry.py`, run by `call-lint.yml:56`.
- **Standard:** OpenSSF Best Practices (automated tests in CI).
- **Recommendation:** Keep it.
- **Needs a decision:** no.

### CSY-42 system/README.md: KEEP (one correction)

- **Severity:** low
- **Category:** documentation accuracy
- **Where:** `system/README.md:11`; `docs/architecture/doc_first_run.md:23,373`
- **Evidence:** It is accurate on the image, the pipeline and installation. It says that crates in `experimental/EXEMPT` "are workspace members that no package ships". Yet `athanor-store`, `agentic-kernel`, `ebpf-sched`, `confidential_computing` and `ebpf` are also unshipped and not exempt (verify.py shipped fails on them). doc_first_run cites `system/README.md:128`, but the file has 64 lines.
- **Standard:** Diátaxis (reference must be true).
- **Recommendation:** Keep it. The sentence becomes true once CSY-03, CSY-06, CSY-08, CSY-09 and CSY-12 are retired. Fix the stale line reference in doc_first_run.
- **Needs a decision:** no.

### CSY-43 system/ermete-compositor (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison; fake component
- **Where:** `origin/main:system/ermete-compositor/` (35 files, 4745 lines of Rust)
- **Evidence:** A Smithay project, "AI-Driven Wayland Compositor". `main.rs` sets up a DRM/KMS probe with a headless fallback, an ECS world, animations (magic lamp, wobbly) and an IPC server. No file creates a Wayland display socket or an xdg-shell state (searched for `ListeningSocket`, `Display::new`, `XdgShellState`, `add_socket`), so it serves no client. iso-v0 uses upstream `cosmic-comp` (session compositor decision 2026-09-09; doc_shell), which is far better.
- **Standard:** OpenSSF Best Practices (prefer maintained upstream).
- **Recommendation:** Do not bring it back. iso-v0's cosmic-comp is the answer.
- **Needs a decision:** no.

### CSY-44 system/ermete-shell-rs (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison; empty placeholder
- **Where:** `origin/main:system/ermete-shell-rs`
- **Evidence:** On main it is a 0-byte regular file (blob `e69de29`), not a directory. The real shell lived in `forge/specs/ermete-shell-rs`, now `forge/specs/athanor-shell-rs` on iso-v0, kept only for the greeter until RA-5's conditions are met.
- **Standard:** n/a.
- **Recommendation:** Nothing to bring back.
- **Needs a decision:** no.

### CSY-45 system/ermete-ai-daemon (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison; empty placeholder
- **Where:** `origin/main:system/ermete-ai-daemon`
- **Evidence:** A 0-byte regular file on main (blob `e69de29`). The daemon lived in `forge/specs/ermete-ai-daemon`, now `forge/specs/athanor-ai-daemon`. That is outside this dimension.
- **Standard:** n/a.
- **Recommendation:** Nothing to bring back.
- **Needs a decision:** no.

### CSY-46 system/ermete-updater-rs (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison
- **Where:** `origin/main:system/ermete-updater-rs/src/main.rs` (194 lines)
- **Evidence:** It runs `bootc upgrade` or `bootc switch <image_ref>` with no signature policy, no trust state and no D-Bus or polkit interface. It checks hourly and exposes nothing. Decision 0022 (RA-6) deleted it. iso-v0's `athanor-update` (UT1-UT7: signed `:stable`, trust state, polkit through bus-api, download-only timer) is far better.
- **Standard:** SLSA; doc_update_trust.
- **Recommendation:** Do not bring it back.
- **Needs a decision:** no.

### CSY-47 system/portal (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison; template content
- **Where:** `origin/main:system/portal/` (Astro Starlight site, 20 files)
- **Evidence:** The unmodified Starlight template: `houston.webp`, and `guides/example.md` and `reference/example.md` with the example content. `scripts/npu_translator.py` "simulates" a connection to the AI daemon. It is not related to iso-v0's `system/athanor-portal`, the xdg portal backend. Note: `astro-toolchain` is still listed in `forge/config/packages.json` (Suspected leftover of this site; the forge dimension owns it).
- **Standard:** Diátaxis (documentation lives in `docs/`).
- **Recommendation:** Do not bring it back. If a public documentation site is wanted, generate it from `docs/` in a spec of its own.
- **Needs a decision:** no.

### CSY-48 system/config (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison
- **Where:** `origin/main:system/config/niri/config.kdl`
- **Evidence:** It holds only a niri configuration. niri was replaced by cosmic-comp for the greeter and the session (decision 2026-09-09).
- **Standard:** n/a.
- **Recommendation:** Do not bring it back.
- **Needs a decision:** no.

### CSY-49 system/bcachefs-root.mount (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison
- **Where:** `origin/main:system/bcachefs-root.mount`
- **Evidence:** It mounts `/dev/disk/by-label/ermete_root` as bcachefs on `/sysroot`. The bcachefs file system is out of the mainline kernel, and the image root is composefs over btrfs/xfs from bootc. Decision 0023 (RA-7) removed it.
- **Standard:** bootc/ostree root mounting by the initrd.
- **Recommendation:** Do not bring it back.
- **Needs a decision:** no.

### CSY-50 system/ARCHITECTURE.md (main only): RETIRE (stay deleted)

- **Severity:** low
- **Category:** era comparison; misleading documentation
- **Where:** `origin/main:system/ARCHITECTURE.md` (233 lines)
- **Evidence:** "Ermete OS v3.0 Architectural Specification", with a repository root on the author's machine. It claims "Formally Verified (AWS Kani Proofs)", "SLSA Level 4", bcachefs snapshots and an XDP firewall. None of these hold in either era. Decision 0017 (RA-1) deleted it. iso-v0 replaces it with arc42-style `docs/architecture/doc_*.md` specs plus `components.toml`.
- **Standard:** arc42/C4; Diátaxis.
- **Recommendation:** Do not bring it back.
- **Needs a decision:** no.
