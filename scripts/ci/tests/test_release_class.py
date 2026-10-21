"""scripts/ci/release_class.py: the class and advisory derivation of PL55
(docs/architecture/doc_pipeline.md, ADR-0127), over a real git repository and fake sources."""

import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import release_class as rc  # noqa: E402

REPO = "ghcr.io/owner/athanor-system"
OLD, NEW = "sha256:" + "1" * 64, "sha256:" + "2" * 64
SECURITY = 'class = "security"\nadvisories = ["CVE-2026-0001"]\n'
STAMP = 1760000000
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}  # fmt: skip


def updateinfo(*updates):
    body = "".join(
        f'<update type="{kind}"><id>{id}</id><issued date="2026-10-01 00:00:00"/>'
        f'<pkglist><collection><package name="openssl" epoch="0" version="3.5.1" release="{rel}" arch="x86_64"/>'
        "</collection></pkglist></update>"
        for id, kind, rel in updates
    )
    return f"<updates>{body}</updates>".encode()


class Fake(rc.Sources):
    def __init__(self, root, old="3.5.1-1.fc45", new="3.5.1-3.fc45", info=None, fail=False):
        self.repo_root = root
        self.pkgs = {
            f"{REPO}@{OLD}": {"openssl": f"0:{old}"},
            f"{REPO}@{NEW}": {"openssl": f"0:{new}"},
        }
        self.info, self.fail = info if info is not None else updateinfo(), fail

    def packages(self, ref):
        return self.pkgs[ref]

    def updateinfo(self, ref):
        if self.fail:
            raise rc.ClassError("updateinfo unavailable")
        return STAMP, self.info


class Derive(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q", "-b", "iso-v0")

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args):
        r = subprocess.run(
            ["git", "-C", str(self.root), *args], capture_output=True, text=True,
            env={**os.environ, **GIT_ENV},
        )  # fmt: skip
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def commit(self, name="x", text=None):
        path = self.root / (f"release/class/{name}.toml" if text else f"{name}.txt")
        path.parent.mkdir(parents=True, exist_ok=True)
        self.n = getattr(self, "n", 0) + 1
        path.write_text(text or f"{name} {self.n}")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", name)
        return self.git("rev-parse", "HEAD")

    def derive(self, base, head, **kw):
        return rc.derive(
            REPO,
            None if base is None else rc.Baseline(f"{REPO}@{OLD}", OLD, base or None),
            rc.Image(f"{REPO}@{NEW}", NEW),
            head,
            Fake(self.root, **kw),
        )

    def test_no_baseline_is_feature(self):
        got = self.derive(None, self.commit())
        self.assertEqual(
            (got["class"], got["advisories"], got["class_reason"], got["baseline_digest"]),
            ("feature", [], "no-baseline", None),
        )

    def test_unlabelled_baseline_runs_only_the_fedora_half(self):
        self.commit("2026-10-21-x", SECURITY)
        head = self.commit()
        got = self.derive("", head)
        self.assertEqual((got["class"], got["advisories"], got["class_reason"]), ("feature", [], "unlabelled-baseline"))
        got = self.derive("", head, info=updateinfo(("FEDORA-2026-aaaa", "security", "2.fc45")))
        self.assertEqual((got["class"], got["advisories"]), ("security", ["FEDORA-2026-aaaa"]))

    def test_security_release_class_toml_in_range_sets_the_class(self):
        base = self.commit()
        head = self.commit("2026-10-21-x", SECURITY)
        got = self.derive(base, head)
        self.assertEqual((got["class"], got["advisories"], got["class_reason"]), ("security", ["CVE-2026-0001"], ""))

    def test_a_file_outside_the_range_is_not_claimed(self):
        self.commit("2026-10-21-x", SECURITY)
        base = self.commit()
        self.assertEqual(self.derive(base, self.commit())["class"], "feature")

    def test_a_file_scoped_to_another_repository_is_not_claimed(self):
        base = self.commit()
        scoped = SECURITY + 'repositories = ["athanor-system-nvidia"]\n'
        head = self.commit("2026-10-21-x", scoped)
        self.assertEqual(self.derive(base, head)["class"], "feature")
        head = self.commit("2026-10-21-y", SECURITY + 'repositories = ["athanor-system"]\n')
        self.assertEqual(self.derive(base, head)["class"], "security")

    def test_fedora_advisory_between_latest_and_candidate_is_claimed(self):
        head = self.commit()
        for rel, ids in (("2.fc45", ["FEDORA-2026-aaaa"]), ("1.fc45", []), ("4.fc45", [])):
            with self.subTest(fixed=rel):
                got = self.derive("", head, info=updateinfo(("FEDORA-2026-aaaa", "security", rel)))
                self.assertEqual(got["advisories"], ids)

    def test_only_security_advisories_count(self):
        got = self.derive("", self.commit(), info=updateinfo(("FEDORA-2026-bbbb", "bugfix", "2.fc45")))
        self.assertEqual((got["class"], got["advisories"]), ("feature", []))

    def test_label_commit_absent_fails(self):
        with self.assertRaisesRegex(rc.ClassError, REPO):
            self.derive("f" * 40, self.commit())

    def test_unavailable_updateinfo_with_a_labelled_baseline_fails(self):
        base = self.commit()
        with self.assertRaisesRegex(rc.ClassError, REPO):
            self.derive(base, self.commit(), fail=True)

    def test_non_ancestor_label_uses_the_range(self):
        trunk = self.commit()
        self.git("switch", "-q", "-c", "side")
        side = self.commit("side")
        self.git("switch", "-q", "iso-v0")
        head = self.commit("2026-10-21-x", SECURITY)
        self.assertNotEqual(trunk, side)
        self.assertEqual(self.derive(side, head)["advisories"], ["CVE-2026-0001"])

    def test_empty_updateinfo_result_is_no_fedora_advisory(self):
        got = self.derive("", self.commit(), info=updateinfo())
        self.assertEqual((got["class"], got["advisories"]), ("feature", []))
        self.assertEqual(got["updateinfo_timestamp"], STAMP)

    def test_advisories_are_the_sorted_union(self):
        base = self.commit()
        both = 'class = "security"\nadvisories = ["FEDORA-2026-aaaa", "CVE-2026-0001"]\n'
        head = self.commit("2026-10-21-x", both)
        got = self.derive(base, head, info=updateinfo(("FEDORA-2026-aaaa", "security", "2.fc45")))
        self.assertEqual(got["advisories"], ["CVE-2026-0001", "FEDORA-2026-aaaa"])

    def test_malformed_release_class_toml_fails(self):
        base = self.commit()
        for text in ('class = "urgent"\nadvisories = ["CVE-2026-0001"]\n', 'class = "security"\nadvisories = "CVE-2026-0001"\n'):
            with self.subTest(text=text):
                head = self.commit("2026-10-21-bad", text)
                with self.assertRaisesRegex(rc.ClassError, REPO):
                    self.derive(base, head)


class GitHalf(Derive):
    def test_a_file_renamed_in_the_range_is_claimed(self):
        self.commit("2026-10-20-old", SECURITY)
        base = self.commit()
        self.git("mv", "release/class/2026-10-20-old.toml", "release/class/2026-10-21-new.toml")
        self.git("commit", "-q", "-m", "rename")
        got = self.derive(base, self.git("rev-parse", "HEAD"))
        self.assertEqual(got["advisories"], ["CVE-2026-0001"])

    def test_a_path_that_breaks_the_naming_convention_fails(self):
        base = self.commit()
        for name in ("2026-10-21-a b", "notes", "2026-10-21-UP"):
            with self.subTest(name=name):
                head = self.commit(name, SECURITY)
                with self.assertRaisesRegex(rc.ClassError, "do not match"):
                    self.derive(base, head)
                self.git("checkout", "-q", "-B", "iso-v0", base)

    def test_a_git_error_is_not_an_absence(self):
        base = self.commit()
        with self.assertRaises(rc.ClassError):
            self.derive(base, "e" * 40)


class Podman(unittest.TestCase):
    listing = "filesystem 0:3-1\nglibc 0:2-1\nopenssl 0:3-1\n"

    def setUp(self):
        self.calls, self.fail = [], {}
        self.tmp = tempfile.TemporaryDirectory()
        self.db = pathlib.Path(self.tmp.name)
        self._run = rc.run
        rc.run = self.fake_run

    def tearDown(self):
        rc.run = self._run
        self.tmp.cleanup()

    def fake_run(self, *args, **kw):
        self.calls.append(args)
        for prefix, error in self.fail.items():
            if args[: len(prefix)] == prefix:
                raise rc.ClassError(error)
        if args[0] == "rpm":
            return self.listing
        if args[:2] == ("podman", "create"):
            return "ctr\n"
        if args[:2] == ("podman", "cp"):
            dest = pathlib.Path(args[3])
            if dest.name == "rpm":
                dest.mkdir()
                (dest / "rpmdb.sqlite").write_text("")
            else:
                dest.write_text('VERSION_ID="45"\n')
        return b"" if kw.get("text") is False else ""

    def sources(self):
        return rc.PodmanSources(pathlib.Path("."))

    def test_an_empty_listing_fails(self):
        self.listing = ""
        with self.assertRaisesRegex(rc.ClassError, "img.*lacks"):
            self.sources().packages("img")

    def test_a_missing_sanity_package_fails(self):
        self.listing = "openssl 0:3-1\nglibc 0:2-1\n"
        with self.assertRaisesRegex(rc.ClassError, "filesystem"):
            self.sources().packages("img")

    def test_a_missing_sqlite_file_fails_before_rpm_runs(self):
        with self.assertRaisesRegex(rc.ClassError, "no rpmdb.sqlite"):
            self.sources()._packages("img", self.db)
        self.assertFalse([c for c in self.calls if c[0] == "rpm"])

    def test_the_image_is_created_and_removed_and_never_started(self):
        self.assertIn("openssl", self.sources().packages("img"))
        verbs = [c[1] for c in self.calls if c[0] == "podman"]
        self.assertEqual(verbs, ["create", "cp", "cp", "rm", "rmi"])

    def test_a_failing_cleanup_does_not_mask_the_read_error(self):
        self.fail = {("podman", "cp"): "no such path", ("podman", "rm"): "busy"}
        with self.assertRaisesRegex(rc.ClassError, "no such path.*busy"):
            self.sources().packages("img")
        self.assertIn(("podman", "rmi", "img"), self.calls)

    def test_curl_is_https_only_and_bounded(self):
        self.sources()._fetch("https://m/x")
        args = self.calls[-1]
        self.assertEqual(args[args.index("--proto") + 1], "=https")
        self.assertEqual(args[args.index("--proto-redir") + 1], "=https")
        self.assertIn("--max-filesize", args)
        self.assertEqual(args[-2:], ("--", "https://m/x"))

    def repomd(self, stamp):
        t = f"<timestamp>{stamp}</timestamp>" if stamp else ""
        digest = hashlib.sha256(b"<updates/>").hexdigest()
        return (
            '<repomd xmlns="http://x"><data type="updateinfo">' + t
            + f'<checksum type="sha256">{digest}</checksum>'
            + '<location href="repodata/u.xml"/></data></repomd>'
        ).encode()  # fmt: skip

    def test_the_timestamp_is_the_one_of_the_repomd_entry(self):
        s = self.sources()
        s._fetch = lambda url: b"<updates/>"
        self.assertEqual(s._updateinfo("https://m/", self.repomd("1760000001.5")), (1760000001, b"<updates/>"))
        self.assertIsNone(s._updateinfo("https://m/", self.repomd(None))[0])
        with self.assertRaisesRegex(rc.ClassError, "not a number"):
            s._updateinfo("https://m/", self.repomd("soon"))


class Malformed(Derive):
    def test_a_bad_epoch_is_a_class_error(self):
        with self.assertRaises(rc.ClassError):
            rc.evr("x:1-2")

    def test_an_advisory_package_without_a_version_is_a_class_error(self):
        info = b'<updates><update type="security"><id>A-1</id><package name="openssl"/></update></updates>'
        with self.assertRaisesRegex(rc.ClassError, REPO):
            self.derive("", self.commit(), info=info)

    def test_an_advisory_id_outside_the_schema_pattern_fails(self):
        with self.assertRaisesRegex(rc.ClassError, REPO):
            self.derive("", self.commit(), info=updateinfo(("FEDORA 2026", "security", "2.fc45")))


class Compare(unittest.TestCase):
    def test_rpm_ordering(self):
        for a, b, want in (
            ("0:3.5.1-2.fc45", "0:3.5.1-10.fc45", -1),
            ("1:1.0-1", "0:9.9-1", 1),
            ("0:1.0~rc1-1", "0:1.0-1", -1),
            ("0:1.0-1", "0:1.0-1", 0),
        ):
            self.assertEqual(rc.evrcmp(a, b), want, (a, b))


if __name__ == "__main__":
    unittest.main()
