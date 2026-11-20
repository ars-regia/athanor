"""Unit tests of scripts/verify.py as a module
(python3 -B -m unittest discover -s scripts/tests -v)."""

import ast
import collections
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"


class ModuleTest(unittest.TestCase):
    def test_no_top_level_name_is_bound_twice(self):
        """A second binding replaces the first for every check that reads the name at call time,
        as a second DEPENDENCY_TABLES once dropped the underscore tables from the boundary check."""
        bound = collections.Counter()
        for node in ast.parse(SCRIPT.read_text()).body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                bound[node.name] += 1
            elif isinstance(node, (ast.Assign, ast.AnnAssign)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                for target in targets:
                    for name in ast.walk(target):
                        if isinstance(name, ast.Name):
                            bound[name.id] += 1
        self.assertEqual([name for name, n in bound.items() if n > 1], [])


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
