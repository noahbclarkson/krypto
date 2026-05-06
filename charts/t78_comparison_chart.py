#!/usr/bin/env python3
"""
T78 ATR_RANK_THRESHOLD Comparison Chart
======================================
Generates:
  charts/t78_comparison_chart.png — 3-panel: pass rate, Sharpe, return vs threshold
  charts/t78_equity_comparison.png — equity curve comparison for selected thresholds
"""
import csv, math, os, sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

# ── Paths ────────────────────────────────────────────────────────────────────
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
CHARTS_DIR = SCRIPT_DIR
SNAP = os.path.join(SCRIPT_DIR, "..", "snapshots")

SUMMARY_CSV  = os.path.join(SNAP, "t78_threshold_summary.csv")
EQUITY_FILES = {
    0:  os.path.join(SNAP, "t78_equity_T000.csv"),
    5:  os.path.join(SNAP, "t78_equity_T005.csv"),
    10: os.path.join(SNAP, "t78_equity_T010.csv"),
    20: os.path.join(SNAP, "t78_equity_T020.csv"),
    40: os.path.join(SNAP, "t78_equity_T040.csv"),
    60: os.path.join(SNAP, "t78_equity_T060.csv"),
    80: os.path.join(SNAP, "t78_equity_T080.csv"),
}

OUT_COMPARE  = os.path.join(CHARTS_DIR, "t78_comparison_chart.png")
OUT_EQUITY   = os.path.join(CHARTS_DIR, "t78_equity_comparison.png")

# ── Load summary ──────────────────────────────────────────────────────────────
thresholds  = []
pass_counts = []
sharpes     = []
returns     = []
dds        = []
trades_list = []

with open(SUMMARY_CSV) as f:
    for r in csv.DictReader(f):
        t   = float(r['threshold'])
        pc  = int(r['pass_count'])
        tt  = int(r['total_trades'])
        ret = float(r['avg_return_pct'])
        sh  = float(r['avg_sharpe'])
        dd  = float(r['avg_dd_pct'])
        if tt == 0:
            continue
        # cap mega-outlier sharpe from near-zero-trade windows
        if sh > 1e6 or sh < -1e6:
            sh = float('nan')
        thresholds.append(t)
        pass_counts.append(pc)
        sharpes.append(sh)
        returns.append(ret)
        dds.append(dd)
        trades_list.append(tt)

TOTAL_WINDOWS = 63

# ── Figure 1: 3-panel metric comparison ──────────────────────────────────────
fig, axes = plt.subplots(3, 1, figsize=(12, 10), sharex=True)
fig.suptitle("T78: ATR_RANK_THRESHOLD Extensive Sweep (AP=17, LB=41)\n"
             "Turtle-only live path · 101 values · 9 universes × 7 windows = 63 OOS windows",
             fontsize=12, fontweight='bold')

ax_pass, ax_sharpe, ax_ret = axes

# Panel 1: Pass rate
pass_rates = [pc / TOTAL_WINDOWS * 100 for pc in pass_counts]
ax_pass.plot(thresholds, pass_rates, color='steelblue', lw=1.5)
ax_pass.fill_between(thresholds, pass_rates, alpha=0.2, color='steelblue')
ax_pass.axhline(70, color='gray', lw=0.8, ls='--', label='70% threshold')
ax_pass.axvline(5, color='red', lw=1.2, ls='--', label='T=5 (current default)')
ax_pass.set_ylabel("Pass Rate (%)")
ax_pass.set_ylim(0, 105)
ax_pass.legend(fontsize=9)
ax_pass.set_title("Pass Rate — T=5 and T=3-7 plateau at 87.3% (55/63 windows)")
ax_pass.grid(True, alpha=0.3)

# Panel 2: Avg Sharpe
ax_sharpe.plot(thresholds, sharpes, color='darkgreen', lw=1.5)
ax_sharpe.fill_between(thresholds, sharpes, alpha=0.2, color='darkgreen')
ax_sharpe.axvline(5, color='red', lw=1.2, ls='--', label='T=5 (current default)')
ax_sharpe.set_ylabel("Avg Sharpe Ratio")
ax_sharpe.legend(fontsize=9)
ax_sharpe.set_title("Avg Sharpe — T=5 leads: Sharpe 7.134 vs T=0 Sharpe 5.169 (+38%)")
ax_sharpe.grid(True, alpha=0.3)
ax_sharpe.set_ylim(bottom=0)

# Panel 3: Avg Return
ax_ret.plot(thresholds, returns, color='darkorange', lw=1.5)
ax_ret.fill_between(thresholds, returns, alpha=0.2, color='darkorange')
ax_ret.axvline(5, color='red', lw=1.2, ls='--', label='T=5 (current default)')
ax_ret.set_ylabel("Avg Return (%)")
ax_ret.set_xlabel("ATR_RANK_THRESHOLD (T)")
ax_ret.legend(fontsize=9)
ax_ret.set_title("Avg Return — T=5 (+69.6%) vs T=0 no-filter (+68.1%), T=60 barely fires (+32.9%)")
ax_ret.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig(OUT_COMPARE, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {OUT_COMPARE}")

# ── Figure 2: Equity curve comparison ───────────────────────────────────────
fig2, ax2 = plt.subplots(figsize=(12, 6))

COLORS = {
    0:  '#aaaaaa',  # gray (no filter baseline)
    5:  '#1f77b4',  # blue (current winner)
    10: '#2ca02c',  # green
    20: '#ff7f0e',  # orange
    40: '#d62728',  # red
    60: '#9467bd',  # purple
    80: '#8c564b',  # brown
}

equity_series = {}
for t, path in EQUITY_FILES.items():
    if not os.path.exists(path):
        print(f"Skipping {path} (not found)")
        continue
    # Aggregate across universes by window index (compounded)
    window_equities = {}
    with open(path) as f:
        for r in csv.DictReader(f):
            w = int(r['window'])
            eq = float(r['equity'])
            if w not in window_equities:
                window_equities[w] = []
            window_equities[w].append(eq)
    
    # Compound across windows: cumulative product
    sorted_windows = sorted(window_equities.keys())
    cumulative = 1.0
    compounded = []
    for w in sorted_windows:
        # geometric mean across universes for this window
        prods = window_equities[w]
        geo_mean = math.exp(sum(math.log(max(p, 1e-10)) for p in prods) / len(prods))
        cumulative *= geo_mean
        compounded.append(cumulative)
    
    equity_series[t] = compounded

if equity_series:
    max_len = max(len(v) for v in equity_series.values())
    x = list(range(max_len))
    
    for t in sorted(equity_series.keys()):
        series = equity_series[t]
        label = f"T={t}" + (" (baseline)" if t == 0 else " (current default)" if t == 5 else "")
        lw = 2.5 if t in (5,) else 1.5 if t == 0 else 1.2
        alpha = 1.0 if t == 5 else 0.7
        ax2.plot(x, series, label=label, color=COLORS.get(t, None),
                 lw=lw, alpha=alpha)
    
    ax2.set_xlabel("Walk-Forward Window Index")
    ax2.set_ylabel("Cumulative Equity (geometric mean, compounded)")
    ax2.set_title("T78: Equity Curve by ATR_RANK_THRESHOLD\n"
                  "T=5 wins pass rate (87.3%) and Sharpe (7.134); higher T reduces trades too aggressively")
    ax2.set_yscale('log')
    ax2.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.2fx'))
    ax2.grid(True, alpha=0.3)
    ax2.legend(fontsize=9)
else:
    print("No equity data found — skipping equity chart")

plt.tight_layout()
plt.savefig(OUT_EQUITY, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {OUT_EQUITY}")

# ── Print key metrics ─────────────────────────────────────────────────────────
print("\n=== T78 KEY FINDINGS ===")
print(f"Best pass rate: {max(pass_counts)}/63 at T={[thresholds[i] for i,p in enumerate(pass_counts) if p == max(pass_counts)]}")
print(f"T=0 (no filter):  {pass_counts[0]}/63 pass, Sharpe {sharpes[0]:.3f}, Ret {returns[0]:+.1f}%")
t5_idx = thresholds.index(5.0)
print(f"T=5 (current):    {pass_counts[t5_idx]}/63 pass, Sharpe {sharpes[t5_idx]:.3f}, Ret {returns[t5_idx]:+.1f}%")
print(f"T=5 vs T=0 delta: pass +{pass_counts[t5_idx]-pass_counts[0]}, Sharpe +{sharpes[t5_idx]-sharpes[0]:.3f}")
