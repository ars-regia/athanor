---
id: ADR-0073
title: "Retire components without a product role"
date: 2026-10-06
status: amended by ADR-0087
issues: [223]
areas: [platform, security, build]
---

# 0073. Retire components without a product role

## Context

Audit 3 judged every component under `system/` and `forge/specs/`, in both the Athanor era
(`iso-v0`) and the Ermete era (`origin/main`). The verdicts are in
`docs/reviews/2026-10-06-audit-3/system-components.md` and `forge-packages.md`.

- **Fake security mechanisms.** Six crates contain them, among them post-quantum key exchange
  with all-zero keys, attestation that fabricates verification, and a greeter whose "TPM
  unseal" is a hash. None of them ships, but `Recommends:` and tier repositories keep them one
  step away.
- **No live specification.** Most of the other Ermete-era crates have none.
- **Stubs in the user image.** Several package directories are stubs or build tools that
  reach the user image through the tier repositories.
- **Both eras.** Where a component exists in both, the `iso-v0` version is the same or better.

Earlier records already retired some of these components, but nothing was deleted: RA-5
(old crates), A2-12 (fleet transport; doc_fleet supersedes D26-D38), and A2-19 (no
telemetry). A2-10 lists `doc_forge_development_guide.md` among the dead documents, but
`iso-v0` has since rewritten that document and cites it.

## Decision

1. **Retire from the tree** every component that has no approved specification item and no
   consumer that ships, keeping git history as the archive:
   - the fleet crates (`athanor-mesh-sync`, `athanor-cloud-rs`, `athanor-cluster-mesh`,
     `athanor-mesh-bus`, `athanor-hypervisor-daemon`);
   - `system/confidential_computing`;
   - the Ermete-era `system/athanor-greeter` (greetd with `athanor-greeter-ui` replaces it);
   - `athanor-agentic-kernel`, `athanor-telemetry`, `athanor-init-oracle` and
     `athanor-net-unikernel`;
   - `system/ebpf`, `athanor-ebpf-sched`, `athanor-oobe`, `athanor-store` and
     `athanor-store-rs`;
   - `athanor-lvfs-rs` (fwupd replaces it), `athanor-mdm-rs`, `athanor-rosenpass`,
     `athanor-sysmon-ebpf`, `athanor-ai-daemon` and `athanor-ananicy`;
   - the frozen `athanor-shell-rs` and its only consumers (`athanor-niri-ipc`, and
     `cliphist`, `matugen` and `dart-sass`, which ship through the tier repositories);
   - the stubs and build tools in the tier repositories (`stage0-bootstrap`,
     `athanor-antigravity`, `athanor-astro-toolchain`, `athanor-cargo-tools`,
     `athanor-ide-bootstrap`, `athanor-qa`, `athanor-bpf-linker`, `athanor-syft`,
     `athanor-cosign`) and `athanor-doctor`.
2. **Replace with Fedora packages:** `athanor-bat`, `buildah` and `osbuild`.
3. **Make real:**
   - `athanor-bus-api`: a specification, a closed PID-reuse window, and nothing that only
     retired crates use;
   - `athanor-tetragon` (TG1);
   - `athanor-keylime` (installed and disabled, A2-10c);
   - `athanor-base-config`, `athanor-system-config` and `athanor-selinux`.

   `athanor-semantic-db` stays out of the image until doc_local_ai item AI8.

4. **Ermete-era components that exist only on `main` stay deleted.** For each of them the
   reports give the reason.
5. **`doc_forge_development_guide.md` stays.** It is the live forge guide on `iso-v0`. This
   amends the dead-documents list of A2-10.
6. **A rewrite of any retired component starts from an approved specification, not from the
   retired code.**

## Consequences

- **Record status.** This record amends the dead-documents list of A2-10
  (`doc_forge_development_guide.md` stays).
- **Delivery.** The work lands as W1 of the Audit 3 program, in two waves:
  1. the crates and package directories that the image manifest does not list;
  2. the stubs that ship through the tier repositories, with their manifest entries and the
     `Requires:` that name them. This wave follows the rework of #169, which applies A2-10.
- **Same pull request as each deletion.** `components.toml`, the workspace members, the
  verifier and every document that names a retired component change with it.
- **Specification changes.** doc_software decision 5, which keeps the storage of
  `athanor-store` for the fleet, is withdrawn: doc_fleet (A2-12) will specify its own
  storage. doc_kernel_profile D5 already retires `athanor-ebpf-sched`, and `components.toml`
  will follow it.
- **New checks.** `verify.py specs` fails on a manifest entry without a package directory,
  and the DAG fails at load on the same condition.
