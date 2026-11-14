#!/usr/bin/env bash
# Builds (if needed) and starts the archisection viewport: Rust kernel API plus
# the built web UI on a single port. Open the printed URL in a browser.
set -euo pipefail

cd "$(dirname "$0")"
PORT="${PORT:-8787}"
export STATIC_DIR="$(pwd)/web/dist"

if [[ ! -x server/target/release/section-server ]]; then
  echo "[1/2] building Rust kernel + server…"
  ( cd server && PATH="$HOME/.cargo/bin:$PATH" cargo build --release )
fi

if [[ ! -f web/dist/index.html ]]; then
  echo "[2/2] building web UI…"
  ( cd web && npm install && npm run build )
fi

echo "archisection UI: http://127.0.0.1:${PORT}/"
exec ./server/target/release/section-server
