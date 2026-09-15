#!/usr/bin/env bash
# Trial install gate (VL-OPS-002): same worktree release binary, fingerprint required.
set -euo pipefail

FINGERPRINT="${VELACLAW_BUILD_FINGERPRINT:-VL-OPS-002-build-fingerprint}"
DEST="${VELACLAW_INSTALL_BIN:-${HOME}/.local/bin/velaclaw}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${VELACLAW_RELEASE_BIN:-$ROOT/target/release/velaclaw}"

if [[ ! -x "$SRC" ]]; then
  echo "error: missing $SRC — build in this worktree:" >&2
  echo "  cargo build --release --features ai-protocol" >&2
  exit 1
fi

# Refuse sandbox / tmp cargo targets unless the operator overrides SRC.
case "$SRC" in
  /tmp/cursor-sandbox-cache/*)
    echo "error: refusing sandbox-cache binary: $SRC" >&2
    exit 1
    ;;
esac

if ! command -v strings >/dev/null 2>&1; then
  echo "error: strings(1) is required to verify the build fingerprint" >&2
  exit 1
fi

if ! strings "$SRC" | grep -q -- "$FINGERPRINT"; then
  echo "error: $SRC does not contain fingerprint $FINGERPRINT" >&2
  echo "  (stale target, wrong worktree, or non-release build)" >&2
  exit 1
fi

mkdir -p "$(dirname "$DEST")"
install -m 755 "$SRC" "$DEST"
echo "installed $SRC -> $DEST"
echo "fingerprint: $FINGERPRINT"
echo "next: systemctl --user restart velaclaw.service  (if using the trial daemon)"
