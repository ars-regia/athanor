# Reference machine

The reference laptop of the shell standard (`doc_shell_standard.md`, ST4 and ST9), read on 2026-10-04.

- Model: Xiaomi Mi Notebook Pro.
- CPU: Intel Core i7-8550U @ 1.80GHz (x86-64-v3 supported).
- Memory: 7995120 kB (8 GB).
- GPUs: Intel UHD Graphics 620 (`8086:5917`, i915, card1) drives the panel; NVIDIA GeForce MX150 (`10de:1d12`, nouveau, card0) drives no output.
- Panel: `card1-eDP-1`, 1920x1080.
- Image: `ghcr.io/hr-mes/athanor-system:37188690695`, digest `sha256:dd278e0676380841c2e4971f8fc200705ad9fd2a50edbea19f1698d73aad68bb` (default image, not `-nvidia`).
- Kernel: `7.2.8-100.azoth.fc43.x86_64`.
- Power: on mains (`ADP0` online).
- Idle: COSMIC screen-off and suspend set to `None` in `~/.config/cosmic/com.system76.CosmicIdle/v1/` for the bench.
- Refresh rate: 60 Hz (`refresh_us` 16667 in every frame line of spike Q3).
- Autologin: `hr-mes` is logged in on boot, after the LUKS passphrase, so the soak survives its own reboots.
  `/etc/greetd/config.toml` is a real file (a copy of `/usr/share/athanor-system-config/greetd.toml` plus
  `[initial_session]` with `command = "/usr/bin/athanor-session"`), and `/etc/tmpfiles.d/10-athanor-greetd.conf`
  overrides the image's file of the same name without its `L+` line, which otherwise restores the symlink at
  every boot. To undo: `sudo rm /etc/tmpfiles.d/10-athanor-greetd.conf /etc/greetd/config.toml && sudo systemd-tmpfiles --create`.
