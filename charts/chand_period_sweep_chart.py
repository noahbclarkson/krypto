#!/usr/bin/env python3
"""
CHAND_PERIOD Sweep — Equity Curve Comparison Chart
Reads: snapshots/chand_period_sweep_equity.csv
Output: charts/chand_period_sweep_comparison.png

Generates equity curve comparison for:
- Baseline (CP=15 — current config.rs value)
- Winner (best CP by avg Sharpe)
- Runner-ups (2nd and 3rd place by avg Sharpe)
- All CP values (lighter lines in background)
"""

import sys
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from pathlib import Path

# ── Config ────────────────────────────────────────────────────────────────────
EQUITY_CSV = Path("snapshots/chand_period_sweep_equity.csv")
SWEEP_CSV  = Path("snapshots/chand_period_sweep.csv")
OUT_PNG    = Path("charts/chand_period_sweep_comparison.png")
KRAKTO_PATH = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto")

EQUITY_CSV = KRAKTO_PATH / "snapshots/chand_period_sweep_equity.csv"
SWEEP_CSV  = KRAKTO_PATH / "snapshots/chand_period_sweep.csv"
OUT_PNG    = KRAKTO_PATH / "charts/chand_period_sweep_comparison.png"

# ── Load data ─────────────────────────────────────────────────────────────────
print(f"Loading equity curves from {EQUITY_CSV}...")
eq_df = pd.read_csv(EQUITY_CSV)
print(f"  Loaded {len(eq_df)} rows, CPs: {sorted(eq_df['cp'].unique())}")

print(f"Loading sweep stats from {SWEEP_CSV}...")
sw_df = pd.read_csv(SWEEP_CSV)

# ── Compute per-CP global stats (for ranking) ─────────────────────────────────
cp_stats = (
    sw_df.groupby('cp')
    .agg(
        n_pass=('pass', 'sum'),
        total=('pass', 'count'),
        avg_sharpe=('sharpe', 'mean'),
        avg_ret=('return_pct', 'mean'),
        worst_dd=('max_dd_pct', 'max'),
        total_trades=('trades', 'sum'),
    )
    .reset_index()
)
cp_stats['n_pass_pct'] = cp_stats['n_pass'] / cp_stats['total'] * 100
cp_stats = cp_stats.sort_values('avg_sharpe', ascending=False).reset_index(drop=True)

print("\n=== Per-CP Rankings (by avg Sharpe) ===")
print(cp_stats[['cp','avg_sharpe','n_pass_pct','avg_ret','worst_dd','total_trades']].to_string(index=False))

# ── Identify key configs ──────────────────────────────────────────────────────
baseline_cp = 15  # current config.rs value

# Find winner + runner-ups (top 3 by avg Sharpe)
top3 = cp_stats.head(3)['cp'].tolist()
winner_cp   = top3[0]
runner1_cp  = top3[1]
runner2_cp  = top3[2]

print(f"\nBaseline:  CP={baseline_cp}")
print(f"Winner:    CP={winner_cp}")
print(f"Runner-up: CP={runner1_cp}")
print(f"Runner-up: CP={runner2_cp}")

# ── Pivot equity curves ───────────────────────────────────────────────────────
# Each cp has a multi-segment equity curve (one segment per universe/window).
# We aggregate multiplicatively across all windows: equity_final[cp] = product of all window equities.
# But the CSV already aggregates via multiplicative product in Rust.
# We just need the final (last bar) equity per cp as the single "portfolio equity" value per bar.

# The equity CSV has (cp, bar, equity) — multiple segments concatenated.
# We need to rebuild a continuous equity curve per cp.
# Since windows are sequential, we can take the last equity per (cp, bar).
# Actually: the Rust code does multiplicative product across windows sequentially.
# So we just need: for each cp, one row per bar index (the last value at that bar).

pivot_eq = (
    eq_df.groupby(['cp', 'bar'])['equity']
    .last()  # if there are duplicate (cp, bar) rows, take the last (shouldn't happen but safety)
    .unstack(level='cp')
)

# Forward-fill any gaps (windows have different lengths)
pivot_eq = pivot_eq.ffill(axis=1).bfill(axis=1)

print(f"\nEquity matrix: {pivot_eq.shape[0]} bars × {pivot_eq.shape[1]} CPs")

# ── Plot ──────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10), sharex=False)
fig.suptitle(
    f"CHAND_PERIOD Sweep — Equity Curve Comparison\n"
    f"CHAND_MULT=2.25 | EP=21 | ATR_P=24 | ATR_M=2.0 | 9 Universes Walk-Forward",
    fontsize=13, fontweight='bold'
)

# Color scheme
all_cps = sorted(eq_df['cp'].unique())
cmap = plt.colormaps.get('coolwarm')
cp_color = {cp: cmap(i / max(len(all_cps)-1, 1)) for i, cp in enumerate(all_cps)}

# --- Top panel: Log-scale equity curves ---
ax1 = axes[0]
x = pivot_eq.index.values

# Plot all CPs as background (thin lines)
for cp in all_cps:
    if cp in pivot_eq.columns:
        y = pivot_eq[cp].values
        if cp in [baseline_cp, winner_cp, runner1_cp, runner2_cp]:
            continue  # plot these highlighted separately
        ax1.plot(x, y, color='lightgray', linewidth=0.5, alpha=0.4)

# Highlighted configs
highlight_configs = [
    (baseline_cp, 'steelblue',   f'Baseline CP={baseline_cp}',    2.0),
    (winner_cp,   'darkgreen',   f'Winner CP={winner_cp}',         2.5),
    (runner1_cp,  'darkorange',   f'Runner-up CP={runner1_cp}',    2.0),
    (runner2_cp,  'purple',       f'Runner-up CP={runner2_cp}',    1.8),
]

for cp, color, label, lw in highlight_configs:
    if cp in pivot_eq.columns:
        y = pivot_eq[cp].values
        ax1.plot(x, y, color=color, linewidth=lw, label=label, zorder=10)

ax1.set_yscale('log')
ax1.set_ylabel('Portfolio Equity (× initial)', fontsize=11)
ax1.set_title('Equity Curves — Log Scale', fontsize=11)
ax1.grid(True, alpha=0.3, which='both')
ax1.legend(loc='upper left', fontsize=9)
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}x'))

# Add a subtle annotation for final values
for cp, color, label, lw in highlight_configs:
    if cp in pivot_eq.columns:
        final_val = pivot_eq[cp].iloc[-1]
        ax1.annotate(f'{final_val:.1f}x', xy=(len(x)-1, final_val),
                     fontsize=8, color=color, va='bottom', ha='right')

# --- Bottom panel: Sharpe by CP (bar chart) ─────────────────────────────────
ax2 = axes[1]

colors_bar = [
    'steelblue' if cp == baseline_cp else
    'darkgreen'  if cp == winner_cp else
    'darkorange' if cp == runner1_cp else
    'purple'     if cp == runner2_cp else
    'lightgray'
    for cp in cp_stats['cp'].values
]

bars = ax2.bar(cp_stats['cp'].astype(str), cp_stats['avg_sharpe'],
              color=colors_bar, edgecolor='none', width=0.7, alpha=0.8)

ax2.axhline(0, color='black', linewidth=0.5)
ax2.set_xlabel('CHAND_PERIOD', fontsize=11)
ax2.set_ylabel('Avg Walk-Forward Sharpe', fontsize=11)
ax2.set_title('Average Sharpe by CHAND_PERIOD', fontsize=11)
ax2.grid(True, alpha=0.3, axis='y')

# Annotate winner
if winner_cp in cp_stats['cp'].values:
    win_row = cp_stats[cp_stats['cp'] == winner_cp].iloc[0]
    ax2.annotate(
        f'Winner\nCP={winner_cp}\nSharpe={win_row["avg_sharpe"]:.3f}\n{win_row["n_pass"]}/{win_row["total"]} pass ({win_row["n_pass_pct"]:.0f}%)',
        xy=(str(winner_cp), win_row['avg_sharpe']),
        xytext=(str(winner_cp), win_row['avg_sharpe'] + 0.15),
        fontsize=8, color='darkgreen', ha='center',
        arrowprops=dict(arrowstyle='->', color='darkgreen', lw=1)
    )

# Annotate baseline
if baseline_cp in cp_stats['cp'].values:
    base_row = cp_stats[cp_stats['cp'] == baseline_cp].iloc[0]
    ax2.annotate(
        f'Baseline\nCP={baseline_cp}\nSharpe={base_row["avg_sharpe"]:.3f}',
        xy=(str(baseline_cp), base_row['avg_sharpe']),
        xytext=(str(baseline_cp), base_row['avg_sharpe'] - 0.1),
        fontsize=8, color='steelblue', ha='center',
        arrowprops=dict(arrowstyle='->', color='steelblue', lw=1)
    )

plt.tight_layout()
OUT_PNG.parent.mkdir(parents=True, exist_ok=True)
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
print(f"\nSaved: {OUT_PNG}")

# ── Print key metrics table ────────────────────────────────────────────────────
print("\n=== Key Configs Summary ===")
print(f"{'Config':<20} {'CP':>4} {'Sharpe':>8} {'Pass':>8} {'AvgRet':>10} {'WorstDD':>10}")
print("-" * 65)
for cp in [baseline_cp, winner_cp, runner1_cp, runner2_cp]:
    row = cp_stats[cp_stats['cp'] == cp]
    if len(row) > 0:
        r = row.iloc[0]
        config_label = 'Baseline' if cp == baseline_cp else ('Winner' if cp == winner_cp else f'Runner-up{"" if cp==runner1_cp else "2"}')
        print(f"{config_label:<20} {cp:>4} {r['avg_sharpe']:>+8.4f} {r['n_pass']:>3}/{r['total']:<4} ({r['n_pass_pct']:>5.1f}%) {r['avg_ret']:>+10.2f}% {r['worst_dd']:>10.2f}%")

plt.close()
print("\nDone.")
