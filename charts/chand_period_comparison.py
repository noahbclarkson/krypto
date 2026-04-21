#!/usr/bin/env python3
"""
CHAND_PERIOD Equity Curve Comparison — Full Universe Breakdown
Plots equity curves for top CHAND_PERIOD candidates with per-universe subplots.
"""

import csv
import os
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

UNIVERSE_NAMES = [
    "Base5", "NoDOGE", "Legacy4", "Legacy5BNB",
    "OldGuardNoBNB", "LargeCaps5", "Legacy3",
    "LowVolume5", "OldGuard4"
]
WINDOWS_PER_UNIVERSE = 6
TARGET_LEN = 252  # resample all windows to this length

def load_equity_csv(path):
    """Return dict: (universe_idx, window_idx) -> equity list"""
    curves = {}
    if not os.path.exists(path):
        return curves
    with open(path) as f:
        reader = csv.reader(f)
        next(reader)
        for row in reader:
            if len(row) < 4:
                continue
            u = int(row[0])
            w = int(row[1])
            bar = int(row[2])
            eq = float(row[3])
            key = (u, w)
            if key not in curves:
                curves[key] = []
            while len(curves[key]) <= bar:
                curves[key].append(None)
            curves[key][bar] = eq
    return curves

def resample(curve, target_len):
    if not curve:
        return [1.0] * target_len
    curve = [v for v in curve if v is not None]
    if not curve:
        return [1.0] * target_len
    if len(curve) == target_len:
        return curve[:]
    if len(curve) < 2:
        return [curve[0]] * target_len
    result = []
    for i in range(target_len):
        t = i / (target_len - 1)
        pos = t * (len(curve) - 1)
        idx = int(pos)
        frac = pos - idx
        if idx + 1 < len(curve):
            v = curve[idx] * (1 - frac) + curve[idx + 1] * frac
        else:
            v = curve[idx]
        result.append(v)
    return result

def mean_curve(curves_dict, uni_idx):
    """Average equity across all windows in a universe, then resample to TARGET_LEN."""
    uni_curves = []
    for w in range(WINDOWS_PER_UNIVERSE):
        key = (uni_idx, w)
        if key in curves_dict:
            rc = resample(curves_dict[key], TARGET_LEN)
            uni_curves.append(rc)
    if not uni_curves:
        return [1.0] * TARGET_LEN
    # Element-wise mean
    result = []
    for bar in range(TARGET_LEN):
        vals = [c[bar] for c in uni_curves]
        result.append(np.mean(vals))
    return result

def global_mean_curve(curves_dict):
    """Average equity across ALL windows globally, then resample."""
    all_curves = []
    for key, eq_list in curves_dict.items():
        rc = resample(eq_list, TARGET_LEN)
        all_curves.append(rc)
    if not all_curves:
        return [1.0] * TARGET_LEN
    result = []
    for bar in range(TARGET_LEN):
        vals = [c[bar] for c in all_curves]
        result.append(np.mean(vals))
    return result

def main():
    # Configs to compare
    configs = [11, 7, 5, 19]
    labels = {
        5:  "CP=5 (runner-up 1, Sharpe 5.825)",
        7:  "CP=7 (WINNER, Sharpe 5.908)",
        11: "CP=11 (baseline, Sharpe 5.526)",
        19: "CP=19 (runner-up 2, Sharpe 5.668)",
    }
    colors = {
        11: "#888888",  # grey baseline
        7:  "#2ecc71",  # green winner
        5:  "#e74c3c",  # red runner-up 1
        19: "#3498db",  # blue runner-up 2
    }

    # Load all equity data
    equity_data = {}
    for cp in configs:
        path = f"/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/equity_chand_cp_{cp}.csv"
        equity_data[cp] = load_equity_csv(path)
        n_curves = len([k for k in equity_data[cp] if k[0] == 0])
        print(f"  CP={cp}: {len(equity_data[cp])} windows loaded")

    # Compute per-universe mean curves for each config
    uni_curves = {}  # cp -> uni_idx -> curve
    global_curves = {}  # cp -> global mean curve
    for cp in configs:
        ed = equity_data[cp]
        uni_curves[cp] = {}
        for ui, uname in enumerate(UNIVERSE_NAMES):
            uni_curves[cp][ui] = mean_curve(ed, ui)
        global_curves[cp] = global_mean_curve(ed)

    # ─── Figure 1: Global aggregated equity (single panel) ───
    fig1, ax1 = plt.subplots(figsize=(14, 8))
    x = np.arange(TARGET_LEN)
    for cp in configs:
        ax1.plot(x, global_curves[cp],
                 label=labels[cp],
                 color=colors[cp],
                 linewidth=2.0, alpha=0.9)
    ax1.set_yscale('log')
    ax1.grid(True, alpha=0.3, linestyle='--')
    ax1.set_xlabel('Bar (walk-forward window, resampled to 252)', fontsize=12)
    ax1.set_ylabel('Equity (log scale)', fontsize=12)
    ax1.set_title('CHAND_PERIOD Equity Curve Comparison\n'
                  'Aggregated across all 9 universes × 54 windows (mean equity)',
                  fontsize=14)
    ax1.legend(loc='upper left', fontsize=11)
    # Annotate final values
    for cp in configs:
        final = global_curves[cp][-1]
        ax1.annotate(f'{final:.3f}x',
                     xy=(TARGET_LEN - 1, final),
                     xytext=(5, 5), textcoords='offset points',
                     fontsize=9, color=colors[cp])
    plt.tight_layout()
    out1 = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/chand_period_comparison.png"
    plt.savefig(out1, dpi=150, bbox_inches='tight')
    plt.close()
    print(f"Saved: {out1}")

    # ─── Figure 2: Per-universe breakdown (3×3 grid) ───
    fig2, axes = plt.subplots(3, 3, figsize=(18, 15))
    axes = axes.flatten()
    for ui, uname in enumerate(UNIVERSE_NAMES):
        ax = axes[ui]
        for cp in configs:
            ax.plot(x, uni_curves[cp][ui],
                    label=labels[cp] if ui == 0 else None,
                    color=colors[cp],
                    linewidth=1.5, alpha=0.85)
        ax.set_yscale('log')
        ax.grid(True, alpha=0.25, linestyle='--')
        ax.set_title(uname, fontsize=11, fontweight='bold')
        ax.set_xlabel('Bar', fontsize=9)
        ax.set_ylabel('Equity', fontsize=9)
        if ui == 0:
            ax.legend(fontsize=7, loc='upper left')
    fig2.suptitle('CHAND_PERIOD Equity Curves by Universe\n'
                   '(mean of 6 windows per universe, log scale)',
                   fontsize=14, y=1.01)
    plt.tight_layout()
    out2 = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/chand_period_comparison_per_universe.png"
    plt.savefig(out2, dpi=120, bbox_inches='tight')
    plt.close()
    print(f"Saved: {out2}")

    # ─── Print summary table ───
    print("\n=== Sweep Summary ===")
    sweep_csv = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/chand_period_hyperopt_summary.csv"
    if os.path.exists(sweep_csv):
        print(f"{'CP':>4} {'Pass':>8} {'Pass%':>8} {'Sharpe':>10} {'AvgRet%':>10} {'Trades':>8}")
        with open(sweep_csv) as f:
            reader = csv.reader(f)
            next(reader)
            for row in reader:
                cp = int(row[0])
                if cp in [5, 7, 11, 19, 40]:
                    print(f"{cp:>4} {row[1]:>8} {row[3]:>8} {row[4]:>10} {row[5]:>10} {row[6]:>8}")

if __name__ == "__main__":
    main()