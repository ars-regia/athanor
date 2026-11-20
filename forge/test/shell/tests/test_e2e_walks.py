"""Every AT-SPI tree walk of an end-to-end suite goes through the retrying helpers of
atspi_check (collect, walk, afresh): a node a rebuilt popover destroyed mid-walk must start
the walk again, not fail the job (#1249). `children` is the unretried primitive."""

import ast
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent
RAW = {"get_child_at_index", "get_child_count", "children"}


def raw_reads(source):
    found = []
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Attribute) and node.attr in RAW:
            found.append((node.lineno, node.attr))
        elif isinstance(node, ast.Name) and node.id in RAW:
            found.append((node.lineno, node.id))
        elif isinstance(node, ast.alias) and node.name in RAW:
            found.append((node.lineno, node.name))
    return found


# The helpers that are the retry or that only run inside it.
HELPERS = {"children", "collect", "walk_once"}
RETRIES = {"afresh", "collect", "walk"}


def unretried(source, names):
    """(function, line) of each raw read, in a function of `names`, outside a call of a retrying helper."""
    found = []

    def visit(node, function, retried):
        if isinstance(node, ast.FunctionDef) and not function:
            function = node.name
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in RETRIES:
            retried = True
        if function in names and function not in HELPERS and not retried and (
            (isinstance(node, ast.Name) and node.id in RAW) or (isinstance(node, ast.Attribute) and node.attr in RAW)
        ):
            found.append((function, node.lineno))
        for child in ast.iter_child_nodes(node):
            visit(child, function, retried)

    visit(ast.parse(source), None, False)
    return found


class E2eWalks(unittest.TestCase):
    def test_no_end_to_end_module_reads_the_tree_outside_the_helpers(self):
        modules = sorted(SHELL.glob("*_e2e.py"))
        self.assertTrue(modules)
        found = {m.name: raw_reads(m.read_text()) for m in modules}
        self.assertEqual({name: hits for name, hits in found.items() if hits}, {})

    def test_the_scan_sees_a_raw_read(self):
        source = "from atspi_check import children\nfor c in children(n): n.get_child_count()\n"
        self.assertEqual(len(raw_reads(source)), 3)

    def test_every_atspi_check_function_an_end_to_end_module_imports_retries_its_reads(self):
        imported = set()
        for module in SHELL.glob("*_e2e.py"):
            for node in ast.walk(ast.parse(module.read_text())):
                if isinstance(node, ast.ImportFrom) and node.module == "atspi_check":
                    imported.update(alias.name for alias in node.names)
        self.assertTrue(imported)
        self.assertEqual(unretried(SHELL.joinpath("atspi_check.py").read_text(), imported), [])

    def test_the_function_scan_sees_an_unretried_read(self):
        source = (
            "def bad(d):\n    return [a for a in children(d)]\n"
            "def good(d):\n    return afresh(lambda: [a for a in children(d)])\n"
        )
        self.assertEqual(unretried(source, {"bad", "good"}), [("bad", 2)])


if __name__ == "__main__":
    unittest.main()
