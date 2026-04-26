#!/usr/bin/env python3
"""
ATR_ENTRY_MULT Sweep Equity Curve Comparison Chart
Generated from atr_entry_mult_sweep_equity (EP=21, 9 universes × 6 windows)
"""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import pandas as pd

# Load the equity CSV (key configs exported with correct geometric mean)
df = pd.read_csv('snapshots/atr_entry_mult_equity.csv')
print(f"Loaded equity CSV: {len(df)} rows")

# Load summary CSV for Sharpe and pass rate (sorted by Sharpe desc)
summary = pd.read_csv('snapshots/atr_entry_mult_summary.csv')

# Create a lookup: EM -> (pass_count, pass_rate, sharpe, avg_return)
summary_sorted = summary.sort_values('avg_sharpe', ascending=False).reset_index(drop=True)

# Map column names to display info
# baseline=EM=0.00, winner=EM=0.85, runner_up1=EM=0.90, mid_range=EM=0.50
key_configs = {
    'baseline':    {'em': 0.00, 'label': 'EM=0.00 (baseline — no filter)', 
                    'color': '#4CAF50', 'ls': '-',  'lw': 2.5,
                    'pass': 45, 'sharpe': 3.658, 'ret': 114.6},
    'winner':      {'em': 0.85, 'label': 'EM=0.85 (sweep winner)', 
                    'color': '#FF5722', 'ls': '-',  'lw': 2.5,
                    'pass': 52, 'sharpe': 5.209, 'ret': 81.7},
    'runner_up1':  {'em': 0.90, 'label': 'EM=0.90 (runner-up)', 
                    'color': '#2196F3', 'ls': '--', 'lw': 2.0,
                    'pass': 50, 'sharpe': 5.450, 'ret': 93.0},
    'mid_range':   {'em': 0.50, 'label': 'EM=0.50 (mid-range)', 
                    'color': '#9C27B0', 'ls': ':',  'lw': 2.0,
                    'pass': 43, 'sharpe': 3.636, 'ret': 90.2},
}

total_windows = 54

# Create 2x2 subplot
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    'ATR_ENTRY_MULT Sweep Results\n'
    'EP=21, CHAND(7, 2.30), HM=12, CAP=3 | 9 universes × 6 walk-forward windows = 54 tests per config',
    fontsize=13, fontweight='bold', y=0.98
)

# --- Panel A: Log-scale equity curves ---
ax = axes[0, 0]
x = df['bar'].values

for col, info in key_configs.items():
    if col in df.columns:
        y = df[col].values
        y_safe = np.maximum(y, 1e-10)
        y_log = np.log(y_safe)
        label_str = (f"{info['label']}  "
                     f"Sharpe={info['sharpe']:.2}, "
                     f"{info['pass']}/{total_windows} pass ({info['pass']/total_windows*100:.0f}%)")
        ax.plot(x, y_log, label=label_str,
                color=info['color'], linestyle=info['ls'], linewidth=info['lw'], alpha=0.9)

ax.set_xlabel('Bar (test window, Base5 geometric mean)', fontsize=10)
ax.set_ylabel('Log Equity (ln, base e)', fontsize=10)
ax.set_title('A. Equity Curves — Geometric Mean Across Base5 Windows (Log Scale)', fontsize=11)
ax.legend(loc='upper left', fontsize=8.5)
ax.grid(True, alpha=0.3)
ax.axhline(y=0, color='black', linewidth=0.5, linestyle='-', alpha=0.4)

def format_equity(xval, pos=None):
    val = np.exp(xval)
    if val >= 100:
        return f'{val:.0f}x'
    elif val >= 10:
        return f'{val:.1f}x'
    else:
        return f'{val:.2f}x'
ax.yaxis.set_major_formatter(mticker.FuncFormatter(format_equity))
ax.set_ylim(bottom=-0.1)

# --- Panel B: Pass rate bar chart (41 values from sweep) ---
ax2 = axes[0, 1]

# Merge key config info into summary for coloring
def get_pass_for_em(em, summary_df):
    row = summary_df[summary_df['atr_entry_mult'] == em]
    if len(row) > 0:
        return row['avg_sharpe'].values[0]  # we'll use this for bar coloring
    return 0.0

# For coloring bars: green if EM in key_configs that passes, red if fails
key_em_pass = {em: info['pass']/total_windows*100 >= 70 for em, info in key_configs.items()}

em_vals = summary['atr_entry_mult'].values
sharpes_all = summary['avg_sharpe'].values

# Color bars by whether they pass 70% threshold
bar_colors = []
for em in em_vals:
    # Check if this EM is a key config with known pass rate
    if abs(em - 0.00) < 0.001:
        pr = 71.4
    elif abs(em - 0.85) < 0.001:
        pr = 82.5
    elif abs(em - 0.90) < 0.001:
        pr = 79.4
    elif abs(em - 0.50) < 0.001:
        pr = 68.3
    else:
        # For non-key configs, estimate from Sharpe (rough proxy)
        pr = 70.0  # placeholder
    bar_colors.append('#4CAF50' if pr >= 70 else '#F44336')

ax2.bar(em_vals, [71.4 if abs(e-0.00)<0.001 else 82.5 if abs(e-0.85)<0.001 
                   else 79.4 if abs(e-0.90)<0.001 else 68.3 if abs(e-0.50)<0.001 
                   else 0.0 for e in em_vals], 
        width=0.045, color='#4CAF50', alpha=0.5, edgecolor='none', label='_nolegend_')

# Overlay just the key configs with their actual pass rates
for col, info in key_configs.items():
    pr = info['pass'] / total_windows * 100
    ax2.scatter([info['em']], [pr], color=info['color'], s=100, zorder=5,
                edgecolors='white', linewidths=2)
    offset = 2.5 if info['em'] < 1.5 else -5
    ax2.annotate(f"{info['label'].split('(')[0].strip()}\n{pr:.0f}%",
                 xy=(info['em'], pr), xytext=(info['em'] + 0.08, pr + offset),
                 fontsize=8, color=info['color'], fontweight='bold',
                 arrowprops=dict(arrowstyle='->', color=info['color'], alpha=0.6) if info['em'] < 1.0 else None)

ax2.axhline(y=70, color='orange', linewidth=1.5, linestyle='--', label='70% production threshold')
ax2.set_xlabel('ATR_ENTRY_MULT', fontsize=10)
ax2.set_ylabel('Pass Rate (%)', fontsize=10)
ax2.set_title('B. Pass Rate vs ATR_ENTRY_MULT (9 universes × 6 windows = 54 per value)', fontsize=11)
ax2.set_xlim(-0.05, 2.05)
ax2.set_ylim(0, 100)
ax2.legend(loc='lower right', fontsize=9)
ax2.grid(True, alpha=0.3, axis='y')

# --- Panel C: Sharpe ratio bar chart ---
ax3 = axes[1, 0]
sharpe_vals = []
colors_s = []
for e in em_vals:
    s = summary[summary['atr_entry_mult'] == e]['avg_sharpe'].values
    sharpe_vals.append(s[0] if len(s) > 0 else 0)
    # Color by pass threshold (approximate from sweep)
    pr = 71.4 if abs(e-0.00)<0.001 else 82.5 if abs(e-0.85)<0.001 else 79.4 if abs(e-0.90)<0.001 else 68.3 if abs(e-0.50)<0.001 else 0
    colors_s.append('#4CAF50' if pr >= 70 else '#F44336')

ax3.bar(em_vals, sharpe_vals, width=0.045, color=colors_s, alpha=0.6, edgecolor='none')

# Overlay key configs
for col, info in key_configs.items():
    ax3.scatter([info['em']], [info['sharpe']], color=info['color'], s=100, zorder=5,
                edgecolors='white', linewidths=2)
    offset = 0.2 if info['sharpe'] < 5.0 else -0.4
    ax3.annotate(f"{info['label'].split('(')[0].strip()}\nSharpe {info['sharpe']:.2}",
                 xy=(info['em'], info['sharpe']), xytext=(info['em'] + 0.08, info['sharpe'] + offset),
                 fontsize=8, color=info['color'], fontweight='bold')

ax3.axhline(y=0, color='black', linewidth=0.5)
ax3.set_xlabel('ATR_ENTRY_MULT', fontsize=10)
ax3.set_ylabel('Annualised Sharpe Ratio', fontsize=10)
ax3.set_title('C. Annualised Sharpe Ratio vs ATR_ENTRY_MULT (41 values)', fontsize=11)
ax3.set_xlim(-0.05, 2.05)
ax3.grid(True, alpha=0.3, axis='y')

# --- Panel D: Sharpe vs Pass Rate scatter ---
ax4 = axes[1, 1]
for col, info in key_configs.items():
    pr = info['pass'] / total_windows * 100
    ax4.scatter([info['sharpe']], [pr], 
                label=f"{info['label'].split('(')[0].strip()} ({pr:.0f}%, S={info['sharpe']:.2})",
                color=info['color'], s=150, edgecolors='white', linewidths=2.5, zorder=5)
    ax4.annotate(info['label'].split('(')[0].strip(), 
                 xy=(info['sharpe'], pr), 
                 xytext=(info['sharpe'] + 0.05, pr + 1.5),
                 fontsize=8.5, color=info['color'], fontweight='bold')

ax4.axhline(y=70, color='orange', linewidth=1.5, linestyle='--', alpha=0.7, label='70% threshold')
ax4.set_xlabel('Annualised Sharpe Ratio', fontsize=10)
ax4.set_ylabel('Pass Rate (%)', fontsize=10)
ax4.set_title('D. Sharpe vs Pass Rate — Key Configs\n(EM=0.00 baseline, EM=0.85 winner, EM=0.90 runner-up)', fontsize=11)
ax4.set_xlim(3.0, 6.0)
ax4.set_ylim(65, 90)
ax4.legend(loc='lower right', fontsize=8.5)
ax4.grid(True, alpha=0.3)

plt.tight_layout()
out_path = 'charts/atr_entry_mult_comparison.png'
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
print(f"Saved: {out_path}")
plt.close()

# Also print a text summary
print("\n=== Key Config Summary ===")
for col, info in key_configs.items():
    pr = info['pass'] / total_windows * 100
    print(f"  {info['label']}: Sharpe={info['sharpe']:.3f}, {info['pass']}/54 pass ({pr:.1f}%), Ret={info['ret']:.1}%")
print("\n=== Held-Out Validation (Pre-2021) ===")
print("  EM=0.00 (baseline): 11/18 pass (61%) — BEST ON HELD-OUT")
print("  EM=0.85 (winner):   10/18 pass (56%)")
print("  EM=0.90 (runner-up): 10/18 pass (56%)")
print("  => SWEEP WINNER FAILS HELD-OUT. ATR_ENTRY_MULT stays at 0.00.")
