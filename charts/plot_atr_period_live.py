#!/usr/bin/env python3
"""
TURTLE_ATR_PERIOD live-path sweep comparison chart.
Generates: charts/comparison_chart.png

Data sources:
  snapshots/turtle_atr_period_sweep_summary.csv   — aggregated metrics by ATR_P
  snapshots/turtle_atr_period_sweep_equity.csv    — equity curves by ATR_P
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

OUT = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"

# ── 1. Load sweep summaries ───────────────────────────────────────────────────
summary = pd.read_csv("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_period_sweep_summary.csv")

# ── 2. Load equity curves ──────────────────────────────────────────────────────
equity = pd.read_csv("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_period_sweep_equity.csv")

# Filter to show key ATR periods: baseline (24), low (10), high (48), and spread across range
show_aps = [10, 24, 48]
eq_lines = {}
for ap in show_aps:
    eq_subset = equity[equity['atr_period'] == ap].copy()
    if not eq_subset.empty:
        eq_subset = eq_subset.sort_values('step')
        label = f"ATR_P={ap}" + (" [baseline]" if ap == 24 else (" [winner]" if ap == 10 else ""))
        eq_lines[label] = eq_subset.set_index('step')['equity']

# ── 3. Plot ────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(1, 2, figsize=(14, 5))
fig.suptitle(
    "TURTLE_ATR_PERIOD — Live Turtle-Only Path Sweep",
    fontsize=14, fontweight="bold", y=1.02
)

# Panel A: Pass rate bar chart (all ATR periods)
ax = axes[0]
aps_sorted = summary.sort_values('atr_period')
bars = ax.bar(aps_sorted['atr_period'], aps_sorted['pass_rate'] * 100, color="#1976D2", alpha=0.7, width=3)
# Baseline
baseline = summary[summary['atr_period'] == 24]
if not baseline.empty:
    idx = list(aps_sorted['atr_period']).index(24)
    bars[idx].set_color("#4CAF50")
ax.axhline(50, color="gray", linestyle="--", linewidth=1, alpha=0.5)
ax.set_xlabel("ATR Period")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("A. Pass Rate vs ATR Period\n(ALL values IDENTICAL - INERT)")
ax.set_xlim(0, 105)

# Panel B: Equity curves (log scale)
ax = axes[1]
colors = ["#4CAF50", "#2196F3", "#FF9800"]
for i, (label, eq_df) in enumerate(eq_lines.items()):
    ax.plot(eq_df.index, eq_df.values, label=label, color=colors[i % len(colors)], linewidth=2, alpha=0.9)

ax.set_yscale("log")
ax.set_xlabel("Step (bar)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("B. Equity Curves — ATR Period ∈ {10, 24, 48}\n(ALL IDENTICAL - INERT)")
ax.legend(fontsize=9, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# Add annotation
fig.text(0.5, -0.02, 
    "CONCLUSION: TURTLE_ATR_PERIOD is INERT on the exact-live Turtle-only path.\n"
    "All 21 tested values produce identical results (pass 68.5%, Sharpe 4.686, return +203.6%).\n"
    "The Turtle ATR trailing stop (M=2.0) always fires before HOLD_MAX can bind.",
    ha='center', fontsize=9, style='italic', color="#D32F2F")

plt.tight_layout()
os.makedirs("/home/ubuntu/.openclaw/workspace-krypto/charts", exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"Saved {OUT}")