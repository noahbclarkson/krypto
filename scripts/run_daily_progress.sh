#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TODAY="${1:-$(date -u +%F)}"

echo "== Daily production progress refresh: $TODAY =="
echo "[1/3] Running exact live-bot source-of-truth harness..."
cargo run --example live_bot_exact_equity --profile sweep 2>&1

echo "[2/3] Rendering exact live-bot chart..."
python3 charts/plot_live_bot_exact.py

echo "[3/3] Updating reports/daily_progress.csv..."
python3 - "$TODAY" <<'PY'
import re
import sys
from pathlib import Path

DAY = sys.argv[1]
ROOT = Path.cwd()
MD = (ROOT / "snapshots" / "live_bot_exact_equity.md").read_text()
REPORT = ROOT / "reports" / "daily_progress.csv"
LEGACY = ROOT / "reports" / "daily_progress_PRE_T67_STALE.csv"

def metric(label: str) -> str:
    m = re.search(rf"\| {re.escape(label)} \| ([^|]+)\|", MD)
    if not m:
        raise SystemExit(f"missing metric: {label}")
    return m.group(1).strip()

# Preserve the old mixed-methodology report once, but do not keep appending stale rows.
if REPORT.exists() and not LEGACY.exists():
    LEGACY.write_text(REPORT.read_text())

equity = metric("Final equity")
sharpe = metric("Daily account Sharpe")
maxdd = metric("Max drawdown")
trades = metric("Trades")
days = metric("Days")
notes = "exact src/live/bot.rs replay; economic account equity; VOL_LOOKBACK configured but unused by bot.rs; research/walk-forward Sharpe excluded"

header = "date,strategy,equity_x,daily_account_sharpe,max_drawdown,trades,days,source,status,notes\n"
new_row = f"{DAY},Exact live bot (T65/T67),{equity},{sharpe},{maxdd},{trades},{days},snapshots/live_bot_exact_equity.md,production_source_of_truth,{notes}\n"

old_rows = []
if REPORT.exists():
    for line in REPORT.read_text().splitlines()[1:]:
        if line and not line.startswith(DAY + ",") and "production_source_of_truth" in line:
            old_rows.append(line + "\n")
REPORT.write_text(header + new_row + "".join(old_rows))
print(new_row.strip())
PY

echo "Done. Chart: $ROOT/charts/live_bot_exact_equity.png"
