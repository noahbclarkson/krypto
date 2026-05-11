#!/usr/bin/env python3
"""
FRESNESS_COOLDOWN hyperopt equity chart.
Generates: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png

Data sources:
  snapshots/t95_fc_sweep_summary.csv      — aggregated metrics by FC
  snapshots/t95_fc_equity_0.csv            — baseline equity curve (FC=0)
  snapshots/t95_fc_equity_93.csv           — winner (FC=93, 82.98% pass)
  snapshots/t95_fc_equity_96.csv           — runner-up (FC=96, 82.98% pass)
  snapshots/t95_fc_equity_97.csv           — runner-up (FC=97, 82.98% pass)
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
from pathlib import Path

WORKSPACE_ROOT = Path("/home/ubuntu/.openclaw/workspace-krypto")
SNAP = WORKSPACE_ROOT / "krypto/snapshots"
OUT = WORKSPACE_ROOT / "charts/comparison_chart.png"

# ── 1. Load sweep summary ─────────────────────────────────────────────────────
summary = pd.read_csv(SNAP / "t95_fc_sweep_summary.csv")
summary = summary.sort_values('fc').reset_index(drop=True)

# ── 2. Load equity curves ──────────────────────────────────────────────────────
eq_curves = {}
for fc_val, label in [(0, "Baseline (FC=0)"), (93, "Winner (FC=93)"), (96, "Runner-up 1 (FC=96)"), (97, "Runner-up 2 (FC=97)")]:
    path = SNAP / f"t95_fc_equity_{fc_val}.csv"
    if path.exists():
        df = pd.read_csv(path)
        df = df.sort_values('bar')
        eq_curves[fc_val] = df['equity'].values
        print(f"Loaded FC={fc_val}: {len(df)} bars, final equity={df['equity'].iloc[-1]:.4f}")
    else:
        print(f"Missing equity file: {path}")

# ── 3. Build combined equity DataFrame ──────────────────────────────────────────
if not eq_curves:
    print("ERROR: No equity curves loaded. Exiting.")
    exit(1)

max_len = max(len(v) for v in eq_curves.values())
n_bars = max_len

# Align by padding shorter curves to max length
aligned = {}
for fc, curve in eq_curves.items():
    aligned[fc] = np.pad(curve, (0, max_len - len(curve)), constant_values=curve[-1] if len(curve) > 0 else 1.0)

# ── 4. Plot ────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "FRESHNESS_COOLDOWN Hyperopt — Exact Live-Bot Path\n"
    "(T95: 101 values FC∈[0..100] × 9 universes × 6 WF windows = 54 windows/value)",
    fontsize=13, fontweight="bold", y=0.98
)

COLORS = {0: "#e74c3c", 93: "#27ae60", 96: "#3498db", 97: "#9b59b6"}
labels = {0: "Baseline (FC=0)", 93: "Winner (FC=93)", 96: "Runner-up 1 (FC=96)", 97: "Runner-up 2 (FC=97)"}

# ── Panel A: Pass rate vs FC ─────────────────────────────────────────────────
ax = axes[0, 0]
ax.plot(summary['fc'], summary['pass_pct'], color="#1976D2", linewidth=1.5, alpha=0.8)
ax.scatter(summary['fc'], summary['pass_pct'], color="#1976D2", s=8, alpha=0.6)
ax.axvline(93, color="#27ae60", linestyle="--", linewidth=1.5, alpha=0.7, label="FC=93 [winner]")
ax.axvline(96, color="#3498db", linestyle="--", linewidth=1.2, alpha=0.7, label="FC=96")
ax.axvline(97, color="#9b59b6", linestyle="--", linewidth=1.2, alpha=0.7, label="FC=97")
ax.axhline(70, color="gray", linestyle=":", linewidth=1, alpha=0.5, label="70% threshold")
ax.set_xlabel("FRESHNESS_COOLDOWN (bars)")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("A. Pass Rate vs FRESHNESS_COOLDOWN\n(9 universes × 6 WF windows)")
ax.legend(fontsize=8)
ax.set_xlim(0, 100)

# ── Panel B: Avg Sharpe vs FC ─────────────────────────────────────────────────
ax = axes[0, 1]
ax.plot(summary['fc'], summary['avg_sharpe'], color="#E91E63", linewidth=1.5, alpha=0.8)
ax.scatter(summary['fc'], summary['avg_sharpe'], color="#E91E63", s=8, alpha=0.6)
ax.axvline(93, color="#27ae60", linestyle="--", linewidth=1.5, alpha=0.7, label="FC=93 [winner]")
ax.axvline(96, color="#3498db", linestyle="--", linewidth=1.2, alpha=0.7, label="FC=96")
ax.axvline(97, color="#9b59b6", linestyle="--", linewidth=1.2, alpha=0.7, label="FC=97")
ax.set_xlabel("FRESHNESS_COOLDOWN (bars)")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("B. Avg Sharpe vs FRESHNESS_COOLDOWN\n(9 universes × 6 WF windows)")
ax.legend(fontsize=8)
ax.set_xlim(0, 100)

# ── Panel C: Equity curves (log scale) ───────────────────────────────────────
ax = axes[1, 0]
x = np.arange(n_bars)
for fc, curve in aligned.items():
    label = labels.get(fc, f"FC={fc}")
    # Replace any inf/nan with last valid value
    curve = np.where(np.isfinite(curve), curve, np.nan)
    curve = pd.Series(curve).ffill().fillna(1.0).values
    ax.semilogy(x, curve, label=label, color=COLORS.get(fc, "gray"), linewidth=1.5, alpha=0.85)

ax.set_xlabel("Bar (trading days from 2021-06)")
ax.set_ylabel("Portfolio Equity (log scale)")
ax.set_title("C. Equity Curves — Baseline vs Winner vs Runner-ups\n(FC=0, 93, 96, 97)")
ax.legend(fontsize=9, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# ── Panel D: Return and MaxDD vs FC ───────────────────────────────────────────
ax = axes[1, 1]
ax2 = ax.twinx()
l1, = ax.plot(summary['fc'], summary['avg_return_pct'], color="#388E3C", linewidth=1.5, alpha=0.8, label="Avg Return (%)")
l2, = ax2.plot(summary['fc'], summary['avg_max_dd_pct'], color="#F57C00", linewidth=1.5, alpha=0.8, label="Avg MaxDD (%)")
ax.axvline(93, color="#27ae60", linestyle="--", linewidth=1.5, alpha=0.7)
ax.set_xlabel("FRESHNESS_COOLDOWN (bars)")
ax.set_ylabel("Avg Return (%)", color="#388E3C")
ax2.set_ylabel("Avg MaxDD (%)", color="#F57C00")
ax.set_title("D. Return & Drawdown vs FRESHNESS_COOLDOWN")
ax.tick_params(axis='y', labelcolor="#388E3C")
ax2.tick_params(axis='y', labelcolor="#F57C00")
lines = [l1, l2]
ax.legend(lines, ["Avg Return (%)", "Avg MaxDD (%)"], fontsize=8)
ax.set_xlim(0, 100)

plt.tight_layout(rect=[0, 0.03, 1, 0.96])
OUT.parent.mkdir(parents=True, exist_ok=True)
plt.savefig(str(OUT), dpi=150, bbox_inches="tight")
print(f"Saved {OUT}")

# ── Print key metrics table ────────────────────────────────────────────────────
print("\n=== Key Metrics ===")
for fc, label in labels.items():
    row = summary[summary['fc'] == fc]
    if not row.empty:
        print(f"  {label}: pass={row['pass_pct'].values[0]:.1f}%, Sharpe={row['avg_sharpe'].values[0]:.3f}, "
              f"return={row['avg_return_pct'].values[0]:.1f}%, MaxDD={row['avg_max_dd_pct'].values[0]:.1f}%")