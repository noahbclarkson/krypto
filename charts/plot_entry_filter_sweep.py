#!/usr/bin/env python3
"""
Plot turtle entry filter sweep results.
Generates comparison charts for ATR entry filter × volume confirmation sweep.

Key metrics plotted:
1. Pass rate by ATR_mult (for vol=none baseline)
2. Average Sharpe by ATR_mult (for vol=none)
3. Equity curves: baseline vs winners (aggregated across all universes/windows)
"""

import csv
import sys
import os
from collections import defaultdict

# ── Load results CSV ──────────────────────────────────────────────────────────
results_file = "snapshots/turtle_entry_filter_results.csv"
equity_file  = "snapshots/turtle_entry_filter_equity.csv"

if not os.path.exists(results_file):
    print(f"ERROR: {results_file} not found — run turtle_entry_filter_sweep first")
    sys.exit(1)

# Parse results
headers = []
rows = []
with open(results_file) as f:
    reader = csv.DictReader(f)
    headers = reader.fieldnames
    for row in reader:
        rows.append(row)

print(f"Loaded {len(rows)} result rows")

# ── Per-universe pass rate ───────────────────────────────────────────────────
# Group by universe, ATR_mult, vol_label
by_uni = defaultdict(lambda: defaultdict(list))
for r in rows:
    by_uni[r['universe']][(r['atr_mult'], r['vol_label'])].append(r)

# ── Global summary ──────────────────────────────────────────────────────────
# Group by (atr_mult, vol_label) — aggregate across all universes/windows
global_stats = defaultdict(lambda: {'pass': 0, 'total': 0, 'sharpe_sum': 0.0,
                                    'ret_sum': 0.0, 'trades': 0, 'win_rate_sum': 0.0})
for r in rows:
    key = (r['atr_mult'], r['vol_label'])
    s = global_stats[key]
    s['pass'] += 1 if r['pass'] == 'true' else 0
    s['total'] += 1
    s['sharpe_sum'] += float(r['sharpe'])
    s['ret_sum'] += float(r['ret_pct'])
    s['trades'] += int(r['trades'])
    s['win_rate_sum'] += float(r['win_rate'])

# Compute averages
for key, s in global_stats.items():
    s['pass_rate'] = s['pass'] / max(s['total'], 1) * 100
    s['avg_sharpe'] = s['sharpe_sum'] / max(s['total'], 1)
    s['avg_ret'] = s['ret_sum'] / max(s['total'], 1)
    s['avg_win_rate'] = s['win_rate_sum'] / max(s['total'], 1)

# Sort by pass_rate desc, then sharpe desc
sorted_keys = sorted(global_stats.keys(),
                     key=lambda k: (global_stats[k]['pass_rate'],
                                    global_stats[k]['avg_sharpe']),
                     reverse=True)

print("\n=== Global Summary (sorted: pass_rate DESC, sharpe DESC) ===")
print(f"{'ATR_mult':>10} {'vol_confirm':>14} {'pass_n':>6} {'total':>6} "
      f"{'pass_rt':>9} {'avg_sharpe':>11} {'avg_ret':>10} {'trades':>8}")
print("-" * 75)
for key in sorted_keys[:15]:  # top 15
    am, vn = key
    s = global_stats[key]
    print(f"{am:>10} {vn:>14} {s['pass']:>6} {s['total']:>6} "
          f"{s['pass_rate']:>8.1f}% {s['avg_sharpe']:>11.4f} {s['avg_ret']:>10.2f} {s['trades']:>8}")

# ── ATR_mult sweep analysis (vol=none only) ──────────────────────────────────
atr_none = {k: v for k, v in global_stats.items() if k[1] == 'none'}
atr_sharpe = sorted(atr_none.keys(), key=lambda k: float(k[0]))
print("\n=== ATR_mult sweep (vol=none) ===")
print(f"{'ATR_mult':>10} {'pass_n':>6} {'total':>6} {'pass_rt':>9} {'avg_sharpe':>11} {'avg_ret':>10} {'trades':>8}")
print("-" * 65)
for k in sorted(atr_sharpe, key=lambda x: float(x[0])):
    s = atr_none[k]
    am = k[0]
    print(f"{am:>10} {s['pass']:>6} {s['total']:>6} {s['pass_rate']:>8.1f}% {s['avg_sharpe']:>11.4f} {s['avg_ret']:>10.2f} {s['trades']:>8}")

# ── Volume confirmation analysis (ATR_mult=0.0 only) ─────────────────────────
vol_0 = {k: v for k, v in global_stats.items() if float(k[0]) == 0.0}
print("\n=== Volume Confirmation sweep (ATR_mult=0.0) ===")
sorted_vol = sorted(vol_0.keys(), key=lambda k: global_stats[k]['pass_rate'], reverse=True)
print(f"{'vol_confirm':>14} {'pass_n':>6} {'total':>6} {'pass_rt':>9} {'avg_sharpe':>11} {'avg_ret':>10} {'trades':>8}")
print("-" * 65)
for k in sorted_vol:
    s = vol_0[k]
    vn = k[1]
    print(f"{vn:>14} {s['pass']:>6} {s['total']:>6} {s['pass_rate']:>8.1f}% {s['avg_sharpe']:>11.4f} {s['avg_ret']:>10.2f} {s['trades']:>8}")

# ── Equity Curve Loading ─────────────────────────────────────────────────────
equity_by_key = defaultdict(list)
with open(equity_file) as f:
    reader = csv.DictReader(f)
    for row in reader:
        key = (row['atr_mult'], row['vol_label'])
        equity_by_key[key].append({
            'universe': row['universe'],
            'window': row['window'],
            'bar': int(row['bar_idx']),
            'equity': float(row['equity'])
        })

print(f"\nLoaded equity curves for {len(equity_by_key)} configurations")

# ── Compute mean equity curve per config ─────────────────────────────────────
# Aggregate across all universes/windows by averaging equity at each bar index
from collections import Counter
eq_means = {}
for key, curves in equity_by_key.items():
    if not curves:
        continue
    # Group by bar_idx
    by_bar = defaultdict(list)
    for pt in curves:
        by_bar[pt['bar']].append(pt['equity'])
    # Compute mean at each bar
    mean_eq = {}
    for bar, eqs in by_bar.items():
        mean_eq[bar] = sum(eqs) / len(eqs)
    sorted_bars = sorted(mean_eq.keys())
    eq_means[key] = (sorted_bars, [mean_eq[b] for b in sorted_bars])

# ── Generate Charts ──────────────────────────────────────────────────────────
try:
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
    HAS_MATPLOTLIB = True
except ImportError:
    HAS_MATPLOTLIB = False
    print("matplotlib not available — skipping chart generation")

if HAS_MATPLOTLIB:
    fig, axes = plt.subplots(2, 2, figsize=(16, 12))
    fig.suptitle('Turtle Entry Filter Sweep: ATR Entry × Vol Confirmation\n'
                 'P=15/M=1.50/ATR=24/HM=45 — 9 universes × 7 windows', fontsize=14, fontweight='bold')

    # ── Plot 1: Pass rate by ATR_mult (vol=none) ────────────────────────────
    ax1 = axes[0, 0]
    am_vals = sorted([float(k[0]) for k in atr_none.keys()])
    pass_rates = [atr_none[(f"{am:.2g}", 'none')]['pass_rate'] for am in am_vals]
    sharpes    = [atr_none[(f"{am:.2g}", 'none')]['avg_sharpe'] for am in am_vals]
    trades_cnt = [atr_none[(f"{am:.2g}", 'none')]['trades'] for am in am_vals]

    color = ['#2ecc71' if pr > 50 else '#e74c3c' if pr < 30 else '#f39c12' for pr in pass_rates]
    bars = ax1.bar([str(am) for am in am_vals], pass_rates, color=color, edgecolor='black', alpha=0.8)
    ax1.axhline(50, color='red', linestyle='--', alpha=0.5, label='50% threshold')
    ax1.set_xlabel('ATR Entry Multiplier')
    ax1.set_ylabel('Pass Rate (%)')
    ax1.set_title('Pass Rate by ATR Entry Filter (vol=none)')
    ax1.grid(axis='y', alpha=0.3)
    for bar, sr, sh in zip(bars, pass_rates, sharpes):
        ax1.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                 f'{sr:.0f}%\nsh={sh:.2f}', ha='center', va='bottom', fontsize=7)

    # ── Plot 2: Equity curves for key ATR_mult values (vol=none) ──────────
    ax2 = axes[0, 1]
    key_am_vals = [0.0, 0.1, 0.2, 0.5, 1.0]
    colors_eq = ['#2ecc71', '#3498db', '#9b59b6', '#e67e22', '#e74c3c']
    for am, c in zip(key_am_vals, colors_eq):
        key = (f"{am:.2g}", 'none')
        if key in eq_means:
            bars, eqs = eq_means[key]
            label = f'ATR_mult={am}' + (' (baseline)' if am == 0.0 else '')
            ax2.plot(bars, eqs, label=label, color=c, linewidth=1.5, alpha=0.85)
    ax2.set_xlabel('Bar index (test window)')
    ax2.set_ylabel('Mean equity (across all universes/windows)')
    ax2.set_title('Equity Curves by ATR Entry Filter (vol=none)')
    ax2.legend(fontsize=8)
    ax2.grid(alpha=0.3)
    ax2.axhline(1.0, color='black', linestyle=':', alpha=0.4)

    # ── Plot 3: Volume confirmation comparison (ATR_mult=0.0) ───────────────
    ax3 = axes[1, 0]
    vol_keys = sorted(vol_0.keys(), key=lambda k: global_stats[k]['pass_rate'], reverse=True)
    vol_labels = [k[1] for k in vol_keys]
    vol_prs = [global_stats[k]['pass_rate'] for k in vol_keys]
    vol_shs = [global_stats[k]['avg_sharpe'] for k in vol_keys]
    vol_trs = [global_stats[k]['trades'] for k in vol_keys]

    colors3 = ['#2ecc71' if v == 'none' else '#3498db' for v in vol_labels]
    bars3 = ax3.bar(vol_labels, vol_prs, color=colors3, edgecolor='black', alpha=0.8)
    ax3.axhline(50, color='red', linestyle='--', alpha=0.5)
    ax3.set_xlabel('Volume Confirmation')
    ax3.set_ylabel('Pass Rate (%)')
    ax3.set_title('Pass Rate by Volume Confirmation (ATR_mult=0.0)')
    ax3.tick_params(axis='x', rotation=20)
    ax3.grid(axis='y', alpha=0.3)
    for bar, pr, sh, tr in zip(bars3, vol_prs, vol_shs, vol_trs):
        ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                 f'{pr:.0f}%\nsh={sh:.2f}\n{tr}d', ha='center', va='bottom', fontsize=7)

    # ── Plot 4: Heatmap — ATR_mult × vol_confirm ────────────────────────────
    ax4 = axes[1, 1]
    # Build matrix
    am_vals_ordered = sorted(set(float(k[0]) for k in global_stats.keys()))
    vol_labels_unique = sorted(set(k[1] for k in global_stats.keys()))
    pr_matrix = []
    for am in am_vals_ordered:
        row = []
        for vn in vol_labels_unique:
            key = (f"{am:.2g}", vn)
            if key in global_stats:
                row.append(global_stats[key]['pass_rate'])
            else:
                row.append(0.0)
        pr_matrix.append(row)

    import numpy as np
    pr_arr = np.array(pr_matrix)
    im = ax4.imshow(pr_arr, aspect='auto', cmap='RdYlGn', vmin=0, vmax=100)
    ax4.set_xticks(range(len(vol_labels_unique)))
    ax4.set_xticklabels(vol_labels_unique, rotation=30, ha='right', fontsize=8)
    ax4.set_yticks(range(len(am_vals_ordered)))
    ax4.set_yticklabels([f'{am:.2g}' for am in am_vals_ordered])
    ax4.set_xlabel('Volume Confirmation')
    ax4.set_ylabel('ATR Entry Multiplier')
    ax4.set_title('Pass Rate Heatmap: ATR_mult × vol_confirm (%)')
    plt.colorbar(im, ax=ax4, label='Pass Rate (%)')

    # Annotate cells
    for i in range(len(am_vals_ordered)):
        for j in range(len(vol_labels_unique)):
            val = pr_arr[i, j]
            color = 'white' if val < 30 else 'black'
            ax4.text(j, i, f'{val:.0f}', ha='center', va='center', color=color, fontsize=7)

    plt.tight_layout(rect=[0, 0, 1, 0.96])
    out_path = 'charts/turtle_entry_filter_comparison.png'
    plt.savefig(out_path, dpi=150, bbox_inches='tight')
    print(f"\nSaved chart: {out_path}")

    # ── Separate equity comparison chart ───────────────────────────────────
    fig2, ax = plt.subplots(figsize=(14, 8))

    # Winners: ATR_mult=0.0 none (baseline), ATR_mult=0.1 SMA10_1.00 (runner-up)
    configs_to_plot = [
        ('0', 'none', '#2ecc71', 'Baseline: ATR×0.0, vol=none', 2.0),
        ('0.1', 'SMA10_1.00', '#3498db', 'Runner-up: ATR×0.1, vol=SMA10×1.0', 1.5),
        ('0.2', 'none', '#9b59b6', 'Runner-up: ATR×0.2, vol=none', 1.5),
    ]

    for am, vn, c, label, lw in configs_to_plot:
        key = (f"{float(am):.2g}" if float(am) == 0.0 else am, vn)
        # Try both formatting variants
        if key not in eq_means:
            key = (f"{float(am):.2f}", vn)
        if key in eq_means:
            bars_list, eqs_list = eq_means[key]
            ax.plot(bars_list, eqs_list, label=label, color=c, linewidth=lw, alpha=0.85)

    ax.set_xlabel('Bar index (test window, aggregated mean)', fontsize=11)
    ax.set_ylabel('Mean equity across 9 universes × 7 windows', fontsize=11)
    ax.set_title('Equity Curve Comparison: Turtle Entry Filters\n'
                 'P=15/M=1.50/ATR=24/HM=45 — Aggregated 9-universe walk-forward', fontsize=12, fontweight='bold')
    ax.legend(fontsize=10)
    ax.grid(alpha=0.3)
    ax.axhline(1.0, color='black', linestyle=':', alpha=0.4, label='initial equity')
    ax.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.2f'))

    # Annotate final values
    for am, vn, c, label, lw in configs_to_plot:
        key = (f"{float(am):.2g}" if float(am) == 0.0 else am, vn)
        if key not in eq_means:
            key = (f"{float(am):.2f}", vn)
        if key in eq_means:
            bars_list, eqs_list = eq_means[key]
            final_eq = eqs_list[-1]
            final_bar = bars_list[-1]

    plt.tight_layout()
    eq_path = 'charts/turtle_entry_filter_equity.png'
    fig2.savefig(eq_path, dpi=150, bbox_inches='tight')
    print(f"Saved equity chart: {eq_path}")

    print("\nDone! Charts saved to:")
    print(f"  {out_path}")
    print(f"  {eq_path}")
else:
    print("matplotlib not available — charts not generated")
