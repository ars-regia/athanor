"""Unit tests of the panics check of `verify.py`: a module declared `#[cfg(test)] mod name;`
is test code, as a `#[cfg(test)]` item is, and code after that item is not
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class TestModuleFilesTest(unittest.TestCase):
    def test_only_modules_declared_under_cfg_test_are_test_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp)
            (src / "lib.rs").write_text("pub mod model;\n#[cfg(test)]\nmod fixture;\n")
            (src / "model.rs").write_text("#[cfg(test)] pub(crate) mod helper;\n")
            found = verify.test_module_files([src / "lib.rs", src / "model.rs"])
        self.assertIn(src / "fixture.rs", found)
        self.assertIn(src / "model" / "helper.rs", found)
        self.assertNotIn(src / "model.rs", found)


class WithoutTestItemsTest(unittest.TestCase):
    def test_code_after_a_test_item_is_kept_and_lines_hold(self):
        src = (
            "#[cfg(test)]\n"
            "fn helper() -> u8 { let s = \"}\"; let c = '}'; 1 }\n"
            "fn real<'a>(x: &'a str) -> u8 { x.parse().unwrap() }\n"
            "#[cfg(test)]\n"
            "#[allow(dead_code)]\n"
            "mod tests {\n"
            '    const RAW: &str = r#"{"a":"}"}"#;\n'
            '    fn t() { let v: [u8; 2] = [0; 2]; v.first().expect("x"); }\n'
            "}\n"
            'fn after() { None::<u8>.expect("after"); }\n'
        )
        kept = verify.without_test_items(src)
        self.assertEqual(kept.count("\n"), src.count("\n"))
        lines = kept.split("\n")
        self.assertNotIn("helper", lines[1])
        self.assertIn(".unwrap()", lines[2])
        self.assertEqual(kept.count(".expect("), 1)
        self.assertIn(".expect(", lines[9])

    def test_a_test_module_declared_with_a_semicolon_ends_there(self):
        kept = verify.without_test_items(
            "#[cfg(test)]\nmod fixture;\nfn f() { g().unwrap() }\n"
        )
        self.assertIn(".unwrap()", kept)
        self.assertNotIn("fixture", kept)

    def test_production_code_is_never_blanked(self):
        prod = '\nfn prod() { panic!("x"); }\n'
        for before in (
            "/// uses #[cfg(test)] for tests",
            "// see #[cfg(test)] here",
            "/* #[cfg(test)] */",
            'const S: &str = "#[cfg(test)]";',
            "struct S { a: u8, #[cfg(test)] b: u8, }",
            "enum E { A, #[cfg(test)] B(u8), C }",
            "fn f(#[cfg(test)] a: u8, b: u8) {}",
            "fn g() -> S { S { a: 1, #[cfg(test)] b: 2 } }",
            "#[cfg(test)] mod t { /* a /* b */ { */ }",
            '#[cfg(test)] const C: &CStr = cr#"a"{b"#;',
            "#[cfg(test)] const Q: [char; 2] = ['\\'', '{'];",
            "#[cfg(test)] fn h<T, U>() where T: Copy, U: Copy { }",
            "#[cfg(test)]\nmacro_rules! m { ($a:expr, $b:expr) => {}; }",
        ):
            with self.subTest(before=before):
                self.assertIn('panic!("x")', verify.without_test_items(before + prod))

    def test_test_items_with_commas_are_blanked_whole(self):
        kept = verify.without_test_items(
            "#[cfg(test)]\nfn h<T, U>(a: T, b: U) where T: Copy, U: Copy { a.unwrap(); }\n"
            "#[cfg(test)]\nmatch x { _ => y.unwrap(), }\n"
        )
        self.assertNotIn("unwrap", kept)


if __name__ == "__main__":
    unittest.main()
