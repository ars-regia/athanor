---
paths:
  - "forge/specs/athanor-shelld/**"
  - "forge/specs/athanor-bar/**"
  - "forge/specs/athanor-dock/**"
  - "forge/specs/athanor-launcher/**"
  - "forge/specs/athanor-layout-chooser/**"
  - "forge/specs/athanor-greeter-ui/**"
  - "forge/specs/athanor-control-center/**"
  - "forge/specs/athanor-desktop-ui/**"
  - "forge/specs/athanor-calmo/**"
  - "forge/specs/athanor-xdg-desktop-portal-athanor/**"
  - "system/athanor-compositor-client/**"
  - "system/athanor-layout/**"
  - "system/athanor-style/**"
  - "system/athanor-portal/**"
  - "system/athanor-controls/**"
  - "system/athanor-preview/**"
  - "system/athanor-preview-render/**"
  - "system/athanor-apps/**"
---

# Shell, GTK and COSMIC

- Keep the application's root `Rc` alive for its whole lifetime; never poll instead. Why: a dropped root kills every weak-ref callback, which looks like timers and monitors that never fire.
- Compare cosmic-panel entries as parsed RON values, not text. Why: cosmic-panel rewrites them as pretty RON.
- Reconfigure layer surfaces in place. Why: cosmic-comp drops the client that destroys a surface of a departed output, or destroys and recreates or unmaps and remaps one.
- Never delete cosmic-panel config directories under a live panel. Why: it loses its config watches and keeps drawing stale state.
- Set `respect_close` on gtk4-layer-shell 1.3 surfaces that must honour `closed`. Why: otherwise the event is ignored.
- Check that a surface really is a layer surface. Why: gtk4-layer-shell falls back silently to a titled window, and PyGObject needs it in `LD_PRELOAD`.
- Run panel-resident surfaces with `GSK_RENDERER=cairo`. Why: the GL renderer costs 216-320 MB RSS per process against 41 MB PSS with cairo.
- Generate COSMIC protocol bindings from the vendored XML. Why: `cosmic-client-toolkit` and `cosmic-protocols` on crates.io are GPL-3.0-only.
- Open the COSMIC launcher and app library with `ActivateAction` and `{"Input":{"input":null}}`, not `Activate`. Why: `Activate` toggles, and the launcher ignores it for its first 100 ms.
- Pass session variables through PAM or the session script, never `Environment=` on greetd.service. Why: greetd starts the session with the PAM environment only.
- Write invisible bidirectional characters as `\u` escapes in source. Why: a literal U+202E is lost through tooling.
