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

    def test_every_form_of_the_store_s_directory_fails(self):
        holes = {
            "system/a/src/path.rs": 'let p = Path::new(".config/athanor/sulphur/input.toml");\n',
            "system/b/src/join.rs": 'let p = home.join("athanor").join("sulphur");\n',
            "system/c/src/split.rs": "let p = home\n    .join(\"athanor\")\n    .join(\"sulphur\");\n",
            "system/d/src/fmt.rs": 'let p = format!("{xdg}/athanor/sulphur/bindings.toml");\n',
            "system/e/src/slice.rs": 'let p: PathBuf = [home, "athanor", "sulphur"].iter().collect();\n',
            "forge/specs/f/data/f.service": "[Service]\nReadWritePaths=%h/.config/athanor/sulphur\n",
            "forge/specs/g/data/g.service.d/10-x.conf": "[Service]\nReadWritePaths=%h/.config/athanor/sulphur/\n",
            "forge/specs/h/data/h.path": "[Path]\nPathChanged=%h/.config/athanor/sulphur/input.toml\n",
            "system/i/src/home.rs": 'const P: &str = "/var/home/me/.config/athanor/sulphur";\n',
            "system/j/src/concat.rs": 'const P: &str = concat!("athanor", "/", "sulphur");\n',
            "system/k/src/slash.rs": 'let p = home.join("athanor/").join("sulphur");\n',
            "system/l/store-namer.sh": 'install -d "$XDG_CONFIG_HOME/athanor/sulphur"\n',
            "forge/test/m/run": '#!/usr/bin/env bash\ncp input.toml ~/.config/athanor/sulphur/\n',
            "system/n/src/tail.rs": 'let p = home.join("athanor").join("sulphur/input.toml");\n',
            "system/o/src/lead.rs": 'let p = home.join(".config/athanor").join("sulphur");\n',
            "system/p/src/double.rs": 'const P: &str = "athanor//sulphur";\n',
            "system/q/src/dot.rs": 'const P: &str = "athanor/./sulphur";\n',
            "forge/specs/r/data/r.service": "[Service]\nExecStart=cp x /etc/skel/.config/athanor/sulphur/\n",
            "system/s/seed-skel.sh": "install -d /usr/etc/skel/.config/athanor/sulphur\n",
        }
        for name, text in holes.items():
            with self.subTest(name):
                found, _ = problems({name: text})
                self.assertEqual(len(found), 1, found)
                self.assertTrue(found[0].startswith(name + ":"), found)

    def test_system_paths_and_comments_pass(self):
        found, _ = problems(
            {
                "system/a/src/ok.rs": 'const L: &str = "/usr/share/athanor/sulphur/privileged-clients";\n'
                "// home.join(\"athanor\").join(\"sulphur\") in a comment\n"
                'const N: &str = "athanor/sulphurous";\n'
                'const M: &str = "other-athanor/sulphur";\n',
                "forge/specs/b/data/b.service": "[Service]\n# ~/.config/athanor/sulphur\n"
                "ReadOnlyPaths=/etc/xdg/athanor/sulphur\n",
                "system/c/store-reader.sh": "ls /var/lib/athanor/sulphur\n# ls ~/.config/athanor/sulphur\n",
                "forge/test/d/notes.txt": "~/.config/athanor/sulphur is not a script\n",
            }
        )
        self.assertEqual(found, [])

    def test_the_repository_passes(self):
        found, _ = verify.preference_store_problems(verify.ROOT)
        self.assertEqual(found, [])

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

    def test_a_write_that_names_no_pub_const_of_its_module_fails(self):
        writes = {
            "literal": 'fn set(store: &Store) {\n    store.update("input.toml", |_, _| Ok(()))\n}\n',
            "private": 'const F: &str = "input.toml";\nfn set(store: &Store) {\n'
            "    store\n        .update(F, |_, _| Ok(()))\n}\n",
            "other": "fn set(store: &Store) {\n    store.update(input::FILE, |_, _| Ok(()))\n}\n",
            "path": 'fn set(store: &Store) {\n    Store::update(store, "input.toml", |_, _| Ok(()))\n}\n',
        }
        for case, source in writes.items():
            with self.subTest(case):
                found, _ = problems({CLIENT + "settings.rs": source})
                self.assertEqual(len(found), 1, found)
                self.assertTrue(found[0].startswith(CLIENT + "settings.rs:"), found)
                self.assertIn("not a `pub const` of this module", found[0])

    def test_a_write_through_the_module_s_pub_const_and_a_test_s_write_pass(self):
        source = INPUT.replace(
            "#[cfg(test)]",
            "fn set(store: &Store) {\n    store.update(FILE, |_, _| Ok(()))\n}\n"
            "fn hash(h: &mut Hasher) {\n    h.update(b\"x\");\n}\n"
            'const DOC: &str = "store.update(OTHER, |_, _| Ok(()))";\n#[cfg(test)]',
        ) + (
            "#[cfg(test)]\nmod tests {\n    fn t(store: &Store) {\n"
            '        store.update("x.toml", |_, _| Ok(()));\n    }\n}\n'
        )
        found, _ = problems({CLIENT + "input.rs": source})
        self.assertEqual(found, [])

    def test_a_write_after_a_test_item_a_comment_or_a_string_that_names_cfg_test_fails(self):
        stray = 'fn set(store: &Store) {\n    store.update("other.toml", |_, _| Ok(()))\n}\n'
        before = {
            "helper": "#[cfg(test)]\nfn helper() {}\n",
            "comment": "// #[cfg(test)]\n",
            "string": 'const S: &str = "#[cfg(test)] mod tests {";\n',
        }
        for case, text in before.items():
            with self.subTest(case):
                found, _ = problems({CLIENT + "settings.rs": text + stray})
                self.assertEqual(len(found), 1, found)
                self.assertIn("writes the store with `\"other.toml\"`", found[0])

    def test_a_file_of_a_row_whose_module_is_not_built_fails_in_another_module(self):
        found, _ = problems(
            {
                CLIENT + "settings.rs": 'pub const X: &str = "keyboard.toml";\n'
                "fn set(store: &Store) {\n    store.update(X, |_, _| Ok(()))\n}\n"
            }
        )
        self.assertEqual(
            found, ["keyboard.toml has writer modules besides keyboard: settings"]
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
