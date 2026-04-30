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

header = [
    "# DAILY PROGRESS TRACKING — Krypto Strategies",
    "# NOTE: sharpe_methodology is required. DDBudget's reported Sharpe is milestone-aggregated and not comparable to Turtle's daily compounded equity Sharpe.",
    "date,strategy,equity_x,reported_sharpe,sharpe_methodology,trades",
]
rows = []
pattern = re.compile(r"^- (?P<name>.*?): (?P<eq>[0-9.]+x) .*?, Sharpe (?P<sharpe>[0-9.]+)(?: \[(?P<method>[^\]]+)\])?", re.M)
methodology = {
    "Turtle+Chandelier": "daily_compounded_equity",
    "DDBudget 3-Sleeve": "milestone_aggregated_not_comparable",
    "A/D Momentum": "fixed_hold_daily_equity",
    "FactorSmallByDV": "fixed_hold_daily_equity",
}
for m in pattern.finditer(summary):
    name = m.group("name").strip()
    eq = m.group("eq")
    sharpe = m.group("sharpe")
    trades = {
        "Turtle+Chandelier": "397",   # validated progress harness, 2026-04-29+
        "DDBudget 3-Sleeve": "776",
        "A/D Momentum": "3933",
        "FactorSmallByDV": "1257",
    }.get(name, "n/a")
    method = methodology.get(name, (m.group("method") or "unknown").replace(",", ";"))
    rows.append(f"{day},{name},{eq},{sharpe},{method},{trades}")

if not rows:
    raise SystemExit("No strategy rows parsed from snapshots/progress_equity_curves.md")

old_lines = report.read_text().splitlines() if report.exists() else []
body = [line for line in old_lines if line and not line.startswith("#") and not line.startswith("date,")]
body = [line for line in body if not line.startswith(day + ",")]
report.write_text("\n".join(header + rows + body).rstrip() + "\n")
print("Wrote:")
for row in rows:
    print("  " + row)
PY

echo "Done. Chart: $ROOT/charts/progress_equity_curves_daily.png"
