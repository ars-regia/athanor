# Vendored protocol descriptions

The COSMIC protocol descriptions this crate generates its bindings from, with
`wayland-scanner` at build time. They are copied unchanged from
[`pop-os/cosmic-protocols`](https://github.com/pop-os/cosmic-protocols) at commit
`c0cff4db14c37ed954983158e4055aa94c7741d9` (2026-09-11), path `unstable/<name>.xml`.
`SHA256SUMS` pins their content: `sha256sum -c SHA256SUMS` in this directory.

The crates published from that repository (`cosmic-protocols`, `cosmic-client-toolkit`)
are GPL-3.0-only and this crate does not link them. Each description carries its own
licence in its `<copyright>` element:

| File                                         | Licence           | Copyright                                                                    |
| -------------------------------------------- | ----------------- | ---------------------------------------------------------------------------- |
| `cosmic-a11y-unstable-v1.xml`                | MIT               | 2025 System76                                                                |
| `cosmic-keyboard-layout-unstable-v1.xml`     | HPND-sell-variant | 2026 System76, Inc                                                           |
| `cosmic-toplevel-info-unstable-v1.xml`       | HPND-sell-variant | 2018 Ilia Bozhinov, 2020 Isaac Freund, 2024 Victoria Brekenfeld              |
| `cosmic-toplevel-management-unstable-v1.xml` | HPND-sell-variant | 2018 Ilia Bozhinov, 2020 Isaac Freund, 2022 wb9688, 2024 Victoria Brekenfeld |
| `cosmic-workspace-unstable-v1.xml`           | HPND-sell-variant | 2019 Christopher Billington, 2020 Ilia Bozhinov, 2022 Victoria Brekenfeld    |
| `cosmic-workspace-unstable-v2.xml`           | HPND-sell-variant | 2025 System76                                                                |

To move to a newer commit: download the six files from that commit, check the licence
of each, update this table, the commit above and `SHA256SUMS`, then run the client's
tests and `forge/test/shell/rig.sh compositor-e2e`.
