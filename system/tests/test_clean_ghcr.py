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

    def test_an_organisation_is_pruned_through_the_orgs_api(self):
        self.registry({"owner_type": "Organization", "user_packages": ["athanor-forge-tier0-repo"], "packages": {
            "athanor-forge-tier0-repo": [version(3, ["101"], 1), version(4, ["102"], 2), version(5, ["103"], 3)],
        }})
        r = subprocess.run(["bash", str(JANITOR), "ars-regia"], capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        deleted = [c[-1] for c in calls if "DELETE" in c]
        self.assertEqual(deleted, ["/orgs/ars-regia/packages/container/athanor-forge-tier0-repo/versions/3"])

    def test_an_unknown_owner_stops_before_any_delete(self):
        self.registry({"owner_type": "Bot", "user_packages": ["athanor-forge-tier0-repo"]})
        r = subprocess.run(["bash", str(JANITOR), "someone"], capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 1)
        self.assertIn("neither a user nor an organisation", r.stderr)


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

    def test_ninety_days_of_pushes_stay_and_promotions_stay_forever(self):
        """UD8: a digest ever promoted (stable-<YYYYMMDD>) is never deleted, however old."""
        versions = [image(5, ["105"], RECENT), *referrers(50, 5, RECENT),
                    image(6, ["70", "stable-20260801"], OLD), *referrers(60, 6),
                    image(7, ["60", "stable-20250501"], OLD), image(8, ["50"], OLD)]
        self.assertEqual(self.prune(versions, {**self.index(5, 52), **self.index(6, 62)}), [8])

    def test_two_promotions_on_one_day_then_a_third_keep_all_three_images(self):
        """Issue 500: stable-<day> moved to B, stable-previous to B, so A had no tag but promoted-<run_id>."""
        versions = [image(1, ["promoted-100"], OLD), *referrers(10, 1),
                    image(2, ["promoted-101", "stable-20260801"], OLD), *referrers(20, 2),
                    image(3, ["promoted-102", "stable", "stable-20260802"], OLD), *referrers(30, 3),
                    image(4, ["103"], OLD)]
        self.assertEqual(self.prune(versions, {**self.index(1, 12), **self.index(2, 22), **self.index(3, 32)}), [4])

    def test_an_untagged_manifest_no_kept_index_lists_is_deleted(self):
        versions = [image(1, ["100", "stable"], OLD), *referrers(10, 1), image(99, [], RECENT)]
        self.assertEqual(self.prune(versions, self.index(1, 12)), [99])


TIER = "athanor-forge-tier0-repo"


class SignedPackages(Tool):
    """Every package keeps what signs the versions it keeps (UD44: the tier repositories are
    signed)."""

    VERSIONS = [image(1, ["latest"], OLD), *referrers(10, 1),
                image(2, ["101"], "2026-06-01"), *referrers(20, 2),
                image(3, ["102"], "2026-07-01"),
                image(4, ["103"], RECENT), *referrers(40, 4)]

    def run_janitor(self, **fixture):
        self.registry({"user_packages": [TIER], "packages": {TIER: self.VERSIONS}, **fixture})
        r = subprocess.run(["bash", str(JANITOR), "hr-mes"], capture_output=True, text=True, env=self.env)
        log = self.dir / "calls.log"
        calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        return r, sorted(int(c[-1].rsplit("/", 1)[1]) for c in calls if "DELETE" in c)

    @staticmethod
    def index(of, member):
        return {f"ghcr.io/hr-mes/{TIER}:sha256-{of:064x}": {"manifests": [{"digest": f"sha256:{member:064x}"}]}}

    def test_the_signatures_of_a_kept_version_stay_and_a_deleted_one_s_go(self):
        r, deleted = self.run_janitor(raw={**self.index(1, 12), **self.index(4, 42)})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(deleted, [2, 20, 21, 22])

    def test_a_version_without_tags_deletes_nothing(self):
        self.VERSIONS = [*SignedPackages.VERSIONS, {"id": 5, "name": f"sha256:{5:064x}", "created_at": f"{OLD}T00:00:00Z", "metadata": {"container": {}}}]
        r, deleted = self.run_janitor(raw={**self.index(1, 12), **self.index(4, 42)})
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(deleted, [])

    def test_an_unreadable_signature_index_deletes_nothing(self):
        r, deleted = self.run_janitor(raw=self.index(4, 42), errors=[f"ghcr.io/hr-mes/{TIER}:sha256-{1:064x}"])
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(deleted, [])


PROMOTED_SYSTEM = [image(1, ["100", "stable", "stable-20260914"], RECENT), image(2, ["90", "stable-previous", "stable-20260801"], OLD),
                   image(3, ["80", "stable-20250501"], OLD), image(4, ["70"], OLD), image(5, ["released-3-101", "101"], RECENT)]


ABSENT = object()  # no athanor-system package: the fake gh answers 404


class Isos(Tool):
    """UD8: athanor-iso keeps the ISO of every promoted run, the runs read from athanor-system."""

    ISOS = [version(11, ["80"], 1), version(12, ["90"], 2), version(13, ["100", "stable"], 3), version(14, ["70"], 0),
            version(15, ["101"], 4), version(16, ["102"], 5), version(17, ["103", "latest"], 6), version(18, [], 7)]
    # Today's registry: no ISO tagged stable yet, so only the athanor-system checks can stop the janitor.
    UNSTABLE_ISOS = [v if v["id"] != 13 else version(13, ["100"], 3) for v in ISOS]

    def prune(self, isos, system=PROMOTED_SYSTEM):
        packages = {"athanor-iso": isos} if system is ABSENT else {"athanor-iso": isos, "athanor-system": system}
        self.registry({"user_packages": ["athanor-iso"], "packages": packages})
        r = subprocess.run(["bash", str(JANITOR), "hr-mes"], capture_output=True, text=True, env=self.env)
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        return r, sorted(int(c[-1].rsplit("/", 1)[1]) for c in calls if "DELETE" in c)

    def test_no_promoted_iso_is_among_the_deletions(self):
        r, deleted = self.prune(self.ISOS)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(deleted, [14, 15, 18])  # never promoted, and not among the two newest or untagged

    def assert_kept(self, system, reason, isos=UNSTABLE_ISOS):
        r, deleted = self.prune(isos, system=system)
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"athanor-iso: {reason}; the promoted runs are unknown, no ISO deleted", r.stderr)
        self.assertEqual(deleted, [])

    def test_an_unreadable_promoted_set_deletes_no_iso(self):
        self.assert_kept(ABSENT, "the athanor-system versions could not be read")

    def test_an_empty_or_null_answer_deletes_no_iso(self):
        for body in ([], None):
            with self.subTest(body=body):
                self.assert_kept(body, "no athanor-system version is tagged stable")

    def test_no_stable_system_image_deletes_no_iso(self):
        self.assert_kept([image(1, ["100", "latest"], RECENT)], "no athanor-system version is tagged stable")

    def test_a_promoted_system_image_without_its_run_id_deletes_no_iso(self):
        self.assert_kept([image(1, ["100", "stable"], RECENT), image(2, ["stable-20260801"], OLD)],
                         "an athanor-system version tagged stable, stable-previous, stable-<YYYYMMDD> or promoted-<run_id> carries no run id")

    def test_two_promotions_on_one_day_then_a_third_keep_all_three_isos(self):
        """Issue 500: the runs come from promoted-<run_id>, which the day tag and stable-previous cannot lose."""
        system = [image(1, ["promoted-80"], OLD), image(2, ["promoted-90", "stable-20260801"], OLD),
                  image(3, ["promoted-100", "stable", "stable-20260802"], RECENT), image(4, ["70"], OLD)]
        r, deleted = self.prune(self.ISOS, system=system)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(deleted, [14, 15, 18])  # ISOs 80, 90 and 100 stay; 70 was never promoted

    def test_a_stable_iso_of_no_promoted_run_deletes_no_iso(self):
        self.assert_kept([image(2, ["90", "stable"], RECENT)],
                         "the ISO tagged stable has no run id that a promoted athanor-system image carries", isos=self.ISOS)


if __name__ == "__main__":
    unittest.main()
