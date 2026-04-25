#!/usr/bin/env python3
"""Chart regime_stress_p7_current results alongside P=28/M=2.0 for comparison."""
import polars as pl
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

# Load both CSV files
p7 = pl.read_csv("snapshots/regime_stress_p7_current.csv")
try:
    p28 = pl.read_csv("snapshots/regime_stress_test.csv")
    has_p28 = True
except:
    has_p28 = False
    print("regime_stress_test.csv not found — P=28 comparison unavailable")

phases = ["P1-2020", "P2-2021", "P3-2019"]

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

# ── Left: Pass rate bar chart ────────────────────────────────────────────────
ax1 = axes[0]
x = np.arange(len(phases))
width = 0.35

p7_passes = [p7.filter(pl.col("phase") == ph, pl.col("pass") == True).height for ph in phases]
p7_totals = [p7.filter(pl.col("phase") == ph).height for ph in phases]
p7_pcts = [p/t*100 for p, t in zip(p7_passes, p7_totals)]

bars = ax1.bar(x, p7_pcts, width, label="P=7/M=2.25", color="#2196F3", alpha=0.85)
ax1.axhline(70, color="orange", ls="--", lw=1.5, label="70% threshold")
ax1.axhline(100, color="green", ls="--", lw=1.5, label="100% pass")

if has_p28:
    p28_passes = [p28.filter(pl.col("phase") == ph, pl.col("pass") == True).height for ph in phases]
    p28_totals = [p28.filter(pl.col("phase") == ph).height for ph in phases]
    p28_pcts = [p/t*100 for p, t in zip(p28_passes, p28_totals)]
    ax1.bar(x + width, p28_pcts, width, label="P=28/M=2.0", color="#4CAF50", alpha=0.85)

ax1.set_xlabel("Phase")
ax1.set_ylabel("Pass Rate (%)")
ax1.set_title("Regime Stress: Pre-2021 Held-Out Pass Rate")
ax1.set_xticks(x + width/2)
ax1.set_xticklabels(phases)
ax1.set_ylim(0, 115)
ax1.legend()
ax1.grid(axis="y", alpha=0.3)

for bar, pct in zip(bars, p7_pcts):
    ax1.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
            f"{pct:.0f}%", ha='center', va='bottom', fontsize=10)

# ── Right: Per-symbol Sharpe comparison ────────────────────────────────────
ax2 = axes[1]
symbols = p7["symbol"].unique()
ph_colors = {"P1-2020": "#2196F3", "P2-2021": "#FF9800", "P3-2019": "#F44336"}

for ph, color in ph_colors.items():
    sub = p7.filter(pl.col("phase") == ph)
    ax2.bar([f"{r['symbol']}\n{ph}" for r in sub.iter_rows(named=True)],
            [r["sharpe"] for r in sub.iter_rows(named=True)],
            label=ph, color=color, alpha=0.75)

ax2.axhline(0, color="black", lw=0.8)
ax2.set_xlabel("Symbol / Phase")
ax2.set_ylabel("Sharpe Ratio")
ax2.set_title("Regime Stress: P=7/M=2.25 — Per-Symbol Sharpe")
ax2.legend()
ax2.grid(axis="y", alpha=0.3)
plt.xticks(rotation=45, ha="right")

plt.suptitle("Regime Stress Test: Current Production Params P=7/M=2.25", fontsize=13, fontweight="bold")
plt.tight_layout()
plt.savefig("charts/regime_stress_p7_current.png", dpi=150, bbox_inches="tight")
print("Saved: charts/regime_stress_p7_current.png")

# ── Summary stats ────────────────────────────────────────────────────────────
print(f"\nP=7/M=2.25 Summary:")
for ph in phases:
    sub = p7.filter(pl.col("phase") == ph)
    passes = sub.filter(pl.col("pass") == True).height
    total = sub.height
    avg_sh = sub["sharpe"].mean()
    avg_ret = sub["return_pct"].mean()
    print(f"  {ph}: {passes}/{total} pass, avg Sharpe {avg_sh:.2f}, avg return {avg_ret:+.1f}%")

overall_pass = p7.filter(pl.col("pass") == True).height
overall_total = p7.height
print(f"\n  OVERALL: {overall_pass}/{overall_total} pass ({overall_pass/overall_total*100:.0f}%)")
print(f"  Status: {'✅ ALL PASS' if overall_pass == overall_total else '⚠️ SOME FAIL'}")
