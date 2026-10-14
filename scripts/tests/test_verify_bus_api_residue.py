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
            "Cargo.toml": 'x25519-dalek = "2"\nring = "0.17"\n'})
        self.assertEqual(len(found), 3)


if __name__ == "__main__":
    unittest.main()
