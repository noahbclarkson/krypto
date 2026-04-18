#!/usr/bin/env python3
"""Chart turtle freshness filter walk-forward results."""

import csv
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
from collections import defaultdict

cooldowns = [0, 3, 5, 10, 15, 20]
universe_data = defaultdict(lambda: defaultdict(list))

with open("snapshots/turtle_freshness_filter_wf.csv") as f:
    reader = csv.DictReader(f)
    for row in reader:
        universe_data[row["universe"]][int(row["cooldown"])].append({
            "ret": float(row["ret"]),
            "sharpe": float(row["sharpe"]),
            "pass": row["pass"] == "true",
            "trades": int(row["trades"]),
            "max_dd": float(row["max_dd"]),
        })

# Aggregate per universe per cooldown
agg = {}
for uni, cd_data in universe_data.items():
    agg[uni] = {}
    for cd in cooldowns:
        wins = [r for r in cd_data[cd] if r["pass"]]
        fails = [r for r in cd_data[cd] if not r["pass"]]
        agg[uni][cd] = {
            "pass_rate": len(wins) / len(cd_data[cd]) * 100 if cd_data[cd] else 0,
            "avg_sharpe": sum(r["sharpe"] for r in cd_data[cd]) / len(cd_data[cd]) if cd_data[cd] else 0,
            "avg_ret": sum(r["ret"] for r in cd_data[cd]) / len(cd_data[cd]) if cd_data[cd] else 0,
            "trades": sum(r["trades"] for r in cd_data[cd]),
        }

# Overall summary across all universes
overall = {}
for cd in cooldowns:
    pass_total = sum(agg[uni][cd]["pass_rate"] for uni in agg) / len(agg)
    sharpe_total = sum(agg[uni][cd]["avg_sharpe"] for uni in agg) / len(agg)
    ret_total = sum(agg[uni][cd]["avg_ret"] for uni in agg) / len(agg)
    trades_total = sum(agg[uni][cd]["trades"] for uni in agg)
    overall[cd] = {"pass_rate": pass_total, "sharpe": sharpe_total, "ret": ret_total, "trades": trades_total}

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

colors = ["#888888", "#2ecc71", "#27ae60", "#1e8449", "#e74c3c", "#c0392b"]
labels = [f"CD={cd}" for cd in cooldowns]

# Pass rate bar chart
bars = axes[0].bar(range(len(cooldowns)), [overall[cd]["pass_rate"] for cd in cooldowns], color=colors, edgecolor="white", linewidth=0.5)
axes[0].set_xticks(range(len(cooldowns)))
axes[0].set_xticklabels([f"CD={cd}" for cd in cooldowns])
axes[0].axhline(60, color="red", linestyle="--", linewidth=1, label="60% threshold")
axes[0].set_ylabel("Pass Rate (%)")
axes[0].set_title("Freshness Filter — Pass Rate by Cooldown\n(9-universe average)", fontsize=11)
axes[0].set_ylim(0, 100)
axes[0].legend()
for bar, cd in zip(bars, cooldowns):
    val = overall[cd]["pass_rate"]
    axes[0].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1.5, f"{val:.0f}%",
                ha="center", va="bottom", fontsize=9, fontweight="bold",
                color="green" if val >= 60 else "red")

# Sharpe bar chart
sharpe_vals = [overall[cd]["sharpe"] for cd in cooldowns]
bar_colors = ["#888888" if cd == 0 else ("#2ecc71" if sh > 0 else "#e74c3c") for cd, sh in zip(cooldowns, sharpe_vals)]
bars2 = axes[1].bar(range(len(cooldowns)), sharpe_vals, color=bar_colors, edgecolor="white", linewidth=0.5)
axes[1].set_xticks(range(len(cooldowns)))
axes[1].set_xticklabels([f"CD={cd}" for cd in cooldowns])
axes[1].axhline(0, color="black", linestyle="-", linewidth=0.5)
axes[1].set_ylabel("Avg Sharpe")
axes[1].set_title("Freshness Filter — Avg Sharpe by Cooldown\n(9-universe average)", fontsize=11)
for bar, cd in zip(bars2, cooldowns):
    val = overall[cd]["sharpe"]
    axes[1].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.02,
                f"{val:.3f}" if val >= 0 else f"{val:.3f}",
                ha="center", va="bottom" if val >= 0 else "top", fontsize=8,
                color="green" if val > 0 else "red")

# Annotation for best
best_cd = max(cooldowns, key=lambda cd: overall[cd]["sharpe"] if overall[cd]["pass_rate"] >= 60 else -999)
axes[1].annotate(f"Best: CD={best_cd}", xy=(cooldowns.index(best_cd), overall[best_cd]["sharpe"]),
                xytext=(cooldowns.index(best_cd), overall[best_cd]["sharpe"] + 0.15),
                ha="center", fontsize=9, color="darkgreen",
                arrowprops=dict(arrowstyle="->", color="darkgreen", lw=1.5))

plt.suptitle("Turtle Signal Freshness Filter — Walk-Forward Results\nCooldown bars after exit before re-entry allowed", fontsize=12, y=1.02)
plt.tight_layout()
plt.savefig("charts/turtle_freshness_filter_wf.png", dpi=150, bbox_inches="tight")
print("Saved charts/turtle_freshness_filter_wf.png")