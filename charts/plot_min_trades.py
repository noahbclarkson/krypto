#!/usr/bin/env python3
"""
Plot MIN_TRADES hyperopt results: comparison chart.

Reads:
  snapshots/turtle_min_trades_agg.csv  — aggregate stats per MT value
  snapshots/turtle_min_trades_equity.csv — equity time series per MT value

Generates:
  charts/turtle_min_trades_comparison.png — comparison chart
  charts/turtle_min_trades_pass_rate.png  — pass rate by MT
  charts/turtle_min_trades_heatmap.png    — pass rate heatmap (universe x MT)
"""
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.colors as mcolors
import numpy as np
import os

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

# ============================================================
# 1. Load aggregate results
# ============================================================
agg = pd.read_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_min_trades_agg.csv')
print(f"Aggregate shape: {agg.shape}")
print(agg.to_string())

# ============================================================
# 2. Determine winner: highest pass_pct, then highest avg_sharpe
# ============================================================
# Sort by pass_pct desc, then avg_sharpe desc
agg_sorted = agg.sort_values(['global_pass_pct', 'avg_sharpe'], ascending=[False, False])
print("\nSorted by pass_pct then sharpe:")
print(agg_sorted[['min_trades', 'global_pass', 'global_total', 'global_pass_pct', 'avg_sharpe']].to_string())

# Winner: highest pass_pct with acceptable avg_sharpe
best_row = agg_sorted.iloc[0]
winner_mt = int(best_row['min_trades'])
baseline_mt = 3  # current hardcoded default

print(f"\nBaseline MT={baseline_mt}, Winner MT={winner_mt}")
print(f"  Winner: {best_row['global_pass_pct']:.1f}% pass, Sharpe {best_row['avg_sharpe']:.3f}")
baseline_row = agg[agg['min_trades'] == baseline_mt].iloc[0]
print(f"  Baseline: {baseline_row['global_pass_pct']:.1f}% pass, Sharpe {baseline_row['avg_sharpe']:.3f}")

# Runner-ups: top 3 by pass_pct (excluding winner)
runners = agg_sorted.head(4)['min_trades'].tolist()
runners = [r for r in runners if r != winner_mt][:3]
print(f"  Runner-ups: {runners}")

# ============================================================
# 3. Load equity time-series
# ============================================================
eq_df = None
try:
    eq_df = pd.read_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_min_trades_equity.csv')
    print(f"\nEquity CSV shape: {eq_df.shape}")
    print(eq_df.head())
except Exception as e:
    print(f"Equity CSV not found or unparseable: {e}")
    eq_df = None

# ============================================================
# 4. Plot 1: Pass rate + Sharpe comparison (bar chart)
# ============================================================
fig, axes = plt.subplots(2, 1, figsize=(14, 12))

ax = axes[0]
x = np.arange(len(agg))
width = 0.35

colors = []
for mt in agg['min_trades']:
    if mt == winner_mt:
        colors.append('#E91E63')  # pink for winner
    elif mt == baseline_mt:
        colors.append('#2196F3')  # blue for baseline
    else:
        colors.append('#90A4AE')  # grey for others

bars_pass = ax.bar(x, agg['global_pass_pct'], width, color=colors, edgecolor='white', linewidth=0.5)
ax.set_ylabel('Pass Rate %')
ax.set_xlabel('MIN_TRADES')
ax.set_title('Turtle+Chandelier: Pass Rate by MIN_TRADES Threshold\n(Pink=WINNER, Blue=BASELINE, Grey=OTHER)', fontsize=13)
ax.set_xticks(x)
ax.set_xticklabels([str(int(v)) for v in agg['min_trades']])
ax.axhline(y=60, color='red', linestyle='--', alpha=0.5, label='60% threshold')
ax.axhline(y=agg['global_pass_pct'].max() * 0.95, color='green', linestyle=':', alpha=0.5)
ax.set_ylim(0, 105)
ax.legend()
ax.grid(True, axis='y', ls='--', alpha=0.3)

# Annotate winner
for bar, mt_val, pct in zip(bars_pass, agg['min_trades'], agg['global_pass_pct']):
    if mt_val == winner_mt:
        ax.annotate(f'WINNER\nMT={winner_mt}\n{pct:.1f}%', 
                    xy=(bar.get_x() + bar.get_width()/2, bar.get_height()),
                    ha='center', va='bottom', fontsize=9, color='#E91E63', fontweight='bold')
    elif mt_val == baseline_mt:
        ax.annotate(f'BASE\nMT={baseline_mt}\n{pct:.1f}%',
                    xy=(bar.get_x() + bar.get_width()/2, bar.get_height()),
                    ha='center', va='bottom', fontsize=9, color='#2196F3', fontweight='bold')

plt.tight_layout()
plt.savefig(f'{OUTDIR}/turtle_min_trades_pass_rate.png', dpi=150, bbox_inches='tight')
print(f"\nSaved {OUTDIR}/turtle_min_trades_pass_rate.png")
plt.close()

# ============================================================
# 5. Plot 2: Equity curve comparison (if available)
# ============================================================
if eq_df is not None and len(eq_df.columns) > 1:
    fig, ax = plt.subplots(figsize=(14, 8))
    
    day_col = eq_df.columns[0]
    mt_cols = [c for c in eq_df.columns if c.startswith('mt_')]
    
    # Normalize each to start at 1.0
    baseline_key = f'mt_{baseline_mt}'
    winner_key = f'mt_{winner_mt}'
    
    for col in mt_cols:
        mt_val = int(col.split('_')[1])
        vals = eq_df[col].dropna().astype(float)
        if len(vals) == 0:
            continue
        
        # Normalize to first valid value
        first_valid = vals.iloc[0] if vals.iloc[0] != 0 else 1.0
        if first_valid == 0:
            first_valid = 1.0
        norm_vals = vals / first_valid
        
        if mt_val == winner_mt:
            ax.plot(norm_vals.values, label=f'MT={mt_val} (WINNER)', 
                    color='#E91E63', linewidth=2.5, zorder=10)
        elif mt_val == baseline_mt:
            ax.plot(norm_vals.values, label=f'MT={mt_val} (BASELINE)', 
                    color='#2196F3', linewidth=2.0, zorder=9)
        elif mt_val in runners:
            ax.plot(norm_vals.values, label=f'MT={mt_val} (runner-up)', 
                    color='#4CAF50', linewidth=1.5, alpha=0.8)
        else:
            ax.plot(norm_vals.values, label=None, 
                    color='#B0BEC5', linewidth=0.5, alpha=0.4)
    
    ax.set_yscale('log')
    ax.set_xlabel('Window # (chronological)')
    ax.set_ylabel('Normalized Equity (log scale)')
    ax.set_title(f'Turtle+Chandelier: Equity Curves by MIN_TRADES\n(Winner MT={winner_mt}, Baseline MT={baseline_mt})', fontsize=13)
    ax.legend(loc='upper left')
    ax.grid(True, which='both', ls='--', alpha=0.3)
    
    plt.tight_layout()
    plt.savefig(f'{OUTDIR}/turtle_min_trades_comparison.png', dpi=150, bbox_inches='tight')
    print(f"Saved {OUTDIR}/turtle_min_trades_comparison.png")
    plt.close()
else:
    print("Equity CSV not available — skipping equity comparison chart")

# ============================================================
# 6. Plot 3: Heatmap — pass rate per universe × MT
# ============================================================
sweep_df = pd.read_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_min_trades_sweep.csv')
# pivot: universe × min_trades -> pass rate
pivot = sweep_df.groupby(['universe', 'min_trades'])['pass'].mean().unstack()
pivot = pivot.fillna(0) * 100

fig, ax = plt.subplots(figsize=(14, 8))
im = ax.imshow(pivot.values, cmap='RdYlGn', aspect='auto', vmin=0, vmax=100)

ax.set_xticks(np.arange(len(pivot.columns)))
ax.set_yticks(np.arange(len(pivot.index)))
ax.set_xticklabels([str(c) for c in pivot.columns], fontsize=9)
ax.set_yticklabels(pivot.index, fontsize=9)
ax.set_xlabel('MIN_TRADES')
ax.set_ylabel('Universe')
ax.set_title('Turtle+Chandelier: Pass Rate % by Universe × MIN_TRADES\n(Green=PASS, Red=FAIL)', fontsize=13)

# Add text annotations
for i in range(len(pivot.index)):
    for j in range(len(pivot.columns)):
        val = pivot.values[i, j]
        color = 'white' if val < 40 or val > 80 else 'black'
        ax.text(j, i, f'{val:.0f}', ha='center', va='center', 
                color=color, fontsize=7, fontweight='bold')

plt.colorbar(im, ax=ax, label='Pass Rate %')
plt.tight_layout()
plt.savefig(f'{OUTDIR}/turtle_min_trades_heatmap.png', dpi=150, bbox_inches='tight')
print(f"Saved {OUTDIR}/turtle_min_trades_heatmap.png")
plt.close()

# ============================================================
# Summary table
# ============================================================
print("\n=== MIN_TRADES SWEEP SUMMARY ===")
print(f"Winner: MT={winner_mt} ({best_row['global_pass_pct']:.1f}% pass, Sharpe {best_row['avg_sharpe']:.3f})")
print(f"Baseline: MT={baseline_mt} ({baseline_row['global_pass_pct']:.1f}% pass, Sharpe {baseline_row['avg_sharpe']:.3f})")
delta_pass = best_row['global_pass_pct'] - baseline_row['global_pass_pct']
delta_sh = best_row['avg_sharpe'] - baseline_row['avg_sharpe']
print(f"Delta: {delta_pass:+.1f}pp pass, {delta_sh:+.3f} Sharpe")
print(f"\nRecommendation: {'Update MIN_TRADES to ' + str(winner_mt) if winner_mt != baseline_mt else 'Keep MIN_TRADES=3 (no improvement found)'}")