#!/usr/bin/env python3
"""
Plot CHAND_MULT hyperopt comparison charts.
Reads: snapshots/chand_mult_sweep.csv
       snapshots/chand_mult_full_validation.csv
       snapshots/chand_mult_*_equity.csv (one per config exported)
Output: charts/chand_mult_comparison.png (and charts/chand_mult_sweep_chart.png)
"""

import pandas as pd
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from pathlib import Path
import warnings
warnings.filterwarnings('ignore')
import os

# Handle both direct run and exec() context
try:
    BASE = Path(__file__).parent.parent
except NameError:
    BASE = Path(os.environ.get('KRYPTO_BASE', '/home/ubuntu/.openclaw/workspace-krypto/krypto'))

# ── 1. Load sweep results ──────────────────────────────────────────────────────
sweep_df = pd.read_csv(BASE / "snapshots/chand_mult_sweep.csv")
full_df  = pd.read_csv(BASE / "snapshots/chand_mult_full_validation.csv")

sweep_df = sweep_df.sort_values('m').reset_index(drop=True)
full_df  = full_df.sort_values('m').reset_index(drop=True)

BASELINE_M = 2.00

# ── 2. Load equity CSVs ────────────────────────────────────────────────────────
EQUITY_FILES = sorted((BASE / "snapshots").glob("chand_mult_*_equity.csv"))
# Only keep files matching M=X.XX format (exclude legacy files like chand_mult_p15_equity.csv)
EQUITY_FILES = [
    f for f in EQUITY_FILES
    if f.stem.replace('chand_mult_', '').replace('_equity', '').replace('.', '', 1).lstrip('0123456789').replace('_', '').replace('equity', '') == ''
]
print(f"Found {len(EQUITY_FILES)} equity files: {[f.name for f in EQUITY_FILES]}")

equity_dfs = {}
for f in EQUITY_FILES:
    key = f.stem.replace("chand_mult_", "").replace("_equity", "")
    try:
        df = pd.read_csv(f)
        equity_dfs[key] = df
        print(f"  {key}: {len(df)} rows, universes={df['universe'].unique()}")
    except Exception as e:
        print(f"  WARNING: could not read {f.name}: {e}")

# ── 3. Determine colors for each config ────────────────────────────────────────
COLORS = {
    "2.00": "#2196F3",  # blue — baseline
    "2.05": "#FF5722",  # deep orange
    "1.90": "#4CAF50",  # green
    "1.80": "#9C27B0",  # purple
}

def get_color(key):
    key_f = float(key)
    if abs(key_f - BASELINE_M) < 0.001:
        return COLORS["2.00"]
    # Generate a deterministic color based on value
    idx = list(sorted(equity_dfs.keys())).index(key)
    cmap = plt.cm.get_cmap('tab10', len(equity_dfs))
    return cmap(idx)

# ── 4. Build per-window aggregated equity for each config ─────────────────────
def build_aggregated_equity(eq_df):
    """
    For each window, normalize equity to start at 1.
    Then aggregate across windows by taking the geometric mean at each bar step.
    Returns (bar_indices, median_equity, mean_equity).
    """
    windows = eq_df['window'].unique()
    max_bars = eq_df['bar'].max()
    n_windows = len(windows)

    # Matrix: windows × bars
    matrix = np.full((n_windows, int(max_bars) + 1), np.nan)
    for wi, w in enumerate(windows):
        wdf = eq_df[eq_df['window'] == w].set_index('bar')
        for bar, row in wdf.iterrows():
            matrix[wi, int(bar)] = row['equity']

    # Geometric mean at each bar (ignoring NaN)
    def geomean_row(row):
        vals = row[~np.isnan(row)]
        if len(vals) == 0:
            return np.nan
        # clip to avoid overflow
        vals = np.clip(vals, 1e-10, 1e10)
        return np.exp(np.mean(np.log(vals)))

    bar_indices = np.arange(int(max_bars) + 1)
    median_eq = np.array([geomean_row(matrix[:, b]) if not np.all(np.isnan(matrix[:, b])) else np.nan
                           for b in bar_indices])
    mean_eq = np.array([np.nanmean(matrix[:, b]) for b in bar_indices])

    return bar_indices, median_eq, mean_eq


# ── 5. Main comparison chart (equity curves) ───────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle("Turtle+Chandelier — CHAND_MULT Fine Hyperopt\n"
             f"Sweep: M ∈ [1.50, 3.00] step 0.05 (31 values) | P=28 fixed | 3 sweep universes",
             fontsize=14, fontweight='bold')

# Panel 1: Sharpe vs M (sweep phase)
ax1 = axes[0, 0]
ax1.plot(sweep_df['m'], sweep_df['avg_sharpe'], 'b-', linewidth=1.5, label='Avg Sharpe')
ax1.axvline(BASELINE_M, color='gray', linestyle='--', linewidth=1, label=f'Baseline M={BASELINE_M}')
# Mark top 3
top3 = sweep_df.nlargest(3, 'avg_sharpe')
for _, row in top3.iterrows():
    ax1.scatter(row['m'], row['avg_sharpe'], zorder=5, s=60,
                color='red' if row['m'] != BASELINE_M else 'blue')
    ax1.annotate(f"M={row['m']:.2f}\nsh={row['avg_sharpe']:.3f}",
                 (row['m'], row['avg_sharpe']),
                 textcoords="offset points", xytext=(5, 5), fontsize=7)
ax1.set_xlabel("CHAND_MULT")
ax1.set_ylabel("Avg OOS Sharpe (3 universes)")
ax1.set_title("Phase 1: Sharpe vs CHAND_MULT (sweep phase)")
ax1.grid(True, alpha=0.3)
ax1.legend()

# Panel 2: Pass rate vs M
ax2 = axes[0, 1]
ax2.plot(sweep_df['m'], sweep_df['pass_pct'], 'g-', linewidth=1.5, label='Pass Rate %')
ax2.axvline(BASELINE_M, color='gray', linestyle='--', linewidth=1, label=f'Baseline M={BASELINE_M}')
ax2.set_xlabel("CHAND_MULT")
ax2.set_ylabel("Pass Rate (%)")
ax2.set_title("Phase 1: Pass Rate vs CHAND_MULT (sweep phase)")
ax2.grid(True, alpha=0.3)
ax2.legend()

# Panel 3: Equity curves (log scale) — baseline + top configs
ax3 = axes[1, 0]
if equity_dfs:
    sorted_keys = sorted(equity_dfs.keys(), key=lambda k: float(k))
    for key in sorted_keys:
        eq_df = equity_dfs[key]
        bars, median_eq, _ = build_aggregated_equity(eq_df)
        color = get_color(key)
        label = f"M={key}"
        if abs(float(key) - BASELINE_M) < 0.001:
            label += " [BASELINE]"
            ax3.plot(bars, median_eq, color=color, linewidth=2.5, label=label, zorder=10)
        elif key in [k for k in sorted_keys[:4]]:
            ax3.plot(bars, median_eq, color=color, linewidth=1.5, alpha=0.8, label=label)
        else:
            ax3.plot(bars, median_eq, color=color, linewidth=0.8, alpha=0.4, label=label)

    ax3.set_yscale('log')
    ax3.set_xlabel("Bar (test window)")
    ax3.set_ylabel("Normalized Equity (log scale)")
    ax3.set_title("Phase 2: Aggregated Equity Curve (3 sweep universes)\nGeometric mean across windows")
    ax3.grid(True, alpha=0.3, which='both')
    ax3.legend(fontsize=8, loc='upper left')
else:
    ax3.text(0.5, 0.5, "No equity data found.\nRun hyperopt first.",
             ha='center', va='center', transform=ax3.transAxes)
    ax3.set_title("Equity Curves (no data)")

# Panel 4: Full validation comparison — baseline vs winner
ax4 = axes[1, 1]
if not full_df.empty:
    configs = []
    sharpes = []
    pass_pcts = []
    colors_bar = []

    sorted_full = full_df.sort_values('avg_sharpe', ascending=False)
    for _, row in sorted_full.iterrows():
        configs.append(f"M={row['m']:.2f}")
        sharpes.append(row['avg_sharpe'])
        pass_pcts.append(row['pass_pct'])
        if abs(row['m'] - BASELINE_M) < 0.001:
            colors_bar.append('#2196F3')
        else:
            colors_bar.append('#FF5722')

    x = np.arange(len(configs))
    bars = ax4.bar(x, sharpes, color=colors_bar, alpha=0.8, width=0.6)
    ax4.set_xticks(x)
    ax4.set_xticklabels(configs, rotation=45, ha='right', fontsize=8)
    ax4.set_ylabel("Avg OOS Sharpe (9 universes)")
    ax4.set_title("Phase 3: Full 9-Universe Validation\nSharpe by CHAND_MULT")
    ax4.grid(True, alpha=0.3, axis='y')
    # Annotate winner
    winner_row = sorted_full.iloc[0]
    ax4.annotate(f"Winner\nsh={winner_row['avg_sharpe']:.3f}\npass={winner_row['pass_pct']:.0f}%",
                 xy=(0, winner_row['avg_sharpe']),
                 xytext=(0.5, winner_row['avg_sharpe'] + 0.3),
                 fontsize=8, ha='left',
                 arrowprops=dict(arrowstyle='->', color='red'))

plt.tight_layout(rect=[0, 0, 1, 0.96])
out_path = BASE / "charts" / "chand_mult_comparison.png"
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Saved: {out_path}")
plt.close()

# ── 6. Sweep-only chart (Sharpe + Pass rate dual axis) ───────────────────────
fig2, ax_s = plt.subplots(figsize=(14, 6))
ax_p = ax_s.twinx()

ln1 = ax_s.plot(sweep_df['m'], sweep_df['avg_sharpe'], 'b-', linewidth=2, label='Avg Sharpe')
ax_s.axvline(BASELINE_M, color='#2196F3', linestyle='--', linewidth=1.5, alpha=0.7,
             label=f'Baseline M={BASELINE_M}')
# Shade the region around winner
if not sweep_df.empty:
    best_row = sweep_df.loc[sweep_df['avg_sharpe'].idxmax()]
    ax_s.axvspan(best_row['m'] - 0.1, best_row['m'] + 0.1,
                 alpha=0.15, color='green', label=f'Best M={best_row["m"]:.2f}')

ln2 = ax_p.plot(sweep_df['m'], sweep_df['pass_pct'], 'g--', linewidth=1.5, alpha=0.7, label='Pass Rate %')

ax_s.set_xlabel("CHAND_MULT", fontsize=12)
ax_s.set_ylabel("Avg OOS Sharpe", color='blue', fontsize=12)
ax_p.set_ylabel("Pass Rate (%)", color='green', fontsize=12)
ax_s.tick_params(axis='y', labelcolor='blue')
ax_p.tick_params(axis='y', labelcolor='green')
ax_s.set_title("CHAND_MULT Fine Sweep: Sharpe & Pass Rate vs Multiplier\n"
               f"Range: 1.50–3.00 step 0.05 (31 values) | Winner: M={best_row['m']:.2f} (Sharpe={best_row['avg_sharpe']:.4})",
               fontsize=13, fontweight='bold')

# Combined legend
lns = ln1 + ln2
labs = [l.get_label() for l in lns]
ax_s.legend(lns, labs, loc='upper right')
ax_s.grid(True, alpha=0.3)

out_path2 = BASE / "charts" / "chand_mult_sweep_chart.png"
plt.savefig(out_path2, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Saved: {out_path2}")
plt.close()

# ── 7. Summary stats print ─────────────────────────────────────────────────────
print("\n==== CHAND_MULT Hyperopt Summary ====")
print(f"Baseline M={BASELINE_M}")
if not full_df.empty:
    winner_row = full_df.loc[full_df['avg_sharpe'].idxmax()]
    baseline_row = full_df[abs(full_df['m'] - BASELINE_M) < 0.01].iloc[0]
    delta = winner_row['avg_sharpe'] - baseline_row['avg_sharpe']
    delta_pct = 100 * delta / baseline_row['avg_sharpe'] if baseline_row['avg_sharpe'] != 0 else 0
    print(f"Winner: M={winner_row['m']:.2f} | Sharpe={winner_row['avg_sharpe']:.4f} | pass={winner_row['pass_pct']:.1f}%")
    print(f"Baseline: M={baseline_row['m']:.2f} | Sharpe={baseline_row['avg_sharpe']:.4f} | pass={baseline_row['pass_pct']:.1f}%")
    print("Delta: {:+.4f} ({:+.1}%)".format(delta, delta_pct))
    rec = f"UPDATE M={winner_row['m']} (better by {delta_pct:.1f}%)" if delta_pct > 1.0 else "M=2.00 is robust — no change needed (delta < 1%)"
    print(f"Recommendation: {rec}")
