"""Every fork's fcNN follows the base release (Fedora 45 rebase plan, task 4a).

Each `fcNN` on a `%global fedora_release`, a `Release:` or a versioned `Requires:` line of
`forge/specs/*/*.spec` must equal N of `ARG FEDORA_VERSION=N` in `system/Containerfile`.
`%changelog` sections are history and are ignored.
"""

import re
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE_ARG = re.compile(r"^ARG FEDORA_VERSION=(\d+)$", re.MULTILINE)
CHECKED = re.compile(
    r"^(?:%global\s+fedora_release\b|Release:|Requires(?:\([^)]*\))?:.*[<>=])"
)
DISTTAG = re.compile(r"\bfc(\d+)\b")


def base_release(root: Path) -> int:
    text = (root / "system" / "Containerfile").read_text(encoding="utf-8")
    found = BASE_ARG.search(text)
    if found is None:
        raise AssertionError(
            "system/Containerfile has no 'ARG FEDORA_VERSION=<N>' line"
        )
    return int(found.group(1))


def fork_pins(root: Path) -> list[tuple[str, int, int]]:
    """(spec path, line number, N of fcN) for every checked line carrying an fcN."""
    pins = []
    for spec in sorted((root / "forge" / "specs").glob("*/*.spec")):
        for number, line in enumerate(spec.read_text(encoding="utf-8").splitlines(), 1):
            if line.startswith("%changelog"):
                break
            if CHECKED.match(line):
                pins += [
                    (str(spec.relative_to(root)), number, int(n))
                    for n in DISTTAG.findall(line)
                ]
    return pins


def mismatches(root: Path) -> tuple[list[str], set[str]]:
    base = base_release(root)
    pins = fork_pins(root)
    bad = [f"{p}:{n}: fc{v}, base is {base}" for p, n, v in pins if v != base]
    return bad, {p for p, _, _ in pins}


class ForkReleasesFollowBase(unittest.TestCase):
    def test_every_fork_names_the_base_release(self):
        bad, specs = mismatches(ROOT)
        self.assertTrue(
            specs, "no spec carries an fcNN: the check would pass vacuously"
        )
        self.assertEqual(bad, [], f"specs with an fcNN: {sorted(specs)}")


class NegativeControl(unittest.TestCase):
    def tree(self, base: int, spec: str) -> Path:
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / "system").mkdir()
        (root / "system" / "Containerfile").write_text(
            f"FROM scratch\nARG FEDORA_VERSION={base}\nENV FEDORA_VERSION=$FEDORA_VERSION\n"
        )
        (root / "forge" / "specs" / "demo").mkdir(parents=True)
        (root / "forge" / "specs" / "demo" / "demo.spec").write_text(spec)
        return root

    def test_stale_fork_fails(self):
        for line in (
            "%global fedora_release 2.fc43",
            "Release: 2.fc43.athanor1",
            "Release:        %{?autorelease}%{!?autorelease:71.fc43}",
            "Requires: cosmic-comp >= 1.10.0-2.fc43.athanor2",
        ):
            with self.subTest(line=line):
                bad, specs = mismatches(self.tree(45, f"Name: demo\n{line}\n"))
                self.assertEqual(len(bad), 1)
                self.assertEqual(len(specs), 1)

    def test_current_fork_passes(self):
        bad, specs = mismatches(self.tree(45, "Release: 1.fc45.athanor1\n"))
        self.assertEqual((bad, len(specs)), ([], 1))

    def test_changelog_is_ignored(self):
        spec = "Release: 1.fc45\n%changelog\n- Release: rebuilt for fc42\n"
        self.assertEqual(mismatches(self.tree(45, spec))[0], [])

    def test_missing_base_arg_fails_loudly(self):
        root = self.tree(45, "Release: 1.fc45\n")
        (root / "system" / "Containerfile").write_text("FROM scratch\n")
        with self.assertRaises(AssertionError):
            base_release(root)


if __name__ == "__main__":
    unittest.main()
