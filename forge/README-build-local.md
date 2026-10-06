# Building a forge package locally

Build a spec the way CI builds it: with `scripts/build_spec.sh` inside the builder image,
the repository mounted at `/workspace` and `forge/` as the working directory. From the
repository root:

```bash
podman run --rm -v "$PWD:/workspace" -w /workspace/forge \
    ghcr.io/hr-mes/athanor-builder:latest bash scripts/build_spec.sh specs/athanor-<name>
```

The RPMs land in `forge/RPMS/`.

`scripts/build_changed_specs.sh [--dry-run] BASE BUILDER_IMAGE` builds, one after the other,
what Spec Build Check builds for the change since the commit `BASE`: the DAG's specs that
changed, or all of them when the builder, `config/rpmmacros` or the shared build scripts
changed. Directories the DAG does not build, such as `specs/azoth`, are skipped.
