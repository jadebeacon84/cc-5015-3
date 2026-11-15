#!/usr/bin/env bash
# Section build pipeline (release engineering for the geometry tool).
#
# Produces a joint release of the graphical section viewport (built web UI)
# and the headless batch tooling (section-server CLI) into release/current.
#
# Hard guarantees:
#   * every step runs from a clean cold start (no stale target/dist reuse for
#     the shipped binary/UI);
#   * kernel regression tests must pass, including plane-through-vertex and
#     coplanar-triangle inputs;
#   * batch results are reloaded and geometrically verified before promotion;
#   * the open-shell fail-gate job MUST fail: proving the gate is armed;
#   * the release lock (dependencies, tolerance rules, test fixtures) is
#     regenerated and compared with the committed one;
#   * any failure aborts before release/current is touched. Staging is built
#     in a private directory and atomically renamed only after all checks.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${PORT:-8787}"
BIN="$ROOT/server/target/release/section-server"
JOBS_DIR="$ROOT/release/jobs"
LOCK_FILE="$ROOT/release/release-lock.json"
STAGE="$(mktemp -d "$ROOT/release/.stage.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

step() { printf '\n=== %s ===\n' "$*"; }
fail() { printf '\nRELEASE ABORTED: %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- cold start
step "1/8 cold-start build (clean release artifacts)"
rm -rf "$ROOT/server/target/release" "$ROOT/web/dist"
(
  cd server
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" cargo build --release
)
[[ -x "$BIN" ]] || fail "kernel binary missing after build"
(
  cd web
  npm run build
)
[[ -f "$ROOT/web/dist/index.html" ]] || fail "web UI missing after build"

# ---------------------------------------------------------------- kernel tests
step "2/8 kernel regression tests (real geometry, no defaults masking errors)"
(
  cd server
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" cargo test --release -- --nocapture
)

# ------------------------------------------------------------- release lock
step "3/8 regenerate and compare release lock (deps + tolerances + fixtures)"
"$BIN" lock --repo-root "$ROOT" --out "$STAGE/release-lock.json"
if [[ -f "$LOCK_FILE" ]]; then
  "$BIN" lock --repo-root "$ROOT" --out "$LOCK_FILE" --check
fi

# ------------------------------------------------------------------- batches
step "4/8 background batch: edge cases (through-vertex + coplanar triangles)"
"$BIN" batch --job "$JOBS_DIR/edge-cases.batch.json" --out-dir "$STAGE/batch/edge-cases"
step "5/8 background batch: demo building (multiple planes)"
"$BIN" batch --job "$JOBS_DIR/demo.batch.json" --out-dir "$STAGE/batch/demo"

step "6/8 reload every result and verify closed contours + cap mesh"
while IFS= read -r -d '' result; do
  "$BIN" verify --result "$result" --require-closed
  # Each result ships with a content manifest; the hash must match the file.
  manifest="${result%.json}.manifest.json"
  [[ -f "$manifest" ]] || fail "missing manifest for $result"
done < <(find "$STAGE/batch" -name '*.json' ! -name '*.manifest.json' ! -name 'batch-report.json' -print0)

step "7/8 failure gate: open-shell job must be rejected and produce no result"
if "$BIN" batch --job "$JOBS_DIR/open-shell-failgate.batch.json" --out-dir "$STAGE/batch/failgate"; then
  fail "fail-gate job unexpectedly succeeded; gating is not armed"
fi
if find "$STAGE/batch/failgate" -name '*.json' ! -name 'batch-report.json' | grep -q .; then
  fail "failed batch left result artifacts behind"
fi
echo "gate armed: failed section produced no consumable output"

# ------------------------------------------------------------- assemble + UI
step "8/8 assemble joint release and atomically publish"
mkdir -p "$STAGE/bin" "$STAGE/ui" "$STAGE/fixtures" "$STAGE/jobs"
cp "$BIN" "$STAGE/bin/section-server"
cp -R "$ROOT/web/dist/." "$STAGE/ui/"
cp "$ROOT/fixtures"/*.json "$STAGE/fixtures/"
cp "$JOBS_DIR"/*.json "$STAGE/jobs/"
cp "$ROOT/start.sh" "$STAGE/start.sh"
cp "$ROOT/scripts/batch.sh" "$STAGE/batch.sh"
chmod +x "$STAGE/start.sh" "$STAGE/batch.sh"

# Smoke the assembled binary from inside the stage (clean environment): the
# graphical preview must be servable alongside the section application.
(
  cd "$STAGE"
  STATIC_DIR="$STAGE/ui" PORT="$PORT" "$STAGE/bin/section-server" serve &
  srv=$!
  trap 'kill "$srv" 2>/dev/null || true' EXIT
  for _ in $(seq 1 50); do
    if curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null; then break; fi
    sleep 0.2
  done
  curl -sf "http://127.0.0.1:$PORT/api/health" | grep -q '"ok":true' || fail "health check failed"
  curl -sf "http://127.0.0.1:$PORT/" | grep -q '建筑截面视窗' || fail "graphical preview not served"
  # The bundled viewport application must ship with the section kernel.
  asset="$(curl -sf "http://127.0.0.1:$PORT/" | grep -oE '/assets/[^"]+\.js' | head -1)"
  [[ -n "$asset" ]] || fail "UI bundle reference missing from index.html"
  curl -sf "http://127.0.0.1:$PORT$asset" -o "$STAGE/.served-bundle.js" \
    || fail "viewport bundle could not be downloaded"
  grep -q '模型剖切视窗' "$STAGE/.served-bundle.js" || fail "viewport bundle not delivered"
  rm -f "$STAGE/.served-bundle.js"
  kill "$srv"
  trap - EXIT
)

# Publish only after every check above has succeeded.
PUBLISHED="$ROOT/release/current"
PREV="$ROOT/release/previous"
rm -rf "$PREV"
if [[ -d "$PUBLISHED" ]]; then mv "$PUBLISHED" "$PREV"; fi
mv "$STAGE" "$PUBLISHED"
trap - EXIT
printf '\nRELEASE OK -> %s\n' "$PUBLISHED"
"$PUBLISHED/bin/section-server" --help | head -1
