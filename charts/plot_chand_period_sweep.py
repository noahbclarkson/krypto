#!/usr/bin/env python3
import csv, os, sys
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

sweep_path = "snapshots/chand_period_sweep.csv"
equity_path = "snapshots/chand_period_equity_curves.csv"
out_path = "charts/chand_period_comparison.png"
BASELINE_CP = 28

results = []
with open(sweep_path) as f:
    reader = csv.DictReader(f)
    for row in reader:
        results.append({
            "cp": int(row["chand_period"]),
            "pass_rate": float(row["pass_rate"]),
            "sharpe": float(row["avg_sharpe"]),
            "ret": float(row["avg_return_pct"]),
            "dd": float(row["avg_max_dd_pct"]),
            "trades": int(row["total_trades"]),
        })

equity_data = {}
if os.path.exists(equity_path):
    with open(equity_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            cp = int(row["chand_period"])
            wi = int(row["window"])
            bar = int(row["bar"])
            eq = float(row["equity"])
            if cp not in equity_data:
                equity_data[cp] = {}
            if wi not in equity_data[cp]:
                equity_data[cp][wi] = []
            while len(equity_data[cp][wi]) <= bar:
                equity_data[cp][wi].append(None)
            equity_data[cp][wi][bar] = eq

sorted_res = sorted(results, key=lambda r: r['cp'])
winner_cp = sorted(results, key=lambda r: (-r['pass_rate'], -r['sharpe']))[0]['cp']
baseline_pr = next(r['pass_rate'] for r in sorted_res if r['cp'] == BASELINE_CP)

fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(f"CHAND_PERIOD Hyperopt Sweep — Base5, 6 Windows (252/252)\nFrozen: EP=21, ATR_P=24, ATR_M=2.0, CHAND_M=2.15, HM=45, CAP=3",
             fontsize=13, fontweight='bold')

ax_pass  = axes[0, 0]
ax_sharp = axes[0, 1]
ax_ret   = axes[1, 0]
ax_eq    = axes[1, 1]

cps = [r['cp'] for r in sorted_res]
pass_rates = [r['pass_rate'] for r in sorted_res]
colors = [(0.05, 0.35, 0.85) if r['cp'] == winner_cp else
          (0.0, 0.6, 0.0)    if r['cp'] == BASELINE_CP else
          (0.7, 0.7, 0.7)    for r in sorted_res]

ax_pass.bar(cps, pass_rates, width=0.8, color=colors, edgecolor='white', linewidth=0.5)
ax_pass.axhline(baseline_pr, color='darkgreen', linestyle='--', linewidth=1.2, alpha=0.7, label=f'Baseline CP={BASELINE_CP} ({baseline_pr:.0f}%)')
ax_pass.set_xlabel("CHAND_PERIOD", fontsize=11)
ax_pass.set_ylabel("Pass Rate (%)", fontsize=11)
ax_pass.set_title("Pass Rate by CHAND_PERIOD", fontsize=12, fontweight='bold')
ax_pass.set_ylim(0, 110)
ax_pass.yaxis.set_major_formatter(mticker.PercentFormatter())
ax_pass.legend(fontsize=9)
ax_pass.grid(axis='y', alpha=0.3)

sharpes = [r['sharpe'] for r in sorted_res]
ax_sharp.plot(cps, sharpes, 'b-', linewidth=2, marker='o', markersize=4, label='Avg Sharpe')
baseline_sh = next(r['sharpe'] for r in sorted_res if r['cp'] == BASELINE_CP)
winner_sh = next(r['sharpe'] for r in sorted_res if r['cp'] == winner_cp)
ax_sharp.axhline(baseline_sh, color='darkgreen', linestyle='--', alpha=0.7, label=f'Baseline CP={BASELINE_CP} ({baseline_sh:.2})')
ax_sharp.scatter([winner_cp], [winner_sh], color='red', s=120, zorder=5, label=f'Winner CP={winner_cp} ({winner_sh:.2})')
ax_sharp.set_xlabel("CHAND_PERIOD", fontsize=11)
ax_sharp.set_ylabel("Avg OOS Sharpe", fontsize=11)
ax_sharp.set_title("Average Sharpe Ratio", fontsize=12, fontweight='bold')
ax_sharp.legend(fontsize=9)
ax_sharp.grid(alpha=0.3)

rets = [r['ret'] for r in sorted_res]
ax_ret.bar(cps, rets, width=0.8, color=colors, edgecolor='white', linewidth=0.5)
baseline_ret = next(r['ret'] for r in sorted_res if r['cp'] == BASELINE_CP)
ax_ret.axhline(baseline_ret, color='darkgreen', linestyle='--', linewidth=1.2, alpha=0.7)
ax_ret.set_xlabel("CHAND_PERIOD", fontsize=11)
ax_ret.set_ylabel("Avg Return (%)", fontsize=11)
ax_ret.set_title("Average Return", fontsize=12, fontweight='bold')
ax_ret.grid(axis='y', alpha=0.3)

if equity_data:
    selected_cps = [r['cp'] for r in sorted(results, key=lambda x: (-x['pass_rate'], -x['sharpe']))[:5]]
    if BASELINE_CP not in selected_cps:
        selected_cps.append(BASELINE_CP)
    selected_cps = sorted(set(selected_cps))
    def color_for_rank(i, total):
        if i == 0: return (0.05, 0.35, 0.85)
        if i == 1: return (0.1, 0.55, 0.85)
        if i == 2: return (0.15, 0.7, 0.85)
        if i == 3: return (0.2, 0.8, 0.5)
        return (0.5, 0.5, 0.5)
    for cp in selected_cps:
        if cp not in equity_data:
            continue
        windows = equity_data[cp]
        n_win = len(windows)
        max_bar = 0
        for wi in range(n_win):
            max_bar = max(max_bar, len(windows[wi]))
        max_bar = min(max_bar, 400)
        aggregated = []
        for bi in range(max_bar):
            vals = []
            for wi in range(n_win):
                eq_list = windows[wi]
                if bi < len(eq_list) and eq_list[bi] is not None:
                    vals.append(eq_list[bi])
            if vals:
                aggregated.append(sum(vals) / len(vals))
            else:
                aggregated.append(None)
        plot_vals = [v for v in aggregated if v is not None and v > 0]
        if not plot_vals:
            continue
        rank = selected_cps.index(cp)
        col = color_for_rank(rank, len(selected_cps))
        label = f"CP={cp}"
        if cp == winner_cp:
            label += " (WINNER)"
        elif cp == BASELINE_CP:
            label += " (BASELINE)"
        ax_eq.plot(range(len(aggregated)), aggregated, color=col, linewidth=1.5, label=label, alpha=0.85)
    ax_eq.set_xlabel("Bar (in-window)", fontsize=11)
    ax_eq.set_ylabel("Portfolio Equity", fontsize=11)
    ax_eq.set_title("Aggregated Equity Curves (avg across windows)", fontsize=12, fontweight='bold')
    ax_eq.set_yscale('log')
    ax_eq.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}x' if v >= 1 else f'{v:.2f}'))
    ax_eq.legend(fontsize=9, loc='upper left')
    ax_eq.grid(alpha=0.3)
    ax_eq.set_ylim(bottom=0.8)
else:
    ax_eq.text(0.5, 0.5, "No equity curve data available.\nRun Rust harness to generate data.",
               ha='center', va='center', transform=ax_eq.transAxes, fontsize=13, color='gray')

plt.tight_layout(rect=[0, 0, 1, 0.96])
os.makedirs("charts", exist_ok=True)
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Chart saved: {out_path}")

print("\n=== CHAND_PERIOD SWEEP SUMMARY ===")
print(f"{'Rank':>4} {'CP':>4} {'Pass%':>6} {'Sharpe':>7} {'Return':>8} {'DD%':>5} {'Trades':>6} {'Note':>10}")
print("-" * 62)
for i, r in enumerate(sorted(results, key=lambda x: (-x['pass_rate'], -x['sharpe']))[:10]):
    note = "← WINNER" if i == 0 else ("← BASELINE" if r['cp'] == BASELINE_CP else "")
    print(f"{i+1:>4} {r['cp']:>4} {r['pass_rate']:>6.0f} {r['sharpe']:>7.2f} {r['ret']:>+8.1f} {r['dd']:>5.1f} {r['trades']:>6} {note:>10}")