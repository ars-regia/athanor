"""Unit tests of the runtime audit probes of the VM acceptance
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ACC = ROOT / "scripts" / "devvm" / "acceptance"
spec = importlib.util.spec_from_file_location("probes", ACC / "probes.py")
probes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probes)

EXPECT = probes.load(ACC / "expectations.toml")
CONTACTS = probes.load(ROOT / "forge" / "config" / "contacts.toml")


def good():
    """Facts of a guest that conforms to the committed expectations."""
    return {
        "listeners": [
            {"proto": "tcp", "address": "0.0.0.0", "port": 22, "process": "sshd"}
        ],
        "firewall": {},
        "units": {u: "masked" for u in EXPECT["units"]["masked"]}
        | {u: "disabled" for u in EXPECT["units"]["disabled"]},
        "security": dict(EXPECT["security"]),
        "avc": [],
        "pam": {
            "check_rc": 0,
            "current": ["local", *EXPECT["pam"]["features"], "with-mdns4"],
            "faillock": {k: str(v) for k, v in EXPECT["pam"]["faillock"].items()},
        },
        "sysctl": {"rc": 0, "stderr": ""},
        "trust": {"reason": "signature"},
        "nix": {"build_rc": 0, "gc_rc": 0, "stderr": ""},
    }


def failed(facts):
    return {c for c, _ in probes.evaluate(facts, EXPECT, CONTACTS)}


class Parsers(unittest.TestCase):
    def test_listeners(self):
        text = 'tcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:(("sshd",pid=1,fd=3))\nudp UNCONN 0 0 [::1]:323 [::]:*\n'
        self.assertEqual(
            probes.parse_listeners(text),
            [
                {"proto": "tcp", "address": "0.0.0.0", "port": 22, "process": "sshd"},
                {"proto": "udp", "address": "::1", "port": 323, "process": ""},
            ],
        )

    def test_security(self):
        text = "→ Overall exposure level for a.service: 5.3 MEDIUM 😐\n→ Overall exposure level for b.service: 1.6 OK 🙂\n"
        self.assertEqual(
            probes.parse_security(text), {"a.service": 5.3, "b.service": 1.6}
        )

    def test_avc(self):
        line = "avc:  denied  { read } for  pid=1 comm=x scontext=system_u:system_r:a_t:s0 tcontext=system_u:object_r:b_t:s0 tclass=file permissive=0"
        self.assertEqual(
            probes.parse_avc(line + "\n" + line + "\nunrelated\n"), ["a_t:b_t:file"]
        )

    def test_faillock_and_unit_files(self):
        self.assertEqual(
            probes.parse_faillock("# deny = 3\ndeny = 5\n unlock_time=600\n"),
            {"deny": "5", "unlock_time": "600"},
        )
        self.assertEqual(
            probes.parse_unit_files("a.service enabled disabled\nb.timer masked -\n"),
            {"a.service": "enabled", "b.timer": "masked"},
        )


class Expectations(unittest.TestCase):
    def test_a_conforming_guest_passes(self):
        self.assertEqual(failed(good()), set())

    def test_the_declared_listener_units_are_contacts(self):
        units = {e["unit"] for e in EXPECT["listener"] if "unit" in e}
        self.assertLessEqual(units, {x["unit"] for x in CONTACTS["listener"]})

    # One negative control per check.
    def test_an_extra_listener_fails(self):
        f = good()
        f["listeners"].append(
            {"proto": "tcp", "address": "0.0.0.0", "port": 4444, "process": "nc"}
        )
        self.assertEqual(failed(f), {"listeners"})

    def test_an_open_firewall_service_fails(self):
        f = good()
        f["firewall"] = {"public": ["ssh"]}
        self.assertEqual(failed(f), {"firewall"})

    def test_a_unit_above_its_ceiling_or_without_one_fails(self):
        f = good()
        f["security"]["athanor-update.service"] += 0.1
        self.assertEqual(failed(f), {"security"})
        f = good()
        f["security"]["athanor-new.service"] = 1.0
        self.assertEqual(failed(f), {"security"})

    def test_an_unlisted_denial_fails(self):
        f = good()
        f["avc"] = ["a_t:b_t:file"]
        self.assertEqual(failed(f), {"selinux"})

    def test_pam_drift_fails(self):
        for mutate in (
            lambda p: p.update(check_rc=1),
            lambda p: p["current"].remove("with-faillock"),
            lambda p: p["faillock"].update(deny="10"),
        ):
            f = good()
            mutate(f["pam"])
            self.assertEqual(failed(f), {"pam"})

    def test_a_sysctl_error_fails(self):
        f = good()
        f["sysctl"] = {"rc": 0, "stderr": "Couldn't write '1' to 'kernel/x'"}
        self.assertEqual(failed(f), {"sysctl"})

    def test_a_unit_in_the_wrong_state_fails(self):
        f = good()
        f["units"]["rpm-ostree-countme.timer"] = "disabled"
        self.assertEqual(failed(f), {"units"})

    def test_an_unverified_image_fails(self):
        f = good()
        f["trust"] = {"reason": "media"}
        self.assertEqual(failed(f), {"trust"})

    def test_a_failed_nix_build_fails(self):
        f = good()
        f["nix"]["build_rc"] = 1
        self.assertEqual(failed(f), {"nix"})


if __name__ == "__main__":
    unittest.main()
