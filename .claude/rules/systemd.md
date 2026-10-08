---
paths:
  - "**/*.service"
  - "**/*.timer"
  - "**/*.target"
  - "**/*.mount"
  - "**/*.preset"
  - "**/*.service.d/**"
  - "**/systemd/*.conf.d/**"
---

# systemd units and D-Bus services

- Account for the previous run at start, not only in `ExecStopPost=`, and test with `--kill-whom=main` and `all`. Why: `ExecStopPost=` runs in the service cgroup and dies with a cgroup-wide or oomd kill.
- A `Type=notify` service that gives up cleanly must send `READY=1` first. Why: otherwise systemd records `Result=protocol` and restarts it.
- Crash tests use `SIGKILL`. Why: a `SIGSEGV` sent with `kill` is absorbed by Rust's stack-guard handler and does not crash the binary.
- Use a nested `ConfigurationDirectory=` for writable config under `ProtectHome=read-only`. Why: `ProtectHome=` keeps `~/.config` read-only whatever Landlock grants.
- Do not rely on `ConfigurationDirectoryMode=` to fix an existing directory. Why: systemd 258 does not chmod directories that already exist.
- Split `RUNTIME_DIRECTORY` on `:` when `RuntimeDirectory=` names two directories. Why: systemd passes both, colon-separated.
- Run `systemctl reset-failed` before a crash-loop test. Why: systemd 258 keeps `NRestarts` after a give-up.
- Check `systemctl is-enabled` after enabling, and check units one at a time. Why: enabling a unit without `[Install]` is a silent no-op, and `systemctl is-active a b c` succeeds if any one is active.
- Enable units in the image with a preset (`80-athanor-*.preset`), never `systemctl enable` in a Containerfile. Why: `system/AGENTS.md`; `tetragon.service` shipped disabled that way.
- In system units, `%U` is the manager's UID (0), not a user's. Why: it silently resolves to root.
- Keep Unix socket paths under 108 bytes. Why: `sun_path` truncates longer paths.
- Do not order a mount `WantedBy=local-fs.target` after `systemd-tmpfiles-setup.service`. Why: it forms a cycle that systemd breaks by dropping the mount.
- Use `dbus-send --print-reply` or `gdbus call` when the D-Bus error name matters. Why: `busctl` hides error names.
