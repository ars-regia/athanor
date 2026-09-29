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

Looked at by hand, with `scripts/devvm/screenshot.sh`: the "update ready" notification after
the `download` stage, the "now running" notification after `apply`, and the administrator
prompt that `busctl --system call os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1 GoBack`
raises when typed in a terminal of the graphical session.

Item 7 has a second half this harness cannot give: a machine really installed from an ISO
that carries the package. After the first pipeline run with the package, `create.sh` a new
VM from that ISO and check that the state reads `media`, then `signature` after the
migration and a restart.

Clean up: `podman rm -f athanor-acc-registry`, `rm -rf ~/.local/share/athanor-devvm/acceptance`.
