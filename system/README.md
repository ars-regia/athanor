<div align="center">
  <h1>Athanor OS — System Services Architecture & OCI Layering Strategy</h1>
  <p><b>An immutable, cloud-native, rolling-release desktop operating system runtime engine.</b></p>
  <p>📚 <b><a href="../docs/architecture/doc_kernel_layer.md">Kernel Layer & Boot</a></b> | <b><a href="../docs/architecture/doc_core_daemons.md">Core Daemons & Security</a></b> | <b><a href="ARCHITECTURE.md">System Architecture</a></b></p>
</div>

---

The `system` sub-project is the assembly pipeline and runtime engine of Athanor OS. It ingests output artifacts from the `forge` (Custom RPMs) and `kernel` (Chimera Kernel) sub-projects, integrating core system daemons on top of the **Fedora Atomic 43** base image to produce the `athanor-system` bootable OCI container.

---

## 1. System Core Daemons

The `system/` directory hosts the core daemons managing system execution, hardware security, network encryption, and containerized app management:

```
+-----------------------------------------------------------------------------------+
|               FLATPAK DECLARATIVE ORCHESTRATOR                                    |
|                        system/athanor-store                                        |
|         - SLSA Level 4 OCI App Management & Cosign Signature Verification        |
+-----------------------------------------------------------------------------------+
|               MESH PQC (POST-QUANTUM CRYPTOGRAPHY BUS)                            |
|                        system/athanor-mesh-bus                                     |
|         - Dilithium5 / ML-DSA Signatures & ML-KEM / Kyber1024 WireGuard P2P       |
+-----------------------------------------------------------------------------------+
|               MICRO-HYPERVISOR ENCLAVE ORCHESTRATOR                               |
|                        system/athanor-hypervisor-daemon                            |
|         - Hardware Confidential Computing: AMD SEV-SNP & Intel TDX Enclaves       |
+-----------------------------------------------------------------------------------+
|               KERNEL AI SCHEDULER                                                 |
|                        system/athanor-ebpf-sched                                   |
|         - Ring-0 sys_execve eBPF Tracepoints, Static Rule Engine AI Prediction, sched_ext        |
+-----------------------------------------------------------------------------------+
```

### 1. Kernel AI Scheduler ([`system/athanor-ebpf-sched`](file:///var/home/athanor/GEMINI/athanor/system/athanor-ebpf-sched))
- **Role:** eBPF Kernel Scheduler Bridge.
- **Implementation:** Intercepts `sys_execve` process creation events via eBPF, passes process metadata to `Static Rules Engine` on the Static Rule Engine, and configures Ring-0 `sched_ext` task latency slice targets (100μs to 20ms) and cgroup v2 `cpu.weight`.

### 2. Micro-Hypervisor Enclave ([`system/athanor-hypervisor-daemon`](file:///var/home/athanor/GEMINI/athanor/system/athanor-hypervisor-daemon))
- **Role:** Hardware Confidential Enclave Daemon.
- **Implementation:** Uses KVM, `vmm-sys-util`, AMD SEV-SNP, and Intel TDX extensions to launch and verify cryptographically sealed hardware enclaves for isolated execution of security workloads.

### 3. Mesh PQC ([`system/athanor-mesh-bus`](file:///var/home/athanor/GEMINI/athanor/system/athanor-mesh-bus))
- **Role:** Post-Quantum Cryptography P2P WireGuard Mesh Bus.
- **Implementation:** Implements key exchange (**ML-KEM-1024 / Kyber1024**) and digital signatures (**Dilithium5 / ML-DSA-87**) over peer-to-peer WireGuard tunnels and ZBus IPC.

### 4. Flatpak Declarative Orchestrator ([`system/athanor-store`](file:///var/home/athanor/GEMINI/athanor/system/athanor-store))
- **Role:** Declarative App Sandbox & OCI Container Engine.
- **Implementation:** Uses strict SLSA Level 4 OCI container images verified with **Cosign** signatures before installation.

---

## 2. Native Rust Core Services

In addition to core system daemons, `system/` hosts native Pure Rust subsystems replacing legacy C components:

1. **`Systemd Monitor` ([`system/Systemd Monitor`](file:///var/home/athanor/GEMINI/athanor/system/Systemd Monitor))**: Asynchronous Tokio & Zbus systemd init supervisor that monitors unit lifecycle and auto-heals failing services.
2. **`PipeWire/WirePlumber` ([`system/PipeWire/WirePlumber`](file:///var/home/athanor/GEMINI/athanor/system/PipeWire/WirePlumber))**: Pure Rust real-time PipeWire session manager and audio stream router.
3. **`athanor-greeter` ([`system/athanor-greeter`](file:///var/home/athanor/GEMINI/athanor/system/athanor-greeter))**: Zero-Trust TPM 2.0 key release & hardware attestation display manager with `ZeroizeOnDrop` memory protection.
4. **`xdg-desktop-portal-athanor` ([`forge/specs/athanor-xdg-desktop-portal-athanor`](file:///var/home/athanor/GEMINI/athanor/forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0))**: Native Rust Zbus 4.4 async desktop portal implementation for SLSA Level 4 Flatpak sandboxes.

---

## 3. Architecture: The 4-Tier Pyramid OCI Layering Strategy

Athanor OS structures the OS into **4 sequential layers** inside `Containerfile` to achieve OCI layer caching efficiency. Lower tiers require reboots to update, while higher tiers can be updated live.

```
+-------------------------------------------------------------------------+
|                  TIER 3: RUST SHELL & APPS (~8 MB)                      |
|           athanor-shell-rs, athanor-settings-rs, athanor-store-rs          |
|              [Layer 1 - Live Updateable without reboot]                 |
+-------------------------------------------------------------------------+
|                TIER 2: DESIGN SYSTEM & STATIC ASSETS (~18 MB)           |
|            Bibata Cursors, Matugen Dynamic Colors, Starship             |
+-------------------------------------------------------------------------+
|          TIER 1: DISPLAY SERVER & CORE USERSPACE SERVICES (~34 MB)      |
|     Cage Wayland Kiosk, Greetd, Niri Compositor, athanor-mesh-bus        |
+-------------------------------------------------------------------------+
|          TIER 0: HARDWARE & KERNEL FOUNDATION (~3.3 GB)                 |
|  Fedora Atomic 43 + Chimera Kernel + athanor-ebpf-sched + SEV-SNP / TDX  |
|       [Layer 0 - Reboot Required for updates]                           |
+-------------------------------------------------------------------------+
```

### Footprint Optimization (-1.1 GB Pruning)
Inside Tier 0 and final image assembly, non-consumer datacenter components are stripped to minimize final OCI image size:
- **Datacenter Firmware Removal (-400 MB)**: Purges `mellanox`, `qlogic`, `netronome`, `liquidio` datacenter network firmware while preserving consumer hardware firmware.
- **DKMS Build Tools Removal (-350 MB)**: Removes `kernel-devel`, `gcc`, and `make` after out-of-tree NVIDIA driver compilation.
- **DNF Cache Purge (-350 MB)**: Strips intermediate metadata from `/var/cache/dnf` and `/var/lib/dnf`.

---

## 4. Display Manager Greeter Subsystem

Pre-login authentication is driven by **`athanor-shell-rs --greeter`** running inside a **Wayland Kiosk `cage`** session configured in `/etc/greetd/config.toml`:
- **Dynamic User Discovery**: Inspects `/etc/passwd` to locate standard user accounts (`UID >= 1000`).
- **Glassmorphic UI**: Translucent cards, avatar frames loading `~/.face`, interactive **Caps Lock Indicator**, password reveal toggle, and integrated power menu.
- **Deployment**: Updated via `deploy-live-rust-greeter.sh` without rebooting.

---

## 5. Build Pipeline & CI/CD Security (`system-build.yml`)

The final OS image is assembled via GitHub Actions:

1. **Docker Buildx Assembly**: Constructs the image using `Containerfile`. Bypasses GitHub Action cache export (`cache-to: type=gha`) for this artifact to prevent GitHub Runner disk space issues.
2. **Push to GHCR**: The image is compressed using ZSTD (`compression=zstd,force-compression=true`) and pushed to the GitHub Container Registry.
3. **Trivy Vulnerability Scan**: The pushed image is scanned by **Aqua Security Trivy** focusing strictly on OS-level CVEs (both `os` and `library`).
4. **Cosign Cryptographic Signature**: The image is signed using **Sigstore Cosign**, verifying build authenticity.

---

## 6. Deployment & Kickstart

### In-Place Atomic Switch (For existing Fedora Atomic systems)
```bash
sudo bootc switch ghcr.io/hr-mes/athanor-system:latest
```

### Automated ISO Build via `bootc-image-builder` (For clean installs)
```bash
sudo podman run --rm -it --privileged --pull=newer \
    --security-opt label=type:unconfined_t \
    -v $(pwd)/output:/output \
    -v $(pwd)/athanor-install.ks:/config.ks \
    quay.io/centos-bootc/bootc-image-builder:latest \
    --type iso --kickstart /config.ks \
    ghcr.io/hr-mes/athanor-system:latest
```


