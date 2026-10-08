# Building a forge package locally

Build a spec the way CI builds it: with `scripts/run_spec_build.sh`, which runs the builder
image twice, `build_spec.sh fetch` with network and then `build_spec.sh build` with
`--network=none`. Run it from `forge/`, inside a git checkout:

```bash
cd forge
bash scripts/run_spec_build.sh ghcr.io/ars-regia/athanor-builder:latest specs/athanor-<name>
```

The RPMs land in `forge/RPMS/`.

`scripts/build_changed_specs.sh [--dry-run] BASE BUILDER_IMAGE` builds, one after the other,
what Spec Build Check builds for the change since the commit `BASE`: the DAG's specs that
changed, or all of them when the builder, `config/rpmmacros` or the shared build scripts
changed. Directories the DAG does not build, such as `specs/azoth`, are skipped.
