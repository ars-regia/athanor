"""Unit tests of the panics check of `verify.py`: a module declared `#[cfg(test)] mod name;`
is test code, as a `#[cfg(test)]` item is, and code after that item is not
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock

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


class PanickingMacrosTest(unittest.TestCase):
    """`assert!` and the other macros that panic are counted as code outside tests, a
    `debug_assert!` is not, and a test item is excluded as before."""

    def counts(self, src):
        with (
            mock.patch.object(verify, "rust_files", lambda: [pathlib.Path("a.rs")]),
            mock.patch.object(verify, "read", lambda p: src),
            mock.patch.object(verify, "BUDGET", {k: 0 for k in {*verify.BUDGET, *MACROS}}),
        ):
            res = verify.check_panics()
        return sorted(p.split(": ")[1].split(" over")[0] for p in res.problems)

    def test_each_panicking_macro_is_counted(self):
        self.assertLessEqual(set(MACROS), set(verify.BUDGET))
        src = "".join(f"fn f{i}() {{ {m}(x); }}\n" for i, m in enumerate(MACROS))
        self.assertEqual(self.counts(src), sorted(MACROS))

    def test_a_macro_counts_with_any_delimiter_and_space(self):
        src = "fn f() { assert! (a); unreachable! {}; todo![]; }\n"
        self.assertEqual(self.counts(src), ["assert!(", "todo!(", "unreachable!("])

    def test_debug_assert_and_test_code_are_not_counted(self):
        src = (
            "fn f() { debug_assert!(a); debug_assert_eq!(a, b); debug_assert_ne!(a, b); }\n"
            "#[cfg(test)]\nmod tests { fn t() { assert!(a); assert_eq!(a, b); todo!(); } }\n"
            "fn g() { // assert!(a)\n}\n"
        )
        self.assertEqual(self.counts(src), [])

    def test_text_that_is_not_a_call_counts_zero(self):
        src = (
            'fn a() { let s = "x.unwrap() assert!(x)"; }\n'
            'fn b() { let s = r#"panic!(\\"q\\") .expect("#; let t = r"todo!(x)"; }\n'
            'fn c() { let s = b"unreachable!(x)"; let e = "\\" .unwrap()"; }\n'
            "fn d() { let q = '\"'; let e = '\\''; } // assert!(x)\n"
            "/* assert!(x) /* nested .unwrap() */ still .expect( */\n"
            "fn e() {}\n"
        )
        self.assertEqual(self.counts(src), [])

    def test_raw_strings_and_character_escapes_are_lexed(self):
        # Each case lexes differently under the plain-string rule: a raw string keeps its inner
        # `"` and ends at no backslash, and an escaped `'\"'` does not open a string.
        cases = (
            ('fn a() { let s = r#"a"b.unwrap()"#; }\n', []),
            ('fn a() { let s = r"C:\\"; x.unwrap(); }\n', [".unwrap()"]),
            ("fn a() { let q = '\\\"'; x.unwrap(); let s = \"a\"; }\n", [".unwrap()"]),
        )
        for src, want in cases:
            with self.subTest(src=src):
                self.assertEqual(self.counts(src), want)

    def test_a_url_in_a_string_does_not_hide_the_call_after_it(self):
        # The old `split("//")` cut the line at the `//` of the URL and missed the unwrap.
        src = 'fn a() { let u = "http://x"; x.unwrap(); }\n'
        self.assertEqual(self.counts(src), [".unwrap()"])

    def test_code_after_literals_and_comments_still_counts(self):
        src = "fn a() { let q = '\"'; /* /* */ x.unwrap() */ assert!(\"m\"); }\n"
        self.assertEqual(self.counts(src), ["assert!("])
        for tail, key in (("x.unwrap()", ".unwrap()"), ("x.expect(1)", ".expect("), ("panic!(1)", "panic!(")):
            with self.subTest(tail=tail):
                src = f"fn a() {{ let s = '\"'; /* /* */ q.unwrap() */ {tail}; }}\n"
                self.assertEqual(self.counts(src), [key])

    def test_a_macro_rules_body_counts_where_it_panics(self):
        src = "macro_rules! m { () => { assert!(x) } }\n"
        self.assertEqual(self.counts(src), ["assert!("])

    def test_without_literals_keeps_line_numbers(self):
        src = 'let a = "x\ny"; /* a\nb */ let b = 1;\n'
        out = verify.without_literals(src)
        self.assertEqual(out.count("\n"), src.count("\n"))
        self.assertEqual(len(out), len(src))


MACROS = (
    "assert!(", "assert_eq!(", "assert_ne!(", "unreachable!(", "todo!(", "unimplemented!(",
)


if __name__ == "__main__":
    unittest.main()
