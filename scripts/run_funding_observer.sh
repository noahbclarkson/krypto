#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$ROOT/logs"
LOCK_FILE="/tmp/krypto_funding_observer.lock"
mkdir -p "$LOG_DIR"

(
  flock -n 9 || { echo "funding observer already running" >&2; exit 0; }
  cd "$ROOT"
  ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "[$ts] funding observer start" >> "$LOG_DIR/funding_observer.log"
  cargo run --example funding_rate_live_observer --profile sweep >> "$LOG_DIR/funding_observer.log" 2>&1
) 9>"$LOCK_FILE"
