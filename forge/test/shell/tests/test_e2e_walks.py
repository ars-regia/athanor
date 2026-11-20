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


class E2eWalks(unittest.TestCase):
    def test_no_end_to_end_module_reads_the_tree_outside_the_helpers(self):
        modules = sorted(SHELL.glob("*_e2e.py"))
        self.assertTrue(modules)
        found = {m.name: raw_reads(m.read_text()) for m in modules}
        self.assertEqual({name: hits for name, hits in found.items() if hits}, {})

    def test_the_scan_sees_a_raw_read(self):
        source = "from atspi_check import children\nfor c in children(n): n.get_child_count()\n"
        self.assertEqual(len(raw_reads(source)), 3)


if __name__ == "__main__":
    unittest.main()
