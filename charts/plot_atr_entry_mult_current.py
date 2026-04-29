#!/usr/bin/env python3
"""
ATR_ENTRY_MULT Fine-Grained Sweep — Comparison Chart
Shows equity curves for Baseline (EM=0.00), Winner (EM=0.94), and Runner-ups.

Data: snapshots/atr_entry_mult_current_equity.csv
Sweep: EM ∈ [0.00..2.00] step 0.01 (201 values) × 9 universes × 6 windows
Params: CHAND(7,2.30)/EP=21/HM=12/CAP=3/ATR(24,2.0)
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import sys

EQUITY_CSV = "snapshots/atr_entry_mult_current_equity.csv"
SUMMARY_CSV = "snapshots/atr_entry_mult_current_summary.csv"
OUT_PNG = "charts/atr_entry_mult_current_comparison.png"

# Top candidates from the sweep
CANDIDATES = {
    "Baseline EM=0.00": 0.00,
    "Winner EM=0.94":  0.94,
    "Runner-up EM=1.07": 1.07,
    "Runner-up EM=0.86": 0.86,
    "Runner-up EM=1.09": 1.09,
}

def load_summary():
    df = pd.read_csv(SUMMARY_CSV)
    df.columns = [c.strip() for c in df.columns]
    return df

def load_equity():
    df = pd.read_csv(EQUITY_CSV)
    df.columns = [c.strip() for c in df.columns]
    return df

def compute_mean_equity(df, em_value):
    """Average equity across all universes and windows for a given EM value."""
    sub = df[np.isclose(df['em'], em_value, atol=0.005)].copy()
    if sub.empty:
        return None, None
    # Group by bar_idx, compute mean equity
    grouped = sub.groupby('bar_idx')['equity'].mean()
    return grouped.index.values, grouped.values

def main():
    df_eq = load_equity()
    df_sum = load_summary()

    # Build summary lookup
    sum_dict = {}
    for _, row in df_sum.iterrows():
        key = round(row['em'], 2)
        sum_dict[key] = {
            'pass': int(row['runs_pass']),
            'total': int(row['runs_total']),
            'pass_pct': row['pass_pct'],
            'sharpe': row['avg_sharpe'],
            'ret': row['avg_ret_pct'],
            'dd': row['avg_dd'],
            'trades': int(row['total_trades']),
        }

    # Figure with 2 subplots: equity curve + pass rate/Sharpe landscape
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [2, 1]})
    fig.suptitle(
        "ATR_ENTRY_MULT Fine-Grained Sweep — Current Production Params\n"
        "CHAND(7,2.30)/EP=21/HM=12/CAP=3/ATR(24,2.0) · 201 values · 9 universes · 54 windows",
        fontsize=13, fontweight='bold'
    )

    # ---- Subplot 1: Equity curves ----
    colors = ['#888888', '#e63946', '#2a9d8f', '#f4a261', '#457b9d']

    for (label, em_val), color in zip(CANDIDATES.items(), colors):
        bars, equity = compute_mean_equity(df_eq, em_val)
        if bars is None:
            print(f"WARNING: No equity data for EM={em_val}", file=sys.stderr)
            continue

        info = sum_dict.get(round(em_val, 2), {})
        pass_pct = info.get('pass_pct', 0)
        sharpe = info.get('sharpe', 0)
        ret = info.get('ret', 0)
        dd = info.get('dd', 0)
        n_pass = info.get('pass', 0)
        n_total = info.get('total', 0)

        ax1.plot(bars, equity, label=label, color=color, linewidth=1.8, alpha=0.9)
        final_eq = equity[-1] if len(equity) > 0 else 1.0
        ax1.annotate(
            f"  {final_eq:.2f}x",
            xy=(bars[-1], final_eq),
            fontsize=8, color=color, va='center'
        )

    ax1.set_ylabel("Mean Equity (avg across universes & windows)", fontsize=10)
    ax1.set_title("Equity Curves — Baseline vs Winners", fontsize=11)
    ax1.legend(loc='upper left', fontsize=9)
    ax1.grid(True, alpha=0.3, linestyle='--')
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.1f}x'))
    ax1.set_xlim(left=0)
    ax1.set_xlabel("Bar Index")

    # ---- Subplot 2: Pass rate + Sharpe landscape ----
    em_values = df_sum['em'].values
    pass_pcts = df_sum['pass_pct'].values
    sharpes = df_sum['avg_sharpe'].values

    # Normalize Sharpe for visualization (scale to 0-100% pass rate range)
    sh_min, sh_max = sharpes.min(), sharpes.max()
    if sh_max > sh_min:
        sharpes_norm = (sharpes - sh_min) / (sh_max - sh_min) * 100
    else:
        sharpes_norm = np.zeros_like(sharpes)

    ax2.fill_between(em_values, pass_pcts, alpha=0.25, color='steelblue', label='Pass Rate %')
    ax2.plot(em_values, pass_pcts, color='steelblue', linewidth=1.5)
    ax2.set_ylabel("Pass Rate (%)", color='steelblue', fontsize=10)
    ax2.tick_params(axis='y', labelcolor='steelblue')
    ax2.set_ylim(50, 85)

    ax22 = ax2.twinx()
    ax22.plot(em_values, sharpes_norm, color='darkorange', linewidth=1.2, alpha=0.7, label='Sharpe (norm)')
    ax22.set_ylabel("Sharpe (normalised 0-100)", color='darkorange', fontsize=10)
    ax22.tick_params(axis='y', labelcolor='darkorange')
    ax22.set_ylim(0, 105)

    # Mark candidates
    for (label, em_val), color in zip(CANDIDATES.items(), colors):
        idx = np.argmin(np.abs(em_values - em_val))
        pp = pass_pcts[idx]
        sh = sharpes[idx]
        ax2.axvline(em_val, color=color, linewidth=1.0, alpha=0.6, linestyle='--')
        ax2.scatter([em_val], [pp], color=color, s=40, zorder=5, edgecolors='white', linewidths=0.5)

    ax2.set_xlabel("ATR_ENTRY_MULT (EM)", fontsize=10)
    ax2.set_title("Pass Rate & Sharpe Landscape — EM ∈ [0.00..2.00] step 0.01", fontsize=11)
    ax2.grid(True, alpha=0.3, linestyle='--')

    # Unified legend for subplot 2
    lines1, labels1 = ax2.get_legend_handles_labels()
    lines2, labels2 = ax22.get_legend_handles_labels()
    ax2.legend(lines1 + lines2, labels1 + labels2, loc='upper right', fontsize=8)

    plt.tight_layout()
    plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight')
    print(f"Saved: {OUT_PNG}")

    # Also print the top summary table
    print("\n========== TOP 10 BY PASS RATE THEN SHARPE ==========")
    top10 = df_sum.sort_values(['runs_pass', 'avg_sharpe'], ascending=[False, False]).head(10)
    print(f"{'EM':<8} {'PASS':>6} {'PASS%':>8} {'SHARPE':>10} {'RET%':>10} {'DD%':>8} {'TRADES':>8}")
    print("-" * 66)
    for _, row in top10.iterrows():
        print(f"{row['em']:<8.2f} {int(row['runs_pass']):>6} {row['pass_pct']:>7.2f}% {row['avg_sharpe']:>10.4f} {row['avg_ret_pct']:>10.4f} {row['avg_dd']:>8.2f} {int(row['total_trades']):>8}")

if __name__ == '__main__':
    main()
