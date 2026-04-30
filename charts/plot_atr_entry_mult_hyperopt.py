#!/usr/bin/env python3
"""
ATR_ENTRY_MULT hyperopt chart:
- Panel 1 (top): pass rate + Sharpe vs ATR_ENTRY_MULT (all 201 values)
- Panel 2 (bottom): log-scale equity curves — Baseline (EM=0.00), top Sharpe (EM=1.07), top Pass+Sharpe (EM=0.94)

Key context:
  EM=0.00  → current production default (no entry filter)
  EM=0.94  → highest pass+Sharpe candidate, but was REJECTED on held-out validation
  EM=1.07  → highest Sharpe (+7.86) but lower pass rate (75.9% vs 74.1% baseline)

Usage: python3 charts/plot_atr_entry_mult_hyperopt.py
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

KRYPT = "/home/ubuntu/.openclaw/workspace-krypto/krypto"

# ── 1. Load ────────────────────────────────────────────────────────────────
summary = pd.read_csv(f"{KRYPT}/snapshots/atr_entry_mult_current_summary.csv")
equity   = pd.read_csv(f"{KRYPT}/snapshots/atr_entry_mult_current_equity.csv")

# ── 2. Candidates ────────────────────────────────────────────────────────────
baseline_val  = 0.00
# Top by pass+Sharpe (robustness-first): EM=0.94 (REJECTED on held-out)
robust_val    = 0.94
# Top by Sharpe (ignoring pass rate): EM=1.07
# Sharpe-top: highest Sharpe in sane range (>74.1% pass baseline), use EM=1.30 cluster
sane = summary[(summary["avg_sharpe"] > 0) & (summary["avg_sharpe"] < 100)]
top_sharpe_val = float(sane.sort_values("avg_sharpe", ascending=False).iloc[0]["em"])

print(f"Baseline       : EM={baseline_val}  → PRODUCTION DEFAULT")
print(f"Robustness-top : EM={robust_val}    → REJECTED (held-out)")
print(f"Sharpe-top     : EM={top_sharpe_val}  → runner-up (lower pass)")

# ── 3. Equity aggregation ───────────────────────────────────────────────────
def agg_equity(df, em):
    """Geometric-mean equity across universes/windows at each bar."""
    sub = df[df["em"] == em].copy()
    sub["uw"] = sub["universe"] + "_W" + sub["window"].astype(str)
    piv = sub.pivot_table(index="bar_idx", columns="uw", values="equity", aggfunc="first")
    log_mean = np.log(piv.replace(0, np.nan)).mean(axis=1, skipna=True)
    return np.exp(log_mean)

base_equity = agg_equity(equity, baseline_val)
robust_equity = agg_equity(equity, robust_val)
sharpe_equity = agg_equity(equity, top_sharpe_val)

# ── 4. Figure ───────────────────────────────────────────────────────────────
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={"height_ratios": [1, 2]})
fig.suptitle("ATR_ENTRY_MULT Hyperopt — 201 Values × 9 Universes × 6 WF Windows",
             fontsize=14, fontweight="bold")

# Panel 1: pass rate + Sharpe vs EM
ax1b = ax1.twinx()
ax1.plot(summary["em"], summary["pass_pct"],  color="tab:blue",  linewidth=1.2, label="Pass Rate %")
ax1b.plot(summary["em"], summary["avg_sharpe"], color="tab:orange", linewidth=1.2, label="Avg Sharpe")

# Reference lines
for val, color, ls, label in [
    (baseline_val,  "tab:blue",  "--", "baseline (EM=0.00)"),
    (robust_val,    "tab:green", "--", "robust-winner (EM=0.94, held-out REJECTED)"),
    (top_sharpe_val,"tab:red",   ":",  f"sharpe-top (EM={top_sharpe_val})"),
]:
    ax1.axvline(val, color=color, linestyle=ls, alpha=0.7, linewidth=1.2, label=label)

# Annotate key points
for val, color, label_offset, label_text in [
    (baseline_val,  "tab:blue",  10,  "Baseline\nEM=0.00\nPass=74.1%"),
    (robust_val,    "tab:green", -18, "Robust EM=0.94\n(REJECTED held-out)"),
    (top_sharpe_val,"tab:red",   -8,  f"Sharpe EM={top_sharpe_val}\nPass=75.9%"),
]:
    row = summary[summary["em"] == val].iloc[0]
    ax1.annotate(label_text,
                 xy=(val, row["pass_pct"]),
                 xytext=(val + label_offset * 0.01, row["pass_pct"] - 5),
                 fontsize=7.5, color=color,
                 arrowprops=dict(arrowstyle="->", color=color, alpha=0.5))

ax1.set_ylabel("Pass Rate %", color="tab:blue")
ax1b.set_ylabel("Avg Sharpe", color="tab:orange")
ax1.set_xlabel("ATR_ENTRY_MULT")
ax1.set_xlim(0, 2.05)
ax1.tick_params(axis="y", labelcolor="tab:blue")
ax1b.tick_params(axis="y", labelcolor="tab:orange")
ax1.set_title("Top: Pass Rate (blue) & Sharpe (orange) vs ATR_ENTRY_MULT\nBottom: Aggregated Log Equity — Baseline vs Candidates")
lines1, labels1 = ax1.get_legend_handles_labels()
ax1.legend(lines1, labels1, loc="upper right", fontsize=7.5)
ax1.grid(True, alpha=0.3)

# Panel 2: equity curves (log scale, dynamic Y)
colors_map = {
    baseline_val:  "#1f77b4",
    robust_val:    "#2ca02c",
    top_sharpe_val:"#d62728",
}
labels_map = {
    baseline_val:  f"Baseline EM={baseline_val}  (PROD DEFAULT)",
    robust_val:    f"Robust EM={robust_val}        (held-out REJECTED)",
    top_sharpe_val: f"Sharpe-top EM={top_sharpe_val}  (runner-up)",
}

for em_val, eq_series in [
    (baseline_val,  base_equity),
    (top_sharpe_val, sharpe_equity),
    (robust_val,    robust_equity),
]:
    x = eq_series.index.values.astype(float)
    ls = "--" if em_val != baseline_val else "-"
    lw = 2.0  if em_val == top_sharpe_val else 1.4
    ax2.plot(x, eq_series.values, color=colors_map[em_val],
             linewidth=lw, linestyle=ls, label=labels_map[em_val], alpha=0.9)

ax2.set_yscale("log")
ax2.set_ylabel("Portfolio Equity (log scale)", fontsize=11)
ax2.set_xlabel("Bar Index (walk-forward aggregated, all 9 universes averaged)", fontsize=10)
ax2.set_title("Aggregated Log Equity — Baseline vs Candidate EM Values\n(geometric mean across universes/windows)", fontsize=9)
ax2.legend(loc="upper left", fontsize=9)
ax2.grid(True, alpha=0.3, which="both")

# Annotate final values
for em_val, eq_series in [
    (baseline_val,  base_equity),
    (top_sharpe_val, sharpe_equity),
    (robust_val,    robust_equity),
]:
    final = eq_series.iloc[-1]
    x_arr = eq_series.index.values.astype(float)
    row   = summary[summary["em"] == em_val].iloc[0]
    ax2.annotate(f"EM={em_val}: {final:.3f}x  Sharpe={row['avg_sharpe']:.2f}",
                 xy=(x_arr[-1], final),
                 xytext=(x_arr[-1] - 350, final * (0.82 if em_val == baseline_val else 0.90)),
                 fontsize=7.5, color=colors_map[em_val],
                 arrowprops=dict(arrowstyle="->", color=colors_map[em_val], alpha=0.6))

ax2.set_xlim(0, max(v.index.max() for v in [base_equity, sharpe_equity, robust_equity]) * 1.03)
fig.tight_layout()
out = f"{KRYPT}/charts/comparison_chart.png"
fig.savefig(out, dpi=150, bbox_inches="tight")
print(f"Saved: {out}")

# ── 5. Metrics ──────────────────────────────────────────────────────────────
print("\n=== Metric Summary ===")
print(f"{'Label':<30} {'EM':>5} {'Pass%':>7} {'Sharpe':>8} {'Ret%':>8} {'DD%':>7} {'Trades':>7}")
print("-" * 75)
for label, em_val in [
    ("Baseline (PROD DEFAULT)",  baseline_val),
    (f"Robustness-top (REJECTED)", robust_val),
    (f"Sharpe-top (runner-up)",     top_sharpe_val),
]:
    row = summary[summary["em"] == em_val].iloc[0]
    print(f"{label:<30} {em_val:>5.2f} {row['pass_pct']:>6.1f}% {row['avg_sharpe']:>8.3f} "
          f"{row['avg_ret_pct']:>7.1f}% {row['avg_dd']:>6.1f}% {int(row['total_trades']):>7}")