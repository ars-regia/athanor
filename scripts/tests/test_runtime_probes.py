"""Unit tests of the runtime audit probes of the VM acceptance
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import re
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ACC = ROOT / "scripts" / "devvm" / "acceptance"
spec = importlib.util.spec_from_file_location("probes", ACC / "probes.py")
probes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probes)

EXPECT = probes.load(ACC / "expectations.toml")
CONTACTS = probes.load(ROOT / "forge" / "config" / "contacts.toml")
# The image's own sysctl.d files, as the guest's `systemd-sysctl --cat-config` prints them.
SYSCTL = probes.parse_sysctl_config(
    "".join(
        p.read_text()
        for p in sorted(ROOT.glob("forge/specs/*/SOURCES/usr/lib/sysctl.d/*.conf"))
    )
)


IMAGE = "localhost:5000/acc/athanor-system:stable"
DIGEST = "sha256:" + "a" * 64
SIGNED = (IMAGE, DIGEST)


def good():
    """Facts of a guest that conforms to the committed expectations."""
    names = (
        "listeners",
        "units",
        "avc",
        "trust",
        "trust:bootc",
        "pam:check",
        "sysctl",
        "sysctl:config",
        "sysctl:live",
        "nix:build",
        "nix:gc",
        "backup",
        "backup:list",
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
        "sysctl_set": SYSCTL,
        "sysctl_live": {k: v for k, (v, _) in SYSCTL.items()},
        "trust": {
            "value": True,
            "reason": "signature",
            "booted_digest": DIGEST,
            "bootc_digest": DIGEST,
            "bootc_image": IMAGE,
            "bootc_signature": "containerPolicy",
            "policy_shipped": True,
            "schema": 2,
            "automatic_security_updates": True,
            "postpone_published": True,
        },
        "backup": ["20261018T100000Z"],
    }


TUNNEL = 5000  # the default ACC_PORT of lib.sh


def failed(facts, tunnel_port=TUNNEL):
    return {c for c, _ in probes.evaluate(facts, EXPECT, CONTACTS, tunnel_port, SIGNED)}


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

    def test_active_zones(self):
        text = "public (default)\n  interfaces: enp1s0\nhome\n  sources: 192.0.2.0/24\n"
        self.assertEqual(probes.parse_active_zones(text), ["public", "home"])

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

    def test_a_sysctl_value_changed_after_boot_fails(self):
        f = good()
        f["sysctl_live"] = {**f["sysctl_live"], "kernel.kptr_restrict": "0"}
        self.assertEqual(failed(f), {"sysctl"})

    def test_a_sysctl_key_the_kernel_lacks_fails_unless_optional(self):
        f = good()
        f["sysctl_live"] = dict(f["sysctl_live"])
        del f["sysctl_live"]["kernel.kptr_restrict"]
        self.assertEqual(failed(f), {"sysctl"})
        f["sysctl_set"] = {**f["sysctl_set"], "kernel.kptr_restrict": ["2", True]}
        self.assertEqual(failed(f), set())

    def test_no_sysctl_key_read_fails(self):
        f = good()
        f["sysctl_set"], f["sysctl_live"] = {}, {}
        self.assertEqual(failed(f), {"sysctl"})

    def test_sysctl_config_last_assignment_wins(self):
        text = (
            "# /usr/lib/sysctl.d/50-default.conf\nnet.core.default_qdisc = fq_codel\n"
            "-net.ipv4.ping_group_range = 0 2147483647\nnet.ipv4.conf.*.rp_filter = 2\n"
            "; a = 1\n# /usr/lib/sysctl.d/99-x.conf\nnet.core.default_qdisc=fq\n"
        )
        self.assertEqual(
            probes.parse_sysctl_config(text),
            {
                "net.core.default_qdisc": ["fq", False],
                "net.ipv4.ping_group_range": ["0 2147483647", True],
            },
        )
        self.assertEqual(SYSCTL["kernel.kptr_restrict"], ["2", False])

    def test_collect_reads_the_live_value_of_each_configured_key(self):
        calls = {
            "/usr/lib/systemd/systemd-sysctl --cat-config": (
                0,
                "a.b = 1\n-c.d = 0 1\n",
                "",
            ),
            "sysctl -e a.b c.d": (0, "a.b = 1\nc.d = 0\t1\n", ""),
        }
        facts = self.collect_with(calls)
        self.assertEqual(facts["sysctl_live"], {"a.b": "1", "c.d": "0\t1"})
        f = good() | {k: facts[k] for k in ("sysctl_set", "sysctl_live")}
        self.assertEqual(failed(f), set())

    def test_a_unit_in_the_wrong_state_fails(self):
        f = good()
        f["units"]["rpm-ostree-countme.timer"] = "disabled"
        self.assertEqual(failed(f), {"units"})

    def test_an_installed_absent_unit_fails(self):
        f = good()
        f["units"]["qemu-guest-agent.service"] = "disabled"
        self.assertEqual(failed(f), {"units"})

    def test_an_unmasked_debug_shell_fails(self):
        f = good()
        f["units"]["debug-shell.service"] = "static"
        self.assertEqual(failed(f), {"units"})

    def test_an_unverified_image_fails(self):
        f = good()
        f["trust"] = {**f["trust"], "reason": "media"}
        self.assertEqual(failed(f), {"trust"})

    def test_a_state_at_schema_1_fails(self):
        f = good()
        f["trust"] = {**f["trust"], "schema": 1}
        self.assertEqual(failed(f), {"trust"})

    def test_a_state_without_the_postpone_field_fails(self):
        f = good()
        f["trust"] = {**f["trust"], "postpone_published": False}
        self.assertEqual(failed(f), {"trust"})

    def test_the_opt_out_left_on_fails(self):
        f = good()
        f["trust"] = {**f["trust"], "automatic_security_updates": False}
        self.assertEqual(failed(f), {"trust"})

    def test_a_booted_image_that_is_not_the_signed_one_fails(self):
        other = "sha256:" + "b" * 64
        for change in (
            # bootc and athanor-update agree, on a digest the registry does not sign.
            {"bootc_digest": other, "booted_digest": other},
            {"bootc_image": "localhost:5000/acc/athanor-system:v2"},
            {"booted_digest": other},
            {"bootc_signature": "insecure"},
            {"policy_shipped": False},
            {"value": False},
        ):
            f = good()
            f["trust"] = f["trust"] | change
            self.assertEqual(failed(f), {"trust"}, change)

    def test_collect_reads_what_bootc_booted(self):
        status = (
            '{"status": {"booted": {"image": {"imageDigest": "%s", "image": '
            '{"image": "%s", "transport": "registry", "signature": "containerPolicy"}}}}}'
        ) % (DIGEST, IMAGE)
        state = '{"booted": {"digest": "%s"}, "policy": {"shipped": true}}' % DIGEST
        facts = self.collect_with(
            {
                "bootc status --format json": (0, status, ""),
                "cat /run/athanor-update/state.json": (0, state, ""),
            }
        )
        self.assertEqual(
            {k: facts["trust"][k] for k in good()["trust"] if k in facts["trust"]},
            {
                "booted_digest": DIGEST,
                "policy_shipped": True,
                "bootc_digest": DIGEST,
                "bootc_image": IMAGE,
                "bootc_signature": "containerPolicy",
                "schema": None,
                "automatic_security_updates": None,
                "postpone_published": False,
            },
        )
        facts = self.collect_with({"bootc status --format json": (0, "{", "")})
        self.assertEqual(facts["rc"]["trust:bootc"]["rc"], 65)

    def test_no_snapshot_after_the_hourly_unit_fails(self):
        f = good()
        f["backup"] = []
        self.assertEqual(failed(f), {"backup"})

    def test_a_listener_on_another_port_fails(self):
        f = good()
        f["listeners"][0]["port"] = 2222
        self.assertEqual(failed(f), {"listeners"})

    def test_a_listener_contact_missing_from_the_expectations_fails(self):
        contacts = {"listener": [*CONTACTS["listener"], {"unit": "telnet.socket"}]}
        self.assertEqual(
            {c for c, _ in probes.evaluate(good(), EXPECT, contacts, TUNNEL, SIGNED)},
            {"listeners"},
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
            "trust:bootc": "trust",
            "pam:check": "pam",
            "sysctl": "sysctl",
            "sysctl:config": "sysctl",
            "sysctl:live": "sysctl",
            "nix:build": "nix",
            "nix:gc": "nix",
            "backup": "backup",
            "backup:list": "backup",
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

    def test_the_tunnel_follows_the_harness_port(self):
        f = good()
        f["listeners"].append(
            {
                "proto": "tcp",
                "address": "127.0.0.1",
                "port": 5001,
                "process": "sshd-session",
            }
        )
        self.assertEqual(failed(f, 5001), set())
        self.assertEqual(failed(f, 5000), {"listeners"})

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
            d
            for c, d in probes.evaluate(facts, EXPECT, CONTACTS, TUNNEL, SIGNED)
            if c == "firewall"
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
                    facts | {"pam": good()["pam"]}, EXPECT, CONTACTS, TUNNEL, SIGNED
                )
            },
        )

    def test_every_shipped_system_service_has_a_ceiling(self):
        # The guest probe fails a unit without a line only once an acceptance runs; this
        # holds the same rule on every pull request (#509: a unit came without its line).
        # Shipped: a non-template athanor-*.service in a SOURCES unit directory of any spec in
        # the repository, or one a spec names under the unit directory outside its comments and
        # its changelog. A unit installed only by a glob or a loop over a tarball is not seen.
        def tracked(*patterns):
            out = subprocess.run(
                ["git", "-C", str(ROOT), "ls-files", "--", *patterns],
                capture_output=True,
                text=True,
                check=True,
            ).stdout
            return [ROOT / line for line in out.splitlines()]

        shipped = {
            p.name
            for p in tracked("*/SOURCES/usr/lib/systemd/system/athanor-*.service")
            if "@" not in p.name
        }
        unit_path = re.compile(
            r"(?:%\{_unitdir\}|/usr/lib/systemd/system)/(athanor-[A-Za-z0-9_.-]*\.service)"
        )
        for spec_file in tracked("*.spec"):
            body = spec_file.read_text().split("\n%changelog", 1)[0]
            for line in body.splitlines():
                if not line.lstrip().startswith("#"):
                    shipped |= set(unit_path.findall(line))
        self.assertEqual(set(EXPECT["security"]), shipped)

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
