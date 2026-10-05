# Building a forge package locally

Build a spec the way CI builds it: with `scripts/build_spec.sh` inside the builder image,
the repository mounted at `/workspace` and `forge/` as the working directory. From the
repository root:

```bash
podman run --rm -v "$PWD:/workspace" -w /workspace/forge \
    ghcr.io/hr-mes/athanor-builder:latest bash scripts/build_spec.sh specs/athanor-<name>
```

The RPMs land in `forge/RPMS/`.

`scripts/build_changed_specs.sh BASE BUILDER_IMAGE` does the same for every spec that
changed since the commit `BASE`; Spec Build Check runs it on pull requests. It skips
`specs/azoth`, which Kernel Build builds.
