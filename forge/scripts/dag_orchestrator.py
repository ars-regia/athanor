#!/usr/bin/env python3
"""
Athanor Forge DAG Orchestrator Engine
Calculates Directed Acyclic Graph (DAG) for RPM dependencies, asks the registry which
custom packages are already built, and outputs topological matrix execution levels for
parallel GitHub Actions execution.
"""

import sys
import os
import glob
import re
import json
import hashlib
import subprocess
import tomllib
from collections import defaultdict, deque

CONFIG_PATH = os.environ.get("CONFIG_PATH", "config/packages.json")
if not os.path.exists(CONFIG_PATH) and os.path.exists("forge/config/packages.json"):
    CONFIG_PATH = "forge/config/packages.json"

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
# check_idempotency.sh reads specs/ and config/ relative to the forge directory.
FORGE_DIR = os.environ.get("FORGE_DIR", os.path.dirname(SCRIPTS_DIR))

SPECS_DIR = os.environ.get("SPECS_DIR", "specs")
if not os.path.exists(SPECS_DIR) and os.path.exists("forge/specs"):
    SPECS_DIR = "forge/specs"



def compute_dir_hash(dir_path):
    """Calculates deterministic SHA256 for a directory."""
    hasher = hashlib.sha256()
    if not os.path.exists(dir_path):
        return hasher.hexdigest()[:16]
        
    for root, dirs, files in sorted(os.walk(dir_path)):
        for name in sorted(files):
            if name.startswith(".") or name.endswith(".swp"):
                continue
            filepath = os.path.join(root, name)
            try:
                with open(filepath, "rb") as f:
                    while chunk := f.read(65536):
                        hasher.update(chunk)
            except OSError:
                pass
    return hasher.hexdigest()[:16]

def workspace_path(manifest, name):
    """The directory a `name = { workspace = true }` dependency of manifest points at, when
    the nearest enclosing workspace declares it with a path, else None."""
    directory = os.path.dirname(os.path.realpath(manifest))
    while True:
        candidate = os.path.join(directory, "Cargo.toml")
        if os.path.isfile(candidate):
            with open(candidate, "rb") as f:
                workspace = tomllib.load(f).get("workspace")
            if workspace is not None:
                dep = workspace.get("dependencies", {}).get(name)
                if isinstance(dep, dict) and "path" in dep:
                    return os.path.realpath(os.path.join(directory, dep["path"]))
                return None
        parent = os.path.dirname(directory)
        if parent == directory:
            return None
        directory = parent


def path_dependencies(spec_dir):
    """The directories of the Cargo path dependencies that the crates under spec_dir reach
    outside it, followed transitively, sorted, including those inherited from a workspace.
    The package builds from their sources too."""
    spec_root = os.path.realpath(spec_dir)
    pending = []
    for root, dirs, files in os.walk(spec_dir):
        dirs[:] = sorted(d for d in dirs if d != "target" and not d.startswith("."))
        if "Cargo.toml" in files:
            pending.append(os.path.join(root, "Cargo.toml"))
    found = set()
    while pending:
        manifest = pending.pop()
        with open(manifest, "rb") as f:
            data = tomllib.load(f)
        tables = [data, *data.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for name, dep in table.get(kind, {}).items():
                    if not isinstance(dep, dict):
                        continue
                    if "path" in dep:
                        target = os.path.realpath(os.path.join(os.path.dirname(manifest), dep["path"]))
                    elif dep.get("workspace") is True:
                        target = workspace_path(manifest, name)
                        if target is None:
                            continue
                    else:
                        continue
                    inside = target == spec_root or target.startswith(spec_root + os.sep)
                    if inside or target in found:
                        continue
                    found.add(target)
                    pending.append(os.path.join(target, "Cargo.toml"))
    return sorted(found)


def package_hash(spec_dir):
    """The hash of a custom package: its spec directory and, when its crates have path
    dependencies outside it, each of those directories, named relative to spec_dir. A
    package without any keeps the plain directory hash."""
    dependencies = path_dependencies(spec_dir)
    if not dependencies:
        return compute_dir_hash(spec_dir)
    hasher = hashlib.sha256(compute_dir_hash(spec_dir).encode())
    spec_root = os.path.realpath(spec_dir)
    for dependency in dependencies:
        hasher.update(os.path.relpath(dependency, spec_root).encode())
        hasher.update(compute_dir_hash(dependency).encode())
    return hasher.hexdigest()[:16]


def parse_spec_dependencies(spec_path):
    """Extracts BuildRequires and Requires from a .spec file."""
    build_requires = set()
    requires = set()
    
    if not os.path.exists(spec_path):
        return build_requires, requires
        
    with open(spec_path, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if line.startswith("BuildRequires:"):
                deps = line.split(":", 1)[1].strip()
                for dep in re.split(r'\s+|,', deps):
                    dep = re.sub(r'[><=].*', '', dep).strip()
                    if dep and not dep.startswith("%"):
                        build_requires.add(dep)
            elif line.startswith("Requires:"):
                deps = line.split(":", 1)[1].strip()
                for dep in re.split(r'\s+|,', deps):
                    dep = re.sub(r'[><=].*', '', dep).strip()
                    if dep and not dep.startswith("%"):
                        requires.add(dep)
                        
    return build_requires, requires

# Packages that dedicated workflows build, outside the DAG.
EXTERNAL_PACKAGES = {"kernel", "kernel-forge"}


def spec_dir_for(pkg):
    """The spec directory of a custom package: specs/athanor-<pkg>, else specs/<pkg>."""
    spec_dir = os.path.join(SPECS_DIR, f"athanor-{pkg}")
    if not os.path.exists(spec_dir):
        spec_dir = os.path.join(SPECS_DIR, pkg)
    return spec_dir


def load_package_manifest():
    """Loads packages.json single source of truth.

    Fails when a custom package, other than an external one, has no spec directory: such an
    entry is built by nothing, and would otherwise surface only as a late build failure.
    """
    if not os.path.exists(CONFIG_PATH):
        return {}
    with open(CONFIG_PATH, "r") as f:
        manifest = json.load(f)
    listed = {
        pkg
        for key, pkgs in manifest.items()
        if key.startswith("custom_")
        for pkg in pkgs
    }
    missing = sorted(
        pkg for pkg in listed - EXTERNAL_PACKAGES if not os.path.isdir(spec_dir_for(pkg))
    )
    if missing:
        sys.exit(
            f"dag_orchestrator: {CONFIG_PATH} lists custom packages with no spec directory "
            f"under {SPECS_DIR}/: {', '.join(missing)}. Remove them from the custom_* lists "
            f"or restore their specs."
        )
    return manifest


def custom_spec_dirs(manifest):
    """The spec directories of the custom packages the DAG builds."""
    return sorted(
        spec_dir_for(pkg)
        for pkg in set(manifest.get("custom_packages", [])) - EXTERNAL_PACKAGES
    )


def build_dag(manifest):
    """Constructs the dependency graph and node metadata."""
    custom_pkgs = manifest.get("custom_packages", [])
    tier0 = manifest.get("custom_tier0", [])
    tier1 = manifest.get("custom_tier1", [])
    tier2 = manifest.get("custom_tier2", [])
    tier3 = manifest.get("custom_tier3", [])
    
    upstream_core = manifest.get("upstream_core", [])
    upstream_desktop = manifest.get("upstream_desktop", [])
    upstream_media = manifest.get("upstream_media", [])
    upstream_cli = manifest.get("upstream_cli", [])
    flatpaks = manifest.get("flatpaks", manifest.get("flatpak_packages", []))
    
    all_custom = set(custom_pkgs)
    all_upstream = set(upstream_core + upstream_desktop + upstream_media + upstream_cli)
    
    # Exclude external packages handled by dedicated workflows (e.g., self-hosted kernel)
    external_pkgs = EXTERNAL_PACKAGES
    
    all_nodes = (all_custom | all_upstream | set(flatpaks)) - external_pkgs
    
    graph = defaultdict(set)       # node -> set of nodes depending on node (outgoing edges)
    in_degree = defaultdict(int)   # node -> number of prerequisites
    prereqs = defaultdict(set)     # node -> set of nodes node depends on
    node_hashes = {}
    node_types = {}
    
    for node in all_nodes:
        in_degree[node] = 0
        if node in all_custom:
            node_types[node] = "custom"
        elif node in flatpaks:
            node_types[node] = "flatpak"
        else:
            node_types[node] = "upstream"
            
    # Add tier dependencies
    for t1 in tier1:
        for t0 in tier0:
            if t0 in all_nodes and t1 in all_nodes:
                graph[t0].add(t1)
                prereqs[t1].add(t0)
                
    for t2 in tier2:
        for t1 in tier1:
            if t1 in all_nodes and t2 in all_nodes:
                graph[t1].add(t2)
                prereqs[t2].add(t1)
                
    for t3 in tier3:
        for t2 in tier2:
            if t2 in all_nodes and t3 in all_nodes:
                graph[t2].add(t3)
                prereqs[t3].add(t2)

    # Parse spec files for direct dependencies
    for pkg in all_custom:
        if pkg not in all_nodes:
            continue
        spec_dir = spec_dir_for(pkg)
        spec_files = glob.glob(os.path.join(spec_dir, "*.spec"))
        
        hash_val = package_hash(spec_dir)
        node_hashes[pkg] = hash_val
        
        if spec_files:
            build_reqs, reqs = parse_spec_dependencies(spec_files[0])
            for dep in build_reqs | reqs:
                clean_dep = dep.replace("athanor-", "")
                if clean_dep in all_nodes and clean_dep != pkg:
                    graph[clean_dep].add(pkg)
                    prereqs[pkg].add(clean_dep)

    for pkg in all_upstream:
        hash_val = hashlib.sha256(f"upstream-{pkg}".encode()).hexdigest()[:16]
        node_hashes[pkg] = hash_val
        
    for pkg in flatpaks:
        fp_dir = os.path.join("flatpaks", pkg)
        hash_val = compute_dir_hash(fp_dir)
        node_hashes[pkg] = hash_val

    for node in all_nodes:
        in_degree[node] = len(prereqs[node])
        
    return all_nodes, graph, prereqs, in_degree, node_hashes, node_types

def content_hash(pkg):
    """The content hash check_idempotency.sh gives a package: the tag its image carries."""
    out = subprocess.run(
        ["bash", os.path.join(SCRIPTS_DIR, "check_idempotency.sh"), "--package", pkg, "--hash-only"],
        cwd=FORGE_DIR, capture_output=True, text=True, check=True,
    ).stdout
    return out.strip().removeprefix("CONTENT_HASH=")


def image_exists(ref):
    """Whether the registry has ref. registry_probe.sh answers present or absent and fails
    on anything else; retry.sh repeats those failures, and one that outlasts the retries
    raises, so a registry that cannot be asked stops the run instead of reading as clean."""
    out = subprocess.run(
        ["bash", os.path.join(SCRIPTS_DIR, "retry.sh"), "bash",
         os.path.join(SCRIPTS_DIR, "registry_probe.sh"), ref],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    return out == "present"


def evaluate_dirty_nodes(all_nodes, node_types, exists=image_exists, hash_of=content_hash):
    """
    A custom package is dirty when the registry has no
    <registry>/<owner>/athanor-forge-<package>:hash-<content hash>. Builds run with
    rpmbuild --nodeps and never consume another package's output, so a dirty package does
    not dirty its dependents. Upstream packages are never built here and flatpaks publish
    nothing: neither is ever dirty.
    """
    owner = os.environ.get("GITHUB_REPOSITORY_OWNER")
    if not owner:
        sys.exit("dag_orchestrator: GITHUB_REPOSITORY_OWNER is not set")
    registry = os.environ.get("REGISTRY_HOST", "ghcr.io")
    dirty_nodes = set()
    for node in sorted(all_nodes):
        if node_types.get(node) != "custom":
            continue
        ref = f"{registry}/{owner}/athanor-forge-{node}:hash-{hash_of(node)}"
        if not exists(ref):
            dirty_nodes.add(node)
    return dirty_nodes

def partition_dag_levels(dirty_nodes, graph, prereqs, node_types):
    """
    Groups dirty nodes into topological execution levels (Level 0, Level 1, Level 2, Flatpaks).
    """
    level_0 = []
    level_1 = []
    level_2 = []
    flatpaks = []
    
    dirty_prereqs = {n: set(p for p in prereqs[n] if p in dirty_nodes) for n in dirty_nodes}
    dirty_in_degree = {n: len(dirty_prereqs[n]) for n in dirty_nodes}
    
    queue = deque([n for n in dirty_nodes if dirty_in_degree[n] == 0])
    level_map = {}
    
    for n in queue:
        level_map[n] = 0

    while queue:
        curr = queue.popleft()
        curr_lvl = level_map[curr]
        
        for neighbor in graph[curr]:
            if neighbor in dirty_nodes:
                level_map[neighbor] = max(level_map.get(neighbor, 0), curr_lvl + 1)
                dirty_in_degree[neighbor] -= 1
                if dirty_in_degree[neighbor] == 0:
                    queue.append(neighbor)

    for node in dirty_nodes:
        if node_types.get(node) == "flatpak":
            flatpaks.append(node)
        elif node_types.get(node) == "custom":
            lvl = level_map.get(node, 0)
            if lvl == 0:
                level_0.append(node)
            elif lvl == 1:
                level_1.append(node)
            else:
                level_2.append(node)
        else:
            pass # [MARTIAL LAW] Do not schedule upstream packages for compilation!
                
    return level_0, level_1, level_2, flatpaks

def main():
    print("🧠 Forge DAG Architect initializing... (registry hash tags)")
    
    manifest = load_package_manifest()
    all_nodes, graph, prereqs, in_degree, node_hashes, node_types = build_dag(manifest)
    
    print(f"📊 DAG Topology built: {len(all_nodes)} nodes analyzed.")
    
    dirty_nodes = evaluate_dirty_nodes(all_nodes, node_types)
    
    level_0, level_1, level_2, flatpaks = partition_dag_levels(dirty_nodes, graph, prereqs, node_types)
    
    has_changes = "true" if len(dirty_nodes) > 0 else "false"
    
    j_lvl0 = json.dumps(level_0)
    j_lvl1 = json.dumps(level_1)
    j_lvl2 = json.dumps(level_2)
    j_fp = json.dumps(flatpaks)
    
    print(f"🚀 DAG Execution Plan calculated:")
    print(f"  -> Level 0 Parallel Nodes ({len(level_0)}): {j_lvl0}")
    print(f"  -> Level 1 Parallel Nodes ({len(level_1)}): {j_lvl1}")
    print(f"  -> Level 2 Parallel Nodes ({len(level_2)}): {j_lvl2}")
    print(f"  -> Flatpak Parallel Nodes ({len(flatpaks)}): {j_fp}")
    print(f"  -> Has Changes: {has_changes}")

    
    # Creazione della Rappresentazione Visiva del DAG (Mermaid) per Github Actions
    mermaid = ["```mermaid", "graph TD;"]
    for node in dirty_nodes:
        safe_node = "n_" + node.replace("-", "_")
        parents = [p for p in prereqs[node] if p in dirty_nodes]
        if parents:
            for p in parents:
                safe_p = "n_" + p.replace("-", "_")
                mermaid.append(f'    {safe_p}["{p}"] --> {safe_node}["{node}"];')
        else:
            mermaid.append(f'    {safe_node}["{node}"];')
    mermaid.append("```")
    mermaid_str = "\n".join(mermaid)
    if len(mermaid_str) > 40000:
        mermaid_str = "```mermaid\ngraph TD;\n    too_large[\"Il DAG supera i 40k caratteri e non puo' essere renderizzato.\"];\n```"
    
    github_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_summary:
        try:
            with open(github_summary, "a") as f:
                f.write("### 🌋 Athanor Forge DAG - Execution Topology\n")
                f.write(mermaid_str + "\n")
        except OSError:
            pass

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a") as f:
            f.write(f"dag_level_0={j_lvl0}\n")
            f.write(f"dag_level_1={j_lvl1}\n")
            f.write(f"dag_level_2={j_lvl2}\n")
            f.write(f"dag_flatpaks={j_fp}\n")
            f.write(f"dirty_count={len(dirty_nodes)}\n")
            f.write(f"has_changes={has_changes}\n")

if __name__ == "__main__":
    if sys.argv[1:] == ["--list-spec-dirs"]:
        spec_dirs = custom_spec_dirs(load_package_manifest())
        if not spec_dirs:
            sys.exit(f"dag_orchestrator: no custom_packages in {CONFIG_PATH}")
        print("\n".join(spec_dirs))
    else:
        main()
