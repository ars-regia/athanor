"""Runtime audit probes of the VM acceptance (issue #312).

    probes.py collect                      on the guest, as the login user: print the facts as JSON
    probes.py check FACTS EXPECT CONTACTS  on the host: compare them with the committed expectations

`collect` only observes (plus one Nix build and one `nix store gc`, as the login user). `check`
is pure, so every comparison is unit-tested (scripts/tests/test_runtime_probes.py). A change of
an expectation is a change of expectations.toml, hence reviewed."""

import json
import os
import re
import subprocess
import sys

import tomllib


def run(argv, sudo=False):
    """(returncode, stdout, stderr) of a command; a missing program reads as 127."""
    try:
        p = subprocess.run(
            (["sudo", "-n"] if sudo else []) + argv,
            capture_output=True,
            check=False,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        return 127, "", f"{argv[0]}: not found"
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


def collect():
    facts = {}
    facts["listeners"] = parse_listeners(run(["ss", "-H", "-ltunp"], sudo=True)[1])
    rc, out, _ = run(["firewall-cmd", "--get-active-zones"], sudo=True)
    zones = {}
    for zone in (
        []
        if rc
        else [line for line in out.splitlines() if line and not line.startswith(" ")]
    ):
        zones[zone] = sorted(
            run(["firewall-cmd", f"--zone={zone}", "--list-services"], sudo=True)[
                1
            ].split()
        )
    facts["firewall"] = None if rc else zones
    units = parse_unit_files(
        run(["systemctl", "list-unit-files", "--no-legend", "--no-pager"])[1]
    )
    facts["units"] = units
    names = [
        u
        for u in units
        if u.startswith("athanor-")
        and u.endswith(".service")
        and not u.endswith("@.service")
    ]
    facts["security"] = parse_security(
        "".join(run(["systemd-analyze", "security", "--no-pager", u])[1] for u in names)
    )
    facts["avc"] = parse_avc(
        run(
            ["journalctl", "-b", "--no-pager", "-o", "cat", "--grep", "avc: +denied"],
            sudo=True,
        )[1]
    )
    facts["pam"] = {
        "check_rc": run(["authselect", "check"], sudo=True)[0],
        "current": run(["authselect", "current", "--raw"], sudo=True)[1].split(),
        "faillock": parse_faillock(run(["cat", "/etc/security/faillock.conf"])[1]),
    }
    rc, _, err = run(["/usr/lib/systemd/systemd-sysctl"], sudo=True)
    facts["sysctl"] = {"rc": rc, "stderr": err.strip()}
    facts["trust"] = json.loads(
        run(["cat", "/run/athanor-update/state.json"])[1] or "{}"
    ).get("verified", {})
    rc1, _, e1 = run(
        [
            "nix",
            "--extra-experimental-features",
            "nix-command",
            "build",
            "--no-link",
            "--impure",
            "--expr",
            NIX_EXPR % os.getpid(),
        ]
    )
    rc2, _, e2 = run(
        ["nix", "--extra-experimental-features", "nix-command", "store", "gc"]
    )
    facts["nix"] = {"build_rc": rc1, "gc_rc": rc2, "stderr": (e1 + e2).strip()[-400:]}
    return facts


# --- evaluation (pure) --------------------------------------------------------------------------


def evaluate(facts, expect, contacts):
    """Every violated expectation as (check, detail); empty when the guest conforms."""
    bad = []

    def fail(check, detail):
        bad.append((check, detail))

    # Listening sockets: each one is declared, and each declared unit is a listener of contacts.toml.
    declared_units = {x["unit"] for x in contacts.get("listener", [])}
    for entry in expect.get("listener", []):
        if entry.get("unit") and entry["unit"] not in declared_units:
            fail(
                "listeners",
                f"{entry['unit']} is in expectations.toml but not a [[listener]] of contacts.toml",
            )
    for s in facts["listeners"]:
        if not any(
            e["process"] == s["process"] and e.get("port", s["port"]) == s["port"]
            for e in expect.get("listener", [])
        ):
            fail(
                "listeners",
                f"undeclared {s['proto']} listener {s['address']}:{s['port']} ({s['process'] or 'unknown process'})",
            )
    fw = facts["firewall"]
    if fw is None and expect.get("firewall", {}).get("required"):
        fail("firewall", "firewall-cmd is absent but the firewall is required")
    for zone, services in (fw or {}).items():
        want = sorted(expect.get("firewall", {}).get("zones", {}).get(zone, []))
        if services != want:
            fail("firewall", f"zone {zone} opens {services}, declared {want}")

    # Exposure ceilings: a unit above its ceiling, or without one, fails.
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

    allowed = set(expect.get("selinux", {}).get("allow", []))
    for key in facts["avc"]:
        if key not in allowed:
            fail("selinux", f"denial {key} is not in the allow-list")

    pam, want = facts["pam"], expect.get("pam", {})
    if pam["check_rc"] != 0:
        fail("pam", f"authselect check exits {pam['check_rc']}")
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

    if facts["sysctl"]["rc"] != 0 or facts["sysctl"]["stderr"]:
        fail(
            "sysctl",
            f"systemd-sysctl exit {facts['sysctl']['rc']}: {facts['sysctl']['stderr'][:200]}",
        )

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

    if facts["nix"]["build_rc"] != 0 or facts["nix"]["gc_rc"] != 0:
        fail(
            "nix",
            f"build exit {facts['nix']['build_rc']}, gc exit {facts['nix']['gc_rc']}: {facts['nix']['stderr']}",
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
