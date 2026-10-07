#!/usr/bin/env bash
# Verify the Verus models. Usage: VERUS=/path/to/verus verus/verify.sh  (or `verus` on PATH)
set -euo pipefail
cd "$(dirname "$0")"
VERUS="${VERUS:-verus}"
exec "$VERUS" lib.rs --crate-type=lib "$@"
