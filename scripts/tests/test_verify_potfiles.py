"""Unit tests of the potfiles check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

PROGRAM = "forge/specs/prog/prog-1.0.0"
SHARED = "system/shared"
TRANSLATED = 'pub fn label() -> String {\n    tr("Open")\n}\n'

# A program with two binaries, each behind a feature, and a shared crate whose pages are gtk
# or sal, as athanor-dock and athanor-apps are.
TREE = {
    "Cargo.toml": '[workspace]\nmembers = []\n\n[workspace.dependencies]\nanyhow = "1"\n',
    f"{PROGRAM}/Cargo.toml": (
        '[package]\nname = "prog"\n\n'
        '[features]\ndefault = ["gtk"]\ngtk = ["shared/gtk"]\ntoolkit = ["shared/sal"]\n\n'
        '[[bin]]\nname = "prog"\npath = "src/main.rs"\n\n'
        '[[bin]]\nname = "prog-toolkit"\npath = "src/toolkit/main.rs"\n\n'
        '[dependencies]\nshared = { path = "../../../../system/shared", default-features = false }\n'
        "anyhow = { workspace = true }\n"
    ),
    f"{PROGRAM}/src/main.rs": "mod ui;\n",
    f"{PROGRAM}/src/ui.rs": TRANSLATED,
    f"{PROGRAM}/src/toolkit/main.rs": "mod dock;\n",
    f"{PROGRAM}/src/toolkit/dock.rs": TRANSLATED,
    f"{SHARED}/Cargo.toml": (
        '[package]\nname = "shared"\n\n'
        '[features]\ndefault = ["gtk"]\ngtk = []\nsal = []\nextra = []\n'
    ),
    f"{SHARED}/src/lib.rs": (
        "pub mod entries;\n"
        '#[cfg(feature = "gtk")]\npub mod row;\n'
        '#[cfg(feature = "sal")]\npub mod sal;\n'
        '#[cfg(feature = "extra")]\nmod extra;\n'
        "#[cfg(test)]\nmod tests;\n"
    ),
    f"{SHARED}/src/entries.rs": (
        '// tr("commented") is no call\n'
        "pub fn none() {}\n\n"
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n"
        '        assert_eq!(tr("{app}"), "{app}");\n    }\n}\n'
    ),
    f"{SHARED}/src/row.rs": TRANSLATED,
    f"{SHARED}/src/sal.rs": TRANSLATED,
    f"{SHARED}/src/extra.rs": TRANSLATED,
    f"{SHARED}/src/tests.rs": TRANSLATED,
}

LISTED = [
    "src/ui.rs",
    "src/toolkit/dock.rs",
    "../../../../system/shared/src/row.rs",
    "../../../../system/shared/src/sal.rs",
]


def with_files(files):
    """TREE with `files` added or replaced."""
    tree = dict(TREE)
    tree.update(files)
    return tree


def edited(path, old, new, tree=None):
    tree = dict(TREE if tree is None else tree)
    assert old in tree[path], (path, old)
    tree[path] = tree[path].replace(old, new)
    return tree


def problems(listed, tree=None):
    with tempfile.TemporaryDirectory() as d:
        root = pathlib.Path(d)
        files = dict(TREE if tree is None else tree)
        files[f"{PROGRAM}/po/POTFILES.in"] = "".join(f"{line}\n" for line in listed)
        for path, text in files.items():
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            (root / path).write_text(text)
        return verify.potfiles_problems(root)


class PotfilesProblemsTest(unittest.TestCase):
    def test_every_translated_source_listed_passes(self):
        self.assertEqual(problems(LISTED), [])

    def test_a_dependency_source_left_out_is_named(self):
        found = problems([line for line in LISTED if not line.endswith("sal.rs")])
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/sal.rs", found[0])

    def test_a_source_of_another_binary_of_the_program_counts(self):
        found = problems([line for line in LISTED if line != "src/toolkit/dock.rs"])
        self.assertEqual(len(found), 1, found)
        self.assertIn(f"{PROGRAM}/src/toolkit/dock.rs", found[0])

    def test_a_dependency_module_behind_a_feature_the_program_never_turns_on_is_not_asked(
        self,
    ):
        # `extra` is never turned on, and `tests` is test code: neither is listed.
        self.assertEqual(problems(LISTED), [])

    def test_a_dependency_feature_the_program_does_not_ask_for_is_off(self):
        tree = dict(TREE)
        tree[f"{PROGRAM}/Cargo.toml"] = tree[f"{PROGRAM}/Cargo.toml"].replace(
            'toolkit = ["shared/sal"]', "toolkit = []"
        )
        listed = [line for line in LISTED if not line.endswith("sal.rs")]
        self.assertEqual(problems(listed, tree), [])

    def test_default_features_of_a_dependency_count_unless_turned_off(self):
        tree = dict(TREE)
        tree[f"{PROGRAM}/Cargo.toml"] = (
            tree[f"{PROGRAM}/Cargo.toml"]
            .replace('gtk = ["shared/gtk"]', "gtk = []")
            .replace(", default-features = false", "")
        )
        found = problems([line for line in LISTED if not line.endswith("row.rs")], tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/row.rs", found[0])

    def test_a_dependency_reached_with_no_feature_at_all_is_still_read(self):
        tree = dict(TREE)
        tree[f"{PROGRAM}/Cargo.toml"] = (
            tree[f"{PROGRAM}/Cargo.toml"]
            .replace('gtk = ["shared/gtk"]', "gtk = []")
            .replace('toolkit = ["shared/sal"]', "toolkit = []")
        )
        tree[f"{SHARED}/src/entries.rs"] = TRANSLATED
        found = problems(["src/ui.rs", "src/toolkit/dock.rs"], tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/entries.rs", found[0])


class ModuleTreeTest(unittest.TestCase):
    """How a module declaration reaches a file, in the shared crate's lib.rs."""

    def found(self, lib, files, listed=LISTED):
        tree = edited(
            f"{SHARED}/src/lib.rs", "pub mod entries;\n", "pub mod entries;\n" + lib
        )
        tree.update({f"{SHARED}/src/{k}": v for k, v in files.items()})
        return problems(listed, tree)

    def test_a_path_attribute_is_followed(self):
        found = self.found(
            '#[path = "elsewhere/p.rs"]\nmod p;\n', {"elsewhere/p.rs": TRANSLATED}
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/elsewhere/p.rs", found[0])

    def test_a_conditional_path_attribute_follows_its_predicate(self):
        def found(predicate, files):
            return self.found(
                f'#[cfg_attr({predicate}, path = "u.rs")]\nmod b;\n', files
            )

        both = {"u.rs": TRANSLATED, "b.rs": TRANSLATED}

        def unlisted(found, name):
            self.assertEqual(len(found), 1, found)
            self.assertIn(f"system/shared/src/{name} calls tr", found[0])
            self.assertIn("is not listed", found[0])

        def missing(found, name):
            self.assertEqual(len(found), 1, found)
            self.assertIn(
                f"declares a module whose file system/shared/src/{name}", found[0]
            )

        # Holds: the path applies, and src/b.rs is not read.
        unlisted(found('not(feature = "extra")', {"u.rs": TRANSLATED}), "u.rs")
        missing(found('not(feature = "extra")', {"b.rs": TRANSLATED}), "u.rs")
        # Does not hold: normal resolution.
        unlisted(found('feature = "extra"', {"b.rs": TRANSLATED}), "b.rs")
        missing(found('feature = "extra"', {"u.rs": TRANSLATED}), "b.rs")
        # Cannot be judged: either file is accepted, neither is a problem.
        unlisted(found('target_os = "linux"', {"u.rs": TRANSLATED}), "u.rs")
        unlisted(found('target_os = "linux"', {"b.rs": TRANSLATED}), "b.rs")
        # With both present, each is compiled for some target, so each is scanned.
        found_both = found('target_os = "linux"', both)
        self.assertEqual(len(found_both), 2, found_both)
        for name in ("u.rs", "b.rs"):
            self.assertTrue(
                any(f"system/shared/src/{name} calls tr" in f for f in found_both)
            )
        missing(found('target_os = "linux"', {}), "u.rs")
        # An unparsable predicate cannot be judged either.
        unlisted(found("not()", {"b.rs": TRANSLATED}), "b.rs")
        unlisted(found("not()", {"u.rs": TRANSLATED}), "u.rs")
        self.assertIsNone(verify.cfg_value("not()", set()))

    def test_each_conditional_path_attribute_is_read_on_its_own(self):
        lib = (
            '#[cfg_attr(feature = "extra", path = "x.rs")] '
            '#[cfg_attr(not(feature = "extra"), path = "w.rs")]\nmod b;\n'
        )
        found = self.found(lib, {"w.rs": TRANSLATED})
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/w.rs calls tr", found[0])

    def test_conditional_paths_that_cannot_be_judged_are_each_accepted(self):
        lib = (
            '#[cfg_attr(target_os = "linux", path = "l.rs")] '
            '#[cfg_attr(target_os = "freebsd", path = "f.rs")]\nmod b;\n'
        )
        for name in ("l.rs", "f.rs", "b.rs"):
            found = self.found(lib, {name: TRANSLATED})
            self.assertEqual(len(found), 1, (name, found))
            self.assertIn(f"system/shared/src/{name} calls tr", found[0])
        found = self.found(lib, {})
        self.assertEqual(len(found), 1, found)
        self.assertIn("declares a module whose file system/shared/src/l.rs", found[0])
        # Each file a build for some target compiles is scanned, not only the first one.
        found = self.found(lib, {"l.rs": TRANSLATED, "f.rs": TRANSLATED})
        self.assertEqual(len(found), 2, found)
        for name in ("l.rs", "f.rs"):
            self.assertTrue(
                any(f"system/shared/src/{name} calls tr" in f for f in found), found
            )

    def test_a_conditional_path_is_split_at_its_own_commas_and_quotes(self):
        # A comma and a parenthesis inside the quoted path do not end the attribute.
        found = self.found(
            '#[cfg_attr(unix, path = "a,)b.rs")]\nmod b;\n', {"a,)b.rs": TRANSLATED}
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/a,)b.rs calls tr", found[0])
        # The commas of a judgeable predicate do not split it: it holds, so u.rs is asked.
        found = self.found(
            '#[cfg_attr(all(not(feature = "extra"), not(feature = "none")), path = "u.rs")]'
            "\nmod b;\n",
            {"b.rs": TRANSLATED},
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("declares a module whose file system/shared/src/u.rs", found[0])
        # A predicate that looks like a path is no path.
        found = self.found(
            '#[cfg_attr(path = "u.rs", unix)]\nmod b;\n', {"u.rs": TRANSLATED}
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("declares a module whose file system/shared/src/b.rs", found[0])

    def test_a_conditional_path_in_an_inline_module_resolves_under_its_name(self):
        found = self.found(
            'mod a {\n    #[cfg_attr(unix, path = "u.rs")]\n    mod b;\n}\n',
            {"a/u.rs": TRANSLATED},
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/a/u.rs calls tr", found[0])

    def test_children_of_a_mod_rs_and_of_a_named_file(self):
        found = self.found(
            "pub mod dir;\npub mod file;\n",
            {
                "dir/mod.rs": "mod a;\n",
                "dir/a.rs": TRANSLATED,
                "file.rs": "mod b;\n",
                "file/b.rs": TRANSLATED,
            },
        )
        self.assertEqual(len(found), 2, found)
        self.assertIn("system/shared/src/dir/a.rs", found[0])
        self.assertIn("system/shared/src/file/b.rs", found[1])

    def test_a_child_of_an_inline_module_resolves_under_its_name(self):
        found = self.found(
            "pub mod outer {\n    pub mod inner;\n}\n", {"outer/inner.rs": TRANSLATED}
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/outer/inner.rs", found[0])

    def test_a_child_of_an_inline_module_turned_off_is_not_asked(self):
        found = self.found(
            '#[cfg(feature = "extra")]\nmod outer {\n    mod inner;\n}\n',
            {"outer/inner.rs": TRANSLATED},
        )
        self.assertEqual(found, [])

    def test_a_declared_module_without_its_file_is_a_problem(self):
        found = self.found("pub mod gone;\n", {})
        self.assertEqual(len(found), 1, found)
        self.assertIn(
            "declares a module whose file system/shared/src/gone.rs", found[0]
        )

    def test_a_module_kept_by_a_negated_predicate_that_cannot_be_judged(self):
        found = self.found(
            '#[cfg(not(target_os = "windows"))]\nmod unix;\n', {"unix.rs": TRANSLATED}
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/src/unix.rs", found[0])

    def test_a_declaration_in_a_block_comment_is_not_one(self):
        self.assertEqual(
            self.found("/* pub mod hidden; */\n", {"hidden.rs": TRANSLATED}), []
        )


class CrateGraphTest(unittest.TestCase):
    """How Cargo.toml reaches roots and dependencies."""

    def test_a_binary_in_its_own_directory_under_src_bin_counts(self):
        tree = with_files(
            {
                f"{PROGRAM}/src/bin/tool/main.rs": "mod page;\n",
                f"{PROGRAM}/src/bin/tool/page.rs": TRANSLATED,
            }
        )
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn(f"{PROGRAM}/src/bin/tool/page.rs", found[0])

    def test_a_bin_path_that_does_not_exist_is_a_problem(self):
        tree = dict(TREE)
        del tree[f"{PROGRAM}/src/toolkit/main.rs"]
        found = problems(
            [line for line in LISTED if line != "src/toolkit/dock.rs"], tree
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn(
            f"names {PROGRAM}/src/toolkit/main.rs, which does not exist", found[0]
        )

    def test_a_lib_path_is_the_dependency_root(self):
        tree = edited(
            f"{SHARED}/Cargo.toml",
            "[features]",
            '[lib]\npath = "lib/start.rs"\n\n[features]',
        )
        tree[f"{SHARED}/lib/start.rs"] = TRANSLATED
        found = problems(LISTED[:2], tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/lib/start.rs", found[0])

    def test_a_target_dependency_is_read(self):
        tree = edited(
            f"{SHARED}/Cargo.toml",
            "extra = []\n",
            "extra = []\n\n[target.'cfg(unix)'.dependencies]\ntdep = { path = \"../tdep\" }\n",
        )
        tree["system/tdep/Cargo.toml"] = '[package]\nname = "tdep"\n'
        tree["system/tdep/src/lib.rs"] = TRANSLATED
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/tdep/src/lib.rs", found[0])

    def optional(self, toolkit):
        # sal turns `inner` on with dep:; gtk asks it for `page` only if something else does.
        tree = edited(
            f"{SHARED}/Cargo.toml",
            "gtk = []\nsal = []\n",
            'gtk = ["inner?/page"]\nsal = ["dep:inner"]\n',
        )
        tree[f"{SHARED}/Cargo.toml"] += (
            '\n[dependencies]\ninner = { path = "../inner", optional = true }\n'
        )
        tree = edited(
            f"{PROGRAM}/Cargo.toml", 'toolkit = ["shared/sal"]', toolkit, tree
        )
        tree["system/inner/Cargo.toml"] = (
            '[package]\nname = "inner"\n\n[features]\npage = []\n'
        )
        tree["system/inner/src/lib.rs"] = '#[cfg(feature = "page")]\npub mod page;\n'
        tree["system/inner/src/page.rs"] = TRANSLATED
        return problems([line for line in LISTED if not line.endswith("sal.rs")], tree)

    def test_an_optional_dependency_turned_on_by_dep_takes_its_weak_features(self):
        found = self.optional('toolkit = ["shared/sal"]')
        names = [p for p in found if "system/inner/src/page.rs" in p]
        self.assertEqual(len(names), 1, found)

    def test_a_weak_feature_alone_does_not_turn_an_optional_dependency_on(self):
        self.assertEqual(self.optional("toolkit = []"), [])

    def test_a_workspace_dependency_resolves_through_the_root_manifest(self):
        tree = edited(
            "Cargo.toml",
            'anyhow = "1"\n',
            'anyhow = "1"\nwsdep = { path = "system/wsdep", features = ["x"] }\n',
        )
        tree[f"{PROGRAM}/Cargo.toml"] += "wsdep = { workspace = true }\n"
        tree["system/wsdep/Cargo.toml"] = (
            '[package]\nname = "wsdep"\n\n[features]\nx = []\n'
        )
        tree["system/wsdep/src/lib.rs"] = '#[cfg(feature = "x")]\nmod page;\n'
        tree["system/wsdep/src/page.rs"] = TRANSLATED
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/wsdep/src/page.rs", found[0])

    def test_a_workspace_dependency_the_workspace_does_not_declare_is_a_problem(self):
        tree = edited("Cargo.toml", 'anyhow = "1"\n', "")
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn(
            "takes anyhow from a workspace that does not declare it", found[0]
        )

    def test_an_unreadable_dependency_manifest_is_a_problem(self):
        tree = dict(TREE)
        del tree[f"{SHARED}/Cargo.toml"]
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/Cargo.toml cannot be read", found[0])

    def test_an_invalid_dependency_manifest_is_a_problem(self):
        tree = with_files({f"{SHARED}/Cargo.toml": "[package\n"})
        found = problems(LISTED, tree)
        self.assertEqual(len(found), 1, found)
        self.assertIn("system/shared/Cargo.toml cannot be read", found[0])


class RustCodeTest(unittest.TestCase):
    def test_comments_and_test_items_are_blanked_strings_kept(self):
        code, masked = verify.rust_code(
            'let a = "x { y"; // tr("c")\n#[cfg(test)]\nmod t { fn f() { tr("z"); } }\nfn g() {}\n'
        )
        self.assertEqual(len(code), len(masked))
        self.assertIn('"x { y"', code)
        self.assertNotIn("x { y", masked)
        self.assertNotIn("tr(", code)
        self.assertIn("fn g() {}", code)

    def test_a_char_literal_quote_opens_no_string(self):
        code, _ = verify.rust_code('let q = \'"\';\nlet s = tr("Open");\n')
        self.assertTrue(verify.GETTEXT_CALL.search(code))

    def test_a_brace_in_a_char_literal_keeps_a_test_module_balanced(self):
        code, _ = verify.rust_code(
            "#[cfg(test)]\nmod t { const C: char = '{'; }\nfn g() { tr(\"Open\"); }\n"
        )
        self.assertTrue(verify.GETTEXT_CALL.search(code))
        code, _ = verify.rust_code(
            "#[cfg(test)]\nmod t { const C: char = '}'; fn f() { tr(\"z\"); } }\n"
        )
        self.assertFalse(verify.GETTEXT_CALL.search(code))

    def test_raw_strings(self):
        code, _ = verify.rust_code(
            '#[cfg(test)]\nmod t { const S: &str = r#"}"#; fn f() { tr("z"); } }\n'
            'fn g() { tr(r#"Open"#); }\n'
        )
        self.assertNotIn('tr("z")', code)
        self.assertTrue(verify.GETTEXT_CALL.search(code))

    def test_raw_byte_strings(self):
        code, _ = verify.rust_code(
            '#[cfg(test)]\nmod t { const S: &[u8] = br#"{\\"#; const P: &[u8] = b"}"; '
            'fn f() { tr("z"); } }\nfn g() { tr(r#"Open"#); }\n'
        )
        self.assertNotIn('tr("z")', code)
        self.assertTrue(verify.GETTEXT_CALL.search(code))

    def test_a_block_comment_is_blanked(self):
        code, _ = verify.rust_code('/* tr("x") */ fn f() {}\n')
        self.assertFalse(verify.GETTEXT_CALL.search(code))

    def test_cfg_predicates(self):
        on = {"gtk"}
        self.assertTrue(verify.cfg_holds('feature = "gtk"', on))
        self.assertFalse(verify.cfg_holds('feature = "sal"', on))
        self.assertFalse(verify.cfg_holds("test", on))
        self.assertTrue(verify.cfg_holds('not(feature = "sal")', on))
        self.assertTrue(verify.cfg_holds('any(feature = "sal", feature = "gtk")', on))
        self.assertFalse(verify.cfg_holds('all(feature = "gtk", test)', on))
        self.assertTrue(verify.cfg_holds('target_os = "linux"', on))
        self.assertTrue(verify.cfg_holds('not(target_os = "linux")', on))
        self.assertTrue(verify.cfg_holds('not(all(unix, feature = "gtk"))', on))
        self.assertFalse(verify.cfg_holds('not(any(unix, feature = "gtk"))', on))


if __name__ == "__main__":
    unittest.main()
