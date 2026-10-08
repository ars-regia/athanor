"""Unit tests of system/upgrade-bytes.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "upgrade-bytes.sh"
REG = "registry.example/owner"
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
BOOT = "io.athanor.azoth-boot.digest"


def digest(n):
    return "sha256:" + f"{n:x}" * 64


def layer(n, size):
    return {
        "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
        "digest": digest(n),
        "size": size,
    }


class UpgradeBytes(Tool):
    def published(
        self,
        from_tag="stable",
        old_layers=((1, 100), (2, 200)),
        new_layers=((1, 100), (3, 300), (4, 400)),
        old_kernel="7.2.9",
        new_kernel="7.2.9",
        errors=(),
    ):
        """Each variant: the candidate under run 412 and, when from_tag is set, the image behind that tag."""
        fx = {"tags": {}, "configs": {}, "raw": {}, "errors": list(errors)}
        lines = []
        for i, name in enumerate(NAMES):
            new, old = digest(10 + i), digest(20 + i)
            lines.append(f"{REG}/{name} 412 {new}")
            fx["raw"][f"{REG}/{name}@{new}"] = {
                "layers": [layer(n, s) for n, s in new_layers]
            }
            fx["configs"][f"{REG}/{name}@{new}"] = {
                "org.opencontainers.image.version": "43.20261008.300",
                "ostree.linux": new_kernel,
                BOOT: digest(30),
            }
            if from_tag:
                fx["tags"][f"{REG}/{name}:{from_tag}"] = old
                fx["raw"][f"{REG}/{name}@{old}"] = {
                    "layers": [layer(n, s) for n, s in old_layers]
                }
                fx["configs"][f"{REG}/{name}@{old}"] = {
                    "org.opencontainers.image.version": "43.20261008.253",
                    "ostree.linux": old_kernel,
                    BOOT: digest(30),
                }
        self.registry(fx)
        (self.dir / "digests.txt").write_text("\n".join(lines) + "\n")

    def measure(self):
        out = self.dir / "metrics" / "upgrade-bytes.json"
        r = self.run_script(
            "--registry",
            REG,
            "--out",
            str(out),
            str(self.dir / "digests.txt"),
            script=SCRIPT,
        )
        return r, (json.loads(out.read_text()) if out.exists() else None)

    def test_only_layers_absent_from_stable_are_downloaded(self):
        self.published()
        r, data = self.measure()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            [v["repository"] for v in data["variants"]], [f"{REG}/{n}" for n in NAMES]
        )
        v = data["variants"][0]
        self.assertEqual(
            (v["from"]["tag"], v["from"]["digest"], v["to"]["digest"]),
            ("stable", digest(20), digest(10)),
        )
        self.assertEqual(
            (v["layers"], v["new_layers"], v["bytes"], v["new_bytes"]), (3, 2, 800, 700)
        )
        self.assertEqual(
            (v["from"]["version"], v["to"]["version"]),
            ("43.20261008.253", "43.20261008.300"),
        )
        self.assertFalse(v["kernel_changed"])
        self.assertIn("| `athanor-system` |", r.stdout)

    def test_without_stable_the_measure_starts_from_latest(self):
        self.published(from_tag="latest")
        r, data = self.measure()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual({v["from"]["tag"] for v in data["variants"]}, {"latest"})
        self.assertEqual(data["variants"][0]["new_bytes"], 700)

    def test_a_first_publication_is_a_full_download(self):
        self.published(from_tag=None)
        r, data = self.measure()
        self.assertEqual(r.returncode, 0, r.stderr)
        v = data["variants"][0]
        self.assertIsNone(v["from"])
        self.assertEqual((v["new_layers"], v["new_bytes"]), (3, 800))
        self.assertTrue(v["kernel_changed"])

    def test_a_new_kernel_is_recorded(self):
        self.published(new_kernel="7.2.10")
        r, data = self.measure()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(all(v["kernel_changed"] for v in data["variants"]))

    def test_a_registry_error_fails_and_writes_nothing(self):
        self.published(errors=[f"{REG}/athanor-system-nvidia:stable"])
        r, data = self.measure()
        self.assertNotEqual(r.returncode, 0)
        self.assertIsNone(data)

    def test_a_malformed_digests_line_is_refused(self):
        self.published()
        (self.dir / "digests.txt").write_text(
            f"{REG}/athanor-system 412 not-a-digest\n"
        )
        r, data = self.measure()
        self.assertNotEqual(r.returncode, 0)
        self.assertIsNone(data)
