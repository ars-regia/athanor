# Update and trust acceptance

Runs section 6 of `docs/architecture/doc_update_trust.md` on the development VM with a
throwaway registry and throwaway keys. Nothing here touches the project registry or key.

    bash forge/scripts/build_rolling_local.sh update     # the RPM under test
    scripts/devvm/acceptance/images.sh                   # registry, keys, ten images (about 20 min)
    scripts/devvm/reset.sh && scripts/devvm/start.sh     # a freshly installed guest
    scripts/devvm/acceptance/run.sh                      # about 90 minutes, a dozen reboots
    scripts/devvm/acceptance/run.sh goback               # resume at a stage

The 31 GB host cannot hold the CI runner guest (16 GB) and this VM (8 GB) together:
`run.sh` refuses to start while a workflow run is in progress. Do not dispatch one meanwhile.

The `probes` stage (issue #312) runs `probes.py` on the finished guest: listening sockets and
firewall zones, `systemd-analyze security` ceilings, SELinux denials since boot, PAM, sysctl,
masked units, the trust state and one Nix build with `nix store gc`, each against
`expectations.toml`. It fails naming the check; `run.sh probes` runs it alone. Change a line of
the expectations only with the review of the exposure it declares.

The `metered` stage (UT12, `doc_shell.md` section 7, item 4) marks the guest's default device
metered with `nmcli device modify`, which changes the applied connection and not the profile on
disk, publishes v2 and runs the check: the update must read available with no error and nothing
staged. It stops the check timer for its duration, because a check started by hand moves the
timer's next run six hours on, and ends with a reboot, which also drops the mark; the `download`
stage then waits for the timer's run two minutes after that boot. A run that stops inside the
stage leaves the timer stopped, and starting it again does not fire a boot trigger already
past: reboot the guest before resuming at `download`.

The `sessions` stage (issue #436) runs the other-session cases of item 2 (UT6, ADR-0103 D22) with
the update downloaded and a shutdown inhibitor held, so that no case can reboot the guest: an
authorised call ends at `Error.Blocked`. A prompt is observed as a `polkit-agent-helper@`
instance opened by the graphical session's agent, the password typed into it (the dev VM's is
the user's name) and the `polkitd` line that grants `os.athanor.update.apply-multi-session`; the
absence of one as no helper, no grant and `Blocked`. Since #433 this harness's own SSH login is a
counting session, so `call_active` arms the call from a user timer and the login is over when it
arrives; `guest_ssh` does not multiplex (no ControlMaster), so every call is a login of its own and
the stage opens none between arming a call and its first check. The run's registry tunnel (`ssh -f -N`) is a login of the same user that lasts the whole run, and the service counts it as a second session: the stage closes it (`untunnel`) before the first case and reopens it at the end. The transient timer is given `AccuracySec=1ms`, as its default of a minute let a call arrive late. `second-count.sh` bind-mounts a
wrapper over `bootc` that holds `bootc upgrade --from-downloaded` at the unlock while it takes a
block inhibitor and opens a session of the other user (the stage's own inhibitor is dropped only
after `nsenter` shows the update service the wrapper, and no harness login is open meanwhile); the prompt is then refused by stopping
the agent (`cosmic-osd`), which ends the call as `NotAuthorized`; the agent is then restarted. Not driven: the greeter's fast
user switch (a session of another uid stands in), a text console (SSH stands in), a prompt that
times out (the agent's removal stands in), and a grant of the second prompt, which restarts the guest.

After `apply`, the notifier's journal must hold exactly one "running version announced" line,
naming the digest and the version the state file reports as booted (`doc_shell.md` section 7,
item 5); two more polls must not repeat it. The stage then stops the check timer and waits for a check it
already started: the check holds the store, and Apply would answer `Busy` instead of
`NothingDownloaded` (#547).

Looked at by hand, with `scripts/devvm/screenshot.sh`: the "update ready" notification after
the `download` stage, the "now running" notification's text after `apply`, and the administrator
prompt that `busctl --system call os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1 GoBack`
raises when typed in a terminal of the graphical session.

Item 7 has a second half this harness cannot give: a machine really installed from an ISO
that carries the package. After the first pipeline run with the package, `create.sh` a new
VM from that ISO and check that the state reads `media`, then `signature` after the
migration and a restart.

Clean up: `systemctl --user stop athanor-acc-registry` when it is active, `podman rm athanor-acc-registry`, `rm -rf ~/.local/share/athanor-devvm/acceptance`.
