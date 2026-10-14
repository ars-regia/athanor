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

The `sessions` stage (issue #436) runs the other-session cases of item 2 (UT6, ADR-0103 D22) with
the update downloaded and a shutdown inhibitor held, so that no case can reboot the guest: an
authorised call ends at `Error.Blocked`. A prompt is observed as a `polkit-agent-helper@`
instance opened by the graphical session's agent, the password typed into it (the dev VM's is
the user's name) and the `polkitd` line that grants `os.athanor.update.apply-multi-session`; the
absence of one as no helper, no grant and `Blocked`. Since #433 this harness's own SSH login is a
counting session, so `call_active` arms the call from a user timer and the login is over when it
arrives; `guest_ssh` does not multiplex (no ControlMaster), so every call is a login of its own and
the stage opens none between arming a call and its first check. `second-count.sh` bind-mounts a
wrapper over `bootc` that holds `bootc upgrade --from-downloaded` at the unlock while it takes a
block inhibitor and opens a session of the other user; the prompt is then refused by stopping
the agent (`cosmic-osd`), which ends the call as `NotAuthorized`. Not driven: the greeter's fast
user switch (a session of another uid stands in), a text console (SSH stands in), a prompt that
times out (the agent's removal stands in), and a grant of the second prompt, which restarts the guest.

Looked at by hand, with `scripts/devvm/screenshot.sh`: the "update ready" notification after
the `download` stage, the "now running" notification after `apply`, and the administrator
prompt that `busctl --system call os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1 GoBack`
raises when typed in a terminal of the graphical session.

Item 7 has a second half this harness cannot give: a machine really installed from an ISO
that carries the package. After the first pipeline run with the package, `create.sh` a new
VM from that ISO and check that the state reads `media`, then `signature` after the
migration and a restart.

Clean up: `podman rm -f athanor-acc-registry`, `rm -rf ~/.local/share/athanor-devvm/acceptance`.
