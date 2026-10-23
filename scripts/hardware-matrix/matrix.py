"""The hardware matrix of release-1.0.md section 4.3 (doc_kernel_profile.md D17).

    matrix.py run RELEASE MACHINE                     on the machine, as the login user: run the
                                                      automatic rows of rows.toml
    matrix.py set RELEASE MACHINE ROW RESULT NOTE [--issue N]
                                                      record a row checked by hand, or triage an
                                                      automatic one (RESULT: pass, fail, untested)
    matrix.py summary RELEASE                         the table of every machine; exits 1 while a
                                                      row has no result or a failure no issue

RELEASE is a version such as 0.9, MACHINE one of the machines of rows.toml. A run is recorded in
docs/operations/hardware-matrix/RELEASE/MACHINE.json, which is committed with the release. `run`
keeps the rows checked by hand and replaces the automatic rows, so an automatic row is triaged
with `set` after the last `run`. A check that cannot run (a command missing, failing or timing
out, a file it cannot read) records `fail` with the reason, never `pass`."""

import datetime
import glob
import importlib.util
import json
import os
import pathlib
import re
import socket
import struct
import subprocess
import sys

import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
RESULTS = ROOT / "docs" / "operations" / "hardware-matrix"
RESULT_VALUES = ("pass", "fail", "untested")
RELEASE = re.compile(r"[0-9]+\.[0-9]+(\.[0-9]+)?")
ISSUE = re.compile(r"[1-9][0-9]*")
TIMEOUT = 30

_spec = importlib.util.spec_from_file_location(
    "probes", ROOT / "scripts" / "devvm" / "acceptance" / "probes.py"
)
probes = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(probes)


class Failed(Exception):
    """A check could not observe what it checks."""


# What a file or a command's output that is missing or not as expected raises.
UNOBSERVED = (OSError, ValueError, KeyError, TypeError, IndexError)


def run(argv):
    """(returncode, stdout, stderr); a missing program reads as 127, a timeout as 124."""
    try:
        # The C locale: nmcli and others translate the words the checks compare.
        p = subprocess.run(
            argv,
            capture_output=True,
            check=False,
            text=True,
            timeout=TIMEOUT,
            env=os.environ | {"LC_ALL": "C"},
        )
    except FileNotFoundError:
        return 127, "", f"{argv[0]}: not found"
    except subprocess.TimeoutExpired:
        return 124, "", f"timed out after {TIMEOUT}s"
    return p.returncode, p.stdout, p.stderr


def need(argv, ok=(0,)):
    """stdout of a command whose exit code is in `ok`; Failed otherwise."""
    rc, out, err = run(argv)
    if rc not in ok:
        raise Failed(f"`{' '.join(argv)}` exited {rc}: {err.strip()[-200:]}")
    return out


# --- automatic checks: each returns (result, evidence) or raises Failed ------------------------


def secure_boot():
    # Only the bare line: "validation is disabled in shim" or "Setup Mode" may follow it.
    text = need(["mokutil", "--sb-state"]).strip()
    return ("pass" if text == "SecureBoot enabled" else "fail"), text


def booted():
    """The booted deployment of `rpm-ostree status --json`."""
    for deployment in json.loads(need(["rpm-ostree", "status", "--json"]))[
        "deployments"
    ]:
        if deployment.get("booted"):
            return deployment
    raise Failed("rpm-ostree lists no booted deployment")


def signed_origin():
    deployment = booted()
    ref = deployment.get("container-image-reference", "")
    evidence = f"{ref} {deployment.get('container-image-reference-digest', '')}".strip()
    return ("pass" if ref.startswith("ostree-image-signed:") else "fail"), evidence


def kernel():
    release = os.uname().release
    return ("pass" if ".azoth." in release else "fail"), release


def selinux():
    text = need(["getenforce"]).strip()
    return ("pass" if text == "Enforcing" else "fail"), text


def avc_keys(text):
    """'source:target:class' of each denial; a denial line that does not parse is kept whole."""
    keys = set()
    for line in text.splitlines():
        if "denied" not in line:
            continue
        try:
            parsed = probes.parse_avc(line)
        except IndexError:
            parsed = []
        keys.update(parsed or [line.strip()[:200]])
    return sorted(keys)


def avc():
    # Without the system journal (the user outside wheel and systemd-journal), --grep matches
    # nothing and exits 1 as if the boot had no denial: PID 1 logs at every boot, so seeing
    # none of its lines means the journal is not readable.
    if not need(
        ["journalctl", "-b", "--no-pager", "-o", "cat", "-n", "1", "_PID=1"]
    ).strip():
        raise Failed("the system journal of this boot is not readable")
    rc, out, err = run(
        ["journalctl", "-b", "--no-pager", "-o", "cat", "--grep", "avc: +denied"]
    )
    # 1 with no output and no message is "no match"; anything else is an error.
    if rc not in (0, 1) or (rc == 1 and (out.strip() or err.strip())):
        raise Failed(f"journalctl --grep exited {rc}: {err.strip()[-200:]}")
    keys = avc_keys(out)
    return ("fail" if keys else "pass"), ", ".join(keys) or "no denial"


def nvidia_module():
    module = pathlib.Path("/sys/module/nvidia")
    if not module.is_dir():
        return "fail", "the nvidia module is not loaded"
    signer = need(["modinfo", "-F", "signer", "nvidia"]).strip()
    version = (module / "version").read_text().strip()
    drm = pathlib.Path("/sys/module/nvidia_drm").is_dir()
    ok = bool(signer) and drm
    evidence = f"version {version}, signer '{signer}', nvidia_drm {drm}"
    return ("pass" if ok else "fail"), evidence


def usb_internal():
    marker = pathlib.Path("/var/lib/athanor/usbguard-internal.done")
    result = need(
        ["systemctl", "show", "-P", "Result", "athanor-usbguard-internal.service"]
    ).strip()
    # Devices only: an interface (a name with ':') has no idVendor.
    devices = [
        pathlib.Path(p).parent
        for p in sorted(glob.glob("/sys/bus/usb/devices/*/authorized"))
        if ":" not in pathlib.Path(p).parent.name
    ]
    blocked = []
    for device in devices:
        if (device / "authorized").read_text().strip() != "1":
            ident = ":".join(
                (device / name).read_text().strip()
                for name in ("idVendor", "idProduct")
            )
            blocked.append(f"{device.name} {ident}")
    ok = marker.exists() and result == "success" and devices and not blocked
    evidence = (
        f"snapshot {'done' if marker.exists() else 'not done'}; unit result {result}; "
        f"{len(devices)} devices, blocked: {', '.join(blocked) or 'none'}"
    )
    return ("pass" if ok else "fail"), evidence


def network():
    up = []
    for line in need(["nmcli", "-t", "-f", "TYPE,STATE", "device"]).splitlines():
        kind, _, state = line.partition(":")
        if kind in ("ethernet", "wifi") and state.startswith("connected"):
            up.append(line)
    return ("pass" if up else "fail"), ", ".join(
        up
    ) or "no wired or wireless connection"


def bluetooth():
    rc, out, err = run(["bluetoothctl", "show"])
    if "No default controller" in out + err:
        return "fail", "no Bluetooth controller (blocked by USBGuard, or absent)"
    if rc != 0:
        raise Failed(f"`bluetoothctl show` exited {rc}: {err.strip()[-200:]}")
    powered = re.search(r"^\s*Powered: (\S+)", out, re.M)
    controller = out.strip().splitlines()[0] if out.strip() else ""
    ok = powered is not None and powered.group(1) == "yes"
    return (
        "pass" if ok else "fail"
    ), f"{controller}, powered {powered and powered.group(1)}"


def battery():
    capacities = sorted(glob.glob("/sys/class/power_supply/BAT*/capacity"))
    if not capacities:
        return "fail", "no battery under /sys/class/power_supply"
    return "pass", ", ".join(
        f"{pathlib.Path(p).parent.name} {pathlib.Path(p).read_text().strip()} %"
        for p in capacities
    )


def failed_units():
    failed = []
    for scope in ([], ["--user"]):
        out = need(["systemctl", *scope, "--failed", "--no-legend", "--plain"])
        failed += [line.split()[0] for line in out.splitlines() if line.strip()]
    return ("fail" if failed else "pass"), ", ".join(failed) or "none"


def shell_units():
    units = ["athanor-bar.service", "athanor-dock.service", "athanor-shelld.service"]
    # is-active exits 3 when a unit is not active; it still prints one state per unit.
    states = need(["systemctl", "--user", "is-active", *units], ok=(0, 3)).split()
    if len(states) != len(units):
        raise Failed(f"systemctl --user is-active printed {states}")
    evidence = ", ".join(f"{u} {s}" for u, s in zip(units, states))
    return ("pass" if states == ["active"] * len(units) else "fail"), evidence


def peer_pid(path):
    """The pid of the process listening on the Unix socket `path`, from SO_PEERCRED."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(TIMEOUT)
        s.connect(path)
        creds = s.getsockopt(
            socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")
        )
    return struct.unpack("3i", creds)[0]


def compositor():
    """Which compositor serves the session: the executable behind the Wayland socket."""
    runtime, name = os.environ.get("XDG_RUNTIME_DIR"), os.environ.get("WAYLAND_DISPLAY")
    if not runtime or not name:
        raise Failed(
            "XDG_RUNTIME_DIR and WAYLAND_DISPLAY are not set: run matrix.py from inside the session"
        )
    exe = os.readlink(f"/proc/{peer_pid(str(pathlib.Path(runtime) / name))}/exe")
    return ("pass" if exe == "/usr/bin/athanor-compositor" else "fail"), exe


CHECKS = {
    f.__name__: f
    for f in (
        secure_boot,
        signed_origin,
        kernel,
        selinux,
        avc,
        nvidia_module,
        usb_internal,
        network,
        bluetooth,
        battery,
        failed_units,
        shell_units,
        compositor,
    )
}


def attempt(check):
    """(result, evidence) of a check, a check that cannot observe recorded as a failure."""
    try:
        return CHECKS[check]()
    except Failed as e:
        return "fail", f"could not check: {e}"
    except UNOBSERVED as e:
        return "fail", f"could not check: {type(e).__name__}: {e}"


# --- the matrix (pure) ---------------------------------------------------------------------------


def load(path=HERE / "rows.toml"):
    with open(path, "rb") as f:
        return tomllib.load(f)


def validate(matrix):
    """Problems of rows.toml: duplicate ids, unknown checks or machines, a row both automatic
    and by hand or neither, an untested entry without a reason."""
    problems, seen = [], set()
    for entry in matrix.get("row", []) + matrix.get("untested", []):
        if entry["id"] in seen:
            problems.append(f"{entry['id']}: duplicate id")
        seen.add(entry["id"])
    for row in matrix.get("row", []):
        if ("check" in row) == ("steps" in row):
            problems.append(f"{row['id']}: needs exactly one of check and steps")
        if "check" in row and row["check"] not in CHECKS:
            problems.append(f"{row['id']}: unknown check {row['check']}")
        unknown = set(row.get("machines", [])) - set(matrix["machines"])
        if not row.get("machines") or unknown:
            problems.append(
                f"{row['id']}: machines must be a non-empty subset of {matrix['machines']}"
            )
    for entry in matrix.get("untested", []):
        if not entry.get("reason", "").strip():
            problems.append(f"{entry['id']}: an untested entry needs a reason")
    return problems


def cell(entry):
    """(text, complete) of one row on one machine."""
    if entry is None:
        return "missing", False
    result = entry.get("result")
    if result not in RESULT_VALUES or not str(entry.get("evidence", "")).strip():
        return f"invalid ({result})", False
    if result == "fail":
        if entry.get("issue"):
            return f"fail (#{entry['issue']})", True
        return "fail (no issue)", False
    return result, True


def summary(matrix, records):
    """(Markdown table, complete) for {machine: record}. Complete when every row has a valid
    result with evidence on each of its machines and every failure names an issue."""
    machines = matrix["machines"]
    lines = [
        "| Row | " + " | ".join(machines) + " |",
        "|---|" + "---|" * len(machines),
    ]
    complete = True
    for row in matrix["row"]:
        cells = []
        for machine in machines:
            if machine not in row["machines"]:
                cells.append("n/a")
                continue
            text, ok = cell(records.get(machine, {}).get("rows", {}).get(row["id"]))
            complete &= ok
            cells.append(text)
        lines.append(f"| {row['title']} | " + " | ".join(cells) + " |")
    lines.append("")
    lines += [
        f"- Untested: {u['title']}: {u['reason']}." for u in matrix.get("untested", [])
    ]
    return "\n".join(lines) + "\n", complete


# --- records ---------------------------------------------------------------------------------------


def record_path(release, machine):
    return RESULTS / release / f"{machine}.json"


def read_record(release, machine):
    path = record_path(release, machine)
    return json.loads(path.read_text()) if path.exists() else {"rows": {}}


def write_record(release, machine, record):
    path = record_path(release, machine)
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_suffix(".json.partial")
    partial.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
    partial.replace(path)


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def host():
    facts = {"hostname": socket.gethostname(), "kernel": os.uname().release}
    try:
        deployment = booted()
    except (Failed, *UNOBSERVED) as e:
        return facts | {"error": f"no booted deployment: {type(e).__name__}: {e}"}
    return facts | {
        "image": deployment.get("container-image-reference", ""),
        "digest": deployment.get("container-image-reference-digest", ""),
        "version": deployment.get("version", ""),
    }


def cmd_run(matrix, release, machine):
    record = read_record(release, machine)
    # An interrupted run must not leave rows of an earlier boot under this boot's header.
    record["rows"] = {
        k: v for k, v in record["rows"].items() if v.get("how") != "automatic"
    }
    record.update(release=release, machine=machine, host=host())
    for row in matrix["row"]:
        if "check" not in row or machine not in row["machines"]:
            continue
        result, evidence = attempt(row["check"])
        record["rows"][row["id"]] = {
            "result": result,
            "evidence": evidence,
            "how": "automatic",
            "at": now(),
        }
        write_record(release, machine, record)
        print(f"{result:8} {row['id']}: {evidence}")
    return 0


def cmd_set(matrix, release, machine, row_id, result, note, issue):
    rows = {row["id"]: row for row in matrix["row"]}
    if row_id not in rows or machine not in rows[row_id]["machines"]:
        sys.exit(f"{row_id}: not a row of {machine}")
    if result not in RESULT_VALUES:
        sys.exit(f"result must be one of {', '.join(RESULT_VALUES)}")
    if not note.strip():
        sys.exit("the note is the evidence: say what was done and seen")
    record = read_record(release, machine)
    record["rows"][row_id] = {
        "result": result,
        "evidence": note,
        "how": "maintainer",
        "at": now(),
    }
    if issue:
        record["rows"][row_id]["issue"] = issue
    write_record(release, machine, record)
    return 0


def main(argv):
    matrix = load()
    problems = validate(matrix)
    if problems:
        sys.exit("rows.toml: " + "; ".join(problems))
    if len(argv) < 2 or not RELEASE.fullmatch(argv[1]):
        sys.exit(__doc__)
    if len(argv) >= 3 and argv[2] not in matrix["machines"]:
        sys.exit(f"MACHINE must be one of {', '.join(matrix['machines'])}")
    if len(argv) == 3 and argv[0] == "run":
        return cmd_run(matrix, argv[1], argv[2])
    if len(argv) in (6, 8) and argv[0] == "set":
        issue = None
        if len(argv) == 8:
            if argv[6] != "--issue" or not ISSUE.fullmatch(argv[7]):
                sys.exit("--issue takes an issue number")
            issue = int(argv[7])
        return cmd_set(matrix, *argv[1:6], issue)
    if len(argv) == 2 and argv[0] == "summary":
        records = {m: read_record(argv[1], m) for m in matrix["machines"]}
        text, complete = summary(matrix, records)
        print(text, end="")
        return 0 if complete else 1
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
