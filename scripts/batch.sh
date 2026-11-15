#!/usr/bin/env bash
# Headless batch wrapper around the same kernel the graphical viewport uses.
# The viewport (./start.sh) remains the primary model-sectioning interface;
# this script only runs saved batch jobs for background processing.
#
# Usage:
#   scripts/batch.sh <job.json> [out-dir] [--require-closed]
#
# A non-zero exit means at least one cut failed and, by construction, no
# result file for that cut exists (atomic writes + failure gating).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JOB="${1:?usage: batch.sh <job.json> [out-dir] [--require-closed]}"
if [[ $# -ge 2 && "$2" != --* ]]; then
  OUT_DIR="$2"
  shift 2
else
  OUT_DIR="$ROOT/release/batch-output"
  shift
fi
EXTRA=("$@")

BIN="$ROOT/server/target/release/section-server"
if [[ ! -x "$BIN" ]]; then
  echo "building kernel (release)…"
  ( cd "$ROOT/server" && PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" cargo build --release )
fi

mkdir -p "$OUT_DIR"
"$BIN" batch --job "$JOB" --out-dir "$OUT_DIR" "${EXTRA[@]}"
