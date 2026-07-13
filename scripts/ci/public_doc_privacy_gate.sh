#!/usr/bin/env bash
# Public-repo privacy gate (DOC-002 / TEST-002 / GOV-001).
# Fail when private maintainer paths or archived-org URL patterns appear in tracked files.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "ripgrep (rg) is required for public_doc_privacy_gate.sh" >&2
  exit 2
fi

# Build forbidden patterns without storing full private repo names as single
# contiguous literals in other docs (this file is excluded from the scan).
PART_A='ai-lib-plans'
PART_B='ai-lib-constitution'
PART_C='active/projects/'
PART_D='github\.com/hiddenpath/'
PART_E='raw\.githubusercontent\.com/hiddenpath/'
PART_F='api\.github\.com/repos/hiddenpath/'
PATTERN="${PART_A}|${PART_B}|${PART_C}|${PART_D}|${PART_E}|${PART_F}"

EXCLUDES=(
  --glob '!**/.git/**'
  --glob '!**/target/**'
  --glob '!**/node_modules/**'
  --glob '!**/Cargo.lock'
  --glob '!**/package-lock.json'
  --glob '!**/ui-chat/dist/**'
  --glob '!scripts/ci/public_doc_privacy_gate.sh'
)

echo "Running public doc privacy gate..."
set +e
HITS="$(rg -n -i --hidden "${EXCLUDES[@]}" -e "$PATTERN" . 2>/dev/null)"
RG_EXIT=$?
set -e

if [ "$RG_EXIT" -eq 2 ]; then
  echo "ripgrep failed while scanning" >&2
  exit 2
fi

if [ "$RG_EXIT" -eq 0 ] && [ -n "$HITS" ]; then
  echo "Public privacy gate FAILED — private/maintainer references found:" >&2
  echo "$HITS" >&2
  echo >&2
  echo "Remove private maintainer planning/governance paths, internal task-tracker" >&2
  echo "paths, and archived-org GitHub URLs from public docs/code/tests." >&2
  echo "See CONTRIBUTING.md section: Public repository privacy." >&2
  exit 1
fi

echo "Public doc privacy gate passed."
