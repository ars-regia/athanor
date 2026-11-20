"""Unit tests of the zbus-replies check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

HEAD = "#[interface(name = \"x.Y\")]\nimpl Y {\n"


def method(body, arg="#[zbus(connection)] conn: &Connection"):
    return f"{HEAD}    async fn m(&self, {arg}) -> fdo::Result<()> {{\n{body}\n    }}\n}}\n"


class ZbusRepliesTest(unittest.TestCase):
    def problems(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "a.rs").write_text(text)
            return verify.zbus_replies([root / "a.rs"], root)

    def test_proxy_built_on_the_serving_connection_fails(self):
        found = self.problems(method("DBusProxy::new(conn).await?.get_name_owner(n).await?;"))
        self.assertEqual(len(found), 1)
        self.assertIn("a.rs:4", found[0])

    def test_helper_awaited_with_the_connection_fails(self):
        self.assertEqual(len(self.problems(method("owned_by(conn, s).await?;"))), 1)

    def test_call_on_the_connection_fails(self):
        self.assertEqual(len(self.problems(method("conn.call_method(d, p, i, m, &()).await?;"))), 1)

    def test_query_connection_through_self_passes(self):
        self.assertEqual(self.problems(method("DBusProxy::new(&self.query).await?;")), [])

    def test_emitters_and_object_server_pass(self):
        body = (
            "emit_closed(conn, &self.state, 1).await;\n"
            "let e = SignalEmitter::new(conn, P)?;\n"
            "conn.object_server().at(P, X).await?;"
        )
        self.assertEqual(self.problems(method(body)), [])

    def test_a_method_without_the_connection_argument_passes(self):
        self.assertEqual(self.problems(method("DBusProxy::new(&c).await?;", arg="x: u32")), [])

    def test_code_outside_an_interface_passes(self):
        text = "async fn f(conn: &Connection) { DBusProxy::new(conn).await; }\n"
        self.assertEqual(self.problems(text), [])

    def test_test_code_is_skipped(self):
        text = "#[cfg(test)]\nmod tests {\n" + method("DBusProxy::new(conn).await?;") + "}\n"
        self.assertEqual(self.problems(text), [])


if __name__ == "__main__":
    unittest.main()
