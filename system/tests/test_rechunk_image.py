"""Unit tests of system/rechunk-image.sh, the rechunk of UD32 (doc_update_delivery.md). A podman
stub records every call and answers from a JSON file of image labels; `podman run` stands for
rpm-ostree and writes the labels of the rechunked image
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "rechunk-image.sh"
SOURCE = "a" * 64
NEW = "b" * 64
OUT = f"localhost/athanor-system-rechunked:{SOURCE[:12]}"
LABELS = {
    "containers.bootc": "1",
    "ostree.bootable": "true",
    "ostree.commit": "aa01c30c",
    "ostree.linux": "7.2.9-100.azoth.fc43.x86_64",
}


class RechunkImage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        self.images = self.dir / "images.json"
        self.calls = self.dir / "calls.log"
        # `podman run` succeeds unless run.status says otherwise, and then the output image
        # carries the labels of rechunked.json.
        (bin_dir / "podman").write_text(
            textwrap.dedent(f"""\
            #!/bin/bash
            set -o pipefail
            printf '%s\\n' "$*" >> {self.calls}
            case "$1 $2" in
              "info --format") echo /store/graph /run/store overlay ;;
              "image inspect")
                jq -ce --arg ref "$5" '.[$ref] // error("image not known: " + $ref)' {self.images} |
                  if [[ $4 == '{{{{.Id}}}}' ]]; then jq -r .id; else jq -c .labels; fi ;;
              "rmi --ignore") ;;
              run\\ *)
                status=$(cat {self.dir}/run.status 2>/dev/null || echo 0)
                [[ $status -eq 0 ]] || exit "$status"
                jq --arg out "{OUT}" --slurpfile new {self.dir}/rechunked.json '.[$out] = $new[0]' {self.images} > {self.images}.new
                mv {self.images}.new {self.images} ;;
              *) echo "unexpected podman $*" >&2; exit 64 ;;
            esac
            """)
        )
        (bin_dir / "podman").chmod(0o755)
        self.env = {"PATH": f"{bin_dir}:/usr/bin:/bin"}
        self.images.write_text(json.dumps({SOURCE: {"id": SOURCE, "labels": LABELS}}))
        self.iidfile = self.dir / "artifacts" / "system-image.iid"
        self.iidfile.parent.mkdir()
        self.iidfile.write_text(f"sha256:{SOURCE}")

    def tearDown(self):
        self.tmp.cleanup()

    def rechunked(self, labels):
        (self.dir / "rechunked.json").write_text(
            json.dumps({"id": NEW, "labels": labels})
        )

    def rechunk(self):
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.iidfile)],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def run_call(self):
        return next(
            c for c in self.calls.read_text().splitlines() if c.startswith("run ")
        )

    def test_the_system_image_is_rechunked_without_a_baseline(self):
        self.rechunked({**LABELS, "ostree.commit": "9693ad96"})
        r = self.rechunk()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.iidfile.read_text().strip(), f"sha256:{NEW}")
        run = self.run_call()
        self.assertIn(
            f"--entrypoint /usr/bin/rpm-ostree {SOURCE} compose build-chunked-oci --bootc"
            f" --format-version=2 --max-layers=120 --from {SOURCE}"
            f" --output containers-storage:{OUT}",
            run,
        )
        # The store is mounted at its own path: its database records that path.
        self.assertIn("-v /store/graph:/store/graph -v /run/store:/run/store", run)
        # Without a baseline: nothing is left under the output name before the run.
        calls = self.calls.read_text().splitlines()
        self.assertLess(calls.index(f"rmi --ignore {OUT}"), calls.index(run))

    def test_a_lost_label_fails_and_keeps_the_iid_file(self):
        self.rechunked({k: v for k, v in LABELS.items() if k != "ostree.linux"})
        r = self.rechunk()
        self.assertEqual(r.returncode, 1)
        self.assertIn("ostree.linux", r.stderr)
        self.assertEqual(self.iidfile.read_text(), f"sha256:{SOURCE}")

    def test_a_changed_label_fails(self):
        self.rechunked({**LABELS, "ostree.bootable": "false"})
        r = self.rechunk()
        self.assertEqual(r.returncode, 1)
        self.assertIn("ostree.bootable", r.stderr)
        self.assertEqual(self.iidfile.read_text(), f"sha256:{SOURCE}")

    def test_a_failed_rechunk_keeps_the_iid_file(self):
        (self.dir / "run.status").write_text("1")
        r = self.rechunk()
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.iidfile.read_text(), f"sha256:{SOURCE}")

    def test_the_iid_file_must_hold_an_image_id(self):
        self.iidfile.write_text("localhost/athanor-system:latest")
        r = self.rechunk()
        self.assertEqual(r.returncode, 2)
        self.assertFalse(self.calls.exists())


if __name__ == "__main__":
    unittest.main()
