---
paths:
  - "forge/test/**"
---

# Acceptance rigs and VM tests

- Run one `forge/test/shell/rig.sh` and one dev VM at a time. Why: two at once exhaust host memory.
- Find the PIDs to signal with `pidof`. Why: `pgrep -x` matches the name truncated to 15 characters, and `pkill -f` over SSH kills the SSH shell itself.
- Convert to RGB before `getbbox()` on a Pillow difference image. Why: on RGBA it looks at alpha only.
- Divide ImageMagick 7.1.2 `compare -metric AE` results by 65535. Why: it prints them scaled.
- Look up AT-SPI buttons by their label, not their accessible name. Why: AT-SPI names them `button`.
- Install test files with `install -m 0644`, not `cp /dev/stdin`. Why: the latter creates mode 0600.
- Before deleting a test directory, grep the workflows and `rig.sh` for its `unittest discover -s` path. Why: discovery of a missing directory fails the job.
- Select the ISO test kickstart from GRUB's command line (`c`), not by editing the menu entry. Why: the full-screen editor wraps at the serial width and boots garbage.
- Use `-serial unix:...,server,nowait`, not `-serial file:`, and prefix commands typed on the serial console with spaces. Why: a file serial is write-only, and the console drops the first characters after heavy output.
- After a reboot request, wait for a new `boot_id`, never a fixed sleep. Why: a sleep races slow boots in both directions.
