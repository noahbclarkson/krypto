#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TODAY="${1:-$(date -u +%F)}"

echo "== Daily progress refresh: $TODAY =="
echo "[1/3] Running validated equity harness..."
cargo run --example progress_equity_curves --profile sweep 2>&1

echo "[2/3] Rendering chart..."
python3 charts/plot_progress.py

echo "[3/3] Updating reports/daily_progress.csv..."
python3 - "$TODAY" <<'PY'
import re
import sys
from pathlib import Path

day = sys.argv[1]
root = Path.cwd()
summary = (root / "snapshots" / "progress_equity_curves.md").read_text()
report = root / "reports" / "daily_progress.csv"

rows = []
pattern = re.compile(r"^- (?P<name>.*?): (?P<eq>[0-9.]+x) .*?, Sharpe (?P<sharpe>[0-9.]+)", re.M)
for m in pattern.finditer(summary):
    name = m.group("name").strip()
    eq = m.group("eq")
    sharpe = m.group("sharpe")
    trades = {
        "Turtle+Chandelier": "397",   # live_turtle_chandelier dry-run, 2026-04-29
        "DDBudget 3-Sleeve": "776",
        "A/D Momentum": "3933",
        "FactorSmallByDV": "1257",
    }.get(name, "n/a")
    rows.append(f"{day},{name},{eq},{sharpe},n/a,{trades}")

if not rows:
    raise SystemExit("No strategy rows parsed from snapshots/progress_equity_curves.md")

old_lines = report.read_text().splitlines() if report.exists() else ["# DAILY PROGRESS TRACKING — Turtle+Chandelier Production"]
kept = [line for line in old_lines if not line.startswith(day + ",")]
if kept and kept[-1].strip():
    kept.append("")
kept.extend(rows)
report.write_text("\n".join(kept).rstrip() + "\n")
print("Wrote:")
for row in rows:
    print("  " + row)
PY

echo "Done. Chart: $ROOT/charts/progress_equity_curves_daily.png"
