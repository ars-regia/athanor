"""The unlock helper's units and package (doc_lock_and_prompts.md, LP8, D26)
(python3 -B -m unittest discover -s forge/specs/athanor-unlockd/tests -v).

The template unit carries polkit 128's polkit-agent-helper@.service sandbox line for line,
with its own ExecStart, plus the four additions of LP8; the spec installs and lists every
file it ships.
"""

import configparser
import re
import unittest
from pathlib import Path

SPEC_DIR = Path(__file__).resolve().parents[1]
SOURCES = SPEC_DIR / "SOURCES"
SPEC = SPEC_DIR / "athanor-unlockd.spec"

# polkit 128, /usr/lib/systemd/system/polkit-agent-helper@.service, [Service], less ExecStart.
POLKIT_HELPER = {
    "Type": "oneshot",
    "DevicePolicy": "closed",
    "SuccessExitStatus": "2",
    "StandardInput": "socket",
    "StandardOutput": "socket",
    "StandardError": "journal",
    "LimitMEMLOCK": "0",
    "LockPersonality": "yes",
    "MemoryDenyWriteExecute": "yes",
    "NoNewPrivileges": "yes",
    "PrivateDevices": "yes",
    "PrivateNetwork": "yes",
    "PrivateTmp": "yes",
    "ProtectControlGroups": "yes",
    "ProtectHome": "yes",
    "ProtectKernelModules": "yes",
    "ProtectKernelLogs": "yes",
    "ProtectKernelTunables": "yes",
    "ProtectSystem": "strict",
    "ProtectClock": "yes",
    "ProtectHostname": "yes",
    "RemoveIPC": "yes",
    "RestrictAddressFamilies": "AF_UNIX",
    "RestrictNamespaces": "yes",
    "RestrictRealtime": "yes",
    "RestrictSUIDSGID": "yes",
    "SystemCallArchitectures": "native",
    "SystemCallFilter": "@system-service",
    "UMask": "0077",
}

ADDITIONS = {
    "ExecStart": "/usr/libexec/athanor-unlockd",
    "ReadWritePaths": "/run/faillock",
    "MemoryMax": "32M",
    "TimeoutStartSec": "300",
    "LimitCORE": "0",
}

SHIPPED = {
    "/usr/libexec/athanor-unlockd": "target/release/athanor-unlockd",
    "/usr/lib/systemd/system/athanor-unlockd.socket": "%{sources}/usr/lib/systemd/system/athanor-unlockd.socket",
    "/usr/lib/systemd/system/athanor-unlockd@.service": "%{sources}/usr/lib/systemd/system/athanor-unlockd@.service",
    "/usr/lib/pam.d/athanor-lock": "%{sources}/usr/lib/pam.d/athanor-lock",
    "/usr/lib/pam.d/athanor-lock-fingerprint": "%{sources}/usr/lib/pam.d/athanor-lock-fingerprint",
}


def unit(path):
    parser = configparser.ConfigParser(strict=True, interpolation=None)
    parser.optionxform = str
    parser.read_string(path.read_text())
    return parser


class TemplateUnit(unittest.TestCase):
    def test_the_service_is_polkits_sandbox_plus_lp8s_additions(self):
        service = unit(SOURCES / "usr/lib/systemd/system/athanor-unlockd@.service")
        self.assertEqual(dict(service["Service"]), {**POLKIT_HELPER, **ADDITIONS})

    def test_the_socket_accepts_one_helper_per_connection_and_never_trips(self):
        socket = unit(SOURCES / "usr/lib/systemd/system/athanor-unlockd.socket")
        self.assertEqual(
            dict(socket["Socket"]),
            {
                "ListenStream": "/run/athanor/unlockd.socket",
                "Accept": "yes",
                "RemoveOnStop": "yes",
                "MaxConnections": "16",
                "MaxConnectionsPerSource": "4",
                "TriggerLimitIntervalSec": "0",
            },
        )
        self.assertEqual(dict(socket["Install"]), {"WantedBy": "sockets.target"})


class Spec(unittest.TestCase):
    def test_the_spec_installs_and_lists_every_file(self):
        spec = SPEC.read_text()
        install = spec.split("%install", 1)[1].split("%post", 1)[0].replace("\\\n", " ")
        installs = {}
        for line in install.splitlines():
            words = line.split()
            if words[:1] == ["install"]:
                installs[words[-1].replace("%{buildroot}", "")] = words[-2]
        self.assertEqual(installs, SHIPPED)
        files = spec.split("%files", 1)[1].split("%changelog", 1)[0].split()
        self.assertEqual(sorted(files), sorted(SHIPPED))
        for source in SHIPPED.values():
            if source.startswith("%{sources}/"):
                self.assertTrue(
                    (SOURCES / source.removeprefix("%{sources}/")).is_file(), source
                )

    def test_the_scriptlets_act_on_the_socket_and_the_package_needs_fprintd_pam(self):
        spec = SPEC.read_text()
        for macro in ("systemd_post", "systemd_preun", "systemd_postun"):
            self.assertIn(f"%{macro} athanor-unlockd.socket", spec.splitlines())
        requires = re.search(r"^Requires:\s*(.+)$", spec, re.M)
        self.assertIsNotNone(requires)
        self.assertEqual(requires.group(1).split(), ["pam", "systemd", "fprintd-pam"])


if __name__ == "__main__":
    unittest.main()
