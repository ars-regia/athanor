"""The hardware matrix of release-1.0.md section 4.3 (doc_kernel_profile.md D17).

    matrix.py run RELEASE MACHINE                       on the machine, as the login user: run
                                                        the automatic rows of rows.toml
    matrix.py set RELEASE MACHINE ROW RESULT NOTE [--issue N]
                                                        record a row checked by hand, or a
                                                        triaged automatic one
    matrix.py summary RELEASE                           the table of every machine; exits 1 while
                                                        a row has no result or a failure no issue

A run is recorded in docs/operations/hardware-matrix/RELEASE/MACHINE.json, which is committed
with the release. `run` only observes and keeps the rows checked by hand; it replaces the automatic
rows, so an automatic row is triaged with `set` after the last `run`."""

import datetime
import glob
import importlib.util
import json
import os
import pathlib
import socket
import sys

import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
RESULTS = ROOT / "docs" / "operations" / "hardware-matrix"
RESULT_VALUES = ("pass", "fail", "untested")

_spec = importlib.util.spec_from_file_location(
    "probes", ROOT / "scripts" / "devvm" / "acceptance" / "probes.py"
)
probes = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(probes)
run = probes.run


# --- automatic checks: each returns (result, evidence) -----------------------------------------


def secure_boot():
    rc, out, err = run(["mokutil", "--sb-state"])
    text = (out + err).strip()
    return ("pass" if "SecureBoot enabled" in text else "fail"), text


def booted():
    """The booted deployment of `rpm-ostree status --json`, or {} when it cannot be read."""
    rc, out, err = run(["rpm-ostree", "status", "--json"])
    if rc != 0:
        return {}
    for deployment in json.loads(out).get("deployments", []):
        if deployment.get("booted"):
            return deployment
    return {}


def signed_origin():
    deployment = booted()
    ref = deployment.get("container-image-reference", "")
    evidence = f"{ref} {deployment.get('container-image-reference-digest', '')}".strip()
    return ("pass" if ref.startswith("ostree-image-signed:") else "fail"), evidence


def kernel():
    release = os.uname().release
    return ("pass" if "azoth" in release else "fail"), release


def selinux():
    rc, out, err = run(["getenforce"])
    text = (out + err).strip()
    return ("pass" if text == "Enforcing" else "fail"), text


def avc():
    rc, out, err = run(
        ["journalctl", "-b", "--no-pager", "-o", "cat", "--grep", "avc: +denied"]
    )
    # journalctl exits 1 when --grep matches nothing.
    if rc not in (0, 1):
        return "fail", f"journalctl exited {rc}: {err.strip()}"
    keys = probes.parse_avc(out)
    return ("fail" if keys else "pass"), ", ".join(keys) or "no denial"


def nvidia_module():
    if not pathlib.Path("/sys/module/nvidia").is_dir():
        return "fail", "the nvidia module is not loaded"
    rc, out, err = run(["modinfo", "-F", "signer", "nvidia"])
    signer = out.strip()
    version = pathlib.Path("/sys/module/nvidia/version").read_text().strip()
    drm = pathlib.Path("/sys/module/nvidia_drm").is_dir()
    ok = bool(signer) and drm
    return (
        "pass" if ok else "fail"
    ), f"version {version}, signer '{signer}', nvidia_drm {drm}"


def usb_internal():
    blocked = []
    for path in sorted(glob.glob("/sys/bus/usb/devices/*/authorized")):
        device = pathlib.Path(path).parent
        if (device / "authorized").read_text().strip() != "1":
            ident = ":".join(
                (device / name).read_text().strip()
                for name in ("idVendor", "idProduct")
                if (device / name).exists()
            )
            blocked.append(f"{device.name} {ident}")
    rc, out, err = run(["systemctl", "is-failed", "athanor-usbguard-internal.service"])
    failed = out.strip() == "failed"
    evidence = f"blocked: {', '.join(blocked) or 'none'}; athanor-usbguard-internal {out.strip()}"
    return ("fail" if blocked or failed else "pass"), evidence


def network():
    rc, out, err = run(["nmcli", "-t", "-f", "TYPE,STATE", "device"])
    up = [
        line
        for line in out.splitlines()
        if line in ("ethernet:connected", "wifi:connected")
    ]
    return ("pass" if up else "fail"), ", ".join(up) or (out + err).strip()


def bluetooth():
    rc, out, err = run(["bluetoothctl", "show"])
    text = out + err
    if "No default controller" in text:
        return "fail", "no Bluetooth controller (blocked by USBGuard, or absent)"
    return ("pass" if "Powered: yes" in text else "fail"), text.strip().splitlines()[0]


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
        rc, out, err = run(["systemctl", *scope, "--failed", "--no-legend", "--plain"])
        failed += [line.split()[0] for line in out.splitlines() if line.strip()]
    return ("fail" if failed else "pass"), ", ".join(failed) or "none"


def shell_units():
    units = ["athanor-bar.service", "athanor-dock.service", "athanor-shelld.service"]
    rc, out, err = run(["systemctl", "--user", "is-active", *units])
    states = out.split()
    evidence = ", ".join(f"{u} {s}" for u, s in zip(units, states)) or err.strip()
    return ("pass" if states == ["active"] * len(units) else "fail"), evidence


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
    )
}


# --- the matrix (pure) ---------------------------------------------------------------------------


def load(path=HERE / "rows.toml"):
    with open(path, "rb") as f:
        return tomllib.load(f)


def validate(matrix):
    """Problems of rows.toml: duplicate ids, unknown checks or machines, a row both automatic
    and by hand or neither."""
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
    return problems


def summary(matrix, records):
    """(Markdown table, complete) for {machine: record}. Complete when every row has a result
    on each of its machines and every failure names an issue."""
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
            entry = records.get(machine, {}).get("rows", {}).get(row["id"])
            if entry is None:
                complete = False
                cells.append("missing")
                continue
            cell = entry["result"]
            if entry["result"] == "fail":
                if entry.get("issue"):
                    cell += f" (#{entry['issue']})"
                else:
                    complete = False
                    cell += " (no issue)"
            cells.append(cell)
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
    path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def cmd_run(matrix, release, machine):
    record = read_record(release, machine)
    deployment = booted()
    record.update(
        release=release,
        machine=machine,
        host={
            "hostname": socket.gethostname(),
            "kernel": os.uname().release,
            "image": deployment.get("container-image-reference", ""),
            "digest": deployment.get("container-image-reference-digest", ""),
            "version": deployment.get("version", ""),
        },
    )
    for row in matrix["row"]:
        if "check" not in row or machine not in row["machines"]:
            continue
        result, evidence = CHECKS[row["check"]]()
        record["rows"][row["id"]] = {
            "result": result,
            "evidence": evidence,
            "how": "automatic",
            "at": now(),
        }
        print(f"{result:8} {row['id']}: {evidence}")
    write_record(release, machine, record)
    return 0


def cmd_set(matrix, release, machine, row_id, result, note, issue):
    rows = {row["id"]: row for row in matrix["row"]}
    if row_id not in rows or machine not in rows[row_id]["machines"]:
        sys.exit(f"{row_id}: not a row of {machine}")
    if result not in RESULT_VALUES:
        sys.exit(f"result must be one of {', '.join(RESULT_VALUES)}")
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
    if len(argv) == 3 and argv[0] == "run":
        return cmd_run(matrix, argv[1], argv[2])
    if len(argv) in (6, 8) and argv[0] == "set":
        issue = None
        if len(argv) == 8:
            if argv[6] != "--issue" or not argv[7].isdigit():
                sys.exit(
                    "usage: matrix.py set RELEASE MACHINE ROW RESULT NOTE [--issue N]"
                )
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
