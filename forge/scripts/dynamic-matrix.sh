#!/bin/bash
set -euo pipefail

echo "🌋 Executing Athanor Forge DAG Orchestration Engine..." >&2

# The engine asks the registry (REGISTRY_HOST, GITHUB_REPOSITORY_OWNER) which packages are built
python3 scripts/dag_orchestrator.py
