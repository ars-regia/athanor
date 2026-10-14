"""Unit tests of the athanor-bus-api residue check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class BusApiResidueTest(unittest.TestCase):
    def problems(self, files):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in files.items():
                path = root / "system" / "athanor-bus-api" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            return verify.bus_api_residue(root)

    def test_clean_crate_passes(self):
        self.assertEqual(self.problems({
            "src/lib.rs": "pub mod polkit;\n", "src/polkit.rs": "",
            "Cargo.toml": '[dependencies]\nserde = { workspace = true }\n'}), [])

    def test_extra_module_fails(self):
        self.assertEqual(len(self.problems({
            "src/lib.rs": "pub mod polkit;\n", "src/socket.rs": "", "src/polkit.rs": ""})), 1)

    def test_mesh_type_and_crypto_dependency_fail(self):
        found = self.problems({
            "src/lib.rs": "pub struct MeshPeerInfo;\n",
            "Cargo.toml": '[dependencies]\nx25519-dalek = "2"\nring = "0.17"\n'})
        self.assertEqual(len(found), 3)

    def test_nested_mesh_module_fails(self):
        found = self.problems({"src/lib.rs": "pub mod polkit;\n", "src/polkit.rs": "",
                               "src/net/peers.rs": "struct MeshPeer;\n"})
        self.assertEqual(len(found), 2)

    def test_tests_dir_and_build_rs_are_searched(self):
        found = self.problems({"src/lib.rs": "", "tests/it.rs": "// MeshBus\n", "build.rs": "// mesh\n"})
        self.assertEqual(len(found), 2)

    def test_ring_in_a_doc_comment_passes(self):
        self.assertEqual(self.problems({"src/lib.rs": "//! A ring buffer.\n", "src/polkit.rs": ""}), [])

    def test_denied_dependencies_in_any_table_fail(self):
        found = self.problems({"src/lib.rs": "", "Cargo.toml": (
            '[dev-dependencies]\nring = "0.17"\n[target.\'cfg(unix)\'.dependencies]\n'
            'pqcrypto-kyber = "1"\n')})
        self.assertEqual(len(found), 2)

    def test_ring_in_a_description_passes(self):
        self.assertEqual(self.problems({"src/lib.rs": "", "Cargo.toml": 'description = "ring"\n'}), [])


if __name__ == "__main__":
    unittest.main()
