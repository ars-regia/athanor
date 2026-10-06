---
paths:
  - "**/*.rs"
---

# Rust

## Concurrency without panics

`panic = "abort"` is set on **both dev and release** (`Cargo.toml`). A panic
does not unwind and cannot be recovered: it ends the process. In a system daemon
that means loss of service; in `athanor-agentic-kernel` or a shell component it
means a lost user session.

- Never `.unwrap()` or `.expect()` on `RwLock` / `Mutex`: a poisoned lock takes
  down everything that depends on the daemon. Propagate with `anyhow::Result`.
- Never `.unwrap()` on `Option` / `Result` in code that runs in a daemon or a
  shell component. Tests may.
- Slice indexing and arithmetic: `release` has `overflow-checks = true`, so an
  overflow that passes silently elsewhere aborts here. Use `checked_*` /
  `saturating_*` where the input is not under your control.

## Errors

- `thiserror` for library error types, `anyhow` for propagation in binaries.
  Both are already workspace dependencies.
- An `Err` is propagated or handled, never swallowed. An empty `catch` or a
  `let _ =` on a `Result` in a security path is a defect.

## Dependencies

Versions live in `[workspace.dependencies]` in the root `Cargo.toml`. In a crate
use `name = { workspace = true }`. **Do not add a direct dependency with its own
version**: it breaks workspace alignment.

Before introducing a new crate, check that no workspace dependency already does
the same job, and ask for confirmation: `deny.toml` enforces licence and source
constraints.

## Before changing

If the symbol is shared between crates, run `codegraph_impact` before touching
it: a changed signature travels further across the workspace than it seems. For
the same change at many sites, use a structural tool such as `ast-grep`.

## Verification

`cargo test -p <crate>` on the crate you touched, then `just lint`.
