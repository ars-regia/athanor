"""Unit tests of the nixpkgs registry bump, without network
(python3 -B -m unittest discover -s forge/specs/athanor-nix-support/tests -v)."""

import datetime
import io
import json
import pathlib
import sys
import tarfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
