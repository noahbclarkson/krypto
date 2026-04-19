#!/usr/bin/env python3
"""
Chandelier P × M 2D Sweep Equity Comparison Chart

Configs:
- baseline_P20_M200: P=20, M=2.00 (prior default)
- current_P20_M215:  P=20, M=2.15 (current default)  
- winner_P15_M150:   P=15, M=1.50 (new 2D sweep winner)
- runner_P15_M155:   P=15, M=1.55 (runner-up)

Generates: charts/chand_pm_comparison.png
"""

import sys
import math

# ── Config ──────────────────────────────────────────────────────────────────
CSV_PATH = "snapshots/chand_pm_equity_comparison.csv"
OUT_PATH = "charts/chand_pm_comparison.png"

# ── Colour palette ──────────────────────────────────────────────────────────
COLORS = {
    "baseline_P20_M200": "#2196F3",  # blue
    "current_P20_M215":  "#FF5722",  # deep orange
    "winner_P15_M150":   "#00C853",  # vivid green
    "runner_P15_M155":   "#AA00FF",  # violet
}

LINES = {
    "baseline_P20_M200": (3.0, "P=20, M=2.00 (prior default)"),
    "current_P20_M215":  (2.5, "P=20, M=2.15 (current default)"),
    "winner_P15_M150":   (2.0, "P=15, M=1.50 (2D sweep WINNER)"),
    "runner_P15_M155":   (1.5, "P=15, M=1.55 (runner-up)"),
}

# ── Load CSV ────────────────────────────────────────────────────────────────
with open(CSV_PATH) as f:
    header = f.readline().strip().split(",")
    configs = header[1:]  # ['baseline_P20_M200', 'current_P20_M215', ...]

    rows = []
    for line in f:
        parts = line.strip().split(",")
        if len(parts) != len(header):
            continue
        rows.append([float(parts[0])] + [float(x) for x in parts[1:]])

if not rows:
    print("ERROR: no data in CSV", file=sys.stderr)
    sys.exit(1)

bars     = [r[0] for r in rows]
equities = {c: [r[i+1] for r in rows] for i, c in enumerate(configs)}

# ── Summary stats ───────────────────────────────────────────────────────────
def final_equity(name, eq):
    fe = eq[-1]
    years = len(eq) / 365
    cagr  = (fe ** (1 / years) - 1) * 100 if fe > 0 and years > 0 else 0
    # Running peak drawdown
    peak = eq[0]
    worst_dd = 0.0
    for e in eq:
        if e > peak: peak = e
        dd = (peak - e) / peak if peak > 0 else 0
        if dd > worst_dd: worst_dd = dd
    return fe, cagr, worst_dd

print("\n=== Equity Summary ===")
for cfg in configs:
    fe, cagr, dd = final_equity(cfg, equities[cfg])
    label = LINES.get(cfg, (1.0, cfg))[1]
    print(f"  {label:<45} final={fe:>18.2f}x  CAGR={cagr:>8.1f}%  DD={dd*100:>6.1f}%")

# ── Plot ────────────────────────────────────────────────────────────────────
try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print("matplotlib not available — skipping chart", file=sys.stderr)
    sys.exit(0)

plt.rcParams.update({
    "font.family": "DejaVu Sans",
    "font.size": 11,
    "axes.spines.top": False,
    "axes.spines.right": False,
})

fig, (ax_eq, ax_dd) = plt.subplots(2, 1, figsize=(14, 10), height_ratios=[3, 1], sharex=True)

# ── Equity (log scale) ──────────────────────────────────────────────────────
for cfg, eq in equities.items():
    color  = COLORS.get(cfg, "#333333")
    lw, lbl = LINES.get(cfg, (1.5, cfg))
    ax_eq.plot(bars, eq, color=color, linewidth=lw, label=lbl, zorder=3)

ax_eq.set_ylabel("Portfolio Equity (× initial)", fontsize=12)
ax_eq.set_title(
    "CHAND_PERIOD × CHAND_MULT 2D Sweep — Equity Curve Comparison\n"
    "Base5 Universe (BTC, ETH, SOL, XRP, DOGE, ADA) · Full History",
    fontsize=13, pad=12
)
ax_eq.set_yscale("log")
ax_eq.yaxis.set_major_formatter(mticker.FuncFormatter(
    lambda v, _: f"{v:,.0f}" if v >= 1 else f"{v:.2f}"
))
ax_eq.grid(True, which="major", alpha=0.3, zorder=0)
ax_eq.grid(True, which="minor", alpha=0.1, zorder=0)
ax_eq.legend(loc="upper left", fontsize=10, framealpha=0.9)
ax_eq.set_xlim(0, len(bars))

# ── Drawdown (linear) ────────────────────────────────────────────────────────
for cfg, eq in equities.items():
    color = COLORS.get(cfg, "#333333")
    lw, lbl = LINES.get(cfg, (1.5, cfg))
    peak = max(eq)
    dd_curve = [min(e / peak - 1, 0) for e in eq]
    ax_dd.fill_between(bars, 0, dd_curve, color=color, alpha=0.15, zorder=3)
    ax_dd.plot(bars, dd_curve, color=color, linewidth=lw * 0.7, zorder=3)

ax_dd.set_ylabel("Drawdown", fontsize=12)
ax_dd.set_xlabel("Bar (daily)", fontsize=12)
ax_dd.yaxis.set_major_formatter(mticker.PercentFormatter(1.0))
ax_dd.grid(True, alpha=0.3, zorder=0)
ax_dd.set_ylim(-1, 0.05)

# Fix subplot spacing
fig.subplots_adjust(hspace=0.07)

# ── Caption with key metrics ─────────────────────────────────────────────────
caption_lines = []
for cfg in configs:
    fe, cagr, dd = final_equity(cfg, equities[cfg])
    label = LINES.get(cfg, (1.0, cfg))[1]
    caption_lines.append(f"{label}: final {fe:.1f}x, CAGR {cagr:.0f}%, DD {dd*100:.0f}%")

fig.text(
    0.5, 0.01,
    "  |  ".join(caption_lines),
    ha="center", va="bottom", fontsize=8.5,
    color="#555555", style="italic"
)

# ── Save ─────────────────────────────────────────────────────────────────────
fig.savefig(OUT_PATH, dpi=150, bbox_inches="tight", facecolor="white")
print(f"\nSaved: {OUT_PATH}")
plt.close(fig)
