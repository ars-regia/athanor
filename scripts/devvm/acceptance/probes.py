"""Runtime audit probes of the VM acceptance (issue #312).

    probes.py collect                      on the guest, as the login user: print the facts as JSON
    probes.py check FACTS EXPECT CONTACTS  on the host: compare them with the committed expectations

`collect` only observes (plus one Nix build and one `nix store gc`, as the login user). `check`
is pure, so every comparison is unit-tested (scripts/tests/test_runtime_probes.py). A change of
an expectation is a change of expectations.toml, hence reviewed."""

import json
import os
import re
import shutil
import subprocess
import sys

import tomllib

TIMEOUT = 600


def run(argv, sudo=False):
    """(returncode, stdout, stderr) of a command; a missing program reads as 127."""
    try:
        p = subprocess.run(
            (["sudo", "-n"] if sudo else []) + argv,
            capture_output=True,
            check=False,
            text=True,
            timeout=TIMEOUT,
        )
    except FileNotFoundError:
        return 127, "", f"{argv[0]}: not found"
    except subprocess.TimeoutExpired:
        return 124, "", f"timed out after {TIMEOUT}s: {' '.join(argv)}"
    return p.returncode, p.stdout, p.stderr


# --- parsers (pure) -------------------------------------------------------------------------


def parse_listeners(text):
    """`ss -H -ltunp` -> [{proto, address, port, process}]."""
    out = []
    for line in text.splitlines():
        f = line.split()
        if len(f) < 5:
            continue
        address, _, port = f[4].rpartition(":")
        m = re.search(r'\(\("([^"]+)"', line)
        out.append(
            {
                "proto": f[0],
                "address": address.strip("[]"),
                "port": int(port) if port.isdigit() else 0,
                "process": m.group(1) if m else "",
            }
        )
    return out


def parse_unit_files(text):
    """`systemctl list-unit-files --no-legend` -> {unit: state}."""
    return {f[0]: f[1] for f in map(str.split, text.splitlines()) if len(f) >= 2}


def parse_avc(text):
    """Journal lines of AVC denials -> sorted unique 'source:target:class' keys."""
    keys = set()
    for line in text.splitlines():
        s, t, c = (
            re.search(rf"{k}=(\S+)", line) for k in ("scontext", "tcontext", "tclass")
        )
        if "denied" in line and s and t and c:
            keys.add(
                f"{s.group(1).split(':')[2]}:{t.group(1).split(':')[2]}:{c.group(1)}"
            )
    return sorted(keys)


def parse_faillock(text):
    """/etc/security/faillock.conf -> {key: value} of the active settings."""
    out = {}
    for line in text.splitlines():
        m = re.match(r"\s*([a-z_]+)\s*=\s*(\S+)", line)
        if m:
            out[m.group(1)] = m.group(2)
    return out


def parse_security(text):
    """`systemd-analyze security` text -> {unit: exposure} on its 0 to 10 scale (the --threshold of
    run.sh is the same figure times ten). The JSON form lists the individual checks, not the total."""
    return {
        m.group(1): float(m.group(2))
        for m in re.finditer(r"Overall exposure level for (\S+): ([0-9.]+)", text)
    }


# --- collector --------------------------------------------------------------------------------

NIX_EXPR = 'derivation { name = "probe-%d"; system = builtins.currentSystem; builder = "/bin/sh"; args = [ "-c" "echo ok > $out" ]; }'
# Exit codes a probe may return besides 0, and why, only with an empty stderr. Any other code, or a timeout (124), fails
# the probe's check: an empty output of a failed command must never read as a clean guest.
ALLOWED_RC = {"avc": (0, 1)}  # journalctl --grep exits 1 when nothing matches
CHECK_OF = {
    "listeners": "listeners",
    "firewall": "firewall",
    "units": "units",
    "avc": "selinux",
    "trust": "trust",
}


def collect():
    facts = {"rc": {}}

    def probe(name, argv, sudo=False):
        rc, out, err = run(argv, sudo=sudo)
        facts["rc"][name] = {"rc": rc, "err": err.strip()[-300:]}
        return out

    facts["listeners"] = parse_listeners(
        probe("listeners", ["ss", "-H", "-ltunp"], sudo=True)
    )
    # Absent means the binary is not installed, decided before sudo can muddy the exit code.
    present = shutil.which("firewall-cmd", path="/usr/bin:/usr/sbin") is not None
    zones = None
    if present:
        out = probe("firewall", ["firewall-cmd", "--get-active-zones"], sudo=True)
        zones = {}
        for zone in [x for x in out.splitlines() if x and not x.startswith(" ")]:
            listed = probe(
                f"firewall:{zone}",
                ["firewall-cmd", f"--zone={zone}", "--list-services"],
                sudo=True,
            )
            zones[zone] = sorted(listed.split())
    facts["firewall"] = zones
    units = parse_unit_files(
        probe("units", ["systemctl", "list-unit-files", "--no-legend", "--no-pager"])
    )
    facts["units"] = units
    names = [
        u
        for u in units
        if u.startswith("athanor-")
        and u.endswith(".service")
        and not u.endswith("@.service")
    ]
    facts["security"] = {}
    for u in names:
        facts["security"] |= parse_security(
            probe(f"security:{u}", ["systemd-analyze", "security", "--no-pager", u])
        )
    facts["avc"] = parse_avc(
        probe(
            "avc",
            ["journalctl", "-b", "--no-pager", "-o", "cat", "--grep", "avc: +denied"],
            sudo=True,
        )
    )
    facts["pam"] = {
        "current": probe(
            "pam:current", ["authselect", "current", "--raw"], sudo=True
        ).split(),
        "faillock": parse_faillock(
            probe("pam:faillock", ["cat", "/etc/security/faillock.conf"])
        ),
    }
    probe("pam:check", ["authselect", "check"], sudo=True)
    facts["sysctl"] = probe(
        "sysctl",
        [
            "journalctl",
            "-b",
            "-u",
            "systemd-sysctl.service",
            "-p",
            "warning",
            "--no-pager",
            "-o",
            "cat",
        ],
        sudo=True,
    ).strip()
    state = probe("trust", ["cat", "/run/athanor-update/state.json"])
    try:
        facts["trust"] = json.loads(state or "{}").get("verified", {})
    except json.JSONDecodeError:
        facts["trust"] = {}
        facts["rc"]["trust"]["err"] = "state.json is not JSON"
        facts["rc"]["trust"]["rc"] = 65
    nix = ["nix", "--extra-experimental-features", "nix-command"]
    probe(
        "nix:build",
        [*nix, "build", "--no-link", "--impure", "--expr", NIX_EXPR % os.getpid()],
    )
    probe("nix:gc", [*nix, "store", "gc"])
    return facts


# --- evaluation (pure) --------------------------------------------------------------------------


def declares(entry, socket):
    """Whether a declared listener covers a socket: process and port, and the address when declared."""
    addresses = entry.get("address", [])
    addresses = [addresses] if isinstance(addresses, str) else addresses
    return (
        entry["process"] == socket["process"]
        and socket["port"] in entry["ports"]
        and (not addresses or socket["address"] in addresses)
    )


def evaluate(facts, expect, contacts):
    """Every violated expectation as (check, detail); empty when the guest conforms."""
    bad = []

    def fail(check, detail):
        bad.append((check, detail))

    # A probe that did not run cleanly says nothing about the guest: its check fails.
    for name, r in sorted(facts["rc"].items()):
        base = name.split(":")[0]
        if r["rc"] != 0 and not (r["rc"] in ALLOWED_RC.get(base, ()) and not r["err"]):
            fail(CHECK_OF.get(base, base), f"probe {name} exited {r['rc']}: {r['err']}")

    # Listening sockets: each one is declared by process and port; the declarations and the
    # listener contacts of contacts.toml cover each other.
    listeners = expect.get("listener", [])
    declared_units = {x["unit"] for x in contacts.get("listener", [])}
    for entry in listeners:
        if entry.get("unit") and entry["unit"] not in declared_units:
            fail(
                "listeners",
                f"{entry['unit']} is in expectations.toml but not a [[listener]] of contacts.toml",
            )
    for unit in sorted(declared_units - {e.get("unit") for e in listeners}):
        fail(
            "listeners",
            f"{unit} is a [[listener]] of contacts.toml but not declared in expectations.toml",
        )
    for s in facts["listeners"]:
        if not any(declares(e, s) for e in listeners):
            fail(
                "listeners",
                f"undeclared {s['proto']} listener {s['address']}:{s['port']} ({s['process'] or 'unknown process'})",
            )
    fw = facts["firewall"]
    if fw is None and expect.get("firewall", {}).get("required"):
        fail("firewall", "firewall-cmd is not installed but the firewall is required")
    for zone, services in (fw or {}).items():
        want = sorted(expect.get("firewall", {}).get("zones", {}).get(zone, []))
        if services != want:
            fail("firewall", f"zone {zone} opens {services}, declared {want}")

    # Exposure ceilings: a unit above its ceiling, without one, or never measured fails.
    ceilings = expect.get("security", {})
    for unit, exposure in sorted(facts["security"].items()):
        if unit not in ceilings:
            fail(
                "security",
                f"{unit} has no declared exposure ceiling (measured {exposure})",
            )
        elif exposure > ceilings[unit]:
            fail(
                "security",
                f"{unit} exposure {exposure} is above its ceiling {ceilings[unit]}",
            )
    for unit in sorted(set(ceilings) - set(facts["security"])):
        fail("security", f"{unit} has a ceiling but was not measured")

    allowed = set(expect.get("selinux", {}).get("allow", []))
    for key in facts["avc"]:
        if key not in allowed:
            fail("selinux", f"denial {key} is not in the allow-list")

    pam, want = facts["pam"], expect.get("pam", {})
    if pam["current"][:1] != [want.get("profile")]:
        fail(
            "pam",
            f"authselect profile {pam['current'][:1]}, declared {want.get('profile')}",
        )
    for feature in want.get("features", []):
        if feature not in pam["current"]:
            fail("pam", f"feature {feature} is not enabled")
    for feature in want.get("absent_features", []):
        if feature in pam["current"]:
            fail("pam", f"feature {feature} is enabled")
    for key, value in want.get("faillock", {}).items():
        if pam["faillock"].get(key) != str(value):
            fail(
                "pam", f"faillock {key} is {pam['faillock'].get(key)}, declared {value}"
            )

    if facts["sysctl"]:
        fail("sysctl", f"systemd-sysctl logged at boot: {facts['sysctl'][:200]}")

    for state in ("masked", "disabled"):
        for unit in expect.get("units", {}).get(state, []):
            got = facts["units"].get(unit, "absent")
            if got != state and not (
                state == "disabled" and got in ("masked", "absent")
            ):
                fail("units", f"{unit} is {got}, declared {state}")

    reason = expect.get("trust", {}).get("reason")
    if facts["trust"].get("reason") != reason:
        fail(
            "trust",
            f"verified.reason is {facts['trust'].get('reason')}, expected {reason}",
        )
    return bad


def load(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def main(argv):
    if argv[:1] == ["collect"]:
        print(json.dumps(collect()))
        return 0
    if len(argv) == 4 and argv[0] == "check":
        with open(argv[1]) as f:
            facts = json.load(f)
        bad = evaluate(facts, load(argv[2]), load(argv[3]))
        for check, detail in bad:
            print(f"FAIL  probe {check}: {detail}")
        for check in sorted(
            {
                "listeners",
                "firewall",
                "security",
                "selinux",
                "pam",
                "sysctl",
                "units",
                "trust",
                "nix",
            }
            - {c for c, _ in bad}
        ):
            print(f"PASS  probe {check}")
        return 1 if bad else 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
