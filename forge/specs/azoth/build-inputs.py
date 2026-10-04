#!/usr/bin/env python3
"""The inputs of the kernel build as JSON (docs/architecture/doc_kernel_build.md,
section 3 step 8 and section 7): the predicate of the pins attestation that publish
attaches to the images, and the key with which the inputs job recognises a kernel
already built from these identical inputs. Only what changes the RPMs goes in: pins,
source manifest, config delta, patches, the certificates compiled in (keys/modules,
keys/revoked), merge rules, the guest kernel (fragment and spec), build.sh with its
build conditions and the environment (builder base, Containerfile, toolchain lock). Not cmdline, boot/, retention.sh or the workflow:
changing them must not rebuild anything."""

import hashlib
import json
import pathlib
import re

k = pathlib.Path(__file__).resolve().parent


def sha(name):
    return hashlib.sha256((k / name).read_bytes()).hexdigest()


# The NVIDIA_* pins concern the external modules (nvidia.sh), not the kernel RPMs.
pins = {
    key: value
    for key, value in re.findall(r"^(\w+)=(.*)$", (k / "pins.env").read_text(), re.M)
    if not key.startswith("NVIDIA_")
}
base = re.search(r"^FROM (\S+)", (k / "builder/Containerfile").read_text(), re.M).group(
    1
)
print(
    json.dumps(
        {
            "pins": pins,
            "sources_sha256": sha("SOURCES/sources.sha256"),
            "kernel_local_sha256": sha("kernel-local"),
            "patches_list_sha256": sha("patches.list"),
            "patches_sha256": {
                p.relative_to(k / "patches").as_posix(): sha(p.relative_to(k))
                for p in sorted((k / "patches").rglob("*.patch"))
            },
            "keys_sha256": {
                p.relative_to(k).as_posix(): sha(p.relative_to(k))
                for d in ("modules", "revoked")
                for p in sorted((k / "keys" / d).glob("*.pem"))
            },
            "fedora_wins_sha256": sha("fedora-wins.list"),
            "microvm_sha256": {
                name: sha(f"microvm/{name}")
                for name in ("kernel-local", "azoth-microvm.spec")
            },
            "build_sh_sha256": sha("build.sh"),
            "bconds_sha256": sha("bconds.sh"),
            "builder_base": base,
            "containerfile_sha256": sha("builder/Containerfile"),
            "toolchain_lock_sha256": sha("builder/toolchain.lock"),
        },
        indent=2,
    )
)
