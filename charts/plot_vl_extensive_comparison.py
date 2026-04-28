#!/usr/bin/env python3
import csv
import math
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path('/home/ubuntu/.openclaw/workspace-krypto/krypto')
SUMMARY_CSV = ROOT / 'snapshots' / 'vl_extensive_current_params_summary.csv'
AGG_CSV = ROOT / 'snapshots' / 'vl_extensive_aggregate_equity.csv'
SELECTED_CSV = ROOT / 'snapshots' / 'vl_extensive_selected_equity.csv'
OUT = ROOT / 'charts' / 'comparison_chart.png'
BASELINE_VL = 9


def load_summary():
    rows = []
    with SUMMARY_CSV.open() as f:
        for row in csv.DictReader(f):
            row['vl'] = int(row['vl'])
            row['pass_count'] = int(row['pass_count'])
            row['total'] = int(row['total'])
            row['positive_universes'] = int(row['positive_universes'])
            row['avg_sharpe'] = float(row['avg_sharpe'])
            row['avg_ret'] = float(row['avg_ret'])
            row['avg_dd'] = float(row['avg_dd'])
            row['base5_pass'] = int(row['base5_pass'])
            row['base5_total'] = int(row['base5_total'])
            row['base5_avg_sharpe'] = float(row['base5_avg_sharpe'])
            row['base5_avg_ret'] = float(row['base5_avg_ret'])
            rows.append(row)
    rows.sort(key=lambda r: (-r['pass_count'], -r['positive_universes'], -r['avg_sharpe'], r['avg_dd']))
    return rows


def load_aggregate():
    data = {}
    with AGG_CSV.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            for key, value in row.items():
                if key == 'step':
                    continue
                data.setdefault(int(key.replace('vl_', '')), []).append(float(value))
    return data


def load_base5_mean_curves():
    curves = {}
    per_vl = {}
    with SELECTED_CSV.open() as f:
        for row in csv.DictReader(f):
            if row['universe'] != 'Base5':
                continue
            vl = int(row['vl'])
            wi = int(row['window'])
            eq = float(row['equity'])
            per_vl.setdefault(vl, {}).setdefault(wi, []).append(eq)

    for vl, win_map in per_vl.items():
        max_len = max(len(v) for v in win_map.values())
        padded = []
        for wi in sorted(win_map):
            curve = win_map[wi]
            padded.append(curve + [curve[-1]] * (max_len - len(curve)))
        curves[vl] = np.mean(np.array(padded), axis=0)
    return curves


def set_dynamic_log_ylim(ax, series_list):
    vals = [v for series in series_list for v in series if v > 0]
    ymin = min(vals)
    ymax = max(vals)
    lo = 10 ** (math.log10(ymin) - 0.05)
    hi = 10 ** (math.log10(ymax) + 0.05)
    ax.set_ylim(lo, hi)


def fmt_label(vl, summary_map, winner_vl):
    row = summary_map[vl]
    tag = []
    if vl == BASELINE_VL:
        tag.append('baseline')
    if vl == winner_vl:
        tag.append('winner')
    label = f"VL={vl}"
    if tag:
        label += f" ({', '.join(tag)})"
    label += f" | pass {row['pass_count']}/{row['total']} | sh {row['avg_sharpe']:.2f}"
    return label


def main():
    summary_rows = load_summary()
    summary_map = {r['vl']: r for r in summary_rows}
    winner_vl = summary_rows[0]['vl']
    aggregate = load_aggregate()
    base5 = load_base5_mean_curves()

    selected_vls = sorted(aggregate.keys())
    colors = ['#1f77b4', '#d62728', '#2ca02c', '#9467bd', '#ff7f0e']
    color_map = {vl: colors[i % len(colors)] for i, vl in enumerate(selected_vls)}

    fig, axes = plt.subplots(2, 1, figsize=(14, 10), constrained_layout=True)

    ax = axes[0]
    agg_series = []
    for vl in selected_vls:
        series = aggregate[vl]
        agg_series.append(series)
        ax.plot(series, linewidth=2.2, color=color_map[vl], label=fmt_label(vl, summary_map, winner_vl))
    ax.set_title('VOL_LOOKBACK Hyperopt — Aggregate Walk-Forward Equity (Current Production Params)', fontsize=13, fontweight='bold')
    ax.set_xlabel('Chained walk-forward step')
    ax.set_ylabel('Equity (log scale)')
    ax.set_yscale('log')
    set_dynamic_log_ylim(ax, agg_series)
    ax.grid(True, alpha=0.3)
    ax.legend(fontsize=9)

    ax2 = axes[1]
    base5_series = []
    for vl in selected_vls:
        series = base5.get(vl)
        if series is None:
            continue
        base5_series.append(series)
        row = summary_map[vl]
        ax2.plot(
            series,
            linewidth=2.2,
            color=color_map[vl],
            label=f"VL={vl} | Base5 {row['base5_pass']}/{row['base5_total']} | sh {row['base5_avg_sharpe']:.2f}"
        )
    ax2.set_title('Base5 Mean Equity Curve (window-aligned average)', fontsize=12, fontweight='bold')
    ax2.set_xlabel('Bar step within walk-forward window')
    ax2.set_ylabel('Equity (log scale)')
    ax2.set_yscale('log')
    set_dynamic_log_ylim(ax2, base5_series)
    ax2.grid(True, alpha=0.3)
    ax2.legend(fontsize=9)

    winner = summary_map[winner_vl]
    baseline = summary_map[BASELINE_VL]
    note = (
        f"Robustness-first winner: VL={winner_vl} | pass {winner['pass_count']}/{winner['total']} | "
        f"positive universes {winner['positive_universes']}/9 | avg Sharpe {winner['avg_sharpe']:.2f}\n"
        f"Baseline VL={BASELINE_VL} | pass {baseline['pass_count']}/{baseline['total']} | "
        f"avg Sharpe {baseline['avg_sharpe']:.2f}"
    )
    fig.text(0.01, 0.01, note, fontsize=9)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(OUT, dpi=180, bbox_inches='tight')
    print(f'Saved {OUT}')


if __name__ == '__main__':
    main()
