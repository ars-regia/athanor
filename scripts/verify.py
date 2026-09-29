#!/usr/bin/env python3
"""
Controlli strutturali di Athanor OS.

Ogni controllo qui corrisponde a un difetto trovato in ANALISI_2026-09-02.md.
Non sono test di stile: sono le sei domande che nessuno stava ponendo, e che
insieme avrebbero intercettato ogni difetto critico e alto di quell'audit.

Uso:
    python3 scripts/verify.py              # tutti i controlli
    python3 scripts/verify.py polkit       # uno solo
    python3 scripts/verify.py --list

Exit code: numero di controlli falliti (0 = tutto a posto).
Nessuna dipendenza oltre a python3 e git. Va eseguito dalla radice del repo.
"""

import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RS_DIRS = ["system", "forge/specs"]

# --------------------------------------------------------------------------- #
# infrastruttura minima
# --------------------------------------------------------------------------- #

CHECKS = {}
BOLD, DIM, RED, GRN, YEL, OFF = "\033[1m", "\033[2m", "\033[31m", "\033[32m", "\033[33m", "\033[0m"
if not sys.stdout.isatty() or os.environ.get("NO_COLOR"):
    BOLD = DIM = RED = GRN = YEL = OFF = ""


def check(name, title):
    def deco(fn):
        fn.title = title
        CHECKS[name] = fn
        return fn
    return deco


class Result:
    def __init__(self):
        self.problems = []
        self.notes = []

    def fail(self, msg):
        self.problems.append(msg)

    def note(self, msg):
        self.notes.append(msg)

    @property
    def ok(self):
        return not self.problems


PRUNE = {"target", ".git", "repo-cache", ".cache", "node_modules",
         "graph-vaults", "graph-pages", ".codegraph", "graphify-out",
         "experimental"}


def walk(base, suffix):
    """os.walk con potatura: rglob su questo repo entra in target/ (decine di
    migliaia di file) e su un filesystem montato ci mette minuti."""
    base = Path(base)
    if not base.is_dir():
        return
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [d for d in dirnames if d not in PRUNE]
        for f in filenames:
            if f.endswith(suffix):
                yield Path(dirpath) / f


def rust_files():
    for d in RS_DIRS:
        yield from walk(ROOT / d, ".rs")


def read(p):
    return p.read_text(encoding="utf-8", errors="replace")


def rel(p):
    try:
        return str(Path(p).resolve().relative_to(ROOT))
    except ValueError:
        return str(p)


# --------------------------------------------------------------------------- #
# 1. workflow — il difetto che ha spento il CI il 2026-08-31
# --------------------------------------------------------------------------- #

@check("workflows", "I workflow GitHub sono validi e non esfiltrano log")
def check_workflows():
    r = Result()
    wf_dir = ROOT / ".github" / "workflows"
    if not wf_dir.is_dir():
        r.fail("nessuna directory .github/workflows")
        return r

    for wf in sorted(wf_dir.glob("*.yml")) + sorted(wf_dir.glob("*.yaml")):
        lines = read(wf).split("\n")

        # 1a. step con solo `name:` -> GitHub rifiuta l'INTERO file
        for i, line in enumerate(lines):
            m = re.match(r"^(\s+)- name:\s*(\S.*)$", line)
            if not m:
                continue
            indent = len(m.group(1))
            body = []
            for nxt in lines[i + 1:]:
                if not nxt.strip():
                    continue
                if len(nxt) - len(nxt.lstrip()) <= indent:
                    break
                body.append(nxt.strip())
            if not any(b.startswith(("run:", "uses:")) for b in body):
                r.fail(f"{wf.name}:{i+1} step senza run/uses -> GitHub rifiuta tutto il file "
                       f"({m.group(2)[:45]})")

        # 1b. blocchi if/fi vuoti -> errore di sintassi bash, uccide lo step
        for i, line in enumerate(lines):
            if re.search(r";\s*then\s*$", line):
                nxt = lines[i + 1].strip() if i + 1 < len(lines) else ""
                if nxt == "fi":
                    r.fail(f"{wf.name}:{i+1} blocco if vuoto -> errore di sintassi bash (exit 2)")

        # 1c. log spediti fuori dal perimetro
        for i, line in enumerate(lines):
            for host in ("webhook.site", "dpaste.com", "pastebin.com", "transfer.sh", "0x0.st"):
                if host in line:
                    r.fail(f"{wf.name}:{i+1} log inviati a un servizio esterno ({host})")

    # 1d. actionlint, se disponibile
    try:
        p = subprocess.run(["actionlint", "-no-color", "-oneline"], cwd=ROOT,
                           capture_output=True, text=True, timeout=120)
        if p.returncode != 0:
            for line in (p.stdout or p.stderr).strip().split("\n")[:15]:
                if line.strip():
                    r.fail(f"actionlint: {line.strip()}")
    except FileNotFoundError:
        r.note("actionlint non installato — installalo, intercetta molto più di questo controllo")
    except Exception as e:
        r.note(f"actionlint non eseguito: {e}")

    return r


# --------------------------------------------------------------------------- #
# 2. kickstart — una direttiva che Anaconda non riconosce ferma l'installazione
# --------------------------------------------------------------------------- #

# La sintassi che l'Anaconda spedito sull'ISO sa leggere. L'ISO è Fedora 43: alzalo
# insieme alla base, non oltre, perché una direttiva introdotta dopo passerebbe qui
# e fallirebbe sul supporto reale.
KICKSTART_SYNTAX = "F43"


@check("kickstart", f"I kickstart sono sintassi valida per l'Anaconda di {KICKSTART_SYNTAX}")
def check_kickstart():
    r = Result()

    for ks in sorted(walk(ROOT, ".ks")):
        # Senza --followincludes un %include viene accettato senza essere letto: è
        # quanto serve, perché collaudo.ks ne ha uno che esiste solo sul supporto di
        # installazione (/run/install/repo) e non nel checkout.
        try:
            p = subprocess.run(["ksvalidator", "--version", KICKSTART_SYNTAX, str(ks)],
                               cwd=ROOT, capture_output=True, text=True, timeout=60)
        except FileNotFoundError:
            r.note("ksvalidator non installato — `dnf install pykickstart`, "
                   "senza di lui nessuno controlla i kickstart prima dell'installazione")
            return r
        except Exception as e:
            r.note(f"ksvalidator non eseguito: {e}")
            return r

        if p.returncode != 0:
            detail = " ".join(p.stderr.split()) or " ".join(p.stdout.split())
            r.fail(f"{rel(ks)}: {detail}")

    return r


# --------------------------------------------------------------------------- #
# 3. polkit — ogni azione applicata dal codice deve essere dichiarata
# --------------------------------------------------------------------------- #

@check("polkit", "Ogni action-id polkit applicata dal codice è dichiarata in un .policy")
def check_polkit():
    r = Result()

    used = {}   # action -> {file:riga}
    for p in rust_files():
        for i, line in enumerate(read(p).split("\n"), 1):
            for a in re.findall(r"os\.athanor\.[a-z0-9_]+\.[a-z0-9_]+", line):
                used.setdefault(a, set()).add(f"{rel(p)}:{i}")

    declared = {}   # action -> file .policy
    for pol in walk(ROOT, ".policy"):
        for a in re.findall(r'id="([^"]+)"', read(pol)):
            declared.setdefault(a, set()).add(rel(pol))

    # un .policy che non viene installato dallo spec non esiste, sul sistema reale
    installed = set()
    for spec in walk(ROOT, ".spec"):
        txt = read(spec)
        for a in re.findall(r"([A-Za-z0-9_.]+\.policy)", txt):
            if "polkit-1/actions" in txt:
                installed.add(a)

    for action in sorted(used):
        if action not in declared:
            where = sorted(used[action])[0]
            r.fail(f"{action}: applicata in {where} ma NON dichiarata in alcun .policy "
                   f"-> CheckAuthorization nega sempre")
        else:
            for f in declared[action]:
                if Path(f).name not in installed:
                    r.fail(f"{action}: dichiarata in {f} ma quel file non è installato "
                           f"in polkit-1/actions da nessuno spec")

    for action in sorted(declared):
        if action not in used:
            r.note(f"{action}: dichiarata in {', '.join(sorted(declared[action]))} "
                   f"ma nessun codice la controlla (policy morta)")

    return r


# --------------------------------------------------------------------------- #
# 4. percorsi runtime — niente artefatti letti da target/ o stato in /tmp
# --------------------------------------------------------------------------- #

# Alberi congelati: codice morto che si mina e si cancella, non si sviluppa. La copia
# congelata di athanor-style è ferma a GTK 0.7 in un workspace suo (doc_shell.md, SH4) e il
# suo Cargo.toml dice "do not develop here", quindi un rilievo là dentro non ha niente da
# dire — e sistemarlo contraddirebbe il congelamento. Il binario della vecchia shell,
# accanto ad essa, resta invece spedito e quindi resta scansionato. L'esclusione sparisce
# insieme all'albero.
FROZEN_TREES = ("forge/specs/athanor-shell-rs/athanor-style-0.7/",)


def is_frozen(relative_path):
    """True se il file sta in un albero congelato: si mina e si cancella, non si sviluppa."""
    return any(relative_path.startswith(tree) for tree in FROZEN_TREES)


def path_problems(relative_path, text):
    """I rilievi di percorso di un file, già formattati con riga e motivo."""
    if is_frozen(relative_path):
        return []
    # un build script gira a build time: può legittimamente parlare di target/
    is_build_script = Path(relative_path).name == "build.rs"
    problems = []
    for i, line in enumerate(text.split("\n"), 1):
        code = line.split("//")[0]
        if not is_build_script and re.search(r'"[^"]*\btarget/[a-z0-9_./-]*"', code):
            problems.append(f"{relative_path}:{i} carica un artefatto da target/ — percorso "
                            f"dell'albero di build, inesistente su un sistema installato")
        if re.search(r'"/tmp/', code):
            problems.append(f"{relative_path}:{i} percorso hard-coded in /tmp — usa "
                            f"/run/athanor (0700) per stato privilegiato")
    return problems


@check("paths", "Nessun artefatto runtime da target/, nessuno stato privilegiato in /tmp")
def check_paths():
    r = Result()
    for p in rust_files():
        for problem in path_problems(rel(p), read(p)):
            r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 5. packaging — un crate che compila e basta non è nel prodotto
# --------------------------------------------------------------------------- #

def has_binary_target(crate_dir):
    """True se il crate produce un eseguibile: src/main.rs, src/bin/ o una sezione [[bin]]."""
    cargo = crate_dir / "Cargo.toml"
    if not cargo.exists():
        return True  # non giudicabile: resta soggetto al controllo
    return ((crate_dir / "src" / "main.rs").exists()
            or (crate_dir / "src" / "bin").is_dir()
            or re.search(r"^\s*\[\[\s*bin\s*\]\]", read(cargo), re.M) is not None)


COSMIC_OVERLAY = "/usr/share/athanor/cosmic-defaults"


def cosmic_defaults_problems(root):
    """How Calmo reaches COSMIC (doc_shell.md, SH5): a data directory of our own, first
    in XDG_DATA_DIRS. cosmic-config resolves system defaults through that variable, so
    the defaults are shipped only if the files exist, the package installs them, and the
    variable is set both for the user manager and for the compositor."""
    root = Path(root)
    problems = []
    generated = root / "system/athanor-style/calmo/generated/cosmic/cosmic"
    if not generated.is_dir() or not any(p.is_file() for p in generated.rglob("*")):
        problems.append("no generated COSMIC defaults: run forge/tools/calmo-cosmic-theme/derive.sh")

    env_file = root / "forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf"
    env_text = read(env_file) if env_file.exists() else ""
    if not re.search(rf"^XDG_DATA_DIRS={re.escape(COSMIC_OVERLAY)}:", env_text, re.M):
        problems.append(f"environment.d: {COSMIC_OVERLAY} is not first in XDG_DATA_DIRS for the user manager")

    session = root / "forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session"
    session_text = read(session) if session.exists() else ""
    if not re.search(rf'^export XDG_DATA_DIRS="?{re.escape(COSMIC_OVERLAY)}:', session_text, re.M):
        problems.append(f"athanor-session does not export XDG_DATA_DIRS with {COSMIC_OVERLAY} first: "
                        f"cosmic-comp and its children are not started by the user manager")

    spec = root / "forge/specs/athanor-calmo/athanor-calmo.spec"
    files = read(spec).split("%files", 1)[-1] if spec.exists() else ""
    for shipped in (COSMIC_OVERLAY, "/usr/lib/environment.d/60-athanor-cosmic-defaults.conf"):
        if not re.search(rf"^{re.escape(shipped)}$", files, re.M):
            problems.append(f"athanor-calmo.spec: %files does not list {shipped}")
    return problems


@check("shipped", "Ogni crate del workspace è impacchettato, o è dichiarato sperimentale")
def check_shipped():
    r = Result()
    cargo = ROOT / "Cargo.toml"
    if not cargo.exists():
        r.fail("nessun Cargo.toml in radice")
        return r

    members = re.findall(r'^\s*"([^"]+)",?\s*$',
                         re.search(r"members\s*=\s*\[(.*?)\]", read(cargo), re.S).group(1),
                         re.M)

    specs_dir = ROOT / "forge" / "specs"
    spec_dirs = {d.name for d in specs_dir.iterdir() if d.is_dir()} if specs_dir.is_dir() else set()

    pkgs_file = ROOT / "forge" / "config" / "packages.json"
    dag = set()
    tiers = set()
    if pkgs_file.exists():
        d = json.loads(read(pkgs_file))
        dag = set(d.get("custom_packages", []))
        for k in d:
            if k.startswith("custom_tier"):
                tiers |= set(d[k])

    exempt_file = ROOT / "experimental" / "EXEMPT"
    exempt = set()
    if exempt_file.exists():
        exempt = {l.strip() for l in read(exempt_file).split("\n")
                  if l.strip() and not l.startswith("#")}

    for m in members:
        name = Path(m).name
        name = re.sub(r"-\d+\.\d+\.\d+$", "", name)
        if name in exempt:
            continue
        if not has_binary_target(ROOT / m):
            # Un crate libreria finisce dentro i binari che lo usano: niente da spedire.
            continue
        short = name.replace("athanor-", "")
        has_spec = name in spec_dirs or f"athanor-{short}" in spec_dirs
        in_dag = name in dag or short in dag
        if not (has_spec and in_dag):
            why = []
            if not has_spec:
                why.append("nessuno .spec")
            if not in_dag:
                why.append("non in packages.json")
            r.fail(f"{name}: {', '.join(why)} -> compila ma non arriva sul sistema. "
                   f"Impacchettalo, spostalo in experimental/, o elencalo in experimental/EXEMPT")

    for p in sorted(dag - tiers):
        r.fail(f"{p}: in custom_packages ma in nessun tier -> costruito e mai installato")
    for p in sorted(tiers - dag):
        r.fail(f"{p}: in un tier ma non in custom_packages -> riferimento pendente")

    for problem in cosmic_defaults_problems(ROOT):
        r.fail(problem)

    return r


# --------------------------------------------------------------------------- #
# 6. documentazione — i link devono risolvere e non essere assoluti
# --------------------------------------------------------------------------- #

@check("docs", "I link nella documentazione risolvono e sono portabili")
def check_docs():
    r = Result()
    targets = ["README.md", "system/README.md", "system/ARCHITECTURE.md",
               "ANALISI_2026-09-02.md", "PIANO_RIPARTENZA.md", "CLAUDE.md", "ROADMAP.md"]
    targets += [rel(p) for p in walk(ROOT / "docs", ".md")]

    for t in targets:
        f = ROOT / t
        if not f.exists():
            continue
        base = f.parent
        for m in re.finditer(r"\[([^\]]*)\]\(([^)]+)\)", read(f)):
            link = m.group(2).split("#")[0].strip()
            if not link or link.startswith(("http://", "https://", "mailto:")):
                continue
            if link.startswith("file://"):
                r.fail(f"{t}: link assoluto della macchina di sviluppo -> {link[:70]}")
                continue
            if not (base / link).exists():
                r.fail(f"{t}: link rotto [{m.group(1)[:30]}] -> {link}")
    return r


# --------------------------------------------------------------------------- #
# 7. panic — il budget attuale è 1 unwrap in 60k righe. Difendilo.
# --------------------------------------------------------------------------- #

# Valori misurati sul repo il 2026-09-02. Sono un cricchetto: si abbassano,
# non si alzano. Se un controllo fallisce qui, propaga con `?`.
BUDGET = {".unwrap()": 0, ".expect(": 2, "panic!(": 2}


@check("panics", "Il budget di panic in codice non di test non cresce")
def check_panics():
    r = Result()
    counts = {k: 0 for k in BUDGET}
    where = {k: [] for k in BUDGET}

    for p in rust_files():
        txt = read(p)
        cut = txt.find("#[cfg(test)]")
        if cut > 0:
            txt = txt[:cut]
        for i, line in enumerate(txt.split("\n"), 1):
            code = line.split("//")[0]
            for k in BUDGET:
                n = code.count(k)
                if n:
                    counts[k] += n
                    where[k].append(f"{rel(p)}:{i}")

    for k, budget in BUDGET.items():
        if counts[k] > budget:
            extra = ", ".join(where[k][:6])
            r.fail(f"{k}: {counts[k]} occorrenze, budget {budget}. Propaga con `?`. "
                   f"Prime: {extra}")
        elif counts[k] < budget:
            r.note(f"{k}: {counts[k]} (budget {budget}) — abbassa il budget in scripts/verify.py")

    return r


# --------------------------------------------------------------------------- #
# 8. polkit, il lato codice — il subject deve essere il CHIAMANTE
# --------------------------------------------------------------------------- #

@check("polkit-subject", "Il subject polkit identifica il chiamante, e i default sono raggiungibili")
def check_polkit_subject():
    r = Result()

    for p in rust_files():
        txt = read(p)
        if "check_authorization" not in txt and "PolicyKitAuthority" not in txt:
            continue
        for i, line in enumerate(txt.split("\n"), 1):
            code = line.split("//")[0]
            if "peer_creds" in code or "peer_credentials" in code:
                r.fail(f"{rel(p)}:{i} peer_creds() in un percorso polkit: restituisce le "
                       f"credenziali del bus all'altro capo del socket, MAI quelle del "
                       f"chiamante. Usa PolkitSubject::system_bus_name(sender)")
            if re.search(r"unix_user_id\(\)\s*==\s*Some\(0\)", code):
                r.fail(f"{rel(p)}:{i} short-circuit su uid 0 derivato dal socket peer: "
                       f"se il bus gira come root questo autorizza chiunque")

    # un default auth_* è irraggiungibile se il sito di chiamata vieta l'interazione
    declared = {}
    for pol in walk(ROOT, ".policy"):
        txt = read(pol)
        for m in re.finditer(r'<action id="([^"]+)">(.*?)</action>', txt, re.S):
            act = re.search(r"<allow_active>(\w+)</allow_active>", m.group(2))
            if act:
                declared[m.group(1)] = act.group(1)

    for p in rust_files():
        for i, line in enumerate(read(p).split("\n"), 1):
            m = re.search(r'"(os\.athanor\.[a-z0-9_.]+)"\s*,\s*(true|false)\s*\)', line)
            if not m:
                continue
            action, interactive = m.group(1), m.group(2) == "true"
            default = declared.get(action)
            if default and default.startswith("auth_") and not interactive:
                r.fail(f"{rel(p)}:{i} {action} è dichiarata {default} ma qui "
                       f"allow_user_interaction=false: polkit non può mostrare il dialogo, "
                       f"quindi l'autorizzazione fallisce sempre. Passa true, o abbassa il "
                       f"default sapendo cosa comporta")

    return r


# --------------------------------------------------------------------------- #
# 9. spec RPM — la sorgente di `install` deve esistere da dove gira %install
# --------------------------------------------------------------------------- #

@check("specs", "Le spec installano da percorsi che esistono dalla loro working directory")
def check_specs():
    r = Result()
    import shlex

    def sections(text):
        """Divide una spec nelle sue sezioni %prep / %build / %install / ..."""
        out, cur = {}, None
        for line in text.split("\n"):
            m = re.match(r"^%(\w+)\b", line)
            if m and m.group(1) in ("prep", "build", "install", "files", "changelog",
                                    "post", "preun", "postun", "description", "package"):
                cur = m.group(1)
                out.setdefault(cur, [])
                continue
            if cur:
                out[cur].append(line)
        return {k: "\n".join(v) for k, v in out.items()}

    def install_source(line):
        """La sorgente di un `install SRC DEST`, o None se la riga non lo è."""
        try:
            toks = shlex.split(line)
        except ValueError:
            return None
        if not toks or toks[0] != "install":
            return None
        pos, skip = [], False
        for tok in toks[1:]:
            if skip:
                skip = False
                continue
            if tok in ("-m", "-o", "-g", "-t", "--mode", "--owner", "--group"):
                skip = True
                continue
            if tok.startswith("-"):
                continue
            pos.append(tok)
        if len(pos) < 2 or "buildroot" not in pos[-1]:
            return None
        return pos[-2]

    for spec in walk(ROOT, ".spec"):
        text = read(spec)
        sec = sections(text)
        prep = sec.get("prep", "")
        # senza %setup/%autosetup rpmbuild non entra in una sottodirectory:
        # %install parte da %{_builddir}, che per questo forge è la radice del repo
        stub_prep = not re.search(r"^\s*%(auto)?setup\b", prep, re.M)

        for i, line in enumerate(text.split("\n"), 1):
            stripped = line.strip()

            # residuo del generatore: crea un file VUOTO che install poi spedisce
            m = re.match(r"^mkdir -p \$\(dirname (\S+)\) && touch \1$", stripped)
            if m:
                r.fail(f"{rel(spec)}:{i} `touch {m.group(1)}` crea un file vuoto nella "
                       f"working directory: se poi viene installato, spedisci 0 byte")
                continue

            if not stub_prep:
                continue
            src = install_source(stripped)
            if src is None:
                continue
            if src.startswith(("%", "$", "/", "~")) or "/" in src:
                continue
            r.fail(f"{rel(spec)}:{i} install da nome nudo `{src}`: %prep non fa "
                   f"%setup, quindi %install gira dalla radice del build e quel file "
                   f"non è lì. Usa il percorso completo dalla radice del repo")

    return r


# --------------------------------------------------------------------------- #
# 10. boundary — COSMIC stays behind the compositor client (doc_shell.md, SH2)
# --------------------------------------------------------------------------- #

# The only places that may depend on COSMIC. The translator and the two layout modules
# leave the list when our bar replaces COSMIC's panel (doc_shell.md, SH2).
COSMIC_ALLOWED = (
    "system/athanor-compositor-client/",
    "forge/tools/calmo-cosmic-theme/",
    "forge/specs/athanor-layout-translator/",
    "system/athanor-layout/src/cosmic.rs",
    "system/athanor-layout/src/apply.rs",
)
BOUNDARY_DIRS = ("system", "forge/specs", "forge/tools")
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "dev_dependencies",
                     "build-dependencies", "build_dependencies")


def cosmic_dependencies(manifest):
    """The crate names of a parsed Cargo manifest's dependencies that are libcosmic or
    cosmic-*. A renamed dependency (`package = "cosmic-..."`) is found by its crate name."""
    tables = [manifest.get(name, {}) for name in DEPENDENCY_TABLES]
    tables += [target.get(name, {}) for target in manifest.get("target", {}).values()
               for name in DEPENDENCY_TABLES]
    tables.append(manifest.get("workspace", {}).get("dependencies", {}))
    found = []
    for table in tables:
        for key, value in table.items():
            crate = value.get("package", key) if isinstance(value, dict) else key
            if crate == "libcosmic" or crate.startswith("cosmic-"):
                found.append(crate)
    return found


def boundary_problems(root):
    """What crosses the COSMIC boundary under root: a crate that depends on COSMIC, or Rust
    code that names a com.system76 configuration, outside COSMIC_ALLOWED."""
    root = Path(root)
    problems = []
    for base in BOUNDARY_DIRS:
        for path in walk(root / base, ".toml"):
            relative = path.relative_to(root).as_posix()
            if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):
                continue
            try:
                manifest = tomllib.loads(read(path))
            except tomllib.TOMLDecodeError as error:
                problems.append(f"{relative}: not valid TOML ({error})")
                continue
            for crate in cosmic_dependencies(manifest):
                problems.append(f"{relative} depends on {crate}: only athanor-compositor-client "
                                f"may depend on COSMIC")
        for path in walk(root / base, ".rs"):
            relative = path.relative_to(root).as_posix()
            if relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):
                continue
            for i, line in enumerate(read(path).split("\n"), 1):
                if "com.system76" in line.split("//")[0]:
                    problems.append(f"{relative}:{i} names a com.system76 configuration: read it "
                                    f"through athanor-compositor-client")
    return problems


@check("boundary", "Only the compositor client depends on COSMIC or reads its configuration")
def check_boundary():
    r = Result()
    for problem in boundary_problems(ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# runner
# --------------------------------------------------------------------------- #

def main(argv):
    if "--list" in argv:
        for n, f in CHECKS.items():
            print(f"  {n:12s} {f.title}")
        return 0

    wanted = [a for a in argv if not a.startswith("-")] or list(CHECKS)
    unknown = [w for w in wanted if w not in CHECKS]
    if unknown:
        print(f"controllo sconosciuto: {', '.join(unknown)}", file=sys.stderr)
        print(f"disponibili: {', '.join(CHECKS)}", file=sys.stderr)
        return 2

    failed = 0
    problems = 0
    print(f"{BOLD}Athanor OS — controlli strutturali{OFF}  {DIM}({ROOT}){OFF}\n")

    for name in wanted:
        fn = CHECKS[name]
        res = fn()
        if res.ok:
            print(f"  {GRN}PASS{OFF}  {BOLD}{name}{OFF}  {GRN}0{OFF}  {DIM}{fn.title}{OFF}")
        else:
            failed += 1
            n = len(res.problems)
            problems += n
            print(f"  {RED}FAIL{OFF}  {BOLD}{name}{OFF}  {RED}{n}{OFF}  {DIM}{fn.title}{OFF}")
            for pr in res.problems[:25]:
                print(f"          {pr}")
            if len(res.problems) > 25:
                print(f"          {DIM}… e altri {len(res.problems) - 25}{OFF}")
        for n in res.notes[:8]:
            print(f"          {YEL}nota{OFF} {DIM}{n}{OFF}")
        print()

    total = len(wanted)
    if failed:
        print(f"{BOLD}Problemi totali: {problems}{OFF}")
        print(f"{RED}{failed}/{total} controlli falliti.{OFF} "
              f"Ogni riga sopra cita file:riga: nessuna va interpretata.")
    else:
        print(f"{GRN}{total}/{total} controlli superati.{OFF}")
    return failed


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
