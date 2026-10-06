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


# Units that sealed LUKS to the TPM or bumped its rollback counter on their own (D42, issue #148).
AUTO_TPM_UNITS = ("athanor-tpm-luks-seal", "athanor-tpm-rollback-check", "athanor-tpm-rollback-update")


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

    # 1e. D43: signing keys only in sign-only jobs
    for problem in signing_problems(ROOT):
        r.fail(problem)

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


# The actions a sign-only job may use, pinned by a full commit SHA (D43).
SIGN_ONLY_ACTIONS = re.compile(
    r"^actions/(?:checkout|download-artifact|upload-artifact)@[0-9a-f]{40}$")
# What builds, installs or runs a locally built image. A sign-only job receives built
# artefacts and runs no build step: a compromised build must not be able to sign (D43).
SIGN_ONLY_FORBIDDEN = re.compile(
    r"\b(?:podman|docker)\s+build\b|\bbuildah\s+(?:bud|build|from|commit)\b|\brpmbuild\b"
    r"|\bcargo\s+(?:build|install|run)\b|\bnix\s+(?:build|run|shell|develop|profile)\b"
    r"|\bapt(?:-get)?\s+install\b|\bdnf5?\s+install\b|\bpip3?\s+install\b"
    r"|\bbuild-image\.sh\b|\bbuild_iso\.sh\b|\bnvidia\.sh\s+build\b|\blocalhost/")
# The only commands a step holding a signing secret may run, whole: the sign scripts (D43).
SIGN_SCRIPTS = re.compile(
    r"^run: bash (?:forge/specs/azoth/signer/run\.sh sign"
    r'|system/sign-images\.sh [\w./-]+ \| tee -a "\$\{GITHUB_STEP_SUMMARY\}")$')


def workflow_jobs(text):
    """The jobs of a workflow as (id, first line number, lines), from the two-space layout
    every workflow of this repository uses: verify.py reads YAML without a YAML parser."""
    lines = text.split("\n")
    jobs, current, inside = [], None, False
    for number, line in enumerate(lines, 1):
        if re.match(r"^jobs:\s*$", line):
            inside = True
            continue
        if inside and re.match(r"^\S", line):
            inside = False
        if not inside:
            continue
        m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if m:
            current = (m.group(1), number, [])
            jobs.append(current)
        elif current is not None:
            current[2].append((number, line))
    return jobs


def job_environment(lines):
    """The environment name a job declares, or None."""
    for i, (_, line) in enumerate(lines):
        m = re.match(r"^    environment:\s*(\S*)\s*$", line)
        if not m:
            continue
        if m.group(1):
            return m.group(1).strip("'\"")
        for _, nxt in lines[i + 1:]:
            n = re.match(r"^      name:\s*(\S+)", nxt)
            if n:
                return n.group(1).strip("'\"")
            if re.match(r"^    \S", nxt):
                break
    return None


def signing_problems(root):
    """D43 (doc_kernel_profile.md, section 12 item 1): a secret of the `signing` environment is
    read only by jobs of that environment; such a job uses no action but checkout,
    download-artifact and upload-artifact pinned by SHA, calls no reusable workflow, and builds,
    installs or runs no locally built image; the environment has a required reviewer."""
    root = Path(root)
    settings = root / ".github/settings/environments.json"
    environments = json.loads(read(settings))
    signing = environments.get("signing")
    if signing is None:
        return [f"{settings.relative_to(root).as_posix()}: no signing environment (D43)"]
    problems = []
    if not signing.get("reviewers"):
        problems.append(f"{settings.relative_to(root).as_posix()}: "
                        "the signing environment has no required reviewer (D43)")
    secrets = re.compile(r"\bsecrets\.(" + "|".join(map(re.escape, signing.get("secrets", []))) + r")\b")
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        for job, _, lines in workflow_jobs(read(wf)):
            code = [(n, line) for n, line in lines if not line.lstrip().startswith("#")]
            environment = job_environment(code)
            if signing.get("secrets"):
                read_secrets = sorted({m for _, line in code for m in secrets.findall(line)})
                for secret in read_secrets if environment != "signing" else ():
                    problems.append(f"{wf.name}: job {job} reads {secret} without environment: signing (D43)")
            if environment != "signing":
                continue
            for _, line in code:
                m = re.match(r"^\s*(?:- )?uses:\s*(\S+)", line)
                if m and not SIGN_ONLY_ACTIONS.match(m.group(1)):
                    problems.append(f"{wf.name}: signing job {job} uses {m.group(1)}: only checkout, "
                                    "download-artifact and upload-artifact pinned by SHA (D43)")
            for n, line in code:
                m = SIGN_ONLY_FORBIDDEN.search(line)
                if m:
                    problems.append(f"{wf.name}:{n}: signing job {job} builds or installs: {m.group(0)} (D43)")
            if signing.get("secrets"):
                problems += secret_steps(wf.name, job, code, secrets)
    return problems


def secret_steps(name, job, code, secrets):
    """A signing secret reaches one step, and that step runs one sign script and nothing else."""
    problems, steps, current = [], [], None
    for n, line in code:
        if re.match(r"^      - ", line):
            current = []
            steps.append(current)
        if current is None:
            if secrets.search(line):
                problems.append(f"{name}:{n}: signing job {job} exposes a signing secret to every step (D43)")
        else:
            current.append((n, line))
    for step in steps:
        if not any(secrets.search(line) for _, line in step):
            continue
        runs = [(n, line.strip()) for n, line in step if re.match(r"^\s+(?:- )?run:", line)]
        if len(runs) != 1 or not SIGN_SCRIPTS.match(runs[0][1]):
            problems.append(f"{name}:{step[0][0]}: signing job {job} hands a signing secret to a step that "
                            "does not run only a sign script (D43)")
    return problems


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


def crates_built_by_specs(root=None):
    """crate -> spec directory, for every `-p <crate>` a spec under forge/specs builds.
    A package may build more than one crate (one crate per program, doc_shell.md SH4)."""
    built = {}
    for spec in walk((root or ROOT) / "forge" / "specs", ".spec"):
        text = read(spec)
        name = re.search(r"^Name:\s*(\S+)", text, re.M)
        if not name:
            continue
        for line in text.split("\n"):
            if "cargo build" not in line:
                continue
            for crate in re.findall(r"-p\s+(\S+)", line):
                built[crate.replace("%{name}", name.group(1))] = spec.parent.name
    return built


UPDATE_SOURCES = "forge/specs/athanor-update/SOURCES"
UPDATE_SHIPPED = [
    "/usr/bin/athanor-update", "/usr/bin/athanor-update-notify", "/usr/libexec/athanor-update/render-policy",
    "/usr/share/athanor/containers/templates/policy.json.in",
    "/usr/share/athanor/containers/templates/attachments-policy.json.in",
    "/usr/share/athanor/containers/templates/athanor.yaml.in",
    "/usr/lib/systemd/system/athanor-update-check.timer", "/usr/lib/systemd/system/athanor-update-check.service",
    "/usr/lib/systemd/system/athanor-update.service", "/usr/lib/systemd/system/athanor-update-state.service",
    "/usr/lib/systemd/system/athanor-update-migrate.service", "/usr/lib/systemd/user/athanor-update-notify.service",
    "/usr/lib/systemd/system-preset/80-athanor-update.preset", "/usr/lib/systemd/user-preset/80-athanor-update.preset",
    "/usr/lib/tmpfiles.d/athanor-update.conf", "/usr/share/dbus-1/system.d/os.athanor.Update1.conf",
    "/usr/share/dbus-1/system-services/os.athanor.Update1.service", "/usr/share/polkit-1/actions/os.athanor.update.policy",
]
SYSTEM_IMAGES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]


def update_trust_problems(root=None):
    """The update and trust package ships what docs/architecture/doc_update_trust.md says
    (UT1, UT3, UT5, UT6, UT7), and the wiring it replaces is gone."""
    root = root or ROOT
    problems = []
    spec = root / "forge/specs/athanor-update/athanor-update.spec"
    if not spec.exists():
        return [f"{rel(spec)}: missing"]
    files = read(spec).split("%files", 1)[-1]
    for shipped in UPDATE_SHIPPED:
        if not re.search(rf"^{re.escape(shipped)}$", files, re.M):
            problems.append(f"athanor-update.spec: %files does not list {shipped}")
        source = root / UPDATE_SOURCES / shipped.lstrip("/")
        if "/usr/bin/" not in shipped and not source.exists():
            problems.append(f"{UPDATE_SOURCES}{shipped}: missing")

    templates = root / UPDATE_SOURCES / "usr/share/athanor/containers/templates"
    scopes = [f"@REGISTRY@/{name}" for name in SYSTEM_IMAGES]
    try:
        policy = json.loads(read(templates / "policy.json.in").replace("@KEY_PATHS@", '"/k.pub"'))
        attachments = json.loads(read(templates / "attachments-policy.json.in"))
        registries = read(templates / "athanor.yaml.in")
    except (OSError, ValueError) as err:
        return problems + [f"policy templates: {err}"]
    for name, doc in (("policy.json.in", policy), ("attachments-policy.json.in", attachments)):
        if doc.get("default") != [{"type": "reject"}]:
            problems.append(f"{name}: `default` must be reject (bootc refuses insecureAcceptAnything; "
                            f"the attachments policy must open our repositories only)")
    docker = policy.get("transports", {}).get("docker", {})
    if sorted(docker) != sorted([""] + scopes):
        problems.append(f"policy.json.in: docker scopes are {sorted(docker)}, expected the three system images and \"\"")
    for scope in scopes:
        for req in docker.get(scope, [{}]):
            if req.get("type") != "sigstoreSigned" or req.get("signedIdentity") != {"type": "matchRepository"} or not req.get("keyPaths"):
                problems.append(f"policy.json.in: {scope} must be sigstoreSigned with keyPaths and matchRepository")
    for transport in ("docker-archive", "oci", "oci-archive", "dir", "containers-storage", "docker-daemon"):
        if policy.get("transports", {}).get(transport) != {"": [{"type": "insecureAcceptAnything"}]}:
            problems.append(f"policy.json.in: transport {transport} must stay open, or podman users lose it")
    if sorted(attachments.get("transports", {}).get("docker", {})) != sorted(scopes):
        problems.append("attachments-policy.json.in: docker scopes must be exactly the three system images")
    declared = re.findall(r"^  (\S+):$", registries, re.M)
    if declared != scopes or registries.count("use-sigstore-attachments: true") != 3:
        problems.append("athanor.yaml.in: must declare use-sigstore-attachments for exactly the three system images "
                        "(a missing entry makes a signed image read as unsigned, a wider scope collides with default.yaml)")

    preset = root / "forge/specs/athanor-system-config/SOURCES/usr/lib/systemd/system-preset/80-athanor-system.preset"
    if preset.exists() and re.search(r"^enable bootc-fetch-apply\.timer$", read(preset), re.M):
        problems.append(f"{rel(preset)}: enables bootc-fetch-apply.timer, which does not exist")
    override = root / "forge/specs/athanor-base-config/SOURCES/usr/lib/systemd/system/bootc-fetch-apply-updates.service.d/override.conf"
    if override.exists():
        problems.append(f"{rel(override)}: calls `bootc upgrade --stage`, a flag bootc 1.16 does not have")
    for literal in walk(root / "forge/specs/athanor-update", ""):
        if literal.is_file() and "target" not in literal.parts and "vectors" not in literal.parts:
            for owner in literal_owners(read(literal)):
                problems.append(f"{rel(literal)}: literal registry owner {owner}; "
                                f"the registry comes from the build's variables")
    # D2: the Secure Boot daemon is retired. D42 (issue #148): nothing in the image seals LUKS to
    # the TPM or increments a rollback counter by itself, so none of these units may be shipped.
    for spec in walk(root / "forge/specs", ".spec"):
        # The changelog may name what was retired: only what the spec installs counts.
        text = read(spec).split("%changelog", 1)[0]
        if "athanor-secure-boot.service" in text:
            problems.append(f"{rel(spec)}: still ships athanor-secure-boot.service (retired by D2)")
        for unit in AUTO_TPM_UNITS:
            if unit in text:
                problems.append(f"{rel(spec)}: ships {unit}, which seals or counts without the user's action (D42)")
    for tree in ("forge", "system"):
        for found in walk(root / tree, ""):
            if found.is_file() and any(found.name.startswith(unit) for unit in AUTO_TPM_UNITS):
                problems.append(f"{rel(found)}: {found.name} is shipped; D42 forbids auto-sealing and the rollback-counter units")
    for name in ("system/Containerfile", "system/athanor-install.ks"):
        installer = root / name
        if installer.exists() and any(unit in read(installer) for unit in AUTO_TPM_UNITS):
            problems.append(f"{name}: names one of {', '.join(AUTO_TPM_UNITS)} (D42)")
    return problems


def image_policy_problems(root=None):
    """system/Containerfile puts the policy in force and system/keys holds public keys only."""
    root = root or ROOT
    problems = []
    containerfile = read(root / "system/Containerfile")
    if not re.search(r"^ARG IMAGE_REGISTRY$", containerfile, re.M) or "render-policy" not in containerfile or "--link-etc /etc" not in containerfile:
        problems.append("system/Containerfile: does not run render-policy --link-etc /etc with ARG IMAGE_REGISTRY: "
                        "the policy is never in force and no machine verifies an image")
    elif containerfile.index("render-policy") > containerfile.index("systemctl preset-all"):
        problems.append("system/Containerfile: render-policy runs after preset-all")
    if "COPY system/keys/ /usr/share/athanor/keys/" not in containerfile:
        problems.append("system/Containerfile: does not copy system/keys to /usr/share/athanor/keys")
    keys = sorted((root / "system/keys").glob("*")) if (root / "system/keys").is_dir() else []
    if not any(key.suffix == ".pub" for key in keys):
        problems.append("system/keys: no *.pub: the rendered policy would name no key")
    for key in keys:
        if key.suffix != ".pub":
            problems.append(f"system/keys/{key.name}: only *.pub files belong under system/keys")
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

    built = crates_built_by_specs()
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
        owner = built.get(name)
        if owner and not has_spec:
            # Built and installed by another package's spec: shipped if that package is.
            has_spec, in_dag = True, owner in dag or owner.replace("athanor-", "") in dag
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
    for problem in update_trust_problems():
        r.fail(problem)
    for problem in image_policy_problems():
        r.fail(problem)

    return r


# --------------------------------------------------------------------------- #
# 6. documentazione — i link devono risolvere e non essere assoluti
# --------------------------------------------------------------------------- #

REQUIREMENT_HEAD = re.compile(r"^\s*(?:[-*]\s+)?\*\*([A-Z]{1,3}\d+[a-z]?)\.")
NEEDS_LINE = re.compile(r"^\s*(?:[-*]\s+|\d+\.\s+)?Needs:\s*(.*)$")
REQUIREMENT_ID = re.compile(r"[A-Z]{1,3}\d+[a-z]?")


def needs_graph(root=None):
    """The cross-specification dependency graph of docs/architecture/doc_session.md, SN11.

    A line `Needs: A1, B2.` belongs to the nearest requirement head (`**A1.` or `- **A1.`) above
    it in the same section: a Markdown heading ends a requirement, so a Needs line below one and
    before the next requirement is a problem, not silently attributed. Fenced code blocks are
    skipped. Returns (problems, graph), graph mapping each requirement to the set it needs."""
    root = root or ROOT
    files = sorted((root / "docs" / "architecture").glob("*.md"))
    defined, needs, problems = {}, {}, []
    for f in files:
        name = f.relative_to(root)
        current, fenced = None, False
        for number, line in enumerate(read(f).splitlines(), 1):
            if line.lstrip().startswith(("```", "~~~")):
                fenced = not fenced
                continue
            if fenced:
                continue
            if line.startswith("#"):
                current = None
                continue
            head = REQUIREMENT_HEAD.match(line)
            if head:
                current = head.group(1)
                defined.setdefault(current, []).append(f"{name}:{number}")
                continue
            m = NEEDS_LINE.match(line)
            if not m:
                continue
            where = f"{name}:{number}"
            if current is None:
                problems.append(f"{where}: Needs line outside a requirement")
                continue
            tokens = [t.strip() for t in m.group(1).rstrip().rstrip(".").split(",")]
            for token in tokens:
                if not REQUIREMENT_ID.fullmatch(token):
                    problems.append(f"{where}: {current} needs {token!r}, not a requirement identifier")
                elif token == current:
                    problems.append(f"{where}: {current} needs itself")
                else:
                    needs.setdefault(current, {})[token] = where
    graph = {}
    for source, targets in needs.items():
        if len(defined[source]) > 1:
            problems.append(f"{source} is defined more than once: {', '.join(defined[source])}")
        for target, where in targets.items():
            if target not in defined:
                problems.append(f"{where}: {source} needs {target}, which no specification defines")
            elif len(defined[target]) > 1:
                problems.append(f"{where}: {source} needs {target}, defined more than once: {', '.join(defined[target])}")
            graph.setdefault(source, set()).add(target)
    # Depth-first search with three colours; a grey node met again closes a cycle.
    colour, stack = {}, []

    def visit(node):
        colour[node] = "grey"
        stack.append(node)
        for nxt in sorted(graph.get(node, ())):
            if colour.get(nxt) == "grey":
                cycle = stack[stack.index(nxt):] + [nxt]
                problems.append("Needs cycle: " + " -> ".join(cycle))
            elif nxt not in colour:
                visit(nxt)
        stack.pop()
        colour[node] = "black"

    for node in sorted(graph):
        if node not in colour:
            visit(node)
    return problems, graph


REGISTER = "docs/architecture/shell-features.md"
REGISTER_STATUSES = ("have", "partial", "missing", "excluded (proposed)")


def register_count_problems(text):
    """Where the Counts section of the shell feature register differs from its rows: each
    `## <surface>` table of `| F-... |` rows against the `### <surface>` table under Counts."""
    rows, stated = {}, {}
    surface, in_counts = None, False
    for line in text.split("\n"):
        if line.startswith("## "):
            surface, in_counts = line[3:].strip(), line.strip() == "## Counts"
            continue
        if in_counts and line.startswith("### "):
            surface = line[4:].strip()
            stated[surface] = {}
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if in_counts and surface in stated and len(cells) == 2 and cells[1].isdigit():
            stated[surface][cells[0]] = int(cells[1])
        elif not in_counts and surface and cells[0].startswith("F-") and len(cells) >= 5:
            rows.setdefault(surface, []).append((cells[0], cells[3]))

    problems = []
    for surface, entries in rows.items():
        for entry, status in entries:
            if status not in REGISTER_STATUSES:
                problems.append(f"{REGISTER}: {entry} has the unknown status '{status}'")
        if surface not in stated:
            problems.append(f"{REGISTER}: surface '{surface}' has no table under Counts")
            continue
        actual = {s: sum(1 for _, st in entries if st == s) for s in REGISTER_STATUSES}
        actual["total"] = len(entries)
        for status, n in actual.items():
            if stated[surface].get(status) != n:
                problems.append(f"{REGISTER}: Counts of '{surface}' say {status} "
                                f"{stated[surface].get(status)}, the rows give {n}")
    for surface in stated.keys() - rows.keys():
        problems.append(f"{REGISTER}: Counts name '{surface}', which has no rows")
    return problems


FENCE = re.compile(r"^\s*(`{3,}|~{3,})")
CODE_SPAN = re.compile(r"(`+)(?!`).*?(?<!`)\1(?!`)", re.S)
LINK = re.compile(r"\[([^\]]*)\]\(([^)]+)\)")


def markdown_links(text):
    """(label, target) of every inline link outside code. Fenced blocks (``` or ~~~, closed
    only by a run of the same character at least as long) and code spans are examples, not
    links: a fence left open runs to the end of the file, as CommonMark has it."""
    prose, fence = [], None
    for line in text.splitlines():
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)
            else:
                prose.append(line)
        elif m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) \
                and not line.strip().lstrip(fence[0]):
            fence = None
            prose.append("")
    # A code span never crosses a paragraph, so a stray backtick stays in its own.
    paragraphs = "\n".join(prose).split("\n\n")
    plain = "\n\n".join(CODE_SPAN.sub(" ", para) for para in paragraphs)
    return [(m.group(1), m.group(2)) for m in LINK.finditer(plain)]


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
        for label, target in markdown_links(read(f)):
            link = target.split("#")[0].strip()
            if not link or link.startswith(("http://", "https://", "mailto:")):
                continue
            if link.startswith("file://"):
                r.fail(f"{t}: link assoluto della macchina di sviluppo -> {link[:70]}")
                continue
            if not (base / link).exists():
                r.fail(f"{t}: link rotto [{label[:30]}] -> {link}")
    problems, graph = needs_graph()
    for problem in problems:
        r.fail(problem)
    edges = sum(len(targets) for targets in graph.values())
    r.note(f"Needs graph: {len(set(graph) | set().union(*graph.values()))} requirements, {edges} edges")
    register = ROOT / REGISTER
    if register.exists():
        for problem in register_count_problems(read(register)):
            r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 7. panic — il budget attuale è 1 unwrap in 60k righe. Difendilo.
# --------------------------------------------------------------------------- #

# Valori misurati sul repo il 2026-09-30, senza tests/, benches/ ed examples/. Sono un cricchetto: si abbassano,
# non si alzano. Se un controllo fallisce qui, propaga con `?`.
BUDGET = {".unwrap()": 0, ".expect(": 2, "panic!(": 1}


def is_test_file(p):
    """Integration tests, benchmarks and examples are not code that runs in a daemon:
    a panic there fails a test, it does not end a service."""
    parts = Path(rel(p)).parts
    return any(d in parts for d in ("tests", "benches", "examples")) or Path(p).name == "tests.rs"


@check("panics", "Il budget di panic in codice non di test non cresce")
def check_panics():
    r = Result()
    counts = {k: 0 for k in BUDGET}
    where = {k: [] for k in BUDGET}

    for p in rust_files():
        if is_test_file(p):
            continue
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
# 7b. riga di comando del kernel — le decisioni di doc_kernel_profile.md
# --------------------------------------------------------------------------- #

# Parameters the kernel profile decided against (docs/architecture/doc_kernel_profile.md,
# "Command line"). A name ending in a dot is a prefix. profile.toml is the single source of
# the base command line and kernel_profile.py check holds its generated files to it; this
# keeps a contradicting parameter out of the manifest and of the variant files beside it.
REJECTED_PARAMETERS = {
    "iommu=pt": "identity mapping for every device, against D16 (lazy translation, strict for untrusted ports)",
    "oops=panic": "the first oops panics before oops_limit is consulted, against D19",
    "zswap.": "swap is on zram and zswap is off, against D15",
    "pti=on": "forces page table isolation on CPUs not affected by Meltdown",
    "amd_iommu=on": "not a parameter of the amd_iommu driver, which logs it as unknown",
    "lam=on": "not an x86 parameter",
    "arm64.mte=on": "not an x86 parameter",
}

# The files that carry the kernel command line: the boot line of azoth and every kargs.d
# file the image ships (the base one generated from profile.toml, the NVIDIA variants').
CMDLINE_FILE = "forge/specs/azoth/cmdline"
KARGS_FILES = [
    "forge/specs/athanor-kernel-profile/SOURCES/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml",
    "system/nvidia/athanor-nvidia-config/SOURCES/usr/lib/bootc/kargs.d/01-nvidia.toml",
]


def cmdline_problems(root=None):
    """No place that writes the kernel command line carries a parameter the profile rejects."""
    root = root or ROOT
    problems = []
    found = {}

    def read_site(relative, extract):
        try:
            found[relative] = extract(read(root / relative))
        except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError) as err:
            problems.append(f"{relative}: cannot read the command line ({err})")

    read_site(CMDLINE_FILE, str.split)
    for relative in KARGS_FILES:
        read_site(relative, lambda text: tomllib.loads(text)["kargs"])

    for site, parameters in found.items():
        for parameter in parameters:
            for rejected, why in REJECTED_PARAMETERS.items():
                if parameter == rejected or (rejected.endswith(".") and parameter.startswith(rejected)):
                    problems.append(f"{site}: {parameter} -> {why}")
    return problems


@check("cmdline", "La riga di comando del kernel non contraddice le decisioni del profilo")
def check_cmdline():
    r = Result()
    for problem in cmdline_problems():
        r.fail(problem)
    return r


PAM_CONTAINERFILE = "system/Containerfile"
NULLOK_GUARD = re.compile(r"^RUN authselect enable-feature without-nullok\b", re.MULTILINE)


def nullok_problems(root=None):
    """The image build drops nullok from every authselect stack (decision A2-23)."""
    root = root or ROOT
    try:
        text = read(root / PAM_CONTAINERFILE)
    except OSError as err:
        return [f"{PAM_CONTAINERFILE}: cannot read ({err})"]
    if not NULLOK_GUARD.search(text):
        return [f"{PAM_CONTAINERFILE}: no 'RUN authselect enable-feature without-nullok' step (A2-23)"]
    return []


@check("pam", "No account authenticates with an empty password (A2-23)")
def check_pam():
    r = Result()
    for problem in nullok_problems():
        r.fail(problem)
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

# Packages that dedicated workflows build, outside the DAG (forge/scripts/dag_orchestrator.py).
EXTERNAL_PACKAGES = {"kernel", "kernel-forge"}


def manifest_spec_problems(root=None):
    """Every custom_* entry of packages.json has a package directory under forge/specs."""
    root = root or ROOT
    manifest = root / "forge" / "config" / "packages.json"
    specs = root / "forge" / "specs"
    if not manifest.is_file():
        return []
    problems = []
    for key, pkgs in json.loads(read(manifest)).items():
        if not key.startswith("custom_"):
            continue
        for pkg in pkgs:
            if pkg in EXTERNAL_PACKAGES:
                continue
            if not ((specs / f"athanor-{pkg}").is_dir() or (specs / pkg).is_dir()):
                problems.append(f"{rel(manifest)}: {key} lists {pkg}, but forge/specs has neither "
                                f"athanor-{pkg} nor {pkg}")
    return problems


@check("specs", "Le spec installano da percorsi che esistono dalla loro working directory")
def check_specs():
    r = Result()
    for problem in manifest_spec_problems():
        r.fail(problem)
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

# SH2: the compositor client is the protocol boundary; the theme tool generates COSMIC's
# theme files. Nothing else may depend on COSMIC, the frozen tree included.
COSMIC_ALLOWED = (
    "system/athanor-compositor-client/",
    "forge/tools/calmo-cosmic-theme/",
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
            if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED):
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
            if relative.startswith(COSMIC_ALLOWED):
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


# Placeholders the unit tests use for an owner: they name no real registry namespace.
PLACEHOLDER_OWNERS = {"owner", "o"}
# Where the pipeline names its images.
REGISTRY_DIRS = (".github/workflows", "scripts", "forge/scripts")
REGISTRY_FILES = ("Justfile", "forge/Justfile", "system/Justfile", "system/Containerfile", "flake.nix")
# The file types that name images: workflows, scripts, recipes, container builds and Nix.
REGISTRY_SUFFIXES = {".yml", ".yaml", ".sh", ".py", ".just", ".nix"}
REGISTRY_NAMES = {"Justfile", "Containerfile"}


def literal_owners(text):
    """The hard-coded ghcr.io owners in text: ghcr.io/ followed by a name, not by a variable."""
    found = re.findall(r"ghcr\.io/([A-Za-z0-9][A-Za-z0-9_.-]*)", text)
    return [f"ghcr.io/{o}" for o in dict.fromkeys(found) if o not in PLACEHOLDER_OWNERS]


def registry_problems(root):
    """Every image the pipeline names comes from REGISTRY_HOST and the repository owner, each
    with a default, never from a literal owner (standing rule of 2026-09-10)."""
    root = Path(root)
    paths = [root / f for f in REGISTRY_FILES]
    for base in REGISTRY_DIRS:
        paths += walk(root / base, "")
    problems = []
    for path in sorted(paths):
        relative = path.relative_to(root)
        # Unit test fixtures need a literal owner to prove the check reports one.
        if (not path.is_file() or relative.parts[:2] == ("scripts", "tests")
                or (path.suffix not in REGISTRY_SUFFIXES and path.name not in REGISTRY_NAMES)):
            continue
        for i, line in enumerate(read(path).split("\n"), 1):
            for owner in literal_owners(line):
                problems.append(f"{relative.as_posix()}:{i}: literal registry owner {owner}; use "
                                f"REGISTRY_HOST and the repository owner, each with a default")
    return problems


@check("registry", "Images come from REGISTRY_HOST and the repository owner, never a literal owner")
def check_registry():
    r = Result()
    for problem in registry_problems(ROOT):
        r.fail(problem)
    return r


# Golden rules 2 and 3 of docs/architecture/doc_forge_development_guide.md.
SPEC_SECTION = re.compile(
    r"^%(package|description|prep|generate_buildrequires|conf|build|install|check|clean|files|"
    r"changelog|pre|post|preun|postun|pretrans|posttrans|preuntrans|postuntrans|verify|"
    r"(?:trans)?filetrigger(?:in|un|postun)|trigger(?:prein|in|un|postun))\b")
SCRIPTLETS = {"pre", "post", "preun", "postun", "pretrans", "posttrans", "preuntrans",
              "postuntrans", "verify", "triggerprein", "triggerin", "triggerun", "triggerpostun",
              "filetriggerin", "filetriggerun", "filetriggerpostun", "transfiletriggerin",
              "transfiletriggerun", "transfiletriggerpostun"}
# /usr and /etc, spelled out or through the macros that expand below them.
IMAGE_PATH = (r"(?:/usr|/etc)\b|%\{?_(?:sysconfdir|prefix|exec_prefix|bindir|sbindir|libdir|"
              r"libexecdir|datadir|datarootdir|includedir|mandir|docdir|unitdir|userunitdir|"
              r"presetdir|userpresetdir|tmpfilesdir|sysusersdir|udevrulesdir|modprobedir|"
              r"sysctldir|environmentdir)\b")
MUTATION = re.compile(r"(?:^|[\s;&|(])(?:cp|mv|install|chmod|chown|chgrp|ln|rm|mkdir|touch|tee|"
                      r"truncate|rsync|sed\s+-i\S*)\s[^;&|]*?(?:" + IMAGE_PATH + r")"
                      r"|>>?\s*(?:" + IMAGE_PATH + r")")
WEAKENING = re.compile(r"\b(?:repo_)?gpgcheck\s*=\s*(?:0|false|no)\b|--nogpgcheck\b"
                       r"|%undefine\s+_(?:fortify_source|hardened_build)\b"
                       r"|%(?:define|global)\s+_(?:fortify_level|hardened_build)\s+0\b"
                       r"|-U_FORTIFY_SOURCE\b|-D_FORTIFY_SOURCE=0\b|-fno-stack-protector\b"
                       r"|-fcf-protection=none\b|-z\s*(?:norelro|execstack)\b|-no-pie\b|-fno-PIE\b")


def forge_rule_problems(root):
    """Rule 2: no scriptlet writes under /usr or /etc, which belong to the image (%install and
    tmpfiles do). Rule 3: no spec turns off a signature check or a hardening flag."""
    root = Path(root)
    problems = []
    for path in sorted(root.glob("forge/specs/*/*.spec")):
        relative = path.relative_to(root).as_posix()
        section = None
        for i, line in enumerate(read(path).split("\n"), 1):
            header = SPEC_SECTION.match(line)
            if header:
                section = header.group(1)
            if section == "changelog":
                break
            if header or line.lstrip().startswith("#"):
                continue
            if section in SCRIPTLETS and MUTATION.search(line):
                problems.append(f"{relative}:{i}: rule 2, %{section} writes under /usr or /etc: "
                                f"install the file in %install or create it with tmpfiles.d")
            if WEAKENING.search(line):
                problems.append(f"{relative}:{i}: rule 3, turns off a signature check or a "
                                f"hardening flag")
    return problems


@check("forge-rules", "Specs keep golden rules 2 and 3 of the forge guide")
def check_forge_rules():
    r = Result()
    for problem in forge_rule_problems(ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# licence: one licence for our own code (maintainer decision A2-3, 2026-10-05)
# --------------------------------------------------------------------------- #

OWN_LICENCE = "GPL-3.0-or-later"
# Specs that package somebody else's software, by repository path: they declare
# upstream's licence, which must be an expression of known SPDX identifiers. Every
# other spec, every Cargo.toml and every nfpm `license:` field is our own code and
# must say OWN_LICENCE.
UPSTREAM_SPECS = {
    "forge/specs/athanor-ananicy/ananicy-cpp.spec",
    "forge/specs/athanor-bat/bat.spec",
    "forge/specs/athanor-bpf-linker/athanor-bpf-linker.spec",
    "forge/specs/athanor-cliphist/athanor-cliphist.spec",
    "forge/specs/athanor-cosign/athanor-cosign.spec",
    "forge/specs/athanor-dart-sass/athanor-dart-sass.spec",
    "forge/specs/athanor-matugen/athanor-matugen.spec",
    "forge/specs/athanor-rosenpass/athanor-rosenpass.spec",
    "forge/specs/athanor-syft/athanor-syft.spec",
    "forge/specs/athanor-tetragon/athanor-tetragon.spec",
    "forge/specs/azoth/microvm/azoth-microvm.spec",
    "forge/specs/cosmic-comp/cosmic-comp.spec",
}
# Crates whose manifests agents may not edit without the maintainer's approval.
PROTECTED_CRATES = {"system/confidential_computing/athanor-attestation/Cargo.toml"}
# Files that carry packaging metadata outside Cargo.toml and *.spec.
NFPM_FILES = ["flake.nix"]
# The SPDX identifiers the repository actually uses. A new one is added here on purpose.
SPDX_IDS = {
    "0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "BSL-1.0", "CC0-1.0",
    "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only", "GPL-3.0-or-later", "ISC",
    "LGPL-2.1-or-later", "LGPL-3.0-or-later", "MIT", "MPL-2.0", "Unicode-3.0",
    "Unlicense", "Zlib",
}
SPDX_EXCEPTIONS = {"LLVM-exception", "Linux-syscall-note"}


def spdx_problem(expr):
    """Why `expr` is not an SPDX expression of known identifiers, or None."""
    tokens = re.findall(r"\(|\)|[^\s()]+", expr)
    if not tokens:
        return "empty"
    depth, expect = 0, "id"  # id: identifier or "(" ; exc: exception id ; op: operator or ")"
    for tok in tokens:
        if tok == "(":
            if expect != "id":
                return "unexpected '('"
            depth += 1
        elif tok == ")":
            depth -= 1
            if expect != "op" or depth < 0:
                return "unbalanced ')'"
        elif expect == "id":
            if tok not in SPDX_IDS:
                return f"'{tok}' is not a known SPDX identifier"
            expect = "op"
        elif expect == "exc":
            if tok not in SPDX_EXCEPTIONS:
                return f"'{tok}' is not a known SPDX exception"
            expect = "op"
        elif tok in ("AND", "OR"):
            expect = "id"
        elif tok == "WITH":
            expect = "exc"
        else:
            return f"expected AND/OR/WITH, found '{tok}'"
    if expect != "op" or depth:
        return "incomplete expression"
    return None


def licence_problems(root=None):
    root = Path(root or ROOT)
    out = []
    git = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard"],
        capture_output=True, text=True)
    if git.returncode:
        return [f"cannot list the repository files: {git.stderr.strip()}"]
    files = [f for f in git.stdout.split("\n") if (root / f).is_file()]
    for f in sorted(files):
        name = f.rsplit("/", 1)[-1]
        if name == "Cargo.toml":
            try:
                pkg = tomllib.loads(read(root / f)).get("package")
            except tomllib.TOMLDecodeError as e:
                out.append(f"{f}: unreadable manifest ({e})")
                continue
            if pkg is None:
                continue
            lic = pkg.get("license")
            if lic != OWN_LICENCE:
                note = " (change awaits maintainer approval: protected crate)" \
                    if f in PROTECTED_CRATES else ""
                out.append(f"{f}: license = {lic!r}, expected {OWN_LICENCE!r}{note}")
        elif name.endswith(".spec"):
            m = re.search(r"^License:\s*(.*?)\s*$", read(root / f), re.M)
            if not m:
                out.append(f"{f}: no License: field")
            elif f in UPSTREAM_SPECS:
                why = spdx_problem(m.group(1))
                if why:
                    out.append(f"{f}: License: {m.group(1)} ({why})")
            elif m.group(1) != OWN_LICENCE:
                out.append(f"{f}: License: {m.group(1)}, expected {OWN_LICENCE}")
        elif f in NFPM_FILES:
            for n, line in enumerate(read(root / f).split("\n"), 1):
                m = re.match(r"""^\s*license:\s*["']?([^"'\s]*)""", line)
                if m and m.group(1) != OWN_LICENCE:
                    out.append(f"{f}:{n}: nfpm license: {m.group(1)}, expected {OWN_LICENCE}")
    if not (root / "LICENSE").is_file():
        out.append("LICENSE: missing at the repository root")
    return out


@check("licence", "own crates and specs declare GPL-3.0-or-later, upstream specs a valid SPDX expression")
def check_licence():
    r = Result()
    for problem in licence_problems():
        r.fail(problem)
    return r


CI_DOC = "docs/architecture/doc_ci.md"
# A workflow file name, bare or under .github/workflows/; not the tail of another path such as
# .github/actions/kvm/action.yml.
WORKFLOW_NAME = re.compile(r"(?<![\w./-])(?:\.github/workflows/)?([\w-][\w.-]*\.ya?ml)\b")
SECRET_OR_VAR = re.compile(r"\b(?:secrets|vars)\.([A-Za-z_][A-Za-z0-9_]*)")


def ci_problems(root):
    """doc_ci.md describes every workflow and names every secret and variable they read, and
    names no workflow that does not exist."""
    root = Path(root)
    doc = root / CI_DOC
    if not doc.is_file():
        return [f"{CI_DOC}: missing"]
    text = read(doc)
    workflows = sorted(p for p in (root / ".github/workflows").glob("*.y*ml")
                       if p.suffix in (".yml", ".yaml"))
    present = {p.name for p in workflows}
    named = set(WORKFLOW_NAME.findall(text))
    problems = [f".github/workflows/{name}: not described in {CI_DOC}"
                for name in sorted(present - named)]
    problems += [f"{CI_DOC}: names {name}, which is not in .github/workflows"
                 for name in sorted(named - present)]
    for path in workflows:
        seen = set()
        for i, line in enumerate(read(path).split("\n"), 1):
            for name in SECRET_OR_VAR.findall(line):
                if name not in seen and not re.search(rf"\b{name}\b", text):
                    seen.add(name)
                    problems.append(f"{path.relative_to(root).as_posix()}:{i}: {name} is not named in {CI_DOC}")
    return problems


@check("ci", "every workflow, secret and variable is described in docs/architecture/doc_ci.md")
def check_ci():
    r = Result()
    for problem in ci_problems(ROOT):
        r.fail(problem)
    return r


DECISION_FIELDS = ("id", "title", "date", "status", "issues", "areas")
DECISION_ID = r"[A-Z0-9][A-Za-z0-9-]*"
DECISION_STATUS = re.compile(
    rf"^(accepted|(?:superseded|amended) by {DECISION_ID}(?:, {DECISION_ID})*)$")


def decision_front_matter(text):
    """Front matter of a decision record as a dict of raw strings, or None."""
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None
    fields = {}
    for line in m.group(1).split("\n"):
        k, sep, v = line.partition(":")
        if sep:
            fields[k.strip()] = v.strip()
    return fields


def decision_problems(root=None):
    root = Path(root or ROOT)
    folder = root / "docs" / "decisions"
    out = []
    files = sorted(p for p in folder.glob("[0-9][0-9][0-9][0-9]-*.md"))
    records = {}
    for f in files:
        name = f.name
        fm = decision_front_matter(read(f))
        if fm is None:
            out.append(f"{name}: no front matter")
            continue
        for field in DECISION_FIELDS:
            if not fm.get(field) and field not in ("issues",):
                out.append(f"{name}: front matter lacks '{field}'")
            elif field == "issues" and field not in fm:
                out.append(f"{name}: front matter lacks 'issues'")
        id_ = fm.get("id", "")
        if id_ and not re.fullmatch(DECISION_ID, id_):
            out.append(f"{name}: malformed id '{id_}'")
        if id_ in records:
            out.append(f"{name}: id {id_} already used by {records[id_][0]}")
        elif id_:
            records[id_] = (name, fm.get("status", ""), fm.get("title", "").strip('"'))
        status = fm.get("status")
        if status and not DECISION_STATUS.match(status):
            out.append(f"{name}: status '{status}' is not accepted, superseded by <id> or amended by <id>")
        for h in ("Context", "Decision", "Consequences"):
            if f"\n## {h}\n" not in read(f):
                out.append(f"{name}: missing section '## {h}'")
    for id_, (name, status, _) in records.items():
        for target in re.findall(DECISION_ID, status.partition(" by ")[2]):
            if target not in records:
                out.append(f"{name}: status points to {target}, which has no record")
    readme = folder / "README.md"
    if not readme.is_file():
        out.append("docs/decisions/README.md: missing")
        return out
    listed = re.findall(
        r"^\| ([^|\s]+) \| (\d{4}) \| \[([^\]]*)\]\(([^)]+)\) \| ([^|]*?) \|",
        read(readme), re.M)
    index_files = [row[3] for row in listed]
    for f in files:
        if f.name not in index_files:
            out.append(f"README.md: index does not list {f.name}")
    for id_, num, title, n, status in listed:
        if not (folder / n).is_file():
            out.append(f"README.md: index lists {n}, which does not exist")
        elif id_ not in records or records[id_][0] != n:
            out.append(f"README.md: index row {id_} does not match {n}")
        elif title != records[id_][2]:
            out.append(f"README.md: index title of {id_} differs from {n}")
        elif status != records[id_][1]:
            out.append(f"README.md: index status of {id_} differs from {n}")
        elif not n.startswith(num + "-"):
            out.append(f"README.md: index number {num} does not match {n}")
    if len(index_files) != len(set(index_files)):
        out.append("README.md: index lists a file twice")
    return out


@check("decisions", "decision records have valid front matter, unique ids, a matching README index and existing targets")
def check_decisions():
    r = Result()
    for problem in decision_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# coverage: every component has an inventory entry (A2-34)
# --------------------------------------------------------------------------- #

COMPONENTS = "docs/architecture/components.toml"
# Every child directory of these is a component; "" is the repository root.
COMPONENT_PARENTS = ("", "system", "forge", "forge/specs", "scripts")
# Children of COMPONENT_PARENTS that hold components instead of being one.
COMPONENT_CONTAINERS = {"docs", "forge", "forge/specs", "scripts", "system"}
COMPONENT_KINDS = {"crate", "rpm spec", "script tool", "config", "image", "tests", "site"}
COMPONENT_AREAS = {"kernel", "build-ci", "signing-update", "security", "shell", "apps",
                   "platform", "docs"}
COMPONENT_STATUSES = {"specified", "out-of-1.0", "missing"}


def component_dirs(files):
    """The component directories that the tracked files imply: each child directory of a
    COMPONENT_PARENTS entry, except hidden directories and the containers."""
    dirs = set()
    for f in files:
        parts = f.split("/")
        for parent in COMPONENT_PARENTS:
            depth = len(parent.split("/")) if parent else 0
            if len(parts) > depth + 1 and "/".join(parts[:depth]) == parent:
                child = "/".join(parts[:depth + 1])
                if not parts[depth].startswith(".") and child not in COMPONENT_CONTAINERS:
                    dirs.add(child)
    return dirs


def entry_problems(root, entry):
    """The problems of one inventory entry, and whether its specification awaits a merge."""
    path = entry["path"]
    out = []
    if not (root / path).exists():
        out.append("the path does not exist")
    if entry.get("kind") not in COMPONENT_KINDS:
        out.append(f"kind {entry.get('kind')!r} is not one of {sorted(COMPONENT_KINDS)}")
    if entry.get("area") not in COMPONENT_AREAS:
        out.append(f"area {entry.get('area')!r} is not one of {sorted(COMPONENT_AREAS)}")
    if not entry.get("purpose"):
        out.append("no purpose")
    status = entry.get("status")
    if status not in COMPONENT_STATUSES:
        out.append(f"status {status!r} is not one of {sorted(COMPONENT_STATUSES)}")
    if status == "out-of-1.0" and not (isinstance(entry.get("issue"), int) and entry["issue"] > 0):
        out.append("out-of-1.0 without an issue number")
    if status == "specified" and not (entry.get("spec") and entry.get("item")):
        out.append("specified without both spec and item")
    pending = False
    spec = entry.get("spec")
    if spec and not (root / "docs/architecture" / spec).is_file():
        if entry.get("branch"):
            pending = True
        else:
            out.append(f"{spec} is absent from docs/architecture")
    return out, pending


def coverage_problems(root=None, files=None):
    """Failures and warnings of the component inventory against the tree. A component
    without a specification (status missing) is a warning, not a failure."""
    root = Path(root or ROOT)
    if files is None:
        git = subprocess.run(["git", "-C", str(root), "ls-files", "--cached", "-z"],
                             capture_output=True, text=True)
        if git.returncode:
            return [f"cannot list the repository files: {git.stderr.strip()}"], []
        files = [f for f in git.stdout.split("\0") if f]
    try:
        entries = tomllib.loads(read(root / COMPONENTS)).get("component", [])
    except FileNotFoundError:
        return [f"{COMPONENTS}: missing"], []
    except tomllib.TOMLDecodeError as e:
        return [f"{COMPONENTS}: unreadable ({e})"], []
    problems, missing, pending, seen = [], [], [], set()
    for n, entry in enumerate(entries, 1):
        path = entry.get("path")
        if not path:
            problems.append(f"{COMPONENTS}: entry {n} has no path")
            continue
        if path in seen:
            problems.append(f"{COMPONENTS}: {path}: listed twice")
        seen.add(path)
        found, waits = entry_problems(root, entry)
        problems += [f"{COMPONENTS}: {path}: {p}" for p in found]
        if waits:
            pending.append(f"{path} ({entry['spec']} on {entry['branch']})")
        if entry.get("status") == "missing":
            missing.append(path)
    for d in sorted(component_dirs(files) - seen):
        problems.append(f"{d}: component directory without an entry in {COMPONENTS}")
    notes = []
    if missing:
        notes.append(f"{len(missing)} components have no specification (status missing): "
                     + ", ".join(missing))
    if pending:
        notes.append(f"{len(pending)} entries cite a specification not merged here yet: "
                     + ", ".join(pending))
    return problems, notes


@check("coverage", "Every component has an inventory entry, and every entry a spec or an issue")
def check_coverage():
    r = Result()
    problems, notes = coverage_problems()
    for problem in problems:
        r.fail(problem)
    for note in notes:
        r.note(note)
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
