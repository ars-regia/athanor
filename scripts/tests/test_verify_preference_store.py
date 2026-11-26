"""Unit tests of the preference-store check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

CLIENT = "system/athanor-sulphur-client/src/"
TABLE = (
    "Decision 38.\n\n"
    "| File | Family | Writer module | Readers | Specification |\n"
    "| --- | --- | --- | --- | --- |\n"
    "| `input.toml` | input | `athanor_sulphur_client::input` | Sulphur | SE15 |\n"
    "| `keyboard.toml` | keyboard | `athanor_sulphur_client::keyboard` | Sulphur | LN9 |\n"
    "\nAfter the table.\n"
)
INPUT = 'pub const FILE: &str = "input.toml";\n#[cfg(test)]\nconst OTHER: &str = "x.toml";\n'


def problems(files, table=TABLE):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        files = {verify.STORE_TABLE_DOC: table, CLIENT + "input.rs": INPUT, **files}
        for name, text in files.items():
            if text is None:
                continue
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.preference_store_problems(root)


class PreferenceStoreTest(unittest.TestCase):
    def test_a_table_whose_built_writers_name_their_file_passes(self):
        found, notes = problems({})
        self.assertEqual(found, [])
        self.assertEqual(
            notes, ["keyboard.toml: its writer module keyboard is not built yet"]
        )

    def test_an_indented_table_and_a_module_before_its_step_pass(self):
        table = "".join(
            "    " + line if line.startswith("|") else line
            for line in TABLE.splitlines(keepends=True)
        )
        found, notes = problems(
            {CLIENT + "keyboard.rs": "pub fn layouts() {}\n"}, table=table
        )
        self.assertEqual(found, [])
        self.assertEqual(
            notes,
            ["keyboard.toml: its writer module keyboard does not write the store yet"],
        )

    def test_code_outside_the_client_that_names_the_store_fails(self):
        found, _ = problems(
            {
                "system/athanor-shelld/src/seed.rs": 'const DIR: &str = "athanor/sulphur";\n'
                "// athanor/sulphur in a comment is not code\n"
                'const SYSTEM: &str = "/usr/share/athanor/sulphur/privileged-clients";\n',
                "forge/specs/x/data/x.service": "[Service]\nConfigurationDirectory=athanor/sulphur\n",
            }
        )
        self.assertEqual(
            sorted(f.split(" ")[0] for f in found),
            ["forge/specs/x/data/x.service:2", "system/athanor-shelld/src/seed.rs:1"],
        )

    def test_a_row_with_two_writer_modules_or_none_fails(self):
        table = TABLE.replace(
            "`athanor_sulphur_client::keyboard`",
            "`athanor_sulphur_client::keyboard`, `athanor_sulphur_client::input`",
        ).replace(
            "`athanor_sulphur_client::input` | Sulphur | SE15", "none | Sulphur | SE15"
        )
        found, _ = problems({}, table=table)
        self.assertEqual(len([f for f in found if "writer modules, not one" in f]), 2)

    def test_a_module_that_writes_another_file_fails(self):
        found, _ = problems(
            {CLIENT + "input.rs": 'pub const FILE: &str = "keyboard.toml";\n'}
        )
        self.assertIn(
            "system/athanor-sulphur-client/src/input.rs: the writer of input.toml in decision 38's "
            "table does not name it",
            found,
        )

    def test_a_second_writer_of_a_file_fails(self):
        found, _ = problems(
            {CLIENT + "settings.rs": 'pub const FILE: &str = "input.toml";\n'}
        )
        self.assertEqual(
            found, ["input.toml has writer modules besides input: settings"]
        )

    def test_a_file_the_table_does_not_list_fails(self):
        found, _ = problems(
            {CLIENT + "bindings.rs": 'pub const FILE: &str = "bindings.toml";\n'}
        )
        self.assertEqual(
            found,
            [
                "system/athanor-sulphur-client/src/bindings.rs writes bindings.toml, which "
                "decision 38's table does not list"
            ],
        )

    def test_no_table_fails(self):
        found, _ = problems({}, table="Decision 38, with no table.\n")
        self.assertTrue(any("no table of the store's files" in f for f in found))


if __name__ == "__main__":
    unittest.main()
