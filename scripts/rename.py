#!/usr/bin/env python3
"""Rinomina Ermete OS in Athanor e il kernel in Azoth (eseguita il 2026-09-05, commit 02bf9c05).

    rename.py [--root DIR] [--dry-run] [--skip-tools] [--cargo CMD] [--actionlint CMD]

Esegue sul working tree di --root (default: il repo che contiene questo file), che deve
essere pulito. In ordine: sposta i percorsi con `git mv` (dal componente piu' alto che
cambia in giu', cosi' la storia segue le directory), riscrive i contenuti dei file di testo
tracciati con le regole ordinate della sezione 2 della specifica preservando i fine riga,
poi verifica: nessun residuo fuori dalle esclusioni (sezione 3), `scripts/verify.py` non
peggiore di prima, `cargo metadata --locked --offline`, `actionlint` non peggiore di
prima. Non committa.

--dry-run   calcola e stampa spostamenti e sostituzioni senza toccare nulla.
--skip-tools salta cargo e actionlint (prova a secco senza toolchain): lo dice in chiaro.
--cargo / --actionlint: il comando da usare, con le sue virgolette (per esempio
"wsl /home/me/.cargo/bin/cargo", "podman run --rm -v 'C:/repo con spazi:/repo' -w /repo
docker.io/rhysd/actionlint:latest").

I file `*.pem` e `*.der` seguono la loro directory ma non cambiano nome: quel `git mv` e'
del maintainer (sezione 4).
"""

import argparse
import re
import shlex
import subprocess
import sys
from collections import Counter
from pathlib import Path

# Regole ordinate (specifica, sezione 2): la prima che combacia vince. Quelle del kernel
# precedono quelle generiche perche' `ermete-kernel` diventa `azoth`, non `athanor-kernel`.
RULES = [
    (r"ermete-kernel-microvm", "azoth-microvm"),
    (r"ermete-kernel-builder", "azoth-builder"),
    (r"ermete-kernel-boot", "azoth-boot"),
    (r"ermete-kernel", "azoth"),
    (r"ermete-os-kernel", "azoth"),
    (r"ermete-os-nvidia", "azoth-nvidia"),
    (r"buildid \.ermete", "buildid .azoth"),
    (r"\.ermete\.(fc\d+|o\d)\b", r".azoth.\1"),
    (r"ERMETE_KERNEL_CACHE", "AZOTH_CACHE"),
    (r"KBUILD_BUILD_USER=ermete", "KBUILD_BUILD_USER=azoth"),
    (r"ermete@forge", "azoth@forge"),
    (r"hr-mes/ermete-os", "hr-mes/athanor"),
    (r"ermete-os-", "athanor-"),
    (r"ermete-os", "athanor"),
    (r"org\.ermete\.", "org.athanor."),
    (r"Ermete OS", "Athanor OS"),
    (r"ErmeteOS", "AthanorOS"),
    (r"ERMETE", "ATHANOR"),
    (r"Ermete", "Athanor"),
    (r"ermete", "athanor"),
]
COMPILED = [(re.compile(p), r) for p, r in RULES]

# Sezione 3: cosa non si tocca. I percorsi sono relativi alla radice, in forma posix.
EXCLUDED_PREFIXES = ("docs/architecture/graph-vaults/",)
EXCLUDED_FILES = {"scripts/rename.py", "docs/architecture/doc_naming.md"}
EXCLUDED_ROOT_GLOBS = ("fix_*.py", "ab_test*.py")
MANUAL_SUFFIXES = (".pem", ".der")
BINARY_SUFFIXES = (
    ".pem",
    ".der",
    ".asc",
    ".gpg",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".ico",
    ".webp",
    ".woff",
    ".woff2",
    ".ttf",
    ".otf",
    ".gz",
    ".xz",
    ".zst",
    ".zip",
    ".tar",
    ".rpm",
    ".ko",
    ".so",
    ".efi",
    ".bin",
    ".img",
    ".iso",
)


def rewrite(text):
    """(testo riscritto, Counter sostituzioni per regola)."""
    counts = Counter()
    for pattern, repl in COMPILED:
        text, n = pattern.subn(repl, text)
        if n:
            counts[pattern.pattern] += n
    return text, counts


def excluded(path):
    return (
        path in EXCLUDED_FILES
        or path.startswith(EXCLUDED_PREFIXES)
        or ("/" not in path and any(Path(path).match(g) for g in EXCLUDED_ROOT_GLOBS))
    )


def rename_path(path):
    """Nuovo percorso: regole applicate componente per componente; .pem/.der tengono il nome."""
    parts = path.split("/")
    new = [rewrite(c)[0] for c in parts]
    if path.endswith(MANUAL_SUFFIXES):
        new[-1] = parts[-1]
    return "/".join(new)


def plan_moves(paths):
    """[(vecchio, nuovo)] di `git mv`, dal componente piu' alto che cambia in giu'."""
    current = {p: p for p in paths}
    target = {p: rename_path(p) for p in paths}
    collisions = Counter(target.values())
    dup = sorted(t for t, n in collisions.items() if n > 1)
    if dup:
        sys.exit("percorsi di destinazione in conflitto: " + " ".join(dup[:5]))
    moves = []
    while True:
        pending = [
            (cur, target[orig]) for orig, cur in current.items() if cur != target[orig]
        ]
        if not pending:
            return moves
        prefixes = {}
        for cur, tgt in pending:
            cp, tp = cur.split("/"), tgt.split("/")
            i = next(k for k in range(len(cp)) if cp[k] != tp[k])
            prefixes.setdefault("/".join(cp[: i + 1]), "/".join(tp[: i + 1]))
        for old, new in sorted(prefixes.items()):
            moves.append((old, new))
            for orig, cur in current.items():
                if cur == old or cur.startswith(old + "/"):
                    current[orig] = new + cur[len(old) :]


def git(root, *args, check=True):
    return subprocess.run(
        ["git", *args], cwd=root, check=check, capture_output=True, text=True
    ).stdout


def tracked(root):
    return [p for p in git(root, "ls-files", "-z").split("\0") if p]


def actionlint_findings(root, cmd):
    """Numero di segnalazioni di actionlint (la base ne ha di rinviate: conta il delta)."""
    out = subprocess.run(
        [*shlex.split(cmd), "-no-color"], cwd=root, capture_output=True, text=True
    ).stdout
    return len(re.findall(r"^\.github/workflows/", out, re.M))


def verify_report(root):
    """{check: (esito, conteggio)} da scripts/verify.py; vuoto se lo script manca."""
    if not (root / "scripts/verify.py").exists():
        return {}
    out = subprocess.run(
        [sys.executable, "scripts/verify.py"], cwd=root, capture_output=True, text=True
    ).stdout
    return {
        m.group(2): (m.group(1), int(m.group(3)))
        for m in re.finditer(r"^\s*(PASS|FAIL)\s+(\w+)\s+(\d+)", out, re.M)
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--skip-tools", action="store_true")
    ap.add_argument("--cargo", default="cargo")
    ap.add_argument("--actionlint", default="actionlint")
    args = ap.parse_args()
    root = args.root.resolve()

    # I file non tracciati non contano: lo script tocca solo i file tracciati.
    if git(root, "status", "--porcelain", "--untracked-files=no").strip():
        sys.exit("working tree non pulito: commit o stash prima della rinomina")
    before = verify_report(root)
    lint_before = (
        None if args.skip_tools or args.dry_run else actionlint_findings(root, args.actionlint)
    )

    paths = tracked(root)
    movable = [p for p in paths if not excluded(p)]
    moves = plan_moves(movable)
    manual = sorted(
        (rename_path(p), Path(p).name, rewrite(Path(p).name)[0])
        for p in movable
        if p.endswith(MANUAL_SUFFIXES) and rewrite(Path(p).name)[0] != Path(p).name
    )
    print(f"percorsi da spostare: {len(moves)} `git mv`")
    for old, new in moves:
        print(f"  {old} -> {new}")
    if not args.dry_run:
        for old, new in moves:
            Path(root, new).parent.mkdir(parents=True, exist_ok=True)
            git(root, "mv", old, new)

    # Contenuti: i file tracciati, ai percorsi nuovi.
    rewritten, skipped, totals = [], [], Counter()
    for old in paths:
        if excluded(old):
            continue
        new = rename_path(old) if not args.dry_run else old
        if new.endswith(BINARY_SUFFIXES):
            continue
        path = root / (rename_path(old) if not args.dry_run else old)
        raw = path.read_bytes()
        if b"\0" in raw[:8192]:
            continue
        # surrogateescape: i byte non UTF-8 (due documenti) restano come sono.
        text = raw.decode("utf-8", "surrogateescape")
        try:
            raw.decode("utf-8")
        except UnicodeDecodeError:
            skipped.append(old)
        new_text, counts = rewrite(text)
        if counts:
            totals.update(counts)
            rewritten.append(rename_path(old))
            if not args.dry_run:
                path.write_bytes(new_text.encode("utf-8", "surrogateescape"))
    print(f"\nfile riscritti: {len(rewritten)}; sostituzioni per regola:")
    for pattern, _ in RULES:
        if totals[pattern]:
            print(f"  {totals[pattern]:6d}  {pattern}")
    if skipped:
        print("\nfile non UTF-8 (riscritti conservando i byte non validi):")
        for p in skipped:
            print(f"  {p}")
    if manual:
        print("\nda rinominare a mano (sezione 4, passo 3):")
        for directory_path, old_name, new_name in manual:
            print(
                f"  git mv {Path(directory_path).parent.as_posix()}/{old_name} .../{new_name}"
            )
    if args.dry_run:
        print("\nprova a secco: nessuna modifica scritta")
        return

    # Verifica 1: nessun residuo fuori dalle esclusioni.
    residual = [
        p
        for p in git(root, "grep", "-il", "ermete", "--", ".", check=False).splitlines()
        if not excluded(p) and not p.endswith(MANUAL_SUFFIXES)
    ]
    if residual:
        print("\nRESIDUI:")
        print(git(root, "grep", "-in", "ermete", "--", *residual, check=False)[:4000])
        sys.exit(f"{len(residual)} file citano ancora ermete")
    print("\nresidui: nessuno fuori dalle esclusioni")

    # Verifica 2: verify.py non peggiore di prima.
    after = verify_report(root)
    worse = {
        k: (before.get(k), v)
        for k, v in after.items()
        if before.get(k, ("FAIL", 0))[0] == "PASS"
        and v[0] == "FAIL"
        or v[1] > before.get(k, ("FAIL", 0))[1]
    }
    if worse:
        sys.exit(f"verify.py peggiorato: {worse}")
    print(
        f"verify.py: {', '.join(f'{k} {v[0]} {v[1]}' for k, v in after.items())} (come prima)"
    )

    # Verifica 3: toolchain.
    if args.skip_tools:
        print("ATTENZIONE: cargo metadata e actionlint saltati (--skip-tools)")
        return
    subprocess.run(
        [
            *shlex.split(args.cargo),
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
        ],
        cwd=root,
        check=True,
        capture_output=True,
    )
    print("cargo metadata --locked --offline: ok")
    lint_after = actionlint_findings(root, args.actionlint)
    if lint_after > lint_before:
        sys.exit(f"actionlint peggiorato: {lint_before} -> {lint_after} segnalazioni")
    print(f"actionlint: {lint_after} segnalazioni (prima {lint_before})")


if __name__ == "__main__":
    main()
