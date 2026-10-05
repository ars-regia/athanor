<div align="center">
  <br />
  <img src="https://raw.githubusercontent.com/hr-mes/athanor/main/.github/logo.png" alt="Athanor OS Logo" width="150"/>
  <h1>🌋 Athanor OS - The Ultimate Cloud-Native Desktop</h1>
  <h3>The Immutable OS, Zero-Trust, Asynchronous Operating Systems.</h3>
  <br />
  
  [![Build Status](https://img.shields.io/badge/Build-Passing-brightgreen?style=for-the-badge&logo=githubactions)](#)
  [![SLSA Level 4](https://img.shields.io/badge/SLSA-Level_4-purple?style=for-the-badge&logo=slsa)](#)
  [![Rust](https://img.shields.io/badge/Rust-1.80+-orange?style=for-the-badge&logo=rust)](#)
  [![GTK4](https://img.shields.io/badge/GTK-4.14_Vulkan-blue?style=for-the-badge&logo=gtk)](#)
  [![Memory](https://img.shields.io/badge/Allocator-Mimalloc-yellow?style=for-the-badge)](#)
  [![Architecture](https://img.shields.io/badge/Architecture-x86__64%20%7C%20ARM64-lightgrey?style=for-the-badge)](#)
  [![PQC](https://img.shields.io/badge/PQC-Dilithium5%20%7C%20ML--KEM-red?style=for-the-badge)](#)
</div>

<hr />

## 📖 Encyclopedic Architectural Index

### 📚 Deep-Dive Technical Documentation
Explore the detailed architectural specifications (generated and maintained by our AI swarm):
- [**Kernel Layer & Boot Sequence**](docs/architecture/doc_kernel_layer.md)
- [**Core Daemons, Security & IPC**](docs/architecture/doc_core_daemons.md)
- [**Desktop Shell: Direction, Layout Model, Stage 1**](docs/architecture/doc_shell.md)
- [**Athanor Cloud Mesh & Sync**](docs/architecture/doc_cloud_mesh.md)
- [**Build System & CI/CD Pipeline**](docs/architecture/doc_build_system.md)
- [**Athanor OS v3.0 Singularity Architecture**](docs/architecture/athanor_singularity_architecture_v3.md)
- [**System Subsystem Architecture**](system/README.md)

### Quick Chapters
1. [The Athanor Paradigm: Beyond Big-Tech](#1-the-athanor-paradigm-beyond-big-tech)

## Audience and support window

- **Audience:** desktops and laptops with UEFI and a CPU that has the x86-64-v3 instruction set: AVX2, BMI1, BMI2, FMA, MOVBE, F16C and LZCNT. Machines without AVX2, such as Celeron and Pentium parts of the Intel N5100 class, are excluded.
- **Base:** Athanor follows the current Fedora release and moves to the next one within 90 days of its release.
- **Today:** the image is built on Fedora 43, with security updates until 2026-12-09. The next base is Fedora 45 (security updates until 2027-11-24), decided 2026-10-06: the move is prepared now on the Fedora 45 beta, and if it is not green by mid-November 2026 the image moves to Fedora 44 instead, so that it never runs on Fedora 43 after its end of life. Both dates come from the Fedora schedule ([F43](https://fedorapeople.org/groups/schedule/f-43/f-43-key-tasks.html), [F45](https://fedorapeople.org/groups/schedule/f-45/f-45-key-tasks.html)) and Fedora may move them.
- **Security updates:** provided while Fedora supports the base release.
- **Reporting vulnerabilities:** see [SECURITY.md](.github/SECURITY.md). **Contributing:** see [CONTRIBUTING.md](.github/CONTRIBUTING.md).

## Architecture
For the full system architecture, please see the [Architecture Document](system/ARCHITECTURE.md).


---
*Athanor OS - Immutable, Zero-Trust, Asynchronous.*
