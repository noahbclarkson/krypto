
import sys, subprocess
try:
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print("matplotlib not available, skipping chart")
    sys.exit(0)

import csv

def load_csv(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append(row)
    return rows

rows = load_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t95_hap_equity.csv')
if not rows:
    print("No equity data")
    sys.exit(0)

headers = list(rows[0].keys())
hap_cols = [h for h in headers if h.startswith('hap_')]
print(f"Loaded {len(rows)} bars, {len(hap_cols)} curves: {hap_cols}")

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), sharex=True)

colors = ['#2E86AB', '#E94F37', '#1B998B', '#F26419', '#9B5DE5']
style  = ['-', '--', '-.', ':', '-']

for i, col in enumerate(hap_cols):
    vals = [float(r[col]) for r in rows]
    bars = list(range(len(vals)))
    ax1.plot(bars, vals, color=colors[i % len(colors)], linestyle=style[i % len(style)],
            linewidth=1.5, label=col.replace('hap_', 'HAP='))

ax1.set_title('T95 HEDGE_ATR_PCT Sweep — Exact-Live Equity Curves\n(Baseline + Top Runners, Walk-Forward OOS)', fontsize=13)
ax1.set_ylabel('Equity (normalized, log scale)')
ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.2f}x'))
ax1.grid(True, alpha=0.3)
ax1.legend(loc='upper left', fontsize=9)

# Drawdown panel.
for i, col in enumerate(hap_cols):
    vals = [float(r[col]) for r in rows]
    peak = vals[0]
    dds = []
    for v in vals:
        if v > peak: peak = v
        dds.append((peak - v) / peak * 100)
    ax2.plot(list(range(len(dds))), dds, color=colors[i % len(colors)],
             linestyle=style[i % len(style)], linewidth=1.5, label=col.replace('hap_', 'HAP='))

ax2.set_title('Drawdown (%)', fontsize=11)
ax2.set_ylabel('Drawdown (%)')
ax2.set_ylim(bottom=0)
ax2.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}%'))
ax2.grid(True, alpha=0.3)
ax2.legend(loc='upper left', fontsize=9)

plt.tight_layout()
out = '/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png'
plt.savefig(out, dpi=150, bbox_inches='tight')
print(f"Chart saved: {out}")
