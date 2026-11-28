# LP18 step 3, the switch to athanor-lock and athanor-unlockd: Implementation Plan

**Status:** proposed, awaiting the maintainer's approval (ADR-0074). Switching the session's locker touches the authentication and lock path (`AGENTS.md`, "Stop and ask before editing"), so code starts once this plan and its open text prerequisites are approved.

**Goal:** Enable `athanor-lock.service` and `athanor-unlockd.socket` by preset, retire `cosmic-greeter` as the session locker from `session-components`, presets, package requirements and Sulphur's allow-list, and verify the switched session in CI, the development VM and on the reference laptop (LP18 step 3, #1592).

**Spec:** `docs/architecture/doc_lock_and_prompts.md`, revision 4, with:
- LP2, LP3, LP4, LP5, LP6, LP8, LP10, LP14, LP15, LP17, LP18 step 3;
- D30, D39, D41, D42, D43;
- section 4, spike L7b (amended 2026-11-25);
- section 5, items 1 to 7 and 11 (input-method grab, D39).

Also `docs/architecture/doc_session.md` SN3, SN6 and SN15; `docs/architecture/doc_own_stack.md` OS4 and OS13.

## Prerequisites (each a gate before step 3)

1. **#1504 merged and judged:** LP18 step 2 tasks 6 and 7 (`athanor-lock` screen, chip, seal, power actions, `MemoryMax=` from spike L5, and maintainer judgement on the reference laptop).
2. **Spike L7b passed on the reference laptop:** on a fresh account, `athanor-idle` reads LP5's defaults from `org.athanor.desktop.idle` (SD3) and user overrides still apply (amended 2026-11-25).
3. **#1573 merged:** the probe of `athanor-lock.service` under D43's in-process confinement (`verify.py separated-units`).
4. **D43 text approved:** the derived part of D43 (`doc_lock_and_prompts.md:431`) is approved in its docs PR before the switch merges.
5. **Section 5, item 11 on Sulphur:** Sulphur's fixture tests and build pass the input-method grab test (`zwp_input_method_v2` receives no key while locked, D39).

## Changes (one logical commit/PR)

### 1. Enable the lock guard and unlockd socket by preset (D30, SN3)

- Add `forge/specs/athanor-lock/athanor-lock-1.0.0/data/80-athanor-lock.preset`:
  ```
  enable athanor-lock.service
  ```
  and install it to `%{_userpresetdir}/80-athanor-lock.preset` in `forge/specs/athanor-lock/athanor-lock.spec` (`%files` and `%description` updated).
- Add `forge/specs/athanor-unlockd/SOURCES/usr/lib/systemd/system-preset/80-athanor-unlockd.preset`:
  ```
  enable athanor-unlockd.socket
  ```
  and install it to `%{_presetdir}/80-athanor-unlockd.preset` in `forge/specs/athanor-unlockd/athanor-unlockd.spec` (`%files` and `%description` updated).
- Keep `athanor-lock.service`'s D43 confinement intact (`NoNewPrivileges=yes`, `SystemCallArchitectures=native`, `RestrictAddressFamilies=AF_UNIX`, no `Protect*=` / `Private*=` directives that create a user namespace).

### 2. Retire cosmic-greeter as the session locker (LP2)

- `forge/specs/athanor-system-config/SOURCES/usr/libexec/athanor/session-components` (`:23`, `:163-164`): remove `cosmic-greeter-locker.scope` from `COMPONENT_SCOPES` and the `session_component "$anchor" cosmic-greeter ...` launch block, update `forge/specs/athanor-system-config/tests/test_session_components.py`, and add a `%changelog` entry for this release in `athanor-system-config.spec`.
- `forge/specs/athanor-system-config/athanor-system-config.spec` (`:19`): drop `cosmic-greeter` from `Requires:` (keeping `athanor-idle` and the greeter session's own `greetd` / `cosmic-comp` dependencies unchanged until #1602).
- `forge/specs/athanor-system-config/SOURCES/usr/lib/systemd/system-preset/80-athanor-display-manager.preset`: drop or adjust the `cosmic-greeter` disable lines only when the `cosmic-greeter` RPM leaves the image (note: `cosmic-greeter` RPM itself retires in #1602 and stays installed only as the fallback screen locker package in `packages.json` / `own-allow.txt`, so keep `disable cosmic-greeter.service` / `disable cosmic-greeter-daemon.service` until #1602 removes the package).
- `forge/scripts/check_image_session.py` and its tests: replace the `cosmic-greeter-locker.scope` / `usr/bin/cosmic-greeter` / `LOCKER_PAM` checks with `athanor-lock.service`, `athanor-unlockd.socket` and `/usr/lib/pam.d/athanor-lock` + `athanor-lock-fingerprint`.

### 3. Sulphur allow-list and verify.py

- `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/privileged-clients` (`:21`): remove `/usr/bin/cosmic-greeter cosmic-greeter-locker.scope ext_session_lock_manager_v1`.
- `scripts/verify.py` (`#UPSTREAM` at `:3305` and #1560's `cosmic-greeter-locker.scope` transient-scope exemption): remove `"/usr/bin/cosmic-greeter": "cosmic-greeter-locker.scope"` and the matching unit-origin exemption test/entry once no shipped allow-list line uses it.

### 4. Dev VM acceptance

- Update `scripts/devvm/lock-acceptance.sh` and `scripts/devvm/default-session-acceptance.sh` to drive and observe `athanor-lock.service` and `LockedHint` (instead of `cosmic-greeter-$SESSION.lock` and `pgrep -x cosmic-greeter`), and verify:
  - `loginctl lock-session`, `Super+Escape`, lid close / suspend (`rtcwake`), and `loginctl unlock-session`;
  - `kill -9` relock within 1 s after 60 s healthy running (`RESTART_RESET=1`);
  - root-side `pam_faillock` tally counted through `athanor-unlockd` (`pam_unix(athanor-lock:auth)` / `faillock`);
  - no second locker (`pgrep -x cosmic-greeter` empty inside the user session) and `athanor-lock.service` active under `athanor-sulphur-session`.

## Verification and Rollback

- **Repo checks:** `python3 scripts/verify.py --known-red-base origin/iso-v0 services pam allow-list separated-units unit-origin line-citations`, `python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests`, `python3 -B -m unittest discover -s forge/scripts/tests`, `python3 -B -m unittest discover -s scripts/tests`.
- **VM & Laptop:** `scripts/devvm/lock-acceptance.sh` and `scripts/devvm/default-session-acceptance.sh` green, followed by maintainer judgement on the reference laptop before merge (LP18).
- **Rollback:** re-enabling `cosmic-greeter` as the session locker is a single revert of this commit.
