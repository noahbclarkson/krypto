#!/usr/bin/env python3
"""
REGIME_ATR_PERIOD hyperopt comparison chart.
Reads from regime_atr_period_summary.csv and regime_ap_per_universe.csv.
"""

import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

SUMMARY_CSV = "snapshots/regime_atr_period_summary.csv"
PER_UNI_CSV = "snapshots/regime_ap_per_universe.csv"
OUTPUT = "charts/regime_ap_comparison.png"

# ─── Load summary ──────────────────────────────────────────────────────────────
summary = {}
with open(SUMMARY_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        ap = int(row["ap"])
        summary[ap] = {
            "pass": int(row["pass_count"]),
            "total": 63,
            "sharpe": float(row["avg_sharpe"]),
            "ret": float(row["avg_return_pct"]),
            "dd": float(row["avg_dd_pct"]),
        }

# ─── Load per-universe pass data ───────────────────────────────────────────────
per_uni = {}  # ap -> uni -> data
with open(PER_UNI_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        uni = row["universe"]
        ap = int(row["ap"])
        if ap not in per_uni:
            per_uni[ap] = {}
        per_uni[ap][uni] = {
            "pass": int(row["pass_count"]),
            "total": int(row["total"]),
            "sharpe": float(row["avg_sharpe"]),
            "ret": float(row["avg_ret"]),
        }

# ─── Sort summary by pass desc ────────────────────────────────────────────────
sorted_aps = sorted(summary.keys(), key=lambda a: (summary[a]["pass"], summary[a]["sharpe"]), reverse=True)
winner_ap = sorted_aps[0]
baseline_ap = 12

ws = summary[winner_ap]
bs = summary[baseline_ap]

top5_aps = sorted_aps[:5]
print(f"Winner: AP={winner_ap} ({ws['pass']}/63 pass, Sharpe={ws['sharpe']:.2f})")
print(f"Baseline: AP={baseline_ap} ({bs['pass']}/63 pass, Sharpe={bs['sharpe']:.2f})")

# ─── Plot ──────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 2]})

colors_map = {
    baseline_ap: "#888888",
    winner_ap: "#FF6B35",
    39: "#1D7EF5",
    41: "#00C9A7",
    63: "#B049F5",
}

# Panel 1: Pass rate vs Sharpe scatter
ax1 = axes[0]
for ap in sorted_aps:
    s = summary[ap]
    color = colors_map.get(ap, "#CCCCCC")
    label = None
    lw = 2.0
    if ap == winner_ap:
        label = f"AP={ap} WINNER"
        lw = 3.0
    elif ap == baseline_ap:
        label = f"AP={ap} baseline"
        lw = 2.5
    ax1.plot(ap, s["pass"], 'o', color=color, markersize=8,
            label=label, linewidth=lw)
    if ap in top5_aps or ap == baseline_ap:
        ax1.annotate(f"  AP={ap}", (ap, s["pass"]),
                     fontsize=8, va='bottom')

ax1.axhline(bs["pass"], color="#888888", linestyle='--', alpha=0.5, linewidth=0.8)
ax1.set_xlabel("REGIME_ATR_PERIOD (AP)", fontsize=11)
ax1.set_ylabel("Pass Rate (windows / 63)", fontsize=11)
ax1.set_title(f"AP={winner_ap} (WINNER): {ws['pass']}/63 pass, Sharpe={ws['sharpe']:.2f}, DD={ws['dd']:.1f}%\n"
             f"AP={baseline_ap} (baseline): {bs['pass']}/63 pass, Sharpe={bs['sharpe']:.2f}, DD={bs['dd']:.1f}%",
             fontsize=11)
ax1.grid(True, alpha=0.3)
ax1.legend(loc='lower right', fontsize=9)
ax1.set_ylim(45, 62)

# Panel 2: Per-universe pass rate bar chart
ax2 = axes[1]
unis = ["Base5", "NoDOGE", "Legacy4", "Legacy5BNB", "OldGuardNoBNB",
        "LargeCaps5", "Legacy3", "LowVolume5", "OldGuard4"]
x_pos = list(range(len(unis)))
width = 0.35

for i, uni in enumerate(unis):
    for ai, (ap, c, lbl) in enumerate([(baseline_ap, colors_map[baseline_ap], f"AP={baseline_ap}"),
                                       (winner_ap, colors_map[winner_ap], f"AP={winner_ap} WINNER")]):
        pu = per_uni.get(ap, {}).get(uni, {"pass": 0, "total": 7})
        rate = pu["pass"] / float(pu["total"]) * 100
        offset = -width/2 if ai == 0 else width/2
        ax2.bar(i + offset, rate, width, color=c, alpha=0.85, label=lbl if i == 0 else "")

ax2.set_xticks(x_pos)
ax2.set_xticklabels(unis, rotation=30, ha='right', fontsize=9)
ax2.set_ylabel("Pass Rate (%)", fontsize=11)
ax2.set_ylim(0, 115)
ax2.axhline(100, color='gray', linestyle='--', alpha=0.5, linewidth=0.8)
ax2.legend(loc='upper right', fontsize=9)
ax2.grid(True, alpha=0.3, axis='y')
ax2.set_title(f"Per-Universe Pass Rate: AP={baseline_ap} (baseline) vs AP={winner_ap} (WINNER)", fontsize=11)

caption = (
    f"AP={winner_ap} wins +{ws['pass']-bs['pass']:.0f} windows ({ws['pass']}/63 vs {bs['pass']}/63), "
    f"Sharpe +{ws['sharpe']-bs['sharpe']:.2f} ({ws['sharpe']:.2f} vs {bs['sharpe']:.2f}), "
    f"DD -{ws['dd']-bs['dd']:.1f}pp ({ws['dd']:.1f}% vs {bs['dd']:.1f}%)"
)
fig.text(0.5, 0.02, caption, ha='center', fontsize=9, style='italic',
         bbox=dict(boxstyle='round', facecolor='white', alpha=0.8))

plt.tight_layout(rect=[0, 0.05, 1, 1])
plt.savefig(OUTPUT, dpi=150, bbox_inches='tight')
print(f"\nSaved: {OUTPUT}")
plt.close()
