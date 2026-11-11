#!/usr/bin/python3
"""Fail when an RPM installed in the system image does not match its spec in the checkout.

Usage: rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE} %{SOURCERPM}\\n' | check_image_rpms.py SPECS_DIR HASHES
system/check-image-rpms.sh runs it inside a built image; HASHES is the {package: content hash}
map of `dag_orchestrator.py --write-hashes` for the same checkout.

The source RPM of each installed package names the spec it was built from (Name: of
SPECS_DIR/*/*.spec). Two defects fail the check:
  - a package built from a spec of the checkout carries another version-release than that spec,
    which is what a stale per-package image in a tier repository produces;
  - a package named athanor-* has no spec in the checkout.
Everything else (Fedora's own packages, the Azoth kernel) is not ours to compare.

The expected version-release is parsed from the spec, strictly: the forge builder has no dist
macro, so %{?dist} is empty; %global macros are substituted; the autorelease fallback is taken
(the builder has no rpmautospec); %{?athanor_content} is `.h` and the first ten hex digits of
the package's content hash, which the DAG build defines (doc_build_system.md, section 5), so a
package built from other inputs fails even when its spec kept the same number. Any other macro
fails loudly, and only for the specs of installed packages, so an unrelated exotic spec cannot
break a build.
"""

import json
import pathlib
import re
import sys

MACRO = re.compile(r"%\{(\??)(\w+)\}")
AUTORELEASE = "%{?autorelease}%{!?autorelease:"


def expand(text, macros, where):
    text = text.replace("%{?dist}", "")
    if text.startswith(AUTORELEASE) and text.endswith("}"):
        text = text[len(AUTORELEASE) : -1]

    def sub(m):
        if m.group(2) == "athanor_content" and m.group(2) not in macros:
            raise ValueError(f"{where}: no content hash for its directory in the hashes")
        if m.group(2) not in macros:
            raise ValueError(f"{where}: macro {m.group(0)} is not supported")
        return macros[m.group(2)]

    for _ in range(5):
        text = MACRO.sub(sub, text)
    if "%" in text:
        raise ValueError(f"{where}: cannot expand {text!r}")
    return text


def read_spec(path, content_hash=None):
    """(name, version, release) of a spec, version and release still unexpanded."""
    macros, tags = {}, {}
    if content_hash:
        macros["athanor_content"] = ".h" + content_hash[:10]
    for line in pathlib.Path(path).read_text().splitlines():
        m = re.match(r"%(?:global|define)\s+(\w+)\s+(.*\S)", line)
        if m:
            macros[m.group(1)] = m.group(2)
        m = re.match(r"(Name|Version|Release):\s*(\S.*?)\s*$", line)
        if m:
            tags.setdefault(m.group(1), m.group(2))
    where = str(path)
    name = expand(tags["Name"], macros, where)
    return name, lambda: expand(tags["Version"], macros, where) + "-" + expand(tags["Release"], macros, where)


def spec_hashes(specs_dir, hashes):
    """{spec directory name: content hash}: a package's directory is athanor-<package>, else
    <package>, as dag_orchestrator.spec_dir_for has it."""
    found = {}
    for pkg, h in hashes.items():
        for d in (f"athanor-{pkg}", pkg):
            if pathlib.Path(specs_dir, d).is_dir():
                found[d] = h
                break
    return found


def load_specs(specs_dir, hashes):
    specs = {}
    by_dir = spec_hashes(specs_dir, hashes)
    for path in sorted(pathlib.Path(specs_dir).glob("*/*.spec")):
        name, nvr = read_spec(path, by_dir.get(path.parent.name))
        if name in specs:
            raise ValueError(f"{path}: spec {name} is also in {specs[name][0]}")
        specs[name] = (path, nvr)
    return specs


def check(rpm_lines, specs):
    """Problems of the installed packages, one string each; `specs` as load_specs returns."""
    problems = []
    for line in rpm_lines:
        name, vr, srpm = line.split()
        if srpm == "(none)":
            continue  # gpg-pubkey and the like
        source = srpm.removesuffix(".src.rpm").rsplit("-", 2)[0]
        if source in specs:
            path, expected = specs[source]
            try:
                want = expected()
            except ValueError as e:
                problems.append(str(e))
                continue
            if vr != want:
                problems.append(f"{name} {vr} is installed, but {path} says {want}")
        elif name.startswith("athanor-"):
            problems.append(f"{name} {vr} has no spec (built from {srpm})")
    return problems


def main():
    lines = [l for l in sys.stdin.read().splitlines() if l.strip()]
    if not lines:
        print("no installed package on stdin", file=sys.stderr)
        return 2
    if len(sys.argv) != 3:
        print("usage: check_image_rpms.py SPECS_DIR HASHES", file=sys.stderr)
        return 2
    hashes = json.loads(pathlib.Path(sys.argv[2]).read_text())
    problems = check(lines, load_specs(sys.argv[1], hashes))
    for p in problems:
        print(p, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
