#!/usr/bin/env bash
# The plan job of promote-stable.yml (doc_pipeline.md PL60): runs every check of a promotion
# and writes $PROMOTE_ARTIFACTS/promotion.json, the record the evidence bundle signs and the
# release approval covers. It moves no tag: only system/promote.sh, in the environment
# release, writes to the registry, and PL60's verify.py rule finds it in no other job.
# Usage: promotion-plan.sh [--hardware-override athanor-system-nvidia] RUN_ID
set -euo pipefail
shopt -s inherit_errexit

# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/promotion-record.sh"
promotion_args "$@"
promotion_record "$(date -u +%Y%m%d)" "$artifacts/promotion.json"
cat "$artifacts/promotion.json"
