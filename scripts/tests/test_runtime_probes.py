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
    names = (
        "listeners",
        "units",
        "avc",
        "trust",
        "pam:check",
        "sysctl",
        "nix:build",
        "nix:gc",
    )
    return {
        "rc": {
            n: {"rc": 0, "err": ""}
            for n in (*names, *(f"security:{u}" for u in EXPECT["security"]))
        },
        "listeners": [
            {"proto": "tcp", "address": "0.0.0.0", "port": 22, "process": "sshd"}
        ],
        "firewall": {},
        "units": {u: "masked" for u in EXPECT["units"]["masked"]}
        | {u: "disabled" for u in EXPECT["units"]["disabled"]},
        "security": dict(EXPECT["security"]),
        "avc": [],
        "pam": {
            "current": ["local", *EXPECT["pam"]["features"], "with-mdns4"],
            "faillock": {k: str(v) for k, v in EXPECT["pam"]["faillock"].items()},
        },
        "sysctl": "",
        "trust": {"reason": "signature"},
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
            lambda p: p["current"].remove("with-faillock"),
            lambda p: p["faillock"].update(deny="10"),
        ):
            f = good()
            mutate(f["pam"])
            self.assertEqual(failed(f), {"pam"})

    def test_a_sysctl_error_fails(self):
        f = good()
        f["sysctl"] = "Couldn't write '1' to 'kernel/x'"
        self.assertEqual(failed(f), {"sysctl"})

    def test_a_unit_in_the_wrong_state_fails(self):
        f = good()
        f["units"]["rpm-ostree-countme.timer"] = "disabled"
        self.assertEqual(failed(f), {"units"})

    def test_an_unverified_image_fails(self):
        f = good()
        f["trust"] = {"reason": "media"}
        self.assertEqual(failed(f), {"trust"})

    def test_a_listener_on_another_port_fails(self):
        f = good()
        f["listeners"][0]["port"] = 2222
        self.assertEqual(failed(f), {"listeners"})

    def test_a_listener_contact_missing_from_the_expectations_fails(self):
        contacts = {"listener": [*CONTACTS["listener"], {"unit": "telnet.socket"}]}
        self.assertEqual(
            {c for c, _ in probes.evaluate(good(), EXPECT, contacts)}, {"listeners"}
        )

    def test_a_ceiling_without_a_measurement_fails(self):
        f = good()
        del f["security"]["athanor-update.service"]
        self.assertEqual(failed(f), {"security"})

    def test_a_failing_probe_fails_its_check(self):
        checks = {
            "listeners": "listeners",
            "units": "units",
            "avc": "selinux",
            "trust": "trust",
            "pam:check": "pam",
            "sysctl": "sysctl",
            "nix:build": "nix",
            "nix:gc": "nix",
            "security:athanor-update.service": "security",
        }
        for name, check in checks.items():
            for rc in (2, 124):
                f = good()
                f["rc"][name]["rc"] = rc
                self.assertEqual(failed(f), {check}, (name, rc))

    def test_the_tunnel_is_accepted_on_loopback_only(self):
        tunnel = {
            "proto": "tcp",
            "address": "127.0.0.1",
            "port": 5000,
            "process": "sshd-session",
        }
        f = good()
        f["listeners"].append(tunnel)
        self.assertEqual(failed(f), set())
        f["listeners"][-1] = tunnel | {"address": "0.0.0.0"}
        self.assertEqual(failed(f), {"listeners"})

    def test_resolved_llmnr_and_sshd_5000_are_not_declared(self):
        for process, port in (("systemd-resolve", 5355), ("sshd", 5000)):
            f = good()
            f["listeners"].append(
                {
                    "proto": "tcp",
                    "address": "127.0.0.1",
                    "port": port,
                    "process": process,
                }
            )
            self.assertEqual(failed(f), {"listeners"})

    def test_a_firewall_zone_with_a_failing_probe_fails_among_good_ones(self):
        f = good()
        f["firewall"] = {"public": ["dhcpv6-client"], "work": []}
        f["rc"] |= {
            "firewall": {"rc": 0, "err": ""},
            "firewall:public": {"rc": 0, "err": ""},
            "firewall:work": {"rc": 1, "err": "boom"},
        }
        self.assertEqual(failed(f) - {"firewall"}, set())
        self.assertEqual(failed(f), {"firewall"})

    def test_firewall_cmd_failing_is_not_absent(self):
        f = good()
        f["firewall"] = {}
        f["rc"]["firewall"] = {"rc": 1, "err": "FirewallD is not running"}
        self.assertEqual(failed(f), {"firewall"})

    def test_the_public_zone_opens_what_the_image_declares(self):
        f = good()
        f["firewall"] = {"public": ["dhcpv6-client", "ssh"]}
        self.assertEqual(failed(f), {"firewall"})

    def test_avc_rc_1_with_stderr_is_an_error(self):
        f = good()
        f["rc"]["avc"] = {"rc": 1, "err": "Failed to open journal"}
        self.assertEqual(failed(f), {"selinux"})

    def test_journalctl_grep_without_a_match_is_clean(self):
        f = good()
        f["rc"]["avc"]["rc"] = 1
        self.assertEqual(failed(f), set())

    def collect_with(self, calls, which=None):
        """collect() with the commands stubbed: calls maps a command line to (rc, stdout, stderr)."""
        real_run, real_which = probes.run, probes.shutil.which
        probes.run = lambda argv, sudo=False: calls.get(" ".join(argv), (0, "", ""))
        probes.shutil.which = lambda *a, **k: which
        try:
            return probes.collect()
        finally:
            probes.run, probes.shutil.which = real_run, real_which

    def firewall_checks(self, facts):
        return [
            d for c, d in probes.evaluate(facts, EXPECT, CONTACTS) if c == "firewall"
        ]

    def test_collect_keeps_one_probe_key_per_zone(self):
        calls = {
            "firewall-cmd --get-active-zones": (
                0,
                "work\n  interfaces: a\npublic\n  interfaces: b\n",
                "",
            ),
            "firewall-cmd --zone=work --list-services": (1, "", "boom"),
            "firewall-cmd --zone=public --list-services": (0, "dhcpv6-client\n", ""),
        }
        facts = self.collect_with(calls, which="/usr/bin/firewall-cmd")
        self.assertEqual(facts["rc"]["firewall:work"]["rc"], 1)
        self.assertEqual(facts["rc"]["firewall:public"]["rc"], 0)
        self.assertTrue(any("firewall:work" in d for d in self.firewall_checks(facts)))

    def test_the_public_zone_as_the_image_declares_it_is_accepted(self):
        calls = {
            "firewall-cmd --get-active-zones": (0, "public\n  interfaces: b\n", ""),
            "firewall-cmd --zone=public --list-services": (0, "dhcpv6-client\n", ""),
        }
        facts = self.collect_with(calls, which="/usr/bin/firewall-cmd")
        self.assertEqual(facts["firewall"], {"public": ["dhcpv6-client"]})
        self.assertEqual(self.firewall_checks(facts), [])
        f = good()
        f["firewall"] = {"public": ["dhcpv6-client"]}
        self.assertEqual(failed(f), set())

    def test_networkmanager_dhcpv6_is_accepted(self):
        f = good()
        f["listeners"].append(
            {"proto": "udp", "address": "::", "port": 546, "process": "NetworkManager"}
        )
        self.assertEqual(failed(f), set())

    def test_an_absent_firewall_cmd_passes_unless_required(self):
        facts = self.collect_with({}, which=None)
        self.assertIsNone(facts["firewall"])
        self.assertNotIn("firewall", facts["rc"])
        self.assertEqual(self.firewall_checks(facts), [])

    def test_a_present_firewall_cmd_that_fails_fails_the_check(self):
        calls = {"firewall-cmd --get-active-zones": (1, "", "FirewallD is not running")}
        facts = self.collect_with(calls, which="/usr/bin/firewall-cmd")
        self.assertEqual(facts["rc"]["firewall"]["rc"], 1)
        self.assertEqual(len(self.firewall_checks(facts)), 1)

    def test_collect_fails_trust_on_a_state_that_is_not_json(self):
        calls = {"cat /run/athanor-update/state.json": (0, "not json", "")}
        real_run, real_which = probes.run, probes.shutil.which
        probes.run = lambda argv, sudo=False: calls.get(" ".join(argv), (0, "", ""))
        probes.shutil.which = lambda *a, **k: None
        try:
            facts = probes.collect()
        finally:
            probes.run, probes.shutil.which = real_run, real_which
        self.assertEqual(facts["rc"]["trust"]["rc"], 65)
        self.assertIn(
            "trust",
            {
                c
                for c, _ in probes.evaluate(
                    facts | {"pam": good()["pam"]}, EXPECT, CONTACTS
                )
            },
        )

    def test_a_timeout_names_the_probe(self):
        probes.TIMEOUT = 1
        try:
            rc, _, err = probes.run(["sleep", "5"])
        finally:
            probes.TIMEOUT = 600
        self.assertEqual(rc, 124)
        self.assertIn("sleep 5", err)


if __name__ == "__main__":
    unittest.main()
