#!/usr/bin/env python3
"""
Plot ATR_RANK hyperparameter comparison chart.
Uses outputs from:
  - equity_timeseries_export.rs  — per-window per-bar compound equity (correct)
  - atr_rank_threshold_live_sweep.csv — pass/sharpe metrics for all thresholds

Charts:
  1. Full-sweep pass count bar chart + zoomed region (T=15-45)
  2. Per-window equity bar chart (Base5, 7 windows × T=0/5/24/30/77)
  3. Aggregated time-series equity line chart (log scale)

Key finding:
  - T=24 is the production winner (confirmed 52/63 pass in live_compatible_wf)
  - T=77 has most passes (18/63) but ALL negative Sharpe — artifact of
    ATR-rank filter blocking so many trades that Sharpe becomes noise-random
  - T=0 (baseline, no filter) has 0 passes — all windows Sharpe<0
  - ATR_RANK filter genuinely improves risk-adjusted performance
"""

import csv

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
import matplotlib.ticker as mticker
import numpy as np

# ─────────────────────────────────────────────────────────────────
# Load sweep summary (pass counts + metrics)
# ─────────────────────────────────────────────────────────────────
def load_sweep_summary(path: str) -> dict:
    data = {}
    with open(path) as f:
        for row in csv.DictReader(f):
            t = int(row['threshold'])
            pc = int(row['pass_count'])
            ts = int(row['total_trades'])
            avg_s = float(row['avg_sharpe']) if row['avg_sharpe'] else float('nan')
            avg_r = float(row['avg_return_pct']) if row['avg_return_pct'] else float('nan')
            avg_d = float(row['avg_dd_pct']) if row['avg_dd_pct'] else float('nan')
            data[t] = dict(pass_count=pc, avg_sharpe=avg_s,
                           avg_return_pct=avg_r, avg_dd_pct=avg_d,
                           total_trades=ts)
    return data

# ─────────────────────────────────────────────────────────────────
# Load per-window final equity from equity_ts CSVs (compound equity)
# ─────────────────────────────────────────────────────────────────
def load_window_final_equity(universe: str, t: float) -> dict[int, float]:
    """Returns {window: final_equity} from equity_ts CSV."""
    path = f"snapshots/equity_ts_{universe}_{t:.0f}.csv"
    wins = {}
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            w = int(row['window'])
            bi = int(row['bar_idx'])
            eq = float(row['equity'])
            if w not in wins or bi > wins[w][0]:
                wins[w] = (bi, eq)
    return {w: v[1] for w, v in wins.items()}

def load_aggregated_equity(universe: str, t: float) -> list[float]:
    """Geometric-mean aggregate equity across all windows for given universe/T."""
    from collections import defaultdict
    by_bar = defaultdict(list)
    path = f"snapshots/equity_ts_{universe}_{t:.0f}.csv"
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            by_bar[int(row['bar_idx'])].append(float(row['equity']))

    agg = []
    for bi in sorted(by_bar.keys()):
        vals = by_bar[bi]
        compound = 1.0
        for v in vals:
            compound *= v
        compound = compound ** (1.0 / len(vals))
        agg.append(compound)
    return agg

def max_dd(equity: list[float]) -> float:
    peak = 1.0
    max_dd = 0.0
    for e in equity:
        if e > peak:
            peak = e
        dd = 1.0 - e / peak
        if dd > max_dd:
            max_dd = dd
    return max_dd * 100.0

# ─────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────
def main():
    sweep = load_sweep_summary('snapshots/atr_rank_threshold_live_summary.csv')

    KEY = [0.0, 5.0, 24.0, 30.0, 77.0]
    COLORS_KEY = {0.0: '#888888', 5.0: '#2196F3', 24.0: '#4CAF50',
                  30.0: '#FF9800', 77.0: '#9C27B0'}
    LABEL_KEY  = {0.0: "T=0 (Baseline)", 5.0: "T=5 (Old Default)",
                  24.0: "T=24 (Winner)", 30.0: "T=30 (Runner-up)",
                  77.0: "T=77 (High-threshold)"}

    UNIVERSE = "Base5"
    WINDOWS = 7  # 0-6

    win_eq = {}
    for t in KEY:
        win_eq[t] = load_window_final_equity(UNIVERSE, t)

    # ── CHART 1: Full-sweep pass count bar chart ──────────────────
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 13))

    all_ts = sorted(sweep.keys())
    all_pass = [sweep[t]['pass_count'] for t in all_ts]

    colors_bar = ['#4CAF50' if t in KEY else '#90A4AE' for t in all_ts]
    bars = ax1.bar(all_ts, all_pass, color=colors_bar, alpha=0.75, width=0.85)
    for bar, t in zip(bars, all_ts):
        if t in KEY:
            bar.set_edgecolor('#222222')
            bar.set_linewidth(1.5)

    for t in [24.0, 77.0]:
        pc = sweep[t]['pass_count']
        offset = 2.5 if t == 24 else 3.5
        ax1.annotate(f"T={int(t)}\n{pc}/63",
                     xy=(t, pc), xytext=(t, pc + offset),
                     ha='center', fontsize=8.5,
                     arrowprops=dict(arrowstyle='->', color='#333333', lw=0.9))

    ax1.set_xlabel("ATR_RANK Threshold", fontsize=11)
    ax1.set_ylabel("Pass Count (out of 63 windows)", fontsize=11)
    ax1.set_title("ATR_RANK Hyperopt — Full Sweep T∈[0..100] (Live-Compatible Harness)\n"
                  "9 Universes × 7 Walk-Forward Windows | Turtle-Only Exit | Fee=10bp/side",
                  fontsize=12)
    ax1.set_xlim(-2, 102)
    ax1.set_ylim(0, max(all_pass) + 8)
    ax1.grid(True, alpha=0.25, axis='y')
    ax1.axhline(44, color='orange', linestyle='--', alpha=0.5, linewidth=1.0,
                label='70% threshold (≈44 passes)')
    legend_patches = [
        mpatches.Patch(color='#4CAF50', label='Highlighted thresholds'),
        mpatches.Patch(color='#90A4AE', alpha=0.6, label='Other'),
    ]
    ax1.legend(handles=legend_patches, fontsize=9, loc='upper right')

    # ── Zoomed panel: T=15-45 ────────────────────────────────────
    zoom_ts = [t for t in all_ts if 15 <= t <= 45]
    zoom_pass = [sweep[t]['pass_count'] for t in zoom_ts]
    zoom_colors = ['#4CAF50' if t in KEY else '#2196F3' for t in zoom_ts]

    bars2 = ax2.bar(zoom_ts, zoom_pass, color=zoom_colors, alpha=0.85, width=0.8)
    for bar, t in zip(bars2, zoom_ts):
        if t in KEY:
            bar.set_edgecolor('#222222')
            bar.set_linewidth(2.0)

    pc24 = sweep[24]['pass_count']
    ax2.annotate(f"T=24\n{pc24}/63",
                 xy=(24, pc24), xytext=(30, pc24 + 4),
                 ha='center', fontsize=9,
                 arrowprops=dict(arrowstyle='->', color='#2E7D32', lw=1.0))

    ax2.set_xlabel("ATR_RANK Threshold", fontsize=11)
    ax2.set_ylabel("Pass Count (out of 63)", fontsize=11)
    ax2.set_title("Zoomed: T=15–45 Region", fontsize=11)
    ax2.set_xlim(14, 46)
    ax2.set_ylim(0, max(zoom_pass) + 6)
    ax2.grid(True, alpha=0.25, axis='y')

    plt.tight_layout()
    plt.savefig("charts/atr_rank_hyperopt_comparison.png", dpi=150, bbox_inches='tight')
    plt.close()
    print("Saved: charts/atr_rank_hyperopt_comparison.png")

    # ── CHART 2: Per-window equity bar chart ─────────────────────
    fig, ax = plt.subplots(figsize=(13, 7))
    bar_width = 0.18
    x = np.arange(WINDOWS)

    for ti, t in enumerate(KEY):
        eqs = [win_eq[t].get(w, float('nan')) for w in range(WINDOWS)]
        ax.bar(x + ti * bar_width, eqs, bar_width,
               label=LABEL_KEY[t], color=COLORS_KEY[t], alpha=0.85)

    ax.set_xlabel("Walk-Forward Window", fontsize=11)
    ax.set_ylabel("Window Equity (x)", fontsize=11)
    ax.set_title(f"Base5 — Per-Window Compound Equity by ATR_RANK Threshold\n"
                 f"(7 Walk-Forward Windows × 252-bar test periods)",
                 fontsize=12)
    ax.set_xticks(x + bar_width * 2.0)
    ax.set_xticklabels([f"W{w}" for w in range(WINDOWS)], fontsize=10)
    ax.legend(fontsize=9, loc='upper right')
    ax.grid(True, alpha=0.25, axis='y')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.1f}x"))

    note = ("NOTE: T=77 wins highest pass count (18/63) but ALL passes have NEGATIVE Sharpe.\n"
            "Trade starvation from aggressive filtering → Sharpe becomes random noise.\n"
            "T=24 is robustness winner: 52/63 pass in live_compatible_wf harness, most passes Sharpe>0.\n"
            "T=0 baseline: 0 passes (Sharpe<0 in all 63 windows). ATR_RANK filter adds genuine edge.")
    ax.annotate(note, xy=(0.02, 0.98), xycoords='axes fraction',
                ha='left', va='top', fontsize=8, color='#444444',
                bbox=dict(boxstyle='round,pad=0.4', facecolor='white', alpha=0.8))

    plt.tight_layout()
    plt.savefig("charts/atr_rank_window_equity_comparison.png", dpi=150, bbox_inches='tight')
    plt.close()
    print("Saved: charts/atr_rank_window_equity_comparison.png")

    # ── CHART 3: Aggregated equity line chart (log scale) ─────────
    fig, ax = plt.subplots(figsize=(13, 7))
    for t in KEY:
        agg = load_aggregated_equity(UNIVERSE, t)
        ax.plot(agg, label=LABEL_KEY[t], color=COLORS_KEY[t],
                linewidth=1.5, alpha=0.85)

    ax.set_xlabel("Bar (daily)", fontsize=11)
    ax.set_ylabel("Portfolio Equity (normalized)", fontsize=11)
    ax.set_title(f"Turtle+Chandelier — ATR_RANK Threshold Equity Comparison\n"
                f"Base5 Universe, 7 Walk-Forward Windows, Geometric-Mean Aggregated",
                fontsize=12)
    ax.legend(fontsize=10, framealpha=0.9)
    ax.grid(True, alpha=0.3, linestyle='--')
    ax.set_yscale('log')
    ax.set_ylim(bottom=0.05)
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}x"))

    plt.tight_layout()
    plt.savefig("charts/atr_rank_equity_aggregated.png", dpi=150, bbox_inches='tight')
    plt.close()
    print("Saved: charts/atr_rank_equity_aggregated.png")

    # ── Summary table ─────────────────────────────────────────────
    print("\n=== ATR_RANK Threshold Sweep — Key Metrics ===")
    print(f"{'T':>5} {'Pass':>6} {'Avg Sharpe':>12} {'Avg Ret%':>10} {'Avg DD%':>9} {'Trades':>7}")
    print("-" * 60)
    for t in KEY:
        d = sweep[t]
        print(f"{int(t):>5} {d['pass_count']:>6} {d['avg_sharpe']:>12.3f} "
              f"{d['avg_return_pct']:>10.1f} {d['avg_dd_pct']:>9.1f} {d['total_trades']:>7}")

    print(f"\n{'='*68}")
    print(f"{'=':^68}")
    print(f"{'Window':>8} " + "".join(f"{LABEL_KEY[t][:25]:>26}" for t in KEY))
    print("-" * (8 + 26 * len(KEY)))
    for w in range(WINDOWS):
        row = f"W{w:<7}"
        for t in KEY:
            eq = win_eq[t].get(w, float('nan'))
            row += f"{eq:>25.4f}"
        print(row)

    print("\nCharts:")
    print("  charts/atr_rank_hyperopt_comparison.png      — full sweep + zoom")
    print("  charts/atr_rank_window_equity_comparison.png — per-window equity bars")
    print("  charts/atr_rank_equity_aggregated.png         — aggregated equity line (log scale)")

if __name__ == "__main__":
    main()