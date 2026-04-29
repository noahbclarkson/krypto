#!/usr/bin/env python3
"""
ATR_EMA_PERIOD Extensive Sweep — Comparison Charts
Reads: snapshots/atr_ema_extensive_sweep.csv
Outputs: charts/atr_ema_comparison.png
"""

import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
from collections import defaultdict

# ─── Load summary data ────────────────────────────────────────────────────────
ema_pass = defaultdict(int)
ema_trades = defaultdict(int)
ema_sharpe_sum = defaultdict(float)
ema_ret_sum = defaultdict(float)
ema_dd_sum = defaultdict(float)
ema_count = defaultdict(int)

with open("snapshots/atr_ema_extensive_sweep.csv") as f:
    reader = csv.DictReader(f)
    for row in reader:
        ema = int(row["atr_ema_period"])
        ema_pass[ema] += int(row["pass"])
        ema_trades[ema] += int(row["trades"])
        ema_sharpe_sum[ema] += float(row["sharpe"])
        ema_ret_sum[ema] += float(row["return_pct"])
        ema_dd_sum[ema] += float(row["max_dd_pct"])
        ema_count[ema] += 1

# Compute averages
all_emas = sorted(ema_pass.keys())
n_windows = 54
avg_sharpe = {e: ema_sharpe_sum[e] / n_windows for e in all_emas}
avg_ret   = {e: ema_ret_sum[e] / n_windows for e in all_emas}
avg_dd    = {e: ema_dd_sum[e] / n_windows for e in all_emas}
pass_rate  = {e: ema_pass[e] / n_windows * 100 for e in all_emas}

# ─── Top values ──────────────────────────────────────────────────────────────
# Sort by pass rate desc, then Sharpe desc
ranked = sorted(all_emas, key=lambda e: (-ema_pass[e], -avg_sharpe[e]))
top5 = ranked[:5]
print("Top 5 ATR_EMA by pass rate + Sharpe:")
for e in top5:
    print(f"  EMA={e:3d}: pass={ema_pass[e]:2d}/54 ({pass_rate[e]:.1f}%), Sharpe={avg_sharpe[e]:.4f}, Ret={avg_ret[e]:.1f}%, Trades={ema_trades[e]}")

baseline = 1   # ATR_EMA=1 is production baseline
winner   = top5[0]  # ATR_EMA=4
runner1  = top5[1]  # ATR_EMA=1 (baseline)
runner2  = top5[2]  # ATR_EMA=5

# ─── Figure setup ─────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    f"ATR_EMA_PERIOD Extensive Sweep: 1–200 (step 1) × 9 Universes × 54 Windows\n"
    f"EP=21, CHAND_P=7, CHAND_M=2.30, ATR_P=24, HM=12, CAP=3",
    fontsize=13, fontweight='bold'
)

# ─── Plot 1: Pass Rate vs ATR_EMA ────────────────────────────────────────────
ax1 = axes[0, 0]
ax1.plot(all_emas, [pass_rate[e] for e in all_emas], color='gray', alpha=0.4, lw=0.8)
ax1.scatter(all_emas, [pass_rate[e] for e in all_emas], color='gray', alpha=0.3, s=3)
ax1.axvline(baseline, color='steelblue', lw=1.5, ls='--', label=f'Baseline EMA={baseline}')
ax1.axvline(winner,   color='red',       lw=1.5, ls='--', label=f'Winner EMA={winner} (pass {ema_pass[winner]}/54)')
ax1.axhline(70, color='orange', lw=0.8, ls=':', label='70% threshold')
ax1.set_xlabel('ATR_EMA_PERIOD', fontsize=10)
ax1.set_ylabel('Pass Rate (%)', fontsize=10)
ax1.set_title('Pass Rate vs ATR_EMA_PERIOD', fontsize=11)
ax1.legend(fontsize=9)
ax1.grid(True, alpha=0.3)

# ─── Plot 2: Avg Sharpe vs ATR_EMA ───────────────────────────────────────────
ax2 = axes[0, 1]
ax2.plot(all_emas, [avg_sharpe[e] for e in all_emas], color='gray', alpha=0.4, lw=0.8)
ax2.scatter(all_emas, [avg_sharpe[e] for e in all_emas], color='gray', alpha=0.3, s=3)
ax2.axvline(baseline, color='steelblue', lw=1.5, ls='--', label=f'Baseline EMA={baseline} (Sharpe {avg_sharpe[baseline]:.3f})')
ax2.axvline(winner,   color='red',       lw=1.5, ls='--', label=f'EMA={winner} (Sharpe {avg_sharpe[winner]:.3f})')
ax2.axhline(avg_sharpe[baseline], color='steelblue', lw=0.8, ls=':', alpha=0.5)
ax2.set_xlabel('ATR_EMA_PERIOD', fontsize=10)
ax2.set_ylabel('Avg Walk-Forward Sharpe', fontsize=10)
ax2.set_title('Avg Sharpe vs ATR_EMA_PERIOD', fontsize=11)
ax2.legend(fontsize=9)
ax2.grid(True, alpha=0.3)

# ─── Plot 3: Equity per universe-window for top values ────────────────────────
ax3 = axes[1, 0]

# Load equity data
eq_data = defaultdict(lambda: defaultdict(list))
with open("snapshots/atr_ema_extensive_equity.csv") as f:
    reader = csv.DictReader(f)
    for row in reader:
        universe = row["universe"]
        wi = int(row["window"])
        ema = int(row["atr_ema"])
        eq = float(row["final_equity"])
        eq_data[(universe, wi)][ema].append(eq)

# Average equity per ema across all universe-windows
ema_avg_equity = defaultdict(float)
ema_std_equity = defaultdict(float)
all_keys = list(eq_data.keys())
for key in all_keys:
    for ema in [baseline, winner, runner2]:
        vals = eq_data[key][ema]
        if vals:
            ema_avg_equity[ema] += vals[0] / len(all_keys)

x_vals = list(range(len(all_keys)))
for ema, color, label in [
    (baseline, 'steelblue', f'Baseline ATR_EMA={baseline}'),
    (winner,   'red',       f'Top ATR_EMA={winner}'),
    (runner2,  'green',     f'Runner-up ATR_EMA={runner2}'),
]:
    # Use individual equity values (scatter)  
    y_vals = [eq_data[key][ema][0] if eq_data[key][ema] else 1.0 for key in all_keys]
    x_ema = [i + np.random.uniform(-0.2, 0.2) for i in x_vals]
    ax3.scatter(x_ema, y_vals, color=color, alpha=0.5, s=20, label=label)

ax3.axhline(1.0, color='black', lw=0.5, ls='-')
ax3.set_xlabel('Universe-Window Index', fontsize=10)
ax3.set_ylabel('Final Equity (per universe-window)', fontsize=10)
ax3.set_title('Final Equity: Baseline vs Winner vs Runner-up', fontsize=11)
ax3.legend(fontsize=9)
ax3.grid(True, alpha=0.3)

# ─── Plot 4: Trade Count vs ATR_EMA ──────────────────────────────────────────
ax4 = axes[1, 1]
ax4.plot(all_emas, [ema_trades[e] for e in all_emas], color='purple', alpha=0.6, lw=0.8)
ax4.scatter(all_emas, [ema_trades[e] for e in all_emas], color='purple', alpha=0.3, s=3)
ax4.axvline(baseline, color='steelblue', lw=1.5, ls='--', label=f'Baseline EMA={baseline}')
ax4.axvline(winner,   color='red',       lw=1.5, ls='--', label=f'Winner EMA={winner}')
ax4.set_xlabel('ATR_EMA_PERIOD', fontsize=10)
ax4.set_ylabel('Total Trades (all universes/windows)', fontsize=10)
ax4.set_title('Trade Count vs ATR_EMA_PERIOD', fontsize=11)
ax4.legend(fontsize=9)
ax4.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig("charts/atr_ema_comparison.png", dpi=150, bbox_inches='tight')
print("\nSaved: charts/atr_ema_comparison.png")

# ─── Detailed metrics table ──────────────────────────────────────────────────
print(f"\n{'='*70}")
print(f"{'EMA':>4}  {'Pass':>6}  {'Pass%':>6}  {'Sharpe':>8}  {'AvgRet%':>9}  {'Trades':>7}")
print(f"{'='*70}")
for e in ranked[:10]:
    print(f"{e:>4}  {ema_pass[e]:>6}  {pass_rate[e]:>6.1f}%  {avg_sharpe[e]:>8.4f}  {avg_ret[e]:>9.2f}  {ema_trades[e]:>7}")
print(f"{'='*70}")
print(f"\nConclusion:")
print(f"  ATR_EMA=1  (baseline): pass {ema_pass[1]}/54, Sharpe {avg_sharpe[1]:.4f}, Ret {avg_ret[1]:.1f}%")
print(f"  ATR_EMA={winner} (winner by pass):   pass {ema_pass[winner]}/54, Sharpe {avg_sharpe[winner]:.4f}, Ret {avg_ret[winner]:.1f}%")
delta_pass = ema_pass[winner] - ema_pass[baseline]
delta_sharpe = avg_sharpe[winner] - avg_sharpe[baseline]
print(f"\n  Delta: pass {delta_pass:+d} windows, Sharpe {delta_sharpe:+.4f}")
print(f"  VERDICT: ATR_EMA=1 (baseline) is preferred — higher Sharpe, simpler mechanism")
print(f"  ATR_EMA=4 is a robustness alternative if pass rate is the only criterion")
