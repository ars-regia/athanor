#!/usr/bin/python3
"""golden_report.py surfaces | report [--since EPOCH] [--elapsed SECONDS] [surface...]

The own compositor's captures against cosmic-comp's goldens (doc_own_stack.md, OS9). `report`
reads the captures `rig.sh golden-report` left in --out, counts for every case the pixels that
differ from its golden (compare.py's measure and tolerance) and writes golden-report.md: per
case the count and the explanation from golden/athanor-differences.toml, or `unexplained`.
It fails on a difference without an explanation, on an explanation without a difference (a
cause that was fixed, or never held), and on an explanation without a cause or an issue, so a
difference never passes as tolerated noise. A case may have several explanations, one per cause.
A differing case that draws a control decision 28 hides under the own compositor names it with
that control's cause and issue (HIDDEN), and no case names a control it does not draw.

`surfaces` prints the surfaces the report runs: every one of cases.SURFACES except the
toolkit's and the greeter's, and the layout cases.
"""

import argparse
import sys
import tomllib
from collections import namedtuple
from pathlib import Path

import cases
import compare

HERE = Path(__file__).resolve().parent
GOLDEN = HERE / "golden"
EXPLANATIONS = GOLDEN / "athanor-differences.toml"
# The toolkit's surfaces have no cosmic-comp golden to match: OS10 compares them with the GTK twin.
TOOLKIT = ("toolkit-chooser",)
# The greeter runs on cosmic-comp in greetd's session until it moves (doc_session.md SN12,
# doc_own_stack.md section 6 step 5): the own compositor has no allow-list line for it.
EXCLUDED = ("greeter",)
# What the compositor logs when it closes a layer surface its client has no grant for (src/layer.rs).
CLOSED = "not granted them was closed"

# doc_own_stack.md, decision 28: the controls the shell hides under the own compositor until their
# replacement comes, each with the cause its entries name and the issue that tracks its return.
HIDDEN = {
    "bar-workspaces": ("decision 28 hides the bar's workspaces module", 835),
    "bar-library": ("decision 28 hides the bar's application library button", 836),
    "bar-a11y-rows": (
        "decision 28 hides the magnifier, inversion and filter rows of the bar's accessibility menu",
        837,
    ),
    "dock-workspaces": ("decision 28 hides the dock's workspaces button", 835),
    "dock-library": ("decision 28 hides the dock's application library button", 836),
}
# The accessible name of each control of HIDDEN that a screen reader's references hold, by
# language: atspi_compare.py and orca_compare.py exempt it under the own compositor (decision 28
# item 3). The rows of the accessibility menu are in no reference: no walk opens the menu.
NAMES = {
    "bar-workspaces": {"en": "Workspaces", "it": "Spazi di lavoro"},
    "bar-library": {"en": "Applications", "it": "Applicazioni"},
    "dock-workspaces": {"en": "Workspaces", "it": "Spazi di lavoro"},
    "dock-library": {"en": "Applications", "it": "Applicazioni"},
}
# rig.sh capture_bar: the bar surfaces that run the bar preset; every other one runs float. The
# bar surfaces run the bar alone, the dock surface the dock alone.
BAR_PRESET_SURFACES = ("bar", "bar-shield")
LAYOUTS = {case.tag: case for case in cases.layout_cases()}

# differing is None when there is no capture to compare.
Result = namedtuple("Result", "surface case differing allowed closed")


def surfaces():
    return [s for s in cases.SURFACES if s not in TOOLKIT + EXCLUDED] + ["layout"]


def expected(wanted):
    """(surface, tag) of every case of the surfaces, in the order of cases.py."""
    found = []
    for surface in wanted:
        if surface == "layout":
            found += [(surface, case.tag) for case in cases.layout_cases()]
        else:
            found += [(surface, case.tag) for case in cases.surface_cases(surface)]
    return found


def differs(result):
    return result.differing is not None and not compare.verdict(result.differing, result.allowed)


def drawn(preset, dock, accessibility_menu=False):
    """The controls of HIDDEN that a bar running `preset` (None: no bar) and a dock set to `dock`
    draw under cosmic-comp, by the presets of doc_bar.md BR7 (workspaces under float and
    minimal, the application library under all three) and the dock setting of doc_shell.md SH7
    (its buttons only on a visible dock)."""
    controls = set()
    if preset in ("float", "minimal"):
        controls.add("bar-workspaces")
    if preset is not None:
        controls.add("bar-library")
    if accessibility_menu:
        controls.add("bar-a11y-rows")
    if dock == "visible":
        controls |= {"dock-workspaces", "dock-library"}
    return controls


def hidden(surface, case):
    """The controls of HIDDEN the case draws under cosmic-comp (drawn)."""
    if surface == "layout":
        layout = LAYOUTS.get(case)
        if layout is None:
            return set()
        preset, dock = layout.preset, layout.dock
    elif surface == "bar" or surface.startswith("bar-"):
        preset, dock = ("bar" if surface in BAR_PRESET_SURFACES else "float"), "-"
    elif surface == "dock":
        preset, dock = None, "visible"
    else:
        return set()
    return drawn(preset, dock, surface == "bar-accessibility")


def exemptions(preset, dock, lang):
    """{accessible name: (cause, issue)} of the controls decision 28 hides in a screen reader's
    scene, a bar running `preset` and a dock set to `dock` (drawn), in `lang` (NAMES)."""
    return {NAMES[c][lang]: HIDDEN[c] for c in sorted(drawn(preset, dock)) if c in NAMES}


def check_hidden(results, by_case):
    """What is wrong with the entries of the controls decision 28 hides."""
    problems, issues = [], dict(HIDDEN.values())
    for r in results:
        drawn = {HIDDEN[control] for control in hidden(r.surface, r.case)}
        named = {(e.get("cause"), e.get("issue")) for e in by_case.get(r.case, []) if e.get("cause") in issues}
        if differs(r):
            problems += [
                f'{r.case}: draws a control decision 28 hides, and no entry names "{cause}" (#{issue})'
                for cause, issue in sorted(drawn - named)
            ]
        problems += [
            f'{r.case}: names "{cause}" (#{issue}), but the case does not draw that control, or its issue is #{issues[cause]}'
            for cause, issue in sorted(named - drawn, key=str)
        ]
    return problems


def check_explanations(explanations):
    """The explanations by case, and what is wrong with their form."""
    by_case, problems = {}, []
    for entry in explanations:
        case = entry.get("case")
        if not isinstance(case, str) or not case:
            problems.append("an explanation has no case")
            continue
        cause, issue = entry.get("cause"), entry.get("issue")
        if any(e.get("cause") == cause for e in by_case.get(case, [])):
            problems.append(f"{case}: the cause is explained twice")
            continue
        by_case.setdefault(case, []).append(entry)
        if not isinstance(cause, str) or not cause.strip():
            problems.append(f"{case}: the explanation has no cause")
        if not isinstance(issue, int) or isinstance(issue, bool) or issue < 1:
            problems.append(f"{case}: the explanation names no issue (a number from 1)")
    return by_case, problems


def report(results, explanations):
    """The report's text and its problems; pure."""
    by_case, problems = check_explanations(explanations)
    problems += check_hidden(results, by_case)
    rows, differing, explained = [], 0, 0
    for r in results:
        if r.differing is None:
            problems.append(f"{r.case}: no capture to compare")
            note, count = "no capture", "-"
        else:
            count = str(r.differing)
            if differs(r):
                differing += 1
                if r.case in by_case:
                    explained += 1
                    note = "; ".join(
                        f"{e.get('cause', '')} (#{e.get('issue', '?')})" for e in by_case[r.case]
                    )
                else:
                    problems.append(
                        f"{r.case}: {r.differing} pixel(s) differ (tolerance {r.allowed}) and nothing explains it"
                    )
                    note = "unexplained"
            else:
                note = ""
                if r.case in by_case:
                    problems.append(
                        f"{r.case}: explained, but {r.differing} pixel(s) differ (tolerance {r.allowed}); remove the entry"
                    )
        if r.closed:
            problems.append(f"{r.case}: the compositor closed the surface (no allow-list line for it)")
        closed = "closed" if r.closed else ""
        rows.append(f"| {r.case} | {count} | {r.allowed} | {closed} | {note.replace('|', '/')} |")
    known = {r.case for r in results}
    problems += [
        f"{case}: explains a case this report does not run" for case in by_case if case not in known
    ]
    lines = [
        "# Golden report: athanor-compositor against cosmic-comp's goldens",
        "",
        f"{len(results)} cases, {differing} differing, {explained} explained, {len(problems)} problem(s).",
        f"Left out: {', '.join(EXCLUDED)}, which runs on cosmic-comp until it moves (doc_session.md SN12).",
        "",
    ]
    if problems:
        lines += ["## Problems", ""] + [f"- {p}" for p in problems] + [""]
    lines += [
        "## Cases",
        "",
        "| case | pixels differing | tolerance | surface closed | explanation |",
        "| --- | ---: | ---: | --- | --- |",
    ] + rows
    return "\n".join(lines) + "\n", problems


def measure(out, wanted, since):
    results = []
    for surface, tag in expected(wanted):
        golden = GOLDEN / surface / f"{tag}.png"
        if not golden.exists():
            sys.exit(f"golden_report.py: no golden image {golden}")
        actual = out / f"{tag}.png"
        allowed = compare.tolerance(golden.parent, tag)
        count = None
        # An image older than the run is a capture of an earlier one.
        if actual.exists() and actual.stat().st_mtime >= since:
            try:
                count = compare.differing_pixels(golden, actual, out / f"{tag}-diff.png")
            except RuntimeError as error:
                print(f"golden_report.py: {tag}: {error}", file=sys.stderr)
        log = out / f"{tag}-compositor.log"
        # A log older than the run is the log of an earlier one.
        closed = (
            log.exists()
            and log.stat().st_mtime >= since
            and CLOSED in log.read_text(encoding="utf-8", errors="replace")
        )
        results.append(Result(surface, tag, count, allowed, closed))
    return results


def main(argv):
    parser = argparse.ArgumentParser(prog="golden_report.py", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("surfaces")
    run = sub.add_parser("report")
    run.add_argument("--out", type=Path, default=Path("/out"))
    run.add_argument("--explanations", type=Path, default=EXPLANATIONS)
    run.add_argument("--since", type=float, default=0.0)
    run.add_argument("--elapsed", type=int)
    run.add_argument("surface", nargs="*")
    args = parser.parse_args(argv)
    if args.command == "surfaces":
        print("\n".join(surfaces()))
        return 0
    wanted = args.surface or surfaces()
    unknown = [s for s in wanted if s not in surfaces()]
    if unknown:
        print(f"golden_report.py: not a surface of the report: {' '.join(unknown)}", file=sys.stderr)
        return 2
    explanations = tomllib.loads(args.explanations.read_text(encoding="utf-8")).get("difference", [])
    results = measure(args.out, wanted, args.since)
    if args.surface:
        # A run of some surfaces judges only the explanations of its own cases.
        ran = {r.case for r in results}
        explanations = [e for e in explanations if e.get("case") in ran]
    text, problems = report(results, explanations)
    if args.elapsed is not None:
        text += f"\nWall time: {args.elapsed // 60} min {args.elapsed % 60} s.\n"
    (args.out / "golden-report.md").write_text(text, encoding="utf-8")
    for problem in problems:
        print(f"FAIL {problem}", file=sys.stderr)
    print(text.split("\n\n")[1])
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
