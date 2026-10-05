# Contributing to Athanor OS

Thank you for your interest in contributing to Athanor OS! We are building a secure, post-quantum, AI-driven operating system. 

## Code of Conduct
This project adheres to the Contributor Covenant. By participating, you are expected to uphold this code. Please report unacceptable behavior privately to the maintainer through a GitHub security advisory (see `SECURITY.md`).

## How to Contribute
1. **Discuss Architecture First:** Before opening a PR for a major feature, please open an Issue to discuss the architectural implications. Athanor OS has a strict Zero-Trust and Panics-Free policy.
2. **Kani Formal Verification:** All Ring-0 and IPC code must be formally verified using `kani`. Run the test suite before submitting.
3. **No Unsafe Code:** Do not introduce `unsafe` blocks in Rust unless mathematically proven and strictly isolated.
4. **Sign your commits:** We enforce DCO (Developer Certificate of Origin) and SLSA L4 compliance. Ensure your commits are GPG/SSH signed.

## Patches Athanor carries need an upstream path
Athanor ships Fedora, COSMIC and Linux with as few local changes as possible. A change to
software we do not own (a kernel or driver patch, a compositor patch, a spec patch) is accepted
only with a written upstream path in the pull request: the upstream issue or merge request, or
the reason it cannot go upstream, and the condition under which Athanor drops its copy. Where
the upstream is a vendor that takes patches (for example the kCFI fixes for the NVIDIA driver
of PR #54), the patch is offered to it, not only carried. A carried patch without an offer, or
without an owner who rebases it at every bump, is not merged.

Changes to the protected paths listed in `.github/CODEOWNERS` (polkit, attestation, the
Containerfile, signing and promotion) need the maintainer's review.

## Reporting vulnerabilities
Never in a public issue or pull request: see `SECURITY.md`.

## Development Setup
Check the `docs/` folder for instructions on how to set up the `athanor-builder` heavy container for local development.
