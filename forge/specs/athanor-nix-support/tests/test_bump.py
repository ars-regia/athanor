"""Unit tests of the nixpkgs registry bump, without network
(python3 -B -m unittest discover -s forge/specs/athanor-nix-support/tests -v)."""

import contextlib
import datetime
import io
import json
import pathlib
import shutil
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
import bump  # noqa: E402

HEADS = {
    "nixos-25.05": "a" * 40,
    "nixos-25.11": "b" * 40,
    "nixos-26.05": "c" * 40,
    "nixos-26.11": "d" * 40,
}


def tar_gz(top="nixpkgs-0123"):
    """A GitHub-like archive: one top directory, a global pax header, an executable, a
    symbolic link, an empty file and an empty directory."""
    buf = io.BytesIO()
    with tarfile.open(
        fileobj=buf,
        mode="w:gz",
        format=tarfile.PAX_FORMAT,
        pax_headers={"comment": "0123"},
    ) as tar:

        def add(name, kind=tarfile.REGTYPE, data=b"", mode=0o644, link=""):
            info = tarfile.TarInfo(f"{top}/{name}" if name else top)
            info.type, info.mode, info.size, info.linkname = kind, mode, len(data), link
            tar.addfile(info, io.BytesIO(data) if kind == tarfile.REGTYPE else None)

        add("", tarfile.DIRTYPE, mode=0o755)
        add("sub/", tarfile.DIRTYPE, mode=0o755)
        add("sub/run.sh", data=b"#!/bin/sh\necho x\n", mode=0o755)
        add("sub/link", tarfile.SYMTYPE, link="../a.txt", mode=0o777)
        add("sub/eight", data=b"abcdefgh")
        add("sub/empty/", tarfile.DIRTYPE, mode=0o755)
        add("a.txt", data=b"hello\n")
        add("B-empty")
    return buf.getvalue()


class BranchSelection(unittest.TestCase):
    def test_a_release_is_taken_from_the_15th_of_the_next_month(self):
        self.assertEqual(
            bump.choose_branch(HEADS, "nixos-25.11", datetime.date(2026, 6, 14)),
            "nixos-25.11",
        )
        self.assertEqual(
            bump.choose_branch(HEADS, "nixos-25.11", datetime.date(2026, 6, 15)),
            "nixos-26.05",
        )

    def test_a_november_release_moves_on_december_15(self):
        self.assertEqual(
            bump.choose_branch(HEADS, "nixos-26.05", datetime.date(2026, 12, 14)),
            "nixos-26.05",
        )
        self.assertEqual(
            bump.choose_branch(HEADS, "nixos-26.05", datetime.date(2026, 12, 15)),
            "nixos-26.11",
        )

    def test_a_settled_release_that_does_not_exist_is_not_taken(self):
        heads = {b: s for b, s in HEADS.items() if b != "nixos-26.11"}
        self.assertEqual(
            bump.choose_branch(heads, "nixos-26.05", datetime.date(2027, 1, 1)),
            "nixos-26.05",
        )

    def test_never_backwards(self):
        self.assertEqual(
            bump.choose_branch(HEADS, "nixos-26.05", datetime.date(2026, 1, 1)),
            "nixos-26.05",
        )

    def test_other_branches_are_ignored(self):
        heads = dict(
            HEADS,
            **{
                "nixos-unstable": "e" * 40,
                "nixos-26.05-small": "f" * 40,
                "nixos-27.02": "0" * 40,
            },
        )
        self.assertEqual(
            bump.choose_branch(heads, "nixos-26.05", datetime.date(2027, 6, 1)),
            "nixos-26.11",
        )

    def test_a_vanished_pinned_branch_fails(self):
        heads = {"nixos-25.05": "a" * 40}
        with self.assertRaises(SystemExit):
            bump.choose_branch(heads, "nixos-26.05", datetime.date(2026, 10, 6))


class NarHash(unittest.TestCase):
    def test_matches_nix(self):
        # `nix hash path --type sha256 --sri` of the same tree unpacked (Nix 2.31.5).
        self.assertEqual(
            bump.nar_hash(bump.tar_tree(tar_gz())),
            "sha256-EJSgZrxvqOXWm9hdZM7Uok8wp4FB1bh7ode5CF2Wgig=",
        )

    def test_the_top_directory_name_does_not_matter(self):
        self.assertEqual(
            bump.nar_hash(bump.tar_tree(tar_gz("x"))),
            bump.nar_hash(bump.tar_tree(tar_gz("y"))),
        )

    def test_hard_links_are_refused(self):
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tar:
            info = tarfile.TarInfo("top/hard")
            info.type, info.linkname = tarfile.LNKTYPE, "top/a"
            tar.addfile(info)
        with self.assertRaises(SystemExit):
            bump.tar_tree(buf.getvalue())


class Pin(unittest.TestCase):
    def test_the_shipped_registry_is_what_the_bump_writes(self):
        pin = bump.read_pin()
        self.assertTrue(bump.BRANCH_RE.match(pin["branch"]))
        self.assertEqual(
            json.dumps(bump.registry_for(pin), indent=2) + "\n",
            bump.REGISTRY.read_text(),
        )

    def test_title_and_body_name_a_branch_change(self):
        old = {"branch": "nixos-26.05", "rev": "c" * 40, "narHash": "sha256-old"}
        new = {"branch": "nixos-26.11", "rev": "d" * 40, "narHash": "sha256-new"}
        result = {"changed": True, "old": old, "new": new}
        self.assertEqual(
            bump.title(result), "chore(nix): bump nixpkgs to nixos-26.11-dddddddddddd"
        )
        self.assertIn("Release branch change", bump.body(result))
        self.assertNotIn(
            "Release branch change",
            bump.body(dict(result, new=dict(new, branch="nixos-26.05"))),
        )
OLD = {"branch": "nixos-26.05", "rev": "c" * 40, "narHash": "sha256-old"}
JUNE = datetime.date(2026, 7, 1)
DECEMBER = datetime.date(2026, 12, 20)


@contextlib.contextmanager
def faked(heads, status="ahead", pin=OLD):
    """The network faked at the function boundary: git ls-remote, the compare API and the
    archive download. Yields the compare and download mocks."""
    with (
        mock.patch.object(bump, "read_pin", return_value=pin),
        mock.patch.object(bump, "remote_heads", return_value=heads),
        mock.patch.object(bump, "compare_status", return_value=status) as cmp,
        mock.patch.object(bump, "archive_nar_hash", return_value="sha256-new") as nar,
    ):
        yield cmp, nar


class Compute(unittest.TestCase):
    def test_unchanged_pin_asks_nothing(self):
        with faked(dict(HEADS, **{"nixos-26.05": OLD["rev"]})) as (cmp, nar):
            result = bump.compute(JUNE)
        self.assertFalse(result["changed"])
        cmp.assert_not_called()
        nar.assert_not_called()

    def test_a_descendant_moves_the_pin_and_hashes_the_archive(self):
        pin = dict(OLD, rev="e" * 40)
        with faked(HEADS, pin=pin) as (cmp, nar):
            result = bump.compute(JUNE)
        self.assertEqual(
            result["new"],
            {
                "branch": "nixos-26.05",
                "rev": HEADS["nixos-26.05"],
                "narHash": "sha256-new",
            },
        )
        cmp.assert_called_once_with(pin["rev"], HEADS["nixos-26.05"])
        nar.assert_called_once_with(HEADS["nixos-26.05"])

    def test_a_rewritten_branch_fails_before_the_download(self):
        for status in ("diverged", "behind"):
            with self.subTest(status=status):
                with faked(HEADS, status=status, pin=dict(OLD, rev="e" * 40)) as (
                    _,
                    nar,
                ):
                    with self.assertRaises(SystemExit):
                        bump.compute(JUNE)
                    nar.assert_not_called()

    def test_a_release_change_accepts_sibling_branches_but_not_a_rewind(self):
        for status, ok in (("diverged", True), ("ahead", True), ("behind", False)):
            with self.subTest(status=status):
                with faked(HEADS, status=status, pin=dict(OLD, rev="e" * 40)):
                    if ok:
                        self.assertEqual(
                            bump.compute(DECEMBER)["new"]["branch"], "nixos-26.11"
                        )
                    else:
                        with self.assertRaises(SystemExit):
                            bump.compute(DECEMBER)


class CompareStatus(unittest.TestCase):
    def test_the_status_is_read_from_the_api_reply_with_the_token(self):
        reply = mock.MagicMock()
        reply.__enter__.return_value = io.BytesIO(b'{"status": "ahead"}')
        with (
            mock.patch.object(
                bump.urllib.request, "urlopen", return_value=reply
            ) as opened,
            mock.patch.dict(bump.os.environ, {"GITHUB_TOKEN": "t"}),
        ):
            self.assertEqual(bump.compare_status("a" * 40, "b" * 40), "ahead")
        req = opened.call_args[0][0]
        self.assertIn("/compare/" + "a" * 40 + "..." + "b" * 40, req.full_url)
        self.assertEqual(req.get_header("Authorization"), "Bearer t")


class Apply(unittest.TestCase):
    def run_apply(self, result):
        tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        reg, branch, out = tmp / "registry.json", tmp / "nixpkgs-branch", tmp / "out"
        reg.write_text("old\n")
        branch.write_text("nixos-26.05\n")
        with (
            mock.patch.object(bump, "compute", return_value=result),
            mock.patch.object(bump, "REGISTRY", reg),
            mock.patch.object(bump, "BRANCH_FILE", branch),
            mock.patch.object(sys, "argv", ["bump.py", "apply", str(out)]),
        ):
            bump.main()
        return reg, branch, out

    def test_a_move_rewrites_the_pin_and_writes_title_and_body(self):
        new = {"branch": "nixos-26.11", "rev": "d" * 40, "narHash": "sha256-new"}
        reg, branch, out = self.run_apply({"changed": True, "old": OLD, "new": new})
        self.assertEqual(json.loads(reg.read_text()), bump.registry_for(new))
        self.assertEqual(branch.read_text(), "nixos-26.11\n")
        self.assertEqual(
            (out / "title").read_text(),
            "chore(nix): bump nixpkgs to nixos-26.11-dddddddddddd\n",
        )
        self.assertIn("Never auto-merged", (out / "body.md").read_text())

    def test_no_move_writes_nothing(self):
        reg, branch, out = self.run_apply({"changed": False, "old": OLD, "new": OLD})
        self.assertEqual(reg.read_text(), "old\n")
        self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
