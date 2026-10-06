---
id: W1-FILES
title: "doc_files decisions (wave 1)"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, files]
---

# 0006. doc_files decisions (wave 1)

## Context

Recorded in the maintainer decision log, section 'Wave 1 maintainer decisions', under the heading doc_files. The options are lettered as in the specification's own question list; the specification lists the alternatives.

## Decision

- 1 a Landlock now, filtered bus with the launch broker (wave 3)
- 2 a operations in the window process with crash journal
- 3 b pure Rust, no RAR in release 1
- 4 b keep contained symlinks, never follow
- 5 a own thumbnail runner
- 6 a no network locations now, own confined mounts later
- 7 a name + localsearch index
- 8 b create .zip and .tar.xz
- 9 a inherit the file manager socket now, own context with the launch broker
- 10 b helper parses and streams, the window writes (amends doc_software decision 7 wording)

## Consequences

Elaborated in `docs/architecture/doc_files.md` (on shell-specs).
