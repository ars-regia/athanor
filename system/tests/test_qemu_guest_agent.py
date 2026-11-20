"""The QEMU guest agent is off until a person turns it on, and then answers an allow-list
(ADR-0144; python3 -B -m unittest discover -s system/tests -v).

qemu-guest-agent comes with base-atomic. Fedora's 90-default.preset enables it and its udev rule
starts it whenever the hypervisor offers the port, so the preset line alone would not turn it off.
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG = ROOT / "forge/specs/athanor-base-config"
SOURCES = CONFIG / "SOURCES"
FEDORA_RULE = "99-qemu-guest-agent.rules"
SELINUX = ROOT / "forge/specs/athanor-selinux"
# The whole allow-list of ADR-0144, decision 2. A new RPC is a choice to make in the ADR first.
ALLOWED = {
    "guest-sync-delimited",
    "guest-sync",
    "guest-ping",
    "guest-info",
    "guest-get-osinfo",
    "guest-get-host-name",
    "guest-get-timezone",
    "guest-get-time",
    "guest-set-time",
    "guest-network-get-interfaces",
    "guest-get-fsinfo",
    "guest-fsfreeze-status",
    "guest-fsfreeze-freeze",
    "guest-fsfreeze-freeze-list",
    "guest-fsfreeze-thaw",
    "guest-fstrim",
    "guest-shutdown",
}
# What a host could use to act inside the guest as root, or to read who is logged in.
REFUSED = re.compile(
    r"^guest-(exec|file-|set-user-password|ssh-|suspend-|set-vcpus|set-memory-blocks|get-users)"
)


def active_lines(path):
    return [
        ln.strip()
        for ln in path.read_text().splitlines()
        if ln.strip() and not ln.lstrip().startswith("#")
    ]


def active_cil(path):
    return [ln.strip() for ln in path.read_text().splitlines() if ln.strip() and not ln.lstrip().startswith(";")]


class OffByDefaultTest(unittest.TestCase):
    def test_the_base_preset_disables_the_agent(self):
        lines = active_lines(
            SOURCES / "usr/lib/systemd/system-preset/80-athanor-base.preset"
        )
        self.assertIn("disable qemu-guest-agent.service", lines)

    def test_no_shipped_preset_enables_the_agent(self):
        for preset in ROOT.glob("forge/specs/*/SOURCES/**/system-preset/*.preset"):
            self.assertNotIn(
                "enable qemu-guest-agent.service", active_lines(preset), preset.name
            )

    def test_a_later_udev_rule_takes_back_the_device_start(self):
        rules = sorted(
            (SOURCES / "usr/lib/udev/rules.d").glob("*qemu-guest-agent*.rules")
        )
        self.assertEqual(len(rules), 1)
        self.assertGreater(rules[0].name, FEDORA_RULE)
        line = " ".join(active_lines(rules[0]))
        self.assertIn('SUBSYSTEM=="virtio-ports"', line)
        self.assertIn('ATTR{name}=="org.qemu.guest_agent.0"', line)
        self.assertIn('ENV{SYSTEMD_WANTS}=""', line)

    def test_the_package_ships_the_udev_rules(self):
        files = (CONFIG / "athanor-base-config.spec").read_text().split("%files", 1)[1]
        self.assertIn("/usr/lib/udev/rules.d/*", files.split("%changelog", 1)[0])


class AllowListTest(unittest.TestCase):
    def setUp(self):
        self.lines = active_lines(
            SOURCES
            / "usr/lib/systemd/system/qemu-guest-agent.service.d/10-athanor.conf"
        )

    def rpcs(self):
        args = [ln for ln in self.lines if ln.startswith("Environment=QEMU_GA_ARGS=")]
        self.assertEqual(len(args), 1)
        value = args[0].split("=", 2)[2]
        self.assertTrue(value.startswith("--allow-rpcs="), value)
        self.assertNotIn("--block-rpcs", value)
        return value.removeprefix("--allow-rpcs=").split(",")

    def test_the_allow_list_is_the_decided_one(self):
        rpcs = self.rpcs()
        self.assertEqual(len(rpcs), len(set(rpcs)))
        self.assertEqual(set(rpcs), ALLOWED)

    def test_the_decided_list_refuses_commands_files_passwords_and_keys(self):
        self.assertEqual(sorted(r for r in ALLOWED if REFUSED.match(r)), [])

    def test_the_agent_runs_without_new_privileges_in_its_own_domain(self):
        # Under no_new_privs the agent stays in init_t unless the policy grants the transition.
        self.assertIn("NoNewPrivileges=yes", self.lines)
        rule = "(allow init_t virt_qemu_ga_t (process2 (nnp_transition nosuid_transition)))"
        self.assertIn(rule, active_cil(SELINUX / "SOURCES/athanor_qemu_ga.cil"))
        spec = (SELINUX / "athanor-selinux.spec").read_text()
        self.assertIn("%{_datadir}/selinux/packages/athanor_qemu_ga.cil", spec)


if __name__ == "__main__":
    unittest.main()
