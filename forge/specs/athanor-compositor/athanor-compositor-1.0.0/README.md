# athanor-compositor

Athanor's own Wayland compositor on [Smithay](https://github.com/Smithay/smithay)
(`docs/architecture/doc_own_stack.md`, milestone C1; ADR-0121). It is a spike in the
workspace: no package ships it, and cosmic-comp stays the image's compositor until revision 2
of the specification accepts this one.

## Smithay

| Field | Value |
| --- | --- |
| Repository | https://github.com/Smithay/smithay |
| Revision | `023b78b1c5a05ca512767cacc07efca55fae8be4` |
| Date | 2026-10-15 |

The revision is pinned in `Cargo.toml`; `deny.toml` admits this repository and no other git
source. A new revision changes both lines and this table in one commit.

## Running

Task 1 runs nested, on Smithay's winit backend, inside a parent Wayland compositor:

```
athanor-compositor --scale 1.5
```

It opens one output the size of its window, at the given scale (1 by default), listens on the
next free `wayland-N` socket and logs that name. In the shell rig, `RIG_COMPOSITOR=athanor`
runs it in place of cosmic-comp (`forge/test/shell/session.sh`).
