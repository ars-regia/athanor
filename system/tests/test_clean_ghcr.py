"""Unit tests of forge/scripts/clean_ghcr.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

JANITOR = pathlib.Path(__file__).resolve().parents[2] / "forge" / "scripts" / "clean_ghcr.sh"


def version(id_, tags, day):
    return {"id": id_, "name": f"sha256:{id_:064x}", "created_at": f"2026-09-{day:02d}T00:00:00Z", "metadata": {"container": {"tags": tags}}}


class Janitor(Tool):
    def test_kernel_packages_are_never_touched(self):
        self.registry({"user_packages": ["azoth", "azoth-nvidia", "athanor-forge-tier0-repo"], "packages": {
            "azoth": [version(1, [], 1)],
            "azoth-nvidia": [version(2, [], 1)],
            "athanor-forge-tier0-repo": [version(3, ["101"], 1), version(4, ["102"], 2), version(5, ["103"], 3),
                               version(6, ["latest"], 0), version(7, [], 4)],
        }})
        r = subprocess.run(["bash", str(JANITOR), "hr-mes"], capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        deleted = sorted(c[-1] for c in calls if "DELETE" in c)
        self.assertEqual(deleted, ["/users/hr-mes/packages/container/athanor-forge-tier0-repo/versions/3",
                                   "/users/hr-mes/packages/container/athanor-forge-tier0-repo/versions/7"])
        self.assertFalse(any("azoth" in " ".join(c) and "versions" in " ".join(c) for c in calls))


NOW = "1789776000"  # 2026-09-19T00:00:00Z
OLD, RECENT = "2026-05-01", "2026-08-01"  # outside and inside the 90 days before NOW


def image(id_, tags, day):
    return {"id": id_, "name": f"sha256:{id_:064x}", "created_at": f"{day}T00:00:00Z", "metadata": {"container": {"tags": tags}}}


def referrers(id_, of, day=OLD):
    """What signs image OF: the .sig tag of containers/image, the cosign 3 index and its untagged member."""
    hex_ = f"{of:064x}"
    return [image(id_, [f"sha256-{hex_}.sig"], day), image(id_ + 1, [f"sha256-{hex_}"], day), image(id_ + 2, [], day)]


class SystemImages(Tool):
    def prune(self, versions, raw=None):
        self.registry({"user_packages": ["athanor-system"], "packages": {"athanor-system": versions}, "raw": raw or {}})
        r = subprocess.run(["bash", str(JANITOR), "hr-mes"], capture_output=True, text=True, env={**self.env, "CLEAN_GHCR_NOW": NOW})
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        return sorted(int(c[-1].rsplit("/", 1)[1]) for c in calls if "DELETE" in c)

    def index(self, of, member):
        return {f"ghcr.io/hr-mes/athanor-system:sha256-{of:064x}": {"manifests": [{"digest": f"sha256:{member:064x}"}]}}

    def test_stable_latest_and_the_previous_stable_stay_however_old_with_everything_that_signs_them(self):
        versions = [image(1, ["100", "stable", "stable-20260101"], OLD), *referrers(10, 1),
                    image(2, ["90", "stable-previous"], OLD), *referrers(20, 2),
                    image(3, ["110", "latest"], OLD), *referrers(30, 3)]
        raw = {**self.index(1, 12), **self.index(2, 22), **self.index(3, 32)}
        self.assertEqual(self.prune(versions, raw), [])

    def test_an_old_image_nothing_names_goes_with_its_signatures(self):
        versions = [image(1, ["100", "stable"], OLD), *referrers(10, 1), image(4, ["80"], OLD), *referrers(40, 4)]
        self.assertEqual(self.prune(versions, {**self.index(1, 12), **self.index(4, 42)}), [4, 40, 41, 42])

    def test_ninety_days_of_pushes_and_of_promotions_stay(self):
        versions = [image(5, ["105"], RECENT), *referrers(50, 5, RECENT),
                    image(6, ["70", "stable-20260801"], OLD), *referrers(60, 6),
                    image(7, ["60", "stable-20260501"], OLD)]
        self.assertEqual(self.prune(versions, {**self.index(5, 52), **self.index(6, 62)}), [7])

    def test_an_untagged_manifest_no_kept_index_lists_is_deleted(self):
        versions = [image(1, ["100", "stable"], OLD), *referrers(10, 1), image(99, [], RECENT)]
        self.assertEqual(self.prune(versions, self.index(1, 12)), [99])


if __name__ == "__main__":
    unittest.main()
