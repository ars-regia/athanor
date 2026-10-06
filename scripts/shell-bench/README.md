# Shell bench

Measures the bar, the dock and `athanor-shelld` on the reference machine against
`docs/architecture/doc_shell_standard.md` (ST5, ST6, ST9). The bench runs on the
desktop and drives the machine over SSH; nothing is installed on the machine.

## Preparing the reference machine

- Athanor installed from the published default image (not `-nvidia`), one user,
  logged in to the desktop, no windows open.
- Display blanking and suspend on idle off for the duration of the bench
  (COSMIC: `screen_off_time`, `suspend_on_ac_time` and `suspend_on_battery_time`
  set to `None` under `~/.config/cosmic/com.system76.CosmicIdle/v1/`).
- SSH with a key only, and a host alias on the desktop:

  ```
  Host athanor-ref
      HostName <address>
      User <user>
  ```

- Passwordless `sudo` for that user: the bench creates `uinput` devices, reads
  `bootc status` and suspends the machine in the soak. It is a test machine (ST9).
- The laptop on mains power.

The facts of the current machine are in `docs/shell-bench/machine.md`.

## Checking the connection

```
python3 -c "import sys; sys.path.insert(0, 'scripts/shell-bench'); import machine; m = machine.Machine('athanor-ref'); print(m.systemctl('is-active', *machine.UNITS).decode())"
```

## Running the bench

```
python3 scripts/shell-bench/bench.py --host athanor-ref [--stages response,idle] [--repeat 50] [--commit C] [--out DIR]
```

- `--stages`: any of `facts`, `response`, `idle`, `memory`, `recovery`, `start`,
  `scenarios`; all of them by default.
- `--repeat`: repetitions per action; a percentile needs at least 50 (ST5).
- `--display`: the session's Wayland socket, `wayland-1` by default.
- Results go to `docs/shell-bench/<date>-<commit>/` (`results.json`, `report.md`)
  unless `--out` names another directory.

## Running the soak

```
python3 scripts/shell-bench/soak.py --host athanor-ref [--hours 24] [--suspend-every 3600] --out DIR
```

It writes `soak.json` in `DIR`. Log lines the soak tolerates are listed in
`allow.txt`, one `<unit> <regex>` per line, each with a reason approved by the
maintainer (ST10).

Every change the bench makes on the machine (environment, layout file, stopped
units) is undone when a stage ends, including when it fails.
