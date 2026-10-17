# Spike C1-P: the Wayland protocols the shell needs from the own compositor

Source-reading spike for task 4 of `docs/superpowers/plans/2026-10-17-spike-c1-smithay-compositor.md`,
written on 2026-10-17 against `origin/iso-v0` at `322d968a`. The question: which Wayland
protocols does the shell bind today, which of them are COSMIC's own, what replaces each COSMIC
protocol (OS5 of `doc_own_stack.md`), whether Smithay has a handler for each protocol at the pinned
revision, and which protocols OS4 withholds from clients that do not match the allow-list. This is
the paper half of the task: nothing here ran, and no code was built. Section 6 holds the
proposals and open questions: sections 1 to 5 are findings, except the last column of section 3,
which is marked as a proposal.

## 1. Inputs

| Input | Reference |
| --- | --- |
| Shell, as of `322d968a` | `system/athanor-compositor-client`, `system/athanor-toolkit`, `system/athanor-apps`, `system/athanor-style`, and the crates under `forge/specs/athanor-{bar,dock,launcher,control-center,greeter-ui,layout-chooser,shelld}`; references below are repository-relative |
| Vendored COSMIC descriptions | `system/athanor-compositor-client/protocols/*.xml`, generated at build time by `wayland-scanner` (`src/protocols.rs`) |
| Smithay | rev `023b78b1c5a05ca512767cacc07efca55fae8be4` (2026-10-15), read in the local clone `/var/tmp/ref/smithay` with `git grep` and `git show` at that revision; references are `smithay:<path>:<line>` |
| The own compositor today | `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src` (task 1 is done, tasks 2 to 10 are not); references are `compositor:<file>:<line>` |
| Privilege model today | `doc_bar.md` section 1 (spike P4), `doc_lock_and_prompts.md`, `doc_languages.md`, `doc_overview.md`, `doc_settings.md`: cosmic-comp 1.8 source as those documents quote it. cosmic-comp itself was not re-read |

Every shell crate that talks to the compositor does so in one of five ways. The GTK4 crates get the
standard globals from GDK and `gtk4-layer-shell`, and bind the COSMIC and extension globals through
`athanor-compositor-client`, which shares GDK's `wl_display` (`connection.rs:135-146`). The toolkit
(`athanor-toolkit`) is a plain `wayland-client` and smithay-client-toolkit client. `athanor-shelld`
and `athanor-layout-chooser` bind nothing of their own. Nothing in the repository binds
`ext-session-lock`, `ext-idle-notify`, `zwp_input_method`, `zwp_virtual_keyboard`, data control or a
`wlr` output protocol: `rg` for each finds no use outside the compositor's own crate and the
documents. Those are planned consumers (section 2.3).

## 2. What each crate binds

### 2.1 `athanor-compositor-client` (the shared crate)

All paths are under `system/athanor-compositor-client/src`. "Ver." is the range passed to `bind`
(`connection.rs:745-758`); a global the compositor does not offer is logged and left empty, and its
actions return `Error::Unavailable`.

| Global | Ver. | Used for | Where |
| --- | --- | --- | --- |
| `wl_output` | min(offered, 4) | connector name from the `name` event, so a window's outputs match GDK's monitors | `connection.rs:737-742, 771, 875` |
| `wl_seat` | 1 to 7 | `activate` takes the seat; `get_keyboard` for the keymap and the layout object | `connection.rs:756, 233-236, 897-899` |
| `wl_keyboard` | from the seat | `keymap` event, parsed for the layout names | `connection.rs:919-944` |
| `wl_shm` | 1 | buffers for window pictures | `connection.rs:753`, `capture.rs:172` |
| `ext_foreign_toplevel_list_v1` | 1 | window list: `toplevel`, then `title`, `app_id`, `done`, `closed` | `connection.rs:760, 995-1086` |
| `ext_workspace_manager_v1` | 1 | workspaces: `workspace_group`, `workspace`, `done`; request `commit` after a COSMIC request | `connection.rs:757, 1146-1195, 272` |
| (group handle) | | `output_enter`, `output_leave`, `workspace_enter`, `workspace_leave`, `removed` | `connection.rs:1197-1236` |
| (workspace handle) | | `name`, `state` (the `active` bit), `removed` | `connection.rs:1238-1274` |
| `ext_foreign_toplevel_image_capture_source_manager_v1` | 1 | `create_source` from the foreign-toplevel handle | `connection.rs:754`, `capture.rs:173, 182` |
| `ext_image_copy_capture_manager_v1` | 1 | `create_session` (no options), session events `buffer_size`, `shm_format`, `done`, `stopped`; `create_frame`, `attach_buffer`, `damage_buffer`, `capture`; frame events `ready`, `failed` | `connection.rs:755`, `capture.rs:176-183, 238-317` |
| `wp_security_context_manager_v1` | 1 | `create_listener`, `set_sandbox_engine` (`os.athanor.shell`), `set_app_id`, `set_instance_id`, `commit`, `destroy`: one context per launched application | `connection.rs:752, 69, 322-340`, `launch.rs:436` |
| `xdg_activation_v1` | GDK's | a token from `app_launch_context().startup_notify_id`, passed to launched applications and to notification actions. Bound by GDK, not by this crate | `connection.rs:342-358` |
| `zcosmic_toplevel_info_v1` | 2 to 3 | section 3 | `connection.rs:746` |
| `zcosmic_toplevel_manager_v1` | 1 to 4 | section 3 | `connection.rs:747` |
| `zcosmic_workspace_manager_v2` | 2 | section 3 | `connection.rs:748` |
| `zcosmic_keyboard_layout_manager_v1` | 1 | section 3 | `connection.rs:749` |
| `cosmic_a11y_manager_v1` | 2 | section 3 | `connection.rs:751` |

`outputs.rs` and `theme.rs` read GDK's monitors and the `cosmic-config` files; they bind nothing.

### 2.2 The surfaces

| Crate | Globals, through | Where |
| --- | --- | --- |
| `athanor-bar` | `zwlr_layer_shell_v1` (layer `Top`, anchors, `auto_exclusive_zone_enable`, keyboard mode `OnDemand`, namespace `athanor-bar`, one surface per monitor) through `gtk4-layer-shell`. Its notification popups: layer `Top`, keyboard `None`, namespace `athanor-notifications`. The compositor client for windows, workspaces, tiling, layouts, accessibility and activation tokens | `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs:186, 458-474, 532`; `ui/popups.rs:40-50`; `ui/tiling.rs:36, 82`; `ui/input.rs:35, 66`; `ui/accessibility.rs:138, 243, 270`; `ui/notifications.rs:526, 701-703`; `layer_guard.rs:27-40` |
| `athanor-dock` | `zwlr_layer_shell_v1` (layer `Top`, `OnDemand`, namespace `athanor-dock`, exclusive zone 0 or automatic). Compositor client for windows, `activate`, `minimize`, `unminimize` and `close`, through `athanor-apps` | `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/mod.rs:54, 153`; `src/ui/surface.rs:146-154, 226, 255-257`; `system/athanor-apps/src/row.rs:127-135, 256, 336, 499-504, 555-560` |
| `athanor-launcher` | `zwlr_layer_shell_v1` (namespace `athanor-launcher`; `set_layer` between `Background` and `Overlay` while mapped, keyboard `None` to `Exclusive`). Compositor client for windows, `activate` and `capture` | `forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/surface.rs:37-42, 62-84`; `src/ui/mod.rs:90, 325`; `src/ui/actions.rs:69, 228` |
| `athanor-control-center` | `zwlr_layer_shell_v1` (namespace `athanor-control-center`; the same layer switching; `OnDemand` after it has the keyboard). Compositor client for the window list (focused output) and `launch` | `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/surface.rs:37-42, 69-84, 104`; `src/ui.rs:63, 221-233, 284-300` |
| `athanor-greeter-ui` | `zwlr_layer_shell_v1` (layer `Overlay`, keyboard `Exclusive`, all four anchors, namespace `greeter`). No compositor client | `forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/ui.rs:529-538`; `layer_guard.rs:12-18` |
| `athanor-layout-chooser` | an ordinary `xdg_toplevel` through GTK (`ApplicationWindow`). No layer shell, no compositor client | `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/src/ui.rs:74` |
| `athanor-shelld` | none: D-Bus only | no match for `wayland` in its `Cargo.toml` or sources |
| `athanor-apps`, `athanor-style` | `gtk4-layer-shell` calls on windows of the surfaces above: the monitor geometry, and re-anchoring and exclusive zones when the layout changes | `system/athanor-apps/src/timing.rs:15, 133`; `system/athanor-style/src/appearance_engine.rs:509-553` |

The `layer_guard` of each surface refuses to start unless `gtk4_layer_shell::is_supported()`, which
needs `zwlr_layer_shell_v1`. Features that set a lower bound on the version the compositor must
offer, from `wlr-layer-shell-unstable-v1.xml` (cargo registry, `wayland-protocols-wlr` 0.3.12):
`set_layer` on a mapped surface needs version 2 (launcher and control center), and keyboard mode
`on_demand` needs version 4 (bar, dock, control center). Smithay offers version 5
(`smithay:src/wayland/shell/wlr_layer/mod.rs:232-233`).

### 2.3 The toolkit and what GTK adds

| Client | Globals | Where |
| --- | --- | --- |
| `athanor-toolkit` driver (smithay-client-toolkit) | `wl_compositor`, `xdg_wm_base` with `xdg_toplevel` and server-side decoration requested, `wl_shm`, `wl_seat` with keyboard and pointer, `wl_output`, and the registry | `system/athanor-toolkit/src/driver.rs:51-55, 289-293, 321-460, 478-487` |
| GTK4 and GDK, for every GTK surface above | Not read in this repository. GDK's Wayland backend binds, as far as this author knows, `wl_compositor`, `wl_subcompositor`, `xdg_wm_base`, `wl_shm`, `wl_seat`, `wl_output`, `wl_data_device_manager`, `zwp_text_input_manager_v3`, `wp_viewporter`, `wp_fractional_scale_manager_v1`, `wp_cursor_shape_manager_v1`, `xdg_activation_v1`, `zwp_linux_dmabuf_v1`, `zxdg_output_manager_v1` and, when offered, more. **Unverified**: the task 4 check reads the real list with `WAYLAND_DEBUG=1` from a GTK client (section 6, Q1) | |
| Planned consumers, from the documents and not from code | `athanor-idle`: `ext_idle_notifier_v1`, `wp_single_pixel_buffer_v1`, `zwlr_output_power_v1`, a layer surface (`doc_session_daemons.md`, SD4). `athanor-lock`: `ext_session_lock_manager_v1` on the main socket (`doc_lock_and_prompts.md`, LP3). `athanor-ime` and `athanor-osk`: `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1` (`doc_languages.md`, LN12). The portal backend: `ext_image_copy_capture_manager_v1` with output and toplevel sources (`doc_portal.md`). Settings, output arrangement: an output-management protocol, not named in the documents (`doc_settings.md:337`, SE-S2, is a spike on which attributes cosmic-comp 1.8.0's output-configuration protocols carry; that no protocol is chosen is this spike's inference) | |

## 3. The COSMIC protocols

The "Used" and "Not used" columns are findings. The last column is a **proposal**: task 4 of the plan
records the requirement only, and OS5 of `doc_own_stack.md` requires a private protocol to be documented
there before it is written. The names and shapes below are not decided (section 6, Q8).

The shell uses a small part of each protocol. "Not used" lists requests and events that the vendored
description carries and `connection.rs` never sends or reads (the generated bindings are
`#[allow(dead_code)]`).

| Protocol | Used | Not used | Replacement (proposal; waits on OS5) |
| --- | --- | --- | --- |
| `zcosmic_toplevel_info_v1` (2 to 3) | `get_cosmic_toplevel(ext handle)` (`connection.rs:1014`); manager event `done`, which commits the pending window (`:1097`); handle events `state` (array of `maximized`, `minimized`, `activated`, `fullscreen`; `model.rs:24-37`), `output_enter`, `output_leave` (`:1120-1141`). Readers: `activated` by the bar, launcher and control center to find the focused output (`notifications.rs:703`, `launcher ui/mod.rs:325`, `control-center ui.rs:232`), `activated` and `minimized` by the dock (`row.rs:133-134`); `outputs` by the same three | handle events `title`, `app_id`, `closed` and `done` (all deprecated since 2 and carried by `ext_foreign_toplevel_handle_v1`), `geometry`, `workspace_enter`, `workspace_leave`, `ext_workspace_enter`, `ext_workspace_leave`; the manager event `finished`; `maximized` and `fullscreen` are decoded and read by no crate outside the client | **No standard protocol covers state or outputs.** `ext-foreign-toplevel-list-v1` has identity, `title`, `app_id`, `done` and `closed` only. A private protocol, `athanor_toplevel_state_v1` (name to settle), keyed by the ext handle: event `state` (`activated`, `minimized`; `maximized` and `fullscreen` only if a reader appears), events `output_enter` and `output_leave` (`wl_output`), event `done`. See Q2 for the alternative, `wlr-foreign-toplevel-management-v1` |
| `zcosmic_toplevel_manager_v1` (1 to 4) | `activate(handle, seat)` (`connection.rs:236`), `set_minimized` (`:242`), `unset_minimized` (`:246`), `close` (`:250`). The capabilities event is ignored (`:1314-1320`) | `set_maximized`, `unset_maximized`, `set_fullscreen`, `unset_fullscreen`, `set_rectangle`, `move_to_workspace`, `move_to_ext_workspace`, `set_sticky`, `unset_sticky` | The same private protocol, as a manager interface: `activate(toplevel, seat)`, `set_minimized`, `unset_minimized`, `close`. `ext-foreign-toplevel-list-v1` has no requests. `xdg_activation_v1` is not a substitute: it hands focus to the app that asks, not to a window a third party names |
| `zcosmic_workspace_manager_v2` (2) | `get_cosmic_workspace(ext workspace)` (`connection.rs:1167`), handle event `tiling_state` (`floating_only`, `tiling_enabled`; `:1285-1293`), handle request `set_tiling_state` followed by the standard `ext_workspace_manager_v1.commit` (`:268-272`). Reader and writer: the bar's tiling toggle (`ui/tiling.rs:36, 82`) | handle events `capabilities`, `state`; requests `rename`, `move_before`, `move_after`, `pin`, `unpin` | **Names, active state and group outputs are standard**: `ext-workspace-v1`, already bound (section 2.1). Only tiling is COSMIC's, and there is no standard for it. A private `athanor_workspace_tiling_v1`: `get_workspace_tiling(ext workspace)`, event `tiling_state`, request `set_tiling_state` |
| `zcosmic_keyboard_layout_manager_v1` (1) | `get_keyboard_layout(wl_keyboard)` (`connection.rs:899`), event `group` (`:956`), request `set_group` (`:284`). The layout names come from the standard `wl_keyboard.keymap` (`:919-944`). Reader and writer: the bar's input-source module (`ui/input.rs:35, 66`) | none (the protocol has this and `destroy`) | No standard protocol. A private `athanor_keyboard_group_v1` with the same shape: `get_keyboard_group(wl_keyboard)`, event `group(u32)`, request `set_group(u32)`. The compositor ignores a group beyond the configured layouts (`connection.rs:277`) |
| `cosmic_a11y_manager_v1` (2) | events `magnifier(active)` and `screen_filter(inverted, filter)` (`connection.rs:976-987`); requests `set_magnifier` (`:291`) and `set_screen_filter` (`:312`); `filter` is `disabled`, `greyscale`, `daltonize_protanopia`, `daltonize_deuteranopia` or `daltonize_tritanopia`. Reader and writer: the bar's accessibility module (`ui/accessibility.rs:138, 243, 270`) | version 3 (`screen_filter2`, `set_screen_filter2`), not bound: the code binds exactly version 2 (`connection.rs:750-751`, `2..=2`). **Divergence**: the approved `doc_accessibility.md:158` (AX10, revision 2) says the client binds "up to version 3" | No standard protocol. A private `athanor_a11y_v1`, the same events and requests. The magnifier and the filters are drawing the compositor itself must do (OS5, third bullet); the protocol is only the switch |

Also read at the source: a client that binds `zcosmic_toplevel_info_v1` at version 2 or later never
receives its own `toplevel` event (`connection.rs:1103-1107`), and
`zcosmic_workspace_manager_v1` is generated only because the toplevel descriptions reference it
(`protocols.rs:38-39`).

## 4. Smithay's coverage and the own compositor today

"Offered" is the state at `322d968a`: `compositor:state.rs:65-93` (registration) and `handlers.rs:292-328` (the xdg-activation handler). "Task" is
the task of the C1 plan in which the protocol lands, or the reason none is planned.

| Protocol | Smithay module (global version) | Offered | Task |
| --- | --- | --- | --- |
| `wl_compositor`, `wl_subcompositor` | `wayland::compositor` (5 through `CompositorState::new`, `mod.rs:696-700`; `new_v6` offers 6; and 1 for the sub-compositor, `mod.rs:722`) | yes (`state.rs:65`) | 1 |
| `xdg_wm_base` | `wayland::shell::xdg` (7; `mod.rs:1250`) | yes (`state.rs:66`) | 1 |
| `wl_shm` | `wayland::shm` (2; `mod.rs:158`) | yes (`state.rs:67`) | 1 |
| `wl_seat` | `wayland::seat` (9; `mod.rs:188`) | yes (`state.rs:78`) | 1 |
| `wl_output`, `zxdg_output_manager_v1` | `wayland::output` (4 and 3; `mod.rs:135, 211`) | yes (`state.rs:69, 93`) | 1 |
| `wp_viewporter`, `wp_fractional_scale_manager_v1`, `wp_presentation`, `wp_cursor_shape_manager_v1` | `wayland::viewporter`, `fractional_scale`, `presentation`, `cursor_shape` | yes (`state.rs:70-73`) | 1 |
| `zwp_linux_dmabuf_v1` | `wayland::dmabuf` | yes, version 3 (`state.rs:159-164`) | 1; version 4 and `wp_linux_drm_syncobj_manager_v1` (`wayland::drm_syncobj`) in 2 and 8 |
| `xdg_activation_v1` | `wayland::xdg_activation` (1; `mod.rs:187`) | yes (`state.rs:74`) | 1 |
| `zwlr_layer_shell_v1` | `wayland::shell::wlr_layer` (5; `mod.rs:232`, per-client filter) | no | 3 |
| `ext_foreign_toplevel_list_v1` | `wayland::foreign_toplevel_list` (1; `mod.rs:315`, per-client filter) | no | 4 |
| `ext_workspace_manager_v1` | **missing** (`git grep -i workspace` finds a comment in `image_capture_source` and the unrelated X11 window manager in `src/xwayland/xwm/mod.rs` and `xwm/surface.rs`) | no | 4: the server side is written in the compositor, on the `ext-workspace-v1` description that `wayland-protocols` (feature `staging`) already ships |
| `ext_image_copy_capture_manager_v1` | `wayland::image_copy_capture` (1; `mod.rs:917`, per-client filter) | no | 4 |
| `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_output_image_capture_source_manager_v1` | `wayland::image_capture_source` (1 each; `mod.rs:379, 513`, per-client filter) | no | 4 (toplevel); output source is for the portal, not C1 |
| `wp_security_context_manager_v1` | `wayland::security_context` (1; `mod.rs:81`, per-client filter) | no | 4 offers it; 5 uses it |
| `ext_session_lock_manager_v1` | `wayland::session_lock` (1; `mod.rs:110`) | no | 4 |
| `ext_idle_notifier_v1` | `wayland::idle_notify` (2; `mod.rs:128`; **no per-client filter**) | no | 4 |
| `zwp_idle_inhibit_manager_v1` | `wayland::idle_inhibit` | no | not planned in C1; notifications honour inhibitors (`doc_session_daemons.md`, SD4), so it follows `ext-idle-notify` |
| `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1`, `zwlr_virtual_pointer_manager_v1` | `wayland::input_method`, `virtual_keyboard`, `virtual_pointer` (1 each; per-client filter; `virtual_pointer/mod.rs:100`) | no | not planned in C1 (Q6) |
| `wp_single_pixel_buffer_v1` | `wayland::single_pixel_buffer` (1; `mod.rs:74`) | no | not planned in C1; `athanor-idle` draws its blank with it (`doc_session_daemons.md:107`, SD4) |
| `zxdg_decoration_manager_v1` | `wayland::shell::xdg::decoration` (1; `mod.rs:153`, with a filter) | no | not planned in C1; the toolkit driver requests server decorations with `WindowDecorations::RequestServer` (`system/athanor-toolkit/src/driver.rs:55`), so without it the toolkit's windows get none |
| `zwp_text_input_manager_v3` | `wayland::text_input` | no | not planned; GTK binds it (Q1) |
| `ext_data_control_manager_v1`, `zwlr_data_control_manager_v1` | `wayland::selection::ext_data_control` (1), `wlr_data_control` (2; per-client filter) | no | not planned in C1 (Q6) |
| `wl_data_device_manager`, `zwp_primary_selection_device_manager_v1` | `wayland::selection::data_device`, `primary_selection` | no | not planned; GTK clients need the first to copy and paste (Q1) |
| `zwlr_output_power_manager_v1` | **missing** | no | no C1 task: `athanor-idle` needs it (SD4); written by the compositor (Q6) |
| `zwlr_output_manager_v1` or equivalent output management | **missing** | no | no C1 task: Settings needs it (SE-S2); no protocol is named in the documents (this spike's inference) |
| `wlr-foreign-toplevel-management-v1` | **missing** (an option in Q2, not required) | no | n/a |
| `zcosmic_toplevel_info_v1`, `zcosmic_toplevel_manager_v1`, `zcosmic_workspace_manager_v2`, `zcosmic_keyboard_layout_manager_v1`, `cosmic_a11y_manager_v1` | **missing, and not wanted** (nothing in Smithay names COSMIC) | no | replacement proposed in section 3 (a standard protocol where one covers it, otherwise a private one documented in `doc_own_stack.md` first, per OS5; Q8) |

Smithay supplies the server half of a protocol: parsing, object lifetime and, for the capture
protocols, the buffer handshake. It does not decide what the compositor does with a request. Every
row from `ext_foreign_toplevel_list_v1` to `ext_session_lock_manager_v1` still needs a handler in the
compositor (a window registry, a frame renderer for capture, the lock's blanking and focus rules).
That is the work of task 4, and none of it exists yet.

## 5. Which protocols are privileged

OS4 lists the privileged set; the right-hand column records how cosmic-comp 1.8 treats the same
protocol, as the documents quote it (`doc_bar.md` section 1, P4).

| Protocol | Privileged under OS4 | Smithay can gate it | cosmic-comp 1.8 (`client_not_sandboxed`) |
| --- | --- | --- | --- |
| `zwlr_layer_shell_v1` | the `top` and `overlay` layers only | per-client filter on the global; a layer-level check is the compositor's own (Q3) | the whole global is withheld from sandboxed clients |
| `ext_foreign_toplevel_list_v1` | yes | filter | withheld (`doc_overview.md`, "the main Wayland socket") |
| `ext_image_copy_capture_manager_v1` | yes | filter | withheld |
| the two image-capture source managers | **inference**: OS4 names only `ext-image-copy-capture`; a source manager has no use without it | filter | withheld (`doc_portal.md` lists them with the copy manager; the filter is not quoted) |
| `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1` | yes | filter | withheld (`doc_languages.md`) |
| `ext_data_control_manager_v1`, `zwlr_data_control_manager_v1` | yes | filter | withheld |
| `ext_session_lock_manager_v1` | yes | filter on the global | withheld (cosmic-comp `src/state.rs:699-700`, per `doc_lock_and_prompts.md`) |
| The private protocols of section 3 | yes, "the compositor's private interface" | filter (the compositor creates the globals) | documented only for the toplevel manager and workspaces (`doc_bar.md:9`, `doc_overview.md:91`) and the keyboard layout (`doc_languages.md:61`); not recorded for the a11y manager |
| `ext_workspace_manager_v1` | not listed in OS4 | no filter exists in the module (missing) | withheld |
| `zwlr_virtual_pointer_manager_v1` | yes (OS4 names the virtual pointer) | filter | not recorded in the documents |
| `wp_security_context_manager_v1` | not listed in OS4 | filter | exported (`doc_lock_and_prompts.md`); its filter is not recorded; the shell binds it as an unconfined client |
| `ext_idle_notifier_v1` | not listed in OS4 | **no filter** in the module | exported (`doc_lock_and_prompts.md`); its filter is not recorded |
| `xdg_wm_base`, `wl_seat` and the core globals, text input, selection (non-data-control) | no | n/a | offered |

How the models differ, from the documents:

- **What decides.** cosmic-comp decides by one bit, whether the connection came through a
  `wp_security_context_v1`. The set of clients that is not sandboxed sees all 53 globals of the main
  socket; one behind a context sees 31, whatever the engine id, except the engine id
  `com.system76.CosmicPanel`, which sees 51 (`doc_bar.md` section 1). OS4 replaces that bit, for
  everything on the list, with the executable under `/usr` and the systemd unit, read from
  `SO_PEERCRED` and a pidfd at connect (task 5). The consequence OS4 names: a string the client
  supplies grants nothing.
- **Unconfined code.** Any process the user starts with the main socket is "not sandboxed" and holds
  every privileged global under cosmic-comp. Under OS4 it holds only the standard ones. OS4 does not
  claim a barrier against unconfined code that can start a unit of its own name.
- **Granularity.** cosmic-comp withholds `zwlr_layer_shell_v1` as a whole. OS4 withholds only the
  `top` and `overlay` layers, which implies that `background` and `bottom` stay open to every
  client. Smithay's filter is per global, not per layer; Q3.
- **Engine id.** The shell's launcher stamps `os.athanor.shell` as the sandbox engine
  (`connection.rs:69, 333`). Under cosmic-comp it makes no difference. Under OS4 it also makes no
  difference to privilege: a client on a security-context listener gets the standard globals only.

## 6. Open questions

These are proposals and unknowns, kept apart from the findings above. None is decided.

1. **The globals GDK needs.** Section 2.3 lists GDK's globals from memory. The task 4 check should
   record the list from `WAYLAND_DEBUG=1` on a GTK4 client against cosmic-comp, and then against
   the own compositor. The compositor offers no `wl_data_device_manager`, no `zwp_text_input_manager_v3`
   and no `zwp_primary_selection_device_manager_v1` today (`compositor-c2`, C1 task 2, not merged at
   `322d968a`, adds `wl_data_device_manager`). Smithay has all three. Copy and paste in a GTK
   surface needs the first. The text-input global is the opposite case: the approved
   `doc_languages.md:69-72` records that GTK 4.20's Wayland input-method module needs
   `zwp_text_input_manager_v3` and, with no input method bound, drops dead keys and Compose, which is
   why the image sets `GTK_IM_MODULE=simple`. Offering text-input-v3 before an input method exists
   would therefore break dead keys, and its absence today is the safe case. It should be added only
   together with the input-method path and the `GTK_IM_MODULE` setting.
2. **Window state and outputs: private protocol or `wlr-foreign-toplevel-management-v1`.** The
   latter carries `activate`, `close`, `set_minimized`, `unset_minimized`, state and
   `output_enter` and `output_leave` in one interface, and is widely implemented. Smithay has no
   handler, so the server side is written either way. Proposed: the private protocol, keyed by the
   ext handle the client already holds (`toplevel.ext`, which `capture.rs:182` also needs), so that
   a window keeps one identity and `title` and `app_id` are not carried twice. The cost is a
   description and a generated binding of our own, as for the COSMIC ones today.
3. **Layer-level privilege.** A client holding the layer-shell global can ask for any layer. To keep
   OS4's rule that only `top` and `overlay` are privileged, the compositor can answer
   an unprivileged client's request for those layers with `closed`, or with a protocol error. A check
   in `get_layer_surface` alone is bypassable: the launcher and the control center call `set_layer`
   on existing surfaces (`athanor-launcher .../src/surface.rs:66, 77`; `athanor-control-center
   .../src/surface.rs:69, 80`; `set_layer` is protocol version 2), so a client could create on
   `background` and then call `set_layer(overlay)`. The check must therefore cover `set_layer` as well,
   or run at commit on the surface's current layer. Whether the answer is `closed` or an error (which
   disconnects the client) is a choice the fixture of task 5 should test, with the `wayland-info`
   check the plan already names.
4. **The security-context manager.** The shell binds it as an unconfined client. A client behind a
   context that could bind it too could mint contexts with an engine id and an app id of its
   choosing. They would still be sandboxed, so not a privilege gain, but OS4 does not list it.
   Proposed: withhold the manager from clients that connected through a context.
5. **Version of the cosmic-comp globals.** The shell binds `zcosmic_toplevel_manager_v1` up to 4,
   `zcosmic_toplevel_info_v1` up to 3. The replacements are ours, so version 1 of each is enough;
   the client crate keeps its COSMIC code until the switch (OS5, SH2) and gains the private
   binding beside it, chosen by what the registry offers. Proposed order inside `Client::connect`:
   the private global first, then the COSMIC one.
6. **Consumers with no C1 task.** `athanor-idle` needs `zwlr_output_power_manager_v1` (missing in
   Smithay), Settings needs an output-management protocol (missing; none is named in the documents), the input-method
   and virtual-keyboard clients need their globals and the OS4 gate, and data control has no
   consumer in the shell. None is in tasks 3 to 7. Proposed: task 5's fixture asserts that the
   Smithay-available privileged globals (input method, virtual keyboard, virtual pointer, data control) are
   withheld from a non-matching client even before a consumer exists, and the missing ones are
   listed as inputs to revision 2.
7. **Capture while locked.** OS4 forbids capture while the session is locked. The capture handler
   must refuse a frame when the lock is active; the fixture of task 4 should assert it. Not
   observable from the shell crates.
8. **The private protocols wait on OS5.** Section 3 proposes four private interfaces: window state and
   management, workspace tiling, keyboard group and accessibility. OS5 requires each to be documented
   in `doc_own_stack.md` before it is written, and task 4 of the plan only records the requirement. The
   names, the shapes and whether they are one protocol or four are for that revision to settle; this
   spike supplies the list of requests and events the shell uses.
