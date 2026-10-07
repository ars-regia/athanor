#!/usr/bin/env python3
"""The kernel profile manifest of Athanor: validation over every role combination and
generation of the effective profile that athanor-profile-check enforces.

Usage:
  kernel_profile.py generate [--manifest PATH] [--out DIR] [--kargs PATH] [--boot-cmdline PATH] [--sysctl PATH]
  kernel_profile.py check    [--manifest PATH] [--out DIR] [--kargs PATH] [--boot-cmdline PATH] [--sysctl PATH]

generate writes one JSON file per allowed role combination into DIR, by default the
package's SOURCES/usr/share/athanor/kernel-profile, and the base kernel command line twice:
as the bootc kargs.d file the package ships (--kargs) and as the command line the kernel
boot matrix uses (--boot-cmdline, forge/specs/azoth/cmdline). Those two files are the only
copies of the command line in the repository. It also writes the base sysctls as the
sysctl.d file the package ships (--sysctl). check composes every combination and fails
on any composition error, and on generated files that differ from the manifest.

Specification: docs/architecture/doc_kernel_profile.md, sections 3, 7 and 12. Standard
library only (tomllib needs Python 3.11 or later), so it runs on the GitHub runner as is.
"""

import argparse
import itertools
import json
import pathlib
import sys
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
MANIFEST = HERE / "profile.toml"
OUT = HERE / "SOURCES" / "usr" / "share" / "athanor" / "kernel-profile"
KARGS = HERE / "SOURCES" / "usr" / "lib" / "bootc" / "kargs.d" / "10-athanor-kernel-profile.toml"
BOOT_CMDLINE = HERE.parent / "azoth" / "cmdline"
# After Fedora's 10-default-yama-scope.conf and 50-coredump.conf, whose ptrace_scope and
# suid_dumpable the base overrides (D47); /etc/sysctl.d stays the administrator's.
SYSCTL = HERE / "SOURCES" / "usr" / "lib" / "sysctl.d" / "90-athanor-kernel-profile.conf"
SCHEMA = 1
KINDS = ("kconfig", "sysctl", "runtime", "cmdline")
FIELDS = {"value", "decision", "locked", "priority", "ordering"}
TOP_LEVEL = {"schema", "base", "fragments", "roles", "rules"}


class ProfileError(Exception):
    """A manifest that cannot be composed; the message names the setting and its sources."""


def definitions(section: dict, where: str) -> dict:
    """{(kind, name): definition} declared by one base, fragment or role section."""
    out = {}
    for kind, table in section.items():
        if kind == "includes":
            if not where.startswith("roles."):
                raise ProfileError(f"{where}: includes is only allowed on a role's own section")
            continue
        if kind not in KINDS:
            raise ProfileError(f"{where}: unknown kind {kind!r}, expected one of {', '.join(KINDS)}")
        if not isinstance(table, dict):
            raise ProfileError(f"{where}.{kind}: expected a table of settings")
        for name, definition in table.items():
            label = f"{where}.{kind}.{name}"
            if not isinstance(definition, dict) or "value" not in definition or "decision" not in definition:
                raise ProfileError(f"{label}: a setting needs a value and a decision")
            unknown = set(definition) - FIELDS
            if unknown:
                raise ProfileError(f"{label}: unknown fields {sorted(unknown)}")
            if definition.get("ordering") not in (None, "max", "min"):
                raise ProfileError(f"{label}: ordering must be max or min")
            if definition.get("locked") and where != "base":
                raise ProfileError(f"{label}: only base settings can be locked")
            if kind == "cmdline":
                parameter(label, where, name, definition["value"])
            out[(kind, name)] = definition
    return out


def parameter(label: str, where: str, name: str, value) -> None:
    """A command line setting is one name=value parameter, declared by the base: the
    image has a single command line until roles get their UKI addons (block P5)."""
    if where != "base":
        raise ProfileError(f"{label}: command line settings are base only until block P5")
    if not isinstance(value, str) or not name or any(c.isspace() or c in '="\\' for c in name):
        raise ProfileError(f"{label}: a command line setting is a parameter name with a string value")
    if not value or any(c.isspace() or c in '"\\' for c in value):
        raise ProfileError(f"{label}: a command line value is non-empty, without spaces, quotes or backslashes")


def shape(value) -> str:
    if isinstance(value, list):
        return "list"
    if isinstance(value, dict):
        return "map"
    if isinstance(value, (bool, int, float, str)):
        return "scalar"
    raise ProfileError(f"unsupported value {value!r}")


def union(first: list, second: list) -> list:
    merged = list(first)
    for item in second:
        if item not in merged:
            merged.append(item)
    return merged


def between_roles(label: str, first: tuple, second: tuple) -> tuple:
    """Two role or fragment definitions of one setting, merged; a conflict raises."""
    (first_where, first_def), (second_where, second_def) = first, second

    def merge(path: str, a, b):
        if shape(a) != shape(b):
            raise ProfileError(f"{path}: {first_where} sets a {shape(a)}, {second_where} a {shape(b)}")
        if shape(a) == "list":
            return union(a, b)
        if shape(a) == "map":
            merged = dict(a)
            for key, item in b.items():
                merged[key] = merge(f"{path}.{key}", a[key], item) if key in a else item
            return merged
        if a == b:
            return a
        a_priority, b_priority = first_def.get("priority", 0), second_def.get("priority", 0)
        if a_priority != b_priority:
            return a if a_priority > b_priority else b
        ordering = first_def.get("ordering")
        numbers = all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (a, b))
        if ordering and ordering == second_def.get("ordering") and numbers:
            return max(a, b) if ordering == "max" else min(a, b)
        raise ProfileError(
            f"{path}: {first_where} sets {a!r} and {second_where} sets {b!r}; give one "
            "definition a higher priority or declare the same ordering on both"
        )

    merged = dict(first_def)
    merged["value"] = merge(label, first_def["value"], second_def["value"])
    merged["priority"] = max(first_def.get("priority", 0), second_def.get("priority", 0))
    return f"{first_where} and {second_where}", merged


def over_base(label: str, base_def: dict, role_where: str, role_def: dict) -> dict:
    """A role definition applied over the base: lists extend, maps merge, scalars replace,
    and a locked base setting refuses any change."""

    def apply(path: str, base, role):
        if shape(base) != shape(role):
            raise ProfileError(f"{path}: base sets a {shape(base)}, {role_where} a {shape(role)}")
        if shape(base) == "list":
            return union(base, role)
        if shape(base) == "map":
            merged = dict(base)
            for key, item in role.items():
                merged[key] = apply(f"{path}.{key}", base[key], item) if key in base else item
            return merged
        return role

    value = apply(label, base_def["value"], role_def["value"])
    if value == base_def["value"]:
        return base_def
    if base_def.get("locked"):
        raise ProfileError(f"{label}: locked in base, and {role_where} changes it to {value!r}")
    return {**base_def, "value": value, "decision": role_def["decision"]}


def role_sections(manifest: dict, role: str) -> list:
    """[(where, section)] a role contributes: its fragments first, then its own section."""
    roles = manifest.get("roles", {})
    fragments = manifest.get("fragments", {})
    if role not in roles:
        raise ProfileError(f"unknown role {role!r}")
    sections = []
    for fragment in roles[role].get("includes", []):
        if fragment not in fragments:
            raise ProfileError(f"roles.{role}: unknown fragment {fragment!r}")
        if "includes" in fragments[fragment]:
            raise ProfileError(f"fragments.{fragment}: a fragment cannot include others")
        sections.append((f"fragments.{fragment}", fragments[fragment]))
    sections.append((f"roles.{role}", roles[role]))
    return sections


def effective(manifest: dict, combination: tuple) -> dict:
    """{(kind, name): definition} in force for a role combination."""
    base = definitions(manifest.get("base", {}), "base")
    contributions: dict = {}
    visited = set()
    for role in combination:
        for where, section in role_sections(manifest, role):
            if where in visited:
                continue
            visited.add(where)
            for key, definition in definitions(section, where).items():
                contributions.setdefault(key, []).append((where, definition))
    result = dict(base)
    for key, sources in contributions.items():
        label = f"{key[0]}.{key[1]}"
        where, definition = sources[0]
        for other in sources[1:]:
            where, definition = between_roles(label, (where, definition), other)
        result[key] = over_base(label, base[key], where, definition) if key in base else definition
    return result


def combinations(manifest: dict) -> tuple:
    """(allowed, refused): every subset of the roles, sorted, split by rules.rejected"""
    roles = sorted(manifest.get("roles", {}))
    rules = manifest.get("rules", {})
    unknown_rules = set(rules) - {"rejected"}
    if unknown_rules:
        raise ProfileError(f"rules: unknown fields {sorted(unknown_rules)}")
    rejected = []
    for pair in rules.get("rejected", []):
        distinct = frozenset(pair)
        if len(distinct) < 2:
            raise ProfileError(f"rules.rejected: {list(pair)!r} must name at least two distinct roles")
        unknown = distinct - set(roles)
        if unknown:
            raise ProfileError(f"rules.rejected names unknown roles {sorted(unknown)}")
        rejected.append(distinct)
    allowed, refused = [], []
    for size in range(len(roles) + 1):
        for combination in itertools.combinations(roles, size):
            if any(pair <= set(combination) for pair in rejected):
                refused.append(combination)
            else:
                allowed.append(combination)
    return allowed, refused


def combination_name(combination: tuple) -> str:
    return "+".join(combination) if combination else "base"


def render(manifest: dict, combination: tuple) -> str:
    """The effective profile of one combination, as the JSON text the checker reads."""
    name = combination_name(combination)
    try:
        settings = effective(manifest, combination)
    except ProfileError as error:
        raise ProfileError(f"combination {name}: {error}") from None
    document = {
        "schema": SCHEMA,
        "combination": name,
        "roles": list(combination),
        "settings": {kind: {} for kind in KINDS},
    }
    for (kind, setting), definition in sorted(settings.items()):
        document["settings"][kind][setting] = {
            "value": definition["value"],
            "decision": definition["decision"],
        }
    return json.dumps(document, indent=2, sort_keys=True) + "\n"


def build(manifest: dict) -> tuple:
    """({file name: JSON text} for every allowed combination, refused combinations)."""
    if manifest.get("schema") != SCHEMA:
        raise ProfileError(f"schema must be {SCHEMA}")
    unknown = set(manifest) - TOP_LEVEL
    if unknown:
        raise ProfileError(f"unknown top-level tables {sorted(unknown)}")
    allowed, refused = combinations(manifest)
    files = {f"{combination_name(c)}.json": render(manifest, c) for c in allowed}
    return files, refused


def base_cmdline(manifest: dict) -> list:
    """The name=value parameters of the base command line, sorted by name."""
    settings = effective(manifest, ())
    return [f"{name}={d['value']}" for (kind, name), d in sorted(settings.items()) if kind == "cmdline"]


def kargs_text(arguments: list) -> str:
    """The bootc kargs.d file of the base command line, which bootc applies to every
    deployment of the image."""
    lines = [
        "# Generated by forge/specs/athanor-kernel-profile/kernel_profile.py from the",
        "# [base.cmdline] table of profile.toml: edit the manifest and regenerate.",
        "kargs = [",
        *(f'    "{argument}",' for argument in arguments),
        "]",
        'match-architectures = ["x86_64"]',
    ]
    return "\n".join(lines) + "\n"


def command_lines(manifest: dict, kargs: pathlib.Path, boot_cmdline: pathlib.Path) -> dict:
    """{path: text} of the two generated copies of the base command line."""
    arguments = base_cmdline(manifest)
    if not arguments:
        raise ProfileError("base.cmdline: the base command line is empty")
    return {kargs: kargs_text(arguments), boot_cmdline: " ".join(arguments) + "\n"}


def sysctl_text(manifest: dict) -> str:
    """The sysctl.d file of the base sysctls, which systemd-sysctl applies at every boot."""
    settings = effective(manifest, ())
    lines = [
        "# Generated by forge/specs/athanor-kernel-profile/kernel_profile.py from the",
        "# [base.sysctl] table of profile.toml: edit the manifest and regenerate.",
        *(f"{name} = {d['value']}" for (kind, name), d in sorted(settings.items()) if kind == "sysctl"),
    ]
    return "\n".join(lines) + "\n"


def main(argv: list | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate and generate the Athanor kernel profile.")
    parser.add_argument("command", choices=("check", "generate"))
    parser.add_argument("--manifest", type=pathlib.Path, default=MANIFEST)
    parser.add_argument("--out", type=pathlib.Path, default=OUT)
    parser.add_argument("--kargs", type=pathlib.Path, default=KARGS)
    parser.add_argument("--boot-cmdline", type=pathlib.Path, default=BOOT_CMDLINE)
    parser.add_argument("--sysctl", type=pathlib.Path, default=SYSCTL)
    args = parser.parse_args(argv)

    try:
        with open(args.manifest, "rb") as handle:
            manifest = tomllib.load(handle)
        files, refused = build(manifest)
        lines = command_lines(manifest, args.kargs, args.boot_cmdline)
        lines[args.sysctl] = sysctl_text(manifest)
    except (OSError, tomllib.TOMLDecodeError, ProfileError) as error:
        print(f"kernel_profile: {error}", file=sys.stderr)
        return 1

    if args.command == "generate":
        args.out.mkdir(parents=True, exist_ok=True)
        for old in args.out.glob("*.json"):
            if old.name not in files:
                old.unlink()
        for name, text in files.items():
            (args.out / name).write_text(text)
        for path, text in lines.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        print(f"kernel_profile: wrote {len(files)} combinations to {args.out}, the command line and the sysctls")
        return 0

    for combination in refused:
        print(f"kernel_profile: rejected combination {combination_name(combination)} (rules.rejected)")
    present = {path.name for path in args.out.glob("*.json")} if args.out.is_dir() else set()
    stale = sorted(
        name
        for name, text in files.items()
        if not (args.out / name).is_file() or (args.out / name).read_text() != text
    )
    stale += sorted(present - set(files))
    stale += [str(path) for path, text in lines.items() if not path.is_file() or path.read_text() != text]
    if stale:
        print(
            "kernel_profile: generated files differ from the manifest, run "
            "`python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py generate`: "
            + ", ".join(stale),
            file=sys.stderr,
        )
        return 1
    print(f"kernel_profile: {len(files)} combinations valid, generated files, command line and sysctls up to date")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
