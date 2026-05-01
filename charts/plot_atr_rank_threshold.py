#!/usr/bin/env python3
"""Chart ATR_RANK_THRESHOLD sweep results."""

import csv
import math
import sys
import re

try:
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print("matplotlib not available, skipping chart")
    sys.exit(0)

# Load sweep detail
sweep_path = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/atr_rank_t_sweep.csv'
summary_path = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/atr_rank_t_summary.csv'

with open(sweep_path) as f:
    detail = list(csv.DictReader(f))
with open(summary_path) as f:
    summary = {int(float(r['threshold'])): r for r in csv.DictReader(f)}

universes = ['Base5', 'NoDOGE', 'Legacy4', 'Legacy5BNB', 'OldGuardNoBNB',
             'LargeCaps5', 'Legacy3', 'LowVolume5', 'OldGuard4']
n_windows = 7

target_thresholds = [0, 5, 24, 39, 80]

# Build return_pct per (universe, window, threshold)
by_ut = {}
for row in detail:
    t = int(float(row['threshold']))
    if t not in target_thresholds:
        continue
    u = row['universe']
    w = int(row['window'])
    ret = float(row['return_pct'])
    by_ut[(u, w, t)] = ret

# Compute per-window compound equity for each (universe, threshold) pair
compound_eq = {t: {} for t in target_thresholds}
for t in target_thresholds:
    for u in universes:
        agg = 1.0
        valid = True
        for w in range(n_windows):
            key = (u, w, t)
            if key in by_ut:
                agg *= (1.0 + by_ut[key] / 100.0)
            else:
                valid = False
                break
        if valid:
            compound_eq[t][u] = agg

# Geometric mean across all universes for each threshold
def geom_mean(threshold):
    vals = [compound_eq[threshold][u] for u in universes if u in compound_eq[threshold]]
    if not vals:
        return 0.0
    return math.pow(math.prod(vals), 1.0 / len(vals))

# Per-window cumulative compound for Base5
cum_by_t = {t: [1.0] for t in target_thresholds}
for t in target_thresholds:
    for w in range(n_windows):
        key = ('Base5', w, t)
        if key in by_ut:
            cum_by_t[t].append(cum_by_t[t][-1] * (1.0 + by_ut[key] / 100.0))
        else:
            cum_by_t[t].append(cum_by_t[t][-1])

# ── Figure 1: Full sweep pass-rate heatmap ──────────────────────────────────
thresholds_all = list(range(0, 101))
pass_by_t = {int(float(r['threshold'])): int(r['pass_count']) for r in summary.values()}
sharpe_by_t = {int(float(r['threshold'])): float(r['avg_sharpe']) for r in summary.values()}
ret_by_t = {int(float(r['threshold'])): float(r['avg_return_pct']) for r in summary.values()}

fig, axes = plt.subplots(2, 1, figsize=(14, 10), sharex=True)

ax = axes[0]
ts = sorted(thresholds_all)
passes = [pass_by_t.get(t, 0) for t in ts]
ax.fill_between(ts, passes, alpha=0.25, color='steelblue')
ax.plot(ts, passes, color='steelblue', lw=1.5)
ax.axvline(5, color='red', lw=1.5, linestyle='--', label='T=5 (current)')
ax.axvline(24, color='green', lw=1.5, linestyle='--', label='T=24 (winner)')
ax.axvline(80, color='orange', lw=1.5, linestyle='--', label='T=80 (conservative)')
ax.set_ylabel('Pass Count (out of 63)')
ax.set_title('ATR_RANK_THRESHOLD Sweep — Live Path (Turtle Exit)\n101 Thresholds × 9 Universes × 7 Windows = 6,363 Runs')
ax.legend()
ax.grid(True, alpha=0.3)
ax.set_ylim(0, 65)

ax2 = axes[1]
sharpes = [sharpe_by_t.get(t, 0) for t in ts]
# Cap extreme values for display (T=90 has 4011778 Sharpe — clear outlier/artifact)
sharpes_capped = [min(s, 50) for s in sharpes]
ax2.plot(ts, sharpes_capped, color='purple', lw=1.5)
ax2.axvline(5, color='red', lw=1.5, linestyle='--', label='T=5 (current)')
ax2.axvline(24, color='green', lw=1.5, linestyle='--', label='T=24 (winner)')
ax2.axvline(80, color='orange', lw=1.5, linestyle='--', label='T=80 (conservative)')
ax2.set_xlabel('ATR_RANK_THRESHOLD (T)')
ax2.set_ylabel('Avg Sharpe Ratio')
ax2.set_title('Avg Sharpe by Threshold (capped at 50 for display)')
ax2.legend()
ax2.grid(True, alpha=0.3)

plt.tight_layout()
path1 = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/atr_rank_threshold_sweep.png'
plt.savefig(path1, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {path1}")

# ── Figure 2: Base5 compound equity per-window for selected thresholds ───────
fig, ax = plt.subplots(figsize=(12, 7))
colors = {0: '#1f77b4', 5: '#ff7f0e', 24: '#2ca02c', 39: '#d62728', 80: '#9467bd'}
labels = {0: 'T=0 (no gate)', 5: 'T=5 (current)', 24: 'T=24 (winner)', 39: 'T=39', 80: 'T=80 (high-gate)'}
markers = {0: 'o', 5: 's', 24: '*', 39: 'D', 80: '^'}
windows = list(range(n_windows + 1))

for t in sorted(target_thresholds):
    ax.plot(windows, cum_by_t[t], color=colors[t], marker=markers[t],
            ms=8, lw=2, label=labels[t])

ax.set_xlabel('Walk-Forward Window', fontsize=12)
ax.set_ylabel('Compound Equity (Base5 × 6 symbols)', fontsize=12)
ax.set_title('Base5 Compound Equity Progression — Selected ATR_RANK_THRESHOLD Values\n(Geometric compounding across 7 walk-forward windows)', fontsize=13)
ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x'))
ax.set_xticks(windows)
ax.set_xticklabels([f'W{w}' for w in windows])
ax.legend(fontsize=10)
ax.grid(True, alpha=0.3)
ax.set_ylim(bottom=0.1)

# Annotate final values
for t in sorted(target_thresholds):
    final = cum_by_t[t][-1]
    ax.annotate(f'{final:.1f}x', xy=(n_windows, final),
                xytext=(n_windows + 0.2, final),
                fontsize=9, color=colors[t])

path2 = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/atr_rank_threshold_base5_equity.png'
plt.savefig(path2, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {path2}")

# ── Figure 3: Geometric-mean equity across all 9 universes ─────────────────
fig, ax = plt.subplots(figsize=(12, 7))
thresholds_sorted = sorted(thresholds_all)

gm_eq = []
for t in thresholds_sorted:
    s = summary.get(t)
    if s:
        # Compute geometric mean from compound equity across universes
        pass_cnt = int(s['pass_count'])
        if pass_cnt == 0:
            gm_eq.append(0.0)
        else:
            # Approximate: use avg_return_pct to compute rough equity
            # Better: compute from per-universe compound
            pass_weight = pass_cnt / 63.0
            # Simple geometric average from the return percentages
            # We'll use the summary's geometric mean
            avg_r = float(s['avg_return_pct'])
            # Convert avg return % to equity multiplier (rough)
            # This isn't perfect but gives relative comparison
            gm_eq.append(pass_weight)  # placeholder
    else:
        gm_eq.append(0.0)

# Actually compute properly from compound_eq (only for target_thresholds)
gm_vals = []
gm_ts = []
for t in target_thresholds:
    vals = [compound_eq[t][u] for u in universes if u in compound_eq[t]]
    if vals:
        gm_vals.append(math.pow(math.prod(vals), 1.0/len(vals)))
        gm_ts.append(t)
    else:
        gm_vals.append(0.0)
        gm_ts.append(t)

# Cap extreme values
gm_capped = [min(v, 50) for v in gm_vals]
ax.fill_between(gm_ts, gm_capped, alpha=0.2, color='teal')
ax.plot(gm_ts, gm_capped, color='teal', lw=2, marker='o', ms=6)
ax.axvline(5, color='red', lw=2, linestyle='--', label='T=5 (current)')
ax.axvline(24, color='green', lw=2, linestyle='--', label='T=24 (winner)')
ax.set_xlabel('ATR_RANK_THRESHOLD (T)', fontsize=12)
ax.set_ylabel('Geometric Mean Equity (9 universes)', fontsize=12)
ax.set_title('Geometric Mean Equity Across All Universes — by Threshold', fontsize=13)
ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.1f}x'))
ax.legend(fontsize=10)
ax.grid(True, alpha=0.3)

path3 = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/atr_rank_threshold_geomean.png'
plt.savefig(path3, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {path3}")

# ── Figure 4: Per-universe compound equity for top thresholds ────────────────
fig, ax = plt.subplots(figsize=(14, 8))
x = list(range(len(universes)))
width = 0.15
offsets = [-2, -1, 0, 1, 2]
colors_bar = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728', '#9467bd']
for i, t in enumerate(sorted(target_thresholds)):
    vals = [compound_eq[t].get(u, float('nan')) for u in universes]
    # Cap at 100 for display
    vals_capped = [min(v, 100) for v in vals]
    ax.bar([xi + offsets[i] * width for xi in x], vals_capped,
           width, color=colors_bar[i], label=labels[t], alpha=0.85)

ax.set_xticks(x)
ax.set_xticklabels(universes, rotation=30, ha='right', fontsize=10)
ax.set_ylabel('Compound Equity (capped at 100x for display)', fontsize=11)
ax.set_xlabel('Universe', fontsize=12)
ax.set_title('Per-Universe Compound Equity — Top Threshold Candidates\n(capped at 100x, log scale would obscure smaller values)', fontsize=13)
ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x'))
ax.legend(fontsize=10)
ax.grid(True, alpha=0.3, axis='y')

path4 = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/atr_rank_threshold_per_universe.png'
plt.savefig(path4, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved {path4}")

print("\nAll charts saved.")
print("Summary for report:")
for t in sorted(target_thresholds):
    s = summary.get(t)
    gm = math.pow(math.prod([compound_eq[t][u] for u in universes if u in compound_eq[t]]), 1.0/9)
    print(f"  T={t:>3}: pass={s['pass_count']}/63, avg_sharpe={float(s['avg_sharpe']):.4f}, avg_ret={float(s['avg_return_pct']):.2f}%, geomean_eq={gm:.3f}x")
